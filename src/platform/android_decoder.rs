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

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::platform::android_playback::PcmRing;

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
}

impl AndroidDecoder {
    pub fn new(
        ring: Arc<PcmRing>,
        cancel: Arc<AtomicBool>,
        sample_rate: Arc<AtomicU64>,
        position_ms: Arc<std::sync::Mutex<u64>>,
    ) -> Self {
        Self {
            ring,
            cancel,
            sample_rate,
            position_ms,
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

        let mut transport = runtime.block_on(async {
            crate::media::transport::HttpRangeStream::open(
                reqwest::Client::new(),
                url,
                headers,
                crate::media::transport::RangePolicy::default(),
            )
            .await
        })
        .map_err(|e| DecoderError::Stream(e.to_string()))?;

        let content_length = transport.total();
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

        let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
        self.sample_rate.store(sample_rate as u64, Ordering::Relaxed);

        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &decoder_opts)
            .map_err(|e| DecoderError::UnsupportedCodec(e.to_string()))?;

        let track_id = track.id;
        let mut sample_buf: Option<SampleBuffer<f32>> = None;

        loop {
            if self.cancel.load(Ordering::Relaxed) {
                return Ok(());
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
                    if sample_buf.is_none() {
                        let spec = *decoded.spec();
                        let duration = decoded.capacity() as u64;
                        sample_buf = Some(SampleBuffer::new(duration, spec));
                    }

                    if let Some(ref mut buf) = sample_buf {
                        buf.copy_interleaved_ref(decoded);
                        let samples = buf.samples();
                        self.feed_pcm(samples);
                    }
                }
                Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
                Err(e) => return Err(DecoderError::Decode(e.to_string())),
            }
        }

        Ok(())
    }

    fn feed_pcm(&self, samples: &[f32]) {
        self.ring.push(samples);

        let sample_rate = self.sample_rate.load(Ordering::Relaxed);
        if sample_rate > 0 {
            let frames = samples.len() / 2;
            let delta = (frames as u64 * 1000) / sample_rate;
            if let Ok(mut pos) = self.position_ms.lock() {
                *pos += delta;
            }
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
        let result = self.runtime.block_on(async {
            self.transport.next_chunk(256 * 1024).await
        });
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
            Err(e) => Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())),
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
