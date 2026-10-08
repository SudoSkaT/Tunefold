//! Shared Android PCM ring consumed by Android audio output.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// PCM sample format used throughout the pipeline.
pub type PcmSample = f32;

/// Number of channels (stereo).
pub const CHANNELS: usize = 2;

/// Lock-free SPSC ring buffer for PCM samples.
/// Producer: decoder thread. Consumer: Android audio output thread.
pub struct PcmRing {
    buffer: UnsafeCell<Box<[PcmSample]>>,
    mask: usize,
    head: AtomicU64,
    tail: AtomicU64,
}

// SAFETY: exactly one producer (the decoder thread) and exactly one consumer (the
// Android audio output thread) touch `buffer`, never concurrently with each other,
// and every index is masked to the allocation. The single-producer invariant is the
// caller's responsibility; see `PcmRing::clear` for where it is easiest to break.
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
    /// Drop-newest policy: if full, incoming samples are discarded.
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

        self.head
            .store(head.wrapping_add(frames_to_write as u64), Ordering::Release);
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

        self.tail
            .store(tail.wrapping_add(frames_to_read as u64), Ordering::Release);
        frames_to_read
    }

    pub fn available_frames(&self) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Relaxed);
        head.wrapping_sub(tail) as usize
    }

    /// Free frames visible to the single producer. The Android decoder waits
    /// for space instead of dropping audio when output falls behind.
    pub fn free_frames(&self) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        (self.mask + 1) - head.wrapping_sub(tail) as usize
    }

    /// Resets the ring to empty.
    ///
    /// The two counters are not reset atomically with respect to each other, so this
    /// is only correct while the producer is **quiescent**. Calling it from a third
    /// thread while the decoder may still be inside `push` lets that producer overwrite
    /// samples the consumer has not read yet, and lets the consumer read samples that
    /// were never written — an audible burst of stale or silent PCM on every stop and
    /// track change. Memory-safe either way, because every index is masked; it is
    /// wrong, not dangerous.
    ///
    /// Callers must join or stop the decoder thread first.
    pub fn clear(&self) {
        self.head.store(0, Ordering::Release);
        self.tail.store(0, Ordering::Release);
    }
}
