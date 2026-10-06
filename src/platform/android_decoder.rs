//! Real Android decoder pipeline for Phase 3.
//!
//! Connects the existing Tunefold stream abstraction to Symphonia,
//! decodes audio, converts to f32 interleaved stereo PCM, and feeds
//! the existing PCM ring buffer + analysis tap.
//!
//! Pipeline:
//!   HTTP stream → Symphonia decoder → PCM conversion → SPSC ring → Oboe
//!                                                        ↓
//!                                                  Analysis Tap

use std::io::SeekFrom;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::analysis::{PcmTap, StreamMeta};
use crate::platform::android_playback::PcmRing;

/// Eventos canónicos de la traza de reproducción. Los nombres son estables:
/// la documentación y los informes los usan literalmente.
pub mod events {
    /// Instante en que el usuario solicita reproducir (relativo a Play tap).
    pub const PLAY_TAP: &str = "PLAY_TAP";
    /// El controlador Android recibió la orden de reproducir.
    pub const CONTROLLER_RECEIVED_PLAY: &str = "CONTROLLER_RECEIVED_PLAY";
    /// Hay metadata del track.
    pub const METADATA_AVAILABLE: &str = "METADATA_AVAILABLE";
    /// Empieza la resolución Track → fuente reproducible.
    pub const SOURCE_RESOLUTION_START: &str = "SOURCE_RESOLUTION_START";
    /// Ya hay `PlayableSource` (URL temporal o `file:`).
    pub const PLAYABLE_SOURCE_AVAILABLE: &str = "PLAYABLE_SOURCE_AVAILABLE";
    /// Empieza la apertura del stream HTTP.
    pub const HTTP_OPEN_START: &str = "HTTP_OPEN_START";
    /// Llegaron las cabeceras de la primera respuesta Range.
    pub const HTTP_FIRST_RESPONSE: &str = "HTTP_FIRST_RESPONSE";
    /// Empieza el probe del contenedor por Symphonia.
    pub const SYMPHONIA_PROBE_START: &str = "SYMPHONIA_PROBE_START";
    /// Termina el probe del contenedor.
    pub const SYMPHONIA_PROBE_END: &str = "SYMPHONIA_PROBE_END";
    /// El decoder produjo su primer paquete comprimido.
    pub const DECODER_FIRST_PACKET: &str = "DECODER_FIRST_PACKET";
    /// El decoder alimentó PCM por primera vez al ring.
    pub const DECODER_FIRST_PCM: &str = "DECODER_FIRST_PCM";
    /// `AudioTrack` existe y está en play.
    pub const AUDIOTRACK_START: &str = "AUDIOTRACK_START";
    /// Primer write positivo a `AudioTrack` (proxy mínimo de salida audible).
    pub const AUDIOTRACK_FIRST_POSITIVE_WRITE: &str = "AUDIOTRACK_FIRST_POSITIVE_WRITE";
    /// TTFA = primer output positivo − Play tap.
    pub const TTFA: &str = "TTFA";
}

/// Una línea de la traza de reproducción, anclada al Play tap.
///
/// `at_us` son microsegundos desde el **Play tap** (no desde el inicio del
/// decoder): es lo que permite calcular TTFA y separar UI/provider, source
/// resolution, HTTP, probe, decode y output startup en una sola línea temporal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceLine {
    pub at_us: u64,
    pub event: String,
    pub detail: String,
}

/// Timeline monótona de una reproducción.
///
/// Rust no comparte reloj con Java, así que el tap se alinea pasando los
/// microsegundos transcurridos entre el Play tap y la entrada al decoder.
#[derive(Default)]
pub struct PlaybackTraceLog {
    origin: Mutex<Option<Instant>>,
    origin_tap_us: Mutex<u64>,
    lines: Mutex<Vec<TraceLine>>,
}

