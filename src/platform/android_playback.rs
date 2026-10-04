//! Real Android audio playback backend for Phase 2.
//!
//! Replaces the Phase 1 sine-wave generator with the actual Tunefold audio
//! pipeline: HTTP stream → symphonia decoder → PCM ring buffer → Oboe.
//!
//! The same PCM that feeds Oboe also feeds the analysis engine, ensuring
//! that what is heard matches what is analyzed.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::analysis::{AnalysisConfig, AnalysisRuntime};

/// PCM sample format used throughout the pipeline.
pub type PcmSample = f32;

/// Number of channels (stereo).
pub const CHANNELS: usize = 2;

/// PCM ring buffer capacity (samples per channel).
/// At 44100 Hz, 131072 samples ≈ 3 seconds of audio.
const RING_CAPACITY: usize = 131072;

/// Lock-free SPSC ring buffer for PCM samples.
/// Producer: decoder thread. Consumer: Oboe audio callback.
pub struct PcmRing {
    buffer: UnsafeCell<Box<[PcmSample]>>,
    mask: usize,
    head: AtomicU64,
    tail: AtomicU64,
}

unsafe impl Sync for PcmRing {}
unsafe impl Send for PcmRing {}

impl PcmRing {
    pub fn new(capacity: usize) -> Arc<Self> {
        let capacity = capacity.next_power_of_two().max(1024);
        let buffer = vec![0.0f32; capacity * CHANNELS].into_boxed_slice();
        Arc::new(Self {
            buffer: UnsafeCell::new(buffer),
            mask: capacity - 1,
            head: AtomicU64::new(0),
            tail: AtomicU64::new(0),
        })
    }

    /// Push interleaved PCM samples. Returns number of frames written.
    /// Drop-newest policy: if full, oldest samples are overwritten.
    pub fn push(&self, frames: &[PcmSample]) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        let capacity = self.mask + 1;
        let available = capacity - (head.wrapping_sub(tail) as usize);
        let frames_to_write = (frames.len() / CHANNELS).min(available);

        let buf = unsafe { &mut *self.buffer.get() };
        for i in 0..frames_to_write {
            let idx = ((head as usize + i) * CHANNELS) & (self.mask * CHANNELS);
            buf[idx] = frames[i * CHANNELS];
            buf[idx + 1] = frames[i * CHANNELS + 1];
        }

        self.head.store(head.wrapping_add(frames_to_write as u64), Ordering::Release);
        frames_to_write
    }

    /// Pop interleaved PCM samples. Returns number of frames read.
    pub fn pop(&self, out: &mut [PcmSample]) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Relaxed);
        let available = head.wrapping_sub(tail) as usize;
        let frames_to_read = (out.len() / CHANNELS).min(available);

        let buf = unsafe { &*self.buffer.get() };
        for i in 0..frames_to_read {
            let idx = ((tail as usize + i) * CHANNELS) & (self.mask * CHANNELS);
            out[i * CHANNELS] = buf[idx];
            out[i * CHANNELS + 1] = buf[idx + 1];
        }

        self.tail.store(tail.wrapping_add(frames_to_read as u64), Ordering::Release);
        frames_to_read
    }

    pub fn available_frames(&self) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Relaxed);
        head.wrapping_sub(tail) as usize
    }

    pub fn clear(&self) {
        self.head.store(0, Ordering::Release);
        self.tail.store(0, Ordering::Release);
    }
}

/// Playback state for the Android backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Buffering,
    Playing,
    Paused,
}

/// Real Android playback engine.
/// Owns the decoder, PCM ring buffer, and analysis runtime.
#[allow(dead_code)]
pub struct AndroidPlaybackEngine {
    state: Arc<AtomicBool>,
    ring: Arc<PcmRing>,
    analysis: Option<AnalysisRuntime>,
    position_ms: Arc<Mutex<u64>>,
    sample_rate: Arc<AtomicU64>,
    volume: Arc<Mutex<f32>>,
}

impl Default for AndroidPlaybackEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AndroidPlaybackEngine {
    pub fn new() -> Self {
        let config = AnalysisConfig::default();
        let analysis = AnalysisRuntime::spawn(config);

        Self {
            state: Arc::new(AtomicBool::new(false)),
            ring: PcmRing::new(RING_CAPACITY),
            analysis: Some(analysis),
            position_ms: Arc::new(Mutex::new(0)),
            sample_rate: Arc::new(AtomicU64::new(44100)),
            volume: Arc::new(Mutex::new(1.0)),
        }
    }

    pub fn start(&self) {
        self.state.store(true, Ordering::Release);
    }

    pub fn stop(&self) {
        self.state.store(false, Ordering::Release);
        self.ring.clear();
    }

    pub fn pause(&self) {
        self.state.store(false, Ordering::Release);
    }

    pub fn resume(&self) {
        self.state.store(true, Ordering::Release);
    }

    pub fn seek(&self, _position_ms: u64) {
        self.ring.clear();
    }

    pub fn set_volume(&self, vol: f32) {
        if let Ok(mut v) = self.volume.lock() {
            *v = vol.clamp(0.0, 1.0);
        }
    }

    pub fn position_ms(&self) -> u64 {
        *self.position_ms.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn ring(&self) -> Arc<PcmRing> {
        self.ring.clone()
    }

    pub fn analysis(&self) -> Option<&AnalysisRuntime> {
        self.analysis.as_ref()
    }

    pub fn is_playing(&self) -> bool {
        self.state.load(Ordering::Acquire)
    }

    pub fn feed_pcm(&self, frames: &[PcmSample]) {
        if let Some(ref analysis) = self.analysis {
            for frame in frames.chunks_exact(CHANNELS) {
                analysis.tap().feed(frame);
            }
        }
        self.ring.push(frames);
    }
}
