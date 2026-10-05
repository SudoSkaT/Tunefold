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
use std::sync::Arc;
use std::time::Duration;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::analysis::{PcmTap, StreamMeta};
use crate::platform::android_playback::PcmRing;

/// Low-rate counters and the current decoder stage surfaced to the Android
/// test host. Updates happen on the decoder thread, never on the audio output.
pub struct DecoderDiagnostics {
    pub(crate) stage: std::sync::Mutex<String>,
    pub(crate) decoded_packets: AtomicU64,
    pub(crate) decoded_frames: AtomicU64,
    pub(crate) ring_frames: AtomicU64,
    pub(crate) analysis_frames: AtomicU64,
}

impl DecoderDiagnostics {
    pub fn new() -> Self {
        Self {
            stage: std::sync::Mutex::new("engine initialized".to_string()),
            decoded_packets: AtomicU64::new(0),
            decoded_frames: AtomicU64::new(0),
            ring_frames: AtomicU64::new(0),
            analysis_frames: AtomicU64::new(0),
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
}

impl AndroidDecoder {
    pub fn new(
        ring: Arc<PcmRing>,
        cancel: Arc<AtomicBool>,
        sample_rate: Arc<AtomicU64>,
        position_ms: Arc<std::sync::Mutex<u64>>,
        analysis_tap: PcmTap,
        playing: Arc<AtomicBool>,
        diagnostics: Arc<DecoderDiagnostics>,
    ) -> Self {
        Self {
            ring,
            cancel,
            sample_rate,
            position_ms,
            analysis_tap,
            playing,
            diagnostics,
        }
    }

    /// Decode an HTTP stream and feed PCM to the ring buffer.
    /// This runs on a dedicated decoder thread (not the audio callback).
    pub fn decode_stream(
        &self,
        url: String,
        headers: Vec<(String, String)>,
    ) -> Result<(), DecoderError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| DecoderError::Stream(e.to_string()))?;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|error| DecoderError::Stream(error.to_string()))?;
        let transport = runtime
            .block_on(crate::media::transport::HttpRangeStream::open(
                client,
                url,
                headers,
                crate::media::transport::RangePolicy::default(),
            ))
            .map_err(|e| DecoderError::Stream(e.to_string()))?;

        let content_length = transport.total();
        self.set_diagnostic(format!("HTTP stream opened · {content_length} bytes"));
        if content_length == 0 {
            return Err(DecoderError::Stream("empty stream".to_string()));
        }

        let reader = StreamReader::new(transport);
        let mss = MediaSourceStream::new(
            Box::new(HttpMediaSource::new(Box::new(reader), content_length)),
            MediaSourceStreamOptions::default(),
        );

        let mut hint = Hint::new();
        hint.with_extension("mp4");

        let format_opts = FormatOptions::default();
        let metadata_opts = MetadataOptions::default();
        let decoder_opts = DecoderOptions::default();

        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &format_opts, &metadata_opts)
            .map_err(|e| DecoderError::Format(e.to_string()))?;

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

            match decoder.decode(&packet) {
                Ok(decoded) => {
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
                        if !self.feed_pcm(samples, channels, &mut stereo) {
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

    fn feed_pcm(&self, samples: &[f32], channels: u16, stereo: &mut Vec<f32>) -> bool {
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
struct StreamReader {
    transport: crate::media::transport::HttpRangeStream,
    position: u64,
    buffer: Vec<u8>,
    buffer_pos: usize,
    eof: bool,
    runtime: tokio::runtime::Runtime,
}

impl StreamReader {
    fn new(transport: crate::media::transport::HttpRangeStream) -> Self {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        Self {
            transport,
            position: 0,
            buffer: Vec::new(),
            buffer_pos: 0,
            eof: false,
            runtime,
        }
    }

    fn fill_buffer(&mut self) -> std::io::Result<bool> {
        if self.eof {
            return Ok(false);
        }
        let result = self
            .runtime
            .block_on(async { self.transport.next_chunk(256 * 1024).await });
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
        self.position = new_pos;
        self.buffer.clear();
        self.buffer_pos = 0;
        self.eof = false;
        Ok(new_pos)
    }
}