impl PlaybackTraceLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fija el origen de la línea temporal: `tap_to_origin_us` son los
    /// microsegundos desde el Play tap hasta `origin` (la entrada al decoder).
    pub fn reset(&self, origin: Instant, tap_to_origin_us: u64) {
        if let Ok(mut slot) = self.origin.lock() {
            *slot = Some(origin);
        }
        if let Ok(mut slot) = self.origin_tap_us.lock() {
            *slot = tap_to_origin_us;
        }
        if let Ok(mut slot) = self.lines.lock() {
            slot.clear();
        }
    }

    /// Registra un evento en microsegundos desde el Play tap.
    pub fn mark(&self, event: &str, detail: &str) {
        let origin = self.origin.lock().ok().and_then(|slot| *slot);
        let tap_us = self
            .origin_tap_us
            .lock()
            .map(|slot| *slot)
            .unwrap_or_default();
        let since_origin = origin
            .map(|at| at.elapsed().as_micros() as u64)
            .unwrap_or_default();
        if let Ok(mut lines) = self.lines.lock() {
            lines.push(TraceLine {
                at_us: tap_us + since_origin,
                event: event.to_string(),
                detail: detail.to_string(),
            });
        }
    }

    /// Copia de las líneas registradas hasta ahora (sin consumir).
    pub fn snapshot(&self) -> Vec<TraceLine> {
        self.lines
            .lock()
            .map(|lines| lines.clone())
            .unwrap_or_default()
    }

    /// Número de líneas registradas.
    pub fn len(&self) -> usize {
        self.lines.lock().map(|lines| lines.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Receptor de telemetría de transporte conectado a la línea temporal del
/// decoder: cada petición Range queda anclada al Play tap.
struct RangeTraceSinkImpl {
    log: Arc<PlaybackTraceLog>,
}

impl crate::media::transport::RangeTraceSink for RangeTraceSinkImpl {
    fn range_request(&self, record: &crate::media::transport::RangeRequestRecord) {
        self.log.mark(
            "RANGE_REQUEST",
            &format!(
                "req={} host={} range={}-{} requested={} received={} status={} headers_us={} total_us={} retry={} class={} reader_pos={} from_seek={}",
                record.request_id,
                record.host,
                record.start,
                record.end,
                record.requested_bytes,
                record.received_bytes,
                record.status
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                record.headers_us,
                record.total_us,
                record.retry,
                record.classification,
                record.reader_position,
                record.from_seek
            ),
        );
    }

    fn event(&self, event: &str, detail: &str) {
        self.log.mark(event, detail);
    }
}

/// Low-rate counters and the current decoder stage surfaced to the Android
/// test host. Updates happen on the decoder thread, never on the audio output.
pub struct DecoderDiagnostics {
    pub(crate) stage: std::sync::Mutex<String>,
    pub(crate) decoded_packets: AtomicU64,
    pub(crate) decoded_frames: AtomicU64,
    pub(crate) ring_frames: AtomicU64,
    pub(crate) analysis_frames: AtomicU64,
    pub(crate) http_open_us: AtomicU64,
    pub(crate) first_response_us: AtomicU64,
    pub(crate) probe_us: AtomicU64,
    pub(crate) first_decode_us: AtomicU64,
    pub(crate) first_pcm_us: AtomicU64,
    pub(crate) range_requests: AtomicU64,
    pub(crate) range_bytes: AtomicU64,
    pub(crate) range_elapsed_ms: AtomicU64,
    /// Microsegundos desde el Play tap hasta la entrada al decoder.
    pub(crate) tap_to_decoder_us: AtomicU64,
    /// Desglose de peticiones Range en la traza (no acumulado).
    pub(crate) seeks: AtomicU64,
    pub(crate) seek_requests: AtomicU64,
    pub(crate) headers_us: AtomicU64,
}

impl DecoderDiagnostics {
    pub fn new() -> Self {
        Self {
            stage: std::sync::Mutex::new("engine initialized".to_string()),
            decoded_packets: AtomicU64::new(0),
            decoded_frames: AtomicU64::new(0),
            ring_frames: AtomicU64::new(0),
            analysis_frames: AtomicU64::new(0),
            http_open_us: AtomicU64::new(0),
            first_response_us: AtomicU64::new(0),
            probe_us: AtomicU64::new(0),
            first_decode_us: AtomicU64::new(0),
            first_pcm_us: AtomicU64::new(0),
            range_requests: AtomicU64::new(0),
            range_bytes: AtomicU64::new(0),
            range_elapsed_ms: AtomicU64::new(0),
            tap_to_decoder_us: AtomicU64::new(0),
            seeks: AtomicU64::new(0),
            seek_requests: AtomicU64::new(0),
            headers_us: AtomicU64::new(0),
        }
    }
}

impl Default for DecoderDiagnostics {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors that can occur during decoding.
#[derive(Debug)]
pub enum DecoderError {
    Stream(String),
    Format(String),
    UnsupportedCodec(String),
    Decode(String),
    Eof,
}

impl std::fmt::Display for DecoderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stream(s) => write!(f, "stream error: {s}"),
            Self::Format(s) => write!(f, "format error: {s}"),
            Self::UnsupportedCodec(s) => write!(f, "unsupported codec: {s}"),
            Self::Decode(s) => write!(f, "decode error: {s}"),
            Self::Eof => write!(f, "end of stream"),
        }
    }
}

impl std::error::Error for DecoderError {}

fn file_path_from_uri(uri: &str) -> Result<std::path::PathBuf, DecoderError> {
    let encoded = uri
        .strip_prefix("file://")
        .or_else(|| uri.strip_prefix("file:"))
        .ok_or_else(|| DecoderError::Stream("unsupported local URI".to_string()))?;
    if !encoded.starts_with('/') {
        return Err(DecoderError::Stream(
            "local URI must contain an absolute path".to_string(),
        ));
    }
    let input = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] == b'%' {
            let Some(pair) = input.get(index + 1..index + 3) else {
                return Err(DecoderError::Stream(
                    "malformed local URI escape".to_string(),
                ));
            };
            let hex = std::str::from_utf8(pair)
                .ok()
                .and_then(|value| u8::from_str_radix(value, 16).ok())
                .ok_or_else(|| DecoderError::Stream("malformed local URI escape".to_string()))?;
            decoded.push(hex);
            index += 3;
        } else {
            decoded.push(input[index]);
            index += 1;
        }
    }
    let path = String::from_utf8(decoded)
        .map_err(|_| DecoderError::Stream("local URI path is not UTF-8".to_string()))?;
    let path = std::path::PathBuf::from(path);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(DecoderError::Stream(
            "local URI must contain an absolute path".to_string(),
        ))
    }
}

/// A reader that implements `MediaSource` for HTTP range streams.
struct HttpMediaSource {
    inner: Box<dyn MediaSource>,
    length: u64,
}

impl HttpMediaSource {
    fn new(reader: Box<dyn MediaSource>, length: u64) -> Self {
        Self {
            inner: reader,
            length,
        }
    }
}

impl std::io::Read for HttpMediaSource {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl std::io::Seek for HttpMediaSource {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

impl MediaSource for HttpMediaSource {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.length)
    }
}

impl MediaSource for StreamReader {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.transport.total())
    }
}

/// Decoder state for the Android backend.
pub struct AndroidDecoder {
    ring: Arc<PcmRing>,
    cancel: Arc<AtomicBool>,
    sample_rate: Arc<AtomicU64>,
    position_ms: Arc<std::sync::Mutex<u64>>,
    analysis_tap: PcmTap,
    playing: Arc<AtomicBool>,
    diagnostics: Arc<DecoderDiagnostics>,
    trace: Arc<PlaybackTraceLog>,
}

impl AndroidDecoder {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ring: Arc<PcmRing>,
        cancel: Arc<AtomicBool>,
        sample_rate: Arc<AtomicU64>,
        position_ms: Arc<std::sync::Mutex<u64>>,
        analysis_tap: PcmTap,
        playing: Arc<AtomicBool>,
        diagnostics: Arc<DecoderDiagnostics>,
        trace: Arc<PlaybackTraceLog>,
    ) -> Self {
        Self {
            ring,
            cancel,
            sample_rate,
            position_ms,
            analysis_tap,
            playing,
            diagnostics,
            trace,
        }
    }

    /// Decode a remote range stream or an explicitly cached local media file.
    /// This runs on a dedicated decoder thread (not the audio callback).
    pub fn decode_stream(
        &self,
        url: String,
        headers: Vec<(String, String)>,
    ) -> Result<(), DecoderError> {
        let decode_started = std::time::Instant::now();
        // Alinea la línea temporal con el Play tap medido en Java.
        self.trace.reset(
            decode_started,
            self.diagnostics.tap_to_decoder_us.load(Ordering::Relaxed),
        );
        let runtime = Arc::new(
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| DecoderError::Stream(e.to_string()))?,
        );
        // Un único runtime conduce TODAS las operaciones async de esta
        // reproducción (apertura + lecturas + seeks). Ver StreamReader.

        let mss = if url.starts_with("file:") {
            if !headers.is_empty() {
                return Err(DecoderError::Stream(
                    "local media source cannot use HTTP headers".to_string(),
                ));
            }
            let path = file_path_from_uri(&url)?;
            let file = std::fs::File::open(&path).map_err(|error| {
                DecoderError::Stream(format!("local media open failed: {error}"))
            })?;
            let content_length = file
                .metadata()
                .map_err(|error| DecoderError::Stream(format!("local media stat failed: {error}")))?
                .len();
            self.set_diagnostic(format!("local media opened · {content_length} bytes"));
            self.trace.mark(
                events::HTTP_OPEN_START,
                &format!("kind=local bytes={content_length}"),
            );
            self.trace
                .mark(events::PLAYABLE_SOURCE_AVAILABLE, "kind=local_media");
            if content_length == 0 {
                return Err(DecoderError::Stream("empty local media file".to_string()));
            }
            MediaSourceStream::new(
                Box::new(HttpMediaSource::new(Box::new(file), content_length)),
                MediaSourceStreamOptions::default(),
            )
        } else {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .map_err(|error| DecoderError::Stream(error.to_string()))?;
            let http_started = std::time::Instant::now();
            self.trace.mark(events::HTTP_OPEN_START, "kind=http");
            let trace_context = crate::media::transport::RangeTraceContext::with_sink(Arc::new(
                RangeTraceSinkImpl {
                    log: self.trace.clone(),
                },
            ));
            // El MISMO runtime debe conducir la apertura y todas las lecturas
            // posteriores: reqwest reutiliza la conexión del pool en el
            // executor donde se estableció, así que cambiar de runtime deja
            // su tarea de I/O aparcada y bloquea la siguiente ventana.
            let transport = runtime
                .block_on(crate::media::transport::HttpRangeStream::open_with_trace(
                    client,
                    url,
                    headers,
                    crate::media::transport::RangePolicy::default(),
                    trace_context.clone(),
                ))
                .map_err(|e| DecoderError::Stream(e.to_string()))?;
            self.diagnostics
                .http_open_us
                .store(http_started.elapsed().as_micros() as u64, Ordering::Relaxed);

            let content_length = transport.total();
            self.set_diagnostic(format!("HTTP stream opened · {content_length} bytes"));
            if content_length == 0 {
                return Err(DecoderError::Stream("empty stream".to_string()));
            }

            let reader = StreamReader::new(
                transport,
                self.diagnostics.clone(),
                runtime.clone(),
                trace_context,
            );
            MediaSourceStream::new(
                Box::new(HttpMediaSource::new(Box::new(reader), content_length)),
                MediaSourceStreamOptions::default(),
            )
        };

        let mut hint = Hint::new();
        hint.with_extension("mp4");

        let format_opts = FormatOptions::default();
        let metadata_opts = MetadataOptions::default();
        let decoder_opts = DecoderOptions::default();

        self.trace.mark(events::SYMPHONIA_PROBE_START, "");
        let probe_started = std::time::Instant::now();
        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &format_opts, &metadata_opts)
            .map_err(|e| DecoderError::Format(e.to_string()))?;
        self.diagnostics.probe_us.store(
            probe_started.elapsed().as_micros() as u64,
            Ordering::Relaxed,
        );
        self.trace.mark(
            events::SYMPHONIA_PROBE_END,
            &format!("probe_us={}", probe_started.elapsed().as_micros()),
        );

        let mut format = probed.format;

        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or_else(|| DecoderError::UnsupportedCodec("no audio track".to_string()))?
            .clone();

        let sample_rate = track
            .codec_params
            .sample_rate
            .filter(|sample_rate| *sample_rate > 0)
            .ok_or_else(|| DecoderError::Format("audio track has no sample rate".to_string()))?;
        let source_channels = track
            .codec_params
            .channels
            .map_or(0, |channels| channels.count());
        self.set_diagnostic(format!(
            "Symphonia track · codec {:?} · {sample_rate} Hz · {source_channels} source channels · PCM f32 interleaved stereo",
            track.codec_params.codec
        ));
        self.sample_rate
            .store(sample_rate as u64, Ordering::Relaxed);
        // The Android PCM ring and AudioTrack bridge are explicitly stereo.
        // Analysis receives the same stereo contract after the channel map.
        self.analysis_tap.announce(StreamMeta {
            sample_rate,
            channels: 2,
        });

        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &decoder_opts)
            .map_err(|e| DecoderError::UnsupportedCodec(e.to_string()))?;

        let track_id = track.id;
        let mut sample_buf: Option<SampleBuffer<f32>> = None;
        let mut stereo = Vec::new();

        loop {
            if self.cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            while !self.playing.load(Ordering::Acquire) {
                if self.cancel.load(Ordering::Relaxed) {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(10));
            }

            let packet = match format.next_packet() {
                Ok(packet) => packet,
                Err(symphonia::core::errors::Error::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                Err(e) => return Err(DecoderError::Decode(e.to_string())),
            };

            if packet.track_id() != track_id {
                continue;
            }

            if self.diagnostics.decoded_packets.load(Ordering::Relaxed) == 0 {
                self.trace.mark(
                    events::DECODER_FIRST_PACKET,
                    &format!("bytes={} ts={:?}", packet.data.len(), packet.ts),
                );
            }

            match decoder.decode(&packet) {
                Ok(decoded) => {
                    if self.diagnostics.decoded_packets.load(Ordering::Relaxed) == 0 {
                        self.diagnostics.first_decode_us.store(
                            decode_started.elapsed().as_micros() as u64,
                            Ordering::Relaxed,
                        );
                    }
                    let packet_count = self
                        .diagnostics
                        .decoded_packets
                        .fetch_add(1, Ordering::Relaxed)
                        + 1;
                    let channels = decoded.spec().channels.count().max(1) as u16;
                    if sample_buf.is_none() {
                        let spec = *decoded.spec();
                        let duration = decoded.capacity() as u64;
                        sample_buf = Some(SampleBuffer::new(duration, spec));
                    }

                    if let Some(ref mut buf) = sample_buf {
                        buf.copy_interleaved_ref(decoded);
                        let samples = buf.samples();
                        if !self.feed_pcm(samples, channels, &mut stereo, decode_started) {
                            return Ok(());
                        }
                    }
                    if packet_count == 1 || packet_count & 63 == 0 {
                        self.set_diagnostic(format!(
                            "decoded {packet_count} packets · {} PCM frames · {sample_rate} Hz · {source_channels} source channels → f32 stereo",
                            self.diagnostics.decoded_frames.load(Ordering::Relaxed),
                        ));
                    }
                }
                Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
                Err(e) => return Err(DecoderError::Decode(e.to_string())),
            }
        }

        self.set_diagnostic(format!(
            "decoder EOF · codec {:?} · {sample_rate} Hz · {source_channels} source channels → f32 interleaved stereo · {} packets · {} frames",
            track.codec_params.codec,
            self.diagnostics.decoded_packets.load(Ordering::Relaxed),
            self.diagnostics.decoded_frames.load(Ordering::Relaxed)
        ));
        Ok(())
    }

    fn feed_pcm(
        &self,
        samples: &[f32],
        channels: u16,
        stereo: &mut Vec<f32>,
        decode_started: std::time::Instant,
    ) -> bool {
        let channels = channels as usize;
        let frames = samples.len() / channels;
        self.diagnostics
            .decoded_frames
            .fetch_add(frames as u64, Ordering::Relaxed);
        stereo.clear();
        stereo.reserve(frames.saturating_mul(2));
        for frame in samples.chunks_exact(channels) {
            let left = frame[0];
            let right = if channels == 1 { left } else { frame[1] };
            stereo.extend_from_slice(&[left, right]);
        }
        let mut offset_frames = 0;
        while offset_frames < frames {
            if self.cancel.load(Ordering::Acquire) {
                return false;
            }

            if !self.playing.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }

            let free_frames = self.ring.free_frames();
            if free_frames == 0 {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }
            let chunk_frames = (frames - offset_frames).min(free_frames);
            let start_sample = offset_frames * 2;
            let end_sample = start_sample + chunk_frames * 2;
            let written_frames = self.ring.push(&stereo[start_sample..end_sample]);
            if written_frames == 0 {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }
            self.diagnostics
                .ring_frames
                .fetch_add(written_frames as u64, Ordering::Relaxed);
            // `first_pcm_us` conserva su semántica original: primer PCM
            // ACEPTADO por el PCM ring, relativo al INICIO DEL DECODER.
            if self
                .diagnostics
                .first_pcm_us
                .compare_exchange(
                    0,
                    decode_started.elapsed().as_micros() as u64,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                self.trace.mark(
                    events::DECODER_FIRST_PCM,
                    &format!(
                        "first_pcm_us={} frames={written_frames}",
                        decode_started.elapsed().as_micros()
                    ),
                );
            }

            let written_samples = written_frames * 2;
            let mut analysis_offset = 0;
            while analysis_offset < written_samples {
                if self.cancel.load(Ordering::Acquire) {
                    return false;
                }
                if !self.playing.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                }
                let accepted = self.analysis_tap.feed_stereo(
                    &stereo
                        [offset_frames * 2 + analysis_offset..offset_frames * 2 + written_samples],
                );
                if accepted == 0 {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                }
                debug_assert_eq!(accepted % 2, 0, "analysis ring split an interleaved frame");
                self.diagnostics
                    .analysis_frames
                    .fetch_add((accepted / 2) as u64, Ordering::Relaxed);
                analysis_offset += accepted;
            }

            let sample_rate = self.sample_rate.load(Ordering::Relaxed);
            if let Some(delta) = (written_frames as u64 * 1000).checked_div(sample_rate) {
                if let Ok(mut pos) = self.position_ms.lock() {
                    *pos += delta;
                }
            }
            offset_frames += written_frames;
        }
        true
    }

    fn set_diagnostic(&self, message: String) {
        if let Ok(mut diagnostic) = self.diagnostics.stage.lock() {
            *diagnostic = message;
        }
    }
}

/// A reader that wraps `HttpRangeStream` and implements `Read + Seek`.
///
/// IMPORTANT — un solo runtime: el `HttpRangeStream` reutiliza conexiones del
/// pool de reqwest cuya tarea de I/O se县级以上n en el executor donde se
/// establecieron. Si la apertura y estas lecturas se condujeran con runtimes
/// distintos, la conexión aparcada nunca volvería a bombear y la siguiente
/// ventana HTTP se quedaría bloqueada para siempre. Por eso `runtime` es el
/// MISMO runtime que abrió el stream, prestado por `decode_stream`.
struct StreamReader {
    transport: crate::media::transport::HttpRangeStream,
    diagnostics: Arc<DecoderDiagnostics>,
    position: u64,
    buffer: Vec<u8>,
    buffer_pos: usize,
    eof: bool,
    runtime: Arc<tokio::runtime::Runtime>,
    trace: crate::media::transport::RangeTraceContext,
}

impl StreamReader {
    fn new(
        transport: crate::media::transport::HttpRangeStream,
        diagnostics: Arc<DecoderDiagnostics>,
        runtime: Arc<tokio::runtime::Runtime>,
        trace: crate::media::transport::RangeTraceContext,
    ) -> Self {
        Self {
            transport,
            diagnostics,
            position: 0,
            buffer: Vec::new(),
            buffer_pos: 0,
            eof: false,
            runtime,
            trace,
        }
    }

    fn fill_buffer(&mut self) -> std::io::Result<bool> {
        if self.eof {
            return Ok(false);
        }
        self.trace.set_reader_position(self.position);
        let result = self
            .runtime
            .block_on(async { self.transport.next_chunk(256 * 1024).await });
        self.sync_metrics();
        match result {
            Ok(Some(chunk)) => {
                if chunk.is_empty() {
                    self.eof = true;
                    Ok(false)
                } else {
                    self.buffer = chunk;
                    self.buffer_pos = 0;
                    Ok(true)
                }
            }
            Ok(None) => {
                self.eof = true;
                Ok(false)
            }
            Err(e) => Err(std::io::Error::other(e.to_string())),
        }
    }

    fn sync_metrics(&self) {
        let metrics = self.transport.metrics();
        self.diagnostics
            .range_requests
            .store(metrics.requests, Ordering::Relaxed);
        self.diagnostics
            .first_response_us
            .store(metrics.first_response_us, Ordering::Relaxed);
        self.diagnostics
            .range_bytes
            .store(metrics.bytes_received, Ordering::Relaxed);
        self.diagnostics
            .range_elapsed_ms
            .store(metrics.elapsed_ms, Ordering::Relaxed);
        self.diagnostics
            .headers_us
            .store(metrics.headers_us, Ordering::Relaxed);
        self.diagnostics
            .seeks
            .store(metrics.seeks, Ordering::Relaxed);
        self.diagnostics
            .seek_requests
            .store(metrics.seek_requests, Ordering::Relaxed);
    }
}

impl std::io::Read for StreamReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.buffer_pos >= self.buffer.len() {
            self.fill_buffer()?;
            if self.buffer_pos >= self.buffer.len() {
                return Ok(0);
            }
        }

        let remaining = &self.buffer[self.buffer_pos..];
        let n = remaining.len().min(buf.len());
        buf[..n].copy_from_slice(&remaining[..n]);
        self.buffer_pos += n;
        self.position += n as u64;
        Ok(n)
    }
}

impl std::io::Seek for StreamReader {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let new_pos = match pos {
            SeekFrom::Start(n) => n,
            SeekFrom::Current(n) => (self.position as i64 + n) as u64,
            SeekFrom::End(n) => (self.transport.total() as i64 + n) as u64,
        };
        if new_pos == self.position {
            return Ok(new_pos);
        }
        let seek_result = self.runtime.block_on(self.transport.seek_to(new_pos));
        self.sync_metrics();
        seek_result.map_err(|error| std::io::Error::other(error.to_string()))?;
        self.position = new_pos;
        self.buffer.clear();
        self.buffer_pos = 0;
        self.eof = new_pos >= self.transport.total();
        Ok(new_pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::transport::RangeRequestRecord;
    use std::io::Read;

    fn payload(n: usize) -> Arc<Vec<u8>> {
        Arc::new((0..n).map(|i| (i % 251) as u8).collect())
    }

    /// Servidor HTTP Range con HTTP/1.1 keep-alive, como los CDN reales.
    ///
    /// El keep-alive es lo que hace que reqwest reutilice la conexión del pool:
    /// su tarea de I/O queda anclada al runtime donde se estableció. Si el
    /// `StreamReader` usara un runtime distinto al de la apertura, la
    /// segunda ventana se quedaría bloqueada para siempre.
    fn keepalive_server(
        ready: Arc<Mutex<Option<String>>>,
        data: Arc<Vec<u8>>,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("server runtime");
            rt.block_on(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                *ready.lock().unwrap() = Some(listener.local_addr().unwrap().to_string());
                loop {
                    let Ok((mut socket, _)) = listener.accept().await else { continue };
                    let data = data.clone();
                    tokio::spawn(async move {
                        loop {
                            let mut head = Vec::new();
                            let mut byte = [0u8; 1];
                            loop {
                                match socket.read(&mut byte).await {
                                    Ok(0) | Err(_) => return,
                                    Ok(_) => {}
                                }
                                head.push(byte[0]);
                                if head.ends_with(b"\r\n\r\n") || head.len() > 8192 {
                                    break;
                                }
                            }
                            let text = String::from_utf8_lossy(&head);
                            let range = text
                                .lines()
                                .find_map(|l| {
                                    let (k, v) = l.split_once(':')?;
                                    k.eq_ignore_ascii_case("range")
                                        .then(|| v.trim().to_string())
                                })
                                .unwrap_or_default();
                            let rest = range.strip_prefix("bytes=").unwrap_or("0-");
                            let (s, e) = rest.split_once('-').unwrap_or((rest, rest));
                            let start: u64 = s.parse().unwrap_or(0);
                            let end: u64 = e.parse().unwrap_or(0);
                            let total = data.len() as u64;
                            if start >= total {
                                let resp = format!(
                                    "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{total}\r\nContent-Length: 0\r\n\r\n"
                                );
                                let _ = socket.write_all(resp.as_bytes()).await;
                                continue;
                            }
                            let end = end.min(total - 1);
                            let body = data[start as usize..=(end as usize)].to_vec();
                            let resp = format!(
                                "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{total}\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nContent-Type: audio/mp4\r\n\r\n",
                                body.len()
                            );
                            if socket.write_all(resp.as_bytes()).await.is_err()
                                || socket.write_all(&body).await.is_err()
                            {
                                return;
                            }
                            let _ = socket.flush().await;
                        }
                    });
                }
            })
        })
    }

    fn await_url(ready: &Arc<Mutex<Option<String>>>) -> String {
        for _ in 0..200 {
            if let Some(addr) = ready.lock().unwrap().clone() {
                return format!("http://{addr}/audio.mp4");
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("el servidor fake nunca escuchó");
    }

    /// REGRESIÓN: `StreamReader` debe poder leer y reposicionar el stream
    /// lógico completo usando el MISMO runtime con el que se abrió, incluso
    /// cuando el servidor reutiliza la conexión (keep-alive).
    ///
    /// Con dos runtimes distintos, la segunda lectura se bloquea para siempre.
    #[test]
    fn stream_reader_reuses_the_opening_runtime_across_windows_and_seeks() {
        let ready = Arc::new(Mutex::new(None));
        let data = payload(1024 * 1024);
        let server = keepalive_server(ready.clone(), data.clone());
        let url = await_url(&ready);

        // Un único runtime: abre Y lee (el invariante que hay que proteger).
        let runtime = Arc::new(
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime"),
        );
        let diagnostics = Arc::new(DecoderDiagnostics::new());
        let trace_context =
            crate::media::transport::RangeTraceContext::with_sink(Arc::new(RangeTraceSinkImpl {
                log: Arc::new(PlaybackTraceLog::new()),
            }));
        let transport = runtime
            .block_on(crate::media::transport::HttpRangeStream::open_with_trace(
                reqwest::Client::new(),
                url,
                Vec::new(),
                crate::media::transport::RangePolicy {
                    initial_window: 32 * 1024,
                    window_size: 32 * 1024,
                    max_retries: 0,
                    retry_delay: Duration::from_millis(10),
                    // Suficientemente holgado para que una regresión se
                    // manifest como bloqueo, no como una caída de red local.
                    request_timeout: Duration::from_secs(10),
                },
                trace_context.clone(),
            ))
            .expect("apertura");

        let mut reader = StreamReader::new(
            transport,
            diagnostics.clone(),
            runtime.clone(),
            trace_context,
        );

        // Lectura много-ventana: exige encadenar peticiones sobre la conexión
        // del pool, que es exactamente donde el runtime previo se colgaba.
        let mut got = Vec::new();
        let mut buf = [0u8; 8192];
        std::thread::scope(|scope| {
            scope
                .spawn(|| loop {
                    match std::io::Read::read(&mut reader, &mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => got.extend_from_slice(&buf[..n]),
                    }
                })
                .join()
                .expect("el hilo lector no debe panicear");
        });
        assert_eq!(
            got.len(),
            data.len(),
            "lectura completa sin bloqueo: con dos runtimes esta aserción nunca se alcanza"
        );
        assert!(got == *data, "los bytes son idénticos al original");
        assert!(
            diagnostics.range_requests.load(Ordering::Relaxed) > 8,
            "la lectura hubo que encadenar varias ventanas"
        );
        drop(server);
    }

    /// REGRESIÓN: un `seek` reposiciona el stream lógico y la lectura
    /// posterior devuelve los bytes correctos de la nueva posición.
    #[test]
    fn stream_reader_seek_returns_bytes_from_the_new_offset() {
        let ready = Arc::new(Mutex::new(None));
        let data = payload(512 * 1024);
        let server = keepalive_server(ready.clone(), data.clone());
        let url = await_url(&ready);

        let runtime = Arc::new(
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime"),
        );
        let trace_context =
            crate::media::transport::RangeTraceContext::with_sink(Arc::new(RangeTraceSinkImpl {
                log: Arc::new(PlaybackTraceLog::new()),
            }));
        let transport = runtime
            .block_on(crate::media::transport::HttpRangeStream::open_with_trace(
                reqwest::Client::new(),
                url,
                Vec::new(),
                crate::media::transport::RangePolicy {
                    initial_window: 16 * 1024,
                    window_size: 16 * 1024,
                    max_retries: 0,
                    retry_delay: Duration::from_millis(10),
                    // Suficientemente holgado para que una regresión se
                    // manifest como bloqueo, no como una caída de red local.
                    request_timeout: Duration::from_secs(10),
                },
                trace_context.clone(),
            ))
            .expect("apertura");
        let mut reader = StreamReader::new(
            transport,
            Arc::new(DecoderDiagnostics::new()),
            runtime,
            trace_context,
        );

        let target = 128 * 1024u64 + 7;
        assert_eq!(
            std::io::Seek::seek(&mut reader, SeekFrom::Start(target)).unwrap(),
            target
        );
        let mut buf = [0u8; 64];
        reader.read_exact(&mut buf).expect("lectura tras el seek");
        assert_eq!(buf[..], data[target as usize..target as usize + 64]);
        drop(server);
    }

    /// La traza registra cada petición Range con sus offsets y su origen.
    #[test]
    fn range_trace_records_every_request_with_offsets_and_seek_origin() {
        #[derive(Default)]
        struct Recorder(Mutex<Vec<RangeRequestRecord>>);

        impl crate::media::transport::RangeTraceSink for Recorder {
            fn range_request(&self, record: &RangeRequestRecord) {
                self.0.lock().unwrap().push(record.clone());
            }
        }

        let ready = Arc::new(Mutex::new(None));
        let data = payload(256 * 1024);
        let server = keepalive_server(ready.clone(), data.clone());
        let url = await_url(&ready);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let recorder = Arc::new(Recorder::default());
        let trace_context = crate::media::transport::RangeTraceContext::with_sink(recorder.clone());
        let mut stream = runtime
            .block_on(crate::media::transport::HttpRangeStream::open_with_trace(
                reqwest::Client::new(),
                url,
                Vec::new(),
                crate::media::transport::RangePolicy {
                    initial_window: 16 * 1024,
                    window_size: 16 * 1024,
                    max_retries: 0,
                    retry_delay: Duration::from_millis(10),
                    // Suficientemente holgado para que una regresión se
                    // manifest como bloqueo, no como una caída de red local.
                    request_timeout: Duration::from_secs(10),
                },
                trace_context.clone(),
            ))
            .expect("apertura");

        // La apertura no viene de un seek.
        assert!(
            !recorder.0.lock().unwrap()[0].from_seek,
            "la petición de apertura no es consecuencia de un seek"
        );

        trace_context.set_reader_position(0);
        while runtime
            .block_on(stream.next_chunk(64 * 1024))
            .unwrap()
            .is_some()
        {}

        let before_seek = recorder.0.lock().unwrap().len();
        runtime.block_on(stream.seek_to(200 * 1024)).expect("seek");
        let mut out = Vec::new();
        while let Some(chunk) = runtime.block_on(stream.next_chunk(64 * 1024)).unwrap() {
            out.extend_from_slice(&chunk);
        }
        assert!(!out.is_empty(), "el seek entregó bytes");

        let records = recorder.0.lock().unwrap().clone();
        assert!(records.len() > before_seek, "hubo peticiones tras el seek");
        let seek_request = records
            .iter()
            .find(|r| r.from_seek)
            .expect("al menos una petición marcada como seek");
        assert_eq!(seek_request.start, 200 * 1024);
        assert_eq!(seek_request.requested_bytes, 16 * 1024);
        assert_eq!(seek_request.status, Some(206));
        assert_eq!(seek_request.classification, "ok");
        assert_eq!(seek_request.reader_position, 200 * 1024);
        assert!(seek_request.received_bytes > 0);
        drop(server);
    }
}
