//! Limitador de picos con lookahead fijo.

use super::smooth::db_to_linear;
use super::{AudioProcessor, DspParams, StreamFormat};
use crate::dsp::DSP_MAX_CHANNELS;

const LOOKAHEAD_MS: f32 = 5.0;
const MAX_LOOKAHEAD_FRAMES: usize = 960;
const DELAY_CAPACITY: usize = 1_024;
const DELAY_MASK: usize = DELAY_CAPACITY - 1;
const DEQUE_CAPACITY: usize = 1_024;
const DEQUE_MASK: usize = DEQUE_CAPACITY - 1;

pub struct LimiterProcessor {
    format: Option<StreamFormat>,
    lookahead: usize,
    delay: Vec<f32>,
    write: usize,
    peaks: Vec<f32>,
    indices: Vec<u64>,
    head: u64,
    tail: u64,
    frame_index: u64,
    gain: f32,
    was_enabled: bool,
}

impl LimiterProcessor {
    pub fn new() -> Self {
        Self {
            format: None,
            lookahead: 0,
            delay: vec![0.0; DELAY_CAPACITY * DSP_MAX_CHANNELS],
            write: 0,
            peaks: vec![0.0; DEQUE_CAPACITY],
            indices: vec![0; DEQUE_CAPACITY],
            head: 0,
            tail: 0,
            frame_index: 0,
            gain: 1.0,
            was_enabled: false,
        }
    }

    fn clear(&mut self) {
        self.delay.fill(0.0);
        self.write = 0;
        self.peaks.fill(0.0);
        self.indices.fill(0);
        self.head = 0;
        self.tail = 0;
        self.frame_index = 0;
        self.gain = 1.0;
        self.was_enabled = false;
    }

    fn push_peak(&mut self, peak: f32) {
        while self.tail > self.head {
            let last = (self.tail - 1) & DEQUE_MASK as u64;
            if self.peaks[last as usize] > peak {
                break;
            }
            self.tail -= 1;
        }
        let index = self.tail & DEQUE_MASK as u64;
        self.peaks[index as usize] = peak;
        self.indices[index as usize] = self.frame_index;
        self.tail += 1;
        while self.head < self.tail {
            let oldest = self.indices[(self.head & DEQUE_MASK as u64) as usize];
            if oldest + self.lookahead as u64 >= self.frame_index {
                break;
            }
            self.head += 1;
        }
    }

    fn window_peak(&self) -> f32 {
        if self.head < self.tail {
            self.peaks[(self.head & DEQUE_MASK as u64) as usize]
        } else {
            0.0
        }
    }
}

impl Default for LimiterProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioProcessor for LimiterProcessor {
    fn name(&self) -> &'static str {
        "limiter"
    }

    fn prepare(&mut self, format: StreamFormat) {
        self.format = Some(format);
        self.lookahead = ((format.sample_rate as f32 * LOOKAHEAD_MS / 1_000.0).round() as usize)
            .clamp(1, MAX_LOOKAHEAD_FRAMES);
        self.clear();
    }

    fn process(&mut self, samples: &mut [f32], params: &DspParams) {
        let Some(format) = self.format else {
            return;
        };
        if !params.limiter.enabled {
            if self.was_enabled {
                self.clear();
            }
            return;
        }
        if !self.was_enabled {
            self.clear();
            self.was_enabled = true;
        }
        let channels = format.channels as usize;
        if channels == 0 || channels > DSP_MAX_CHANNELS || self.lookahead == 0 {
            return;
        }

        let ceiling = db_to_linear(params.limiter.ceiling_db).clamp(1e-4, 1.0);
        let attack_frames = ((params.limiter.attack_ms * format.sample_rate as f32 / 1_000.0)
            .round() as usize)
            .clamp(1, self.lookahead);
        let release_frames = ((params.limiter.release_ms * format.sample_rate as f32 / 1_000.0)
            .round() as usize)
            .max(1);
        let attack_coeff = (-3.0 / attack_frames as f32).exp();
        let release_coeff = (-1.0 / release_frames as f32).exp();

        let frames = samples.len() / channels;
        for frame in 0..frames {
            let base = frame * channels;
            let mut peak = 0.0f32;
            for channel in 0..channels {
                peak = peak.max(samples[base + channel].abs());
            }
            if !peak.is_finite() {
                peak = 0.0;
            }
            self.push_peak(peak);
            let window_peak = self.window_peak();
            let target = if window_peak > ceiling {
                (ceiling / window_peak).min(1.0)
            } else {
                1.0
            };
            if target < self.gain {
                self.gain = target + (self.gain - target) * attack_coeff;
            } else {
                self.gain = target + (self.gain - target) * release_coeff;
            }
            self.gain = self.gain.clamp(0.0, 1.0);

            let read = (self.write + DELAY_CAPACITY - self.lookahead) & DELAY_MASK;
            for channel in 0..channels {
                let input = samples[base + channel];
                let delayed = self.delay[read * DSP_MAX_CHANNELS + channel];
                let mut output = (delayed * self.gain).clamp(-ceiling, ceiling);
                if !output.is_finite() {
                    output = 0.0;
                }
                samples[base + channel] = output;
                self.delay[self.write * DSP_MAX_CHANNELS + channel] = input;
            }
            self.write = (self.write + 1) & DELAY_MASK;
            self.frame_index += 1;
        }
    }

    fn reset(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(ceiling_db: f32) -> DspParams {
        let mut params = DspParams::flat();
        params.limiter.enabled = true;
        params.limiter.ceiling_db = ceiling_db;
        params.limiter.attack_ms = 5.0;
        params.limiter.release_ms = 100.0;
        params
    }

    #[test]
    fn disabled_limiter_is_bit_exact() {
        let mut processor = LimiterProcessor::new();
        processor.prepare(StreamFormat::new(48_000, 2));
        let input = [0.25f32, -0.5, 0.75, -0.125];
        let mut output = input;
        processor.process(&mut output, &DspParams::flat());
        assert_eq!(output, input);
    }

    #[test]
    fn output_never_exceeds_the_ceiling() {
        let mut processor = LimiterProcessor::new();
        processor.prepare(StreamFormat::new(48_000, 2));
        let params = params(-6.0);
        let ceiling = db_to_linear(-6.0);
        let mut samples: Vec<f32> = (0..48_000)
            .flat_map(|frame| {
                let value =
                    (2.0 * std::f32::consts::PI * 1_000.0 * frame as f32 / 48_000.0).sin() * 0.99;
                [value, value]
            })
            .collect();
        processor.process(&mut samples, &params);
        assert!(samples.iter().all(|sample| sample.abs() <= ceiling + 1e-4));
    }

    #[test]
    fn quiet_material_is_not_attenuated() {
        let mut processor = LimiterProcessor::new();
        processor.prepare(StreamFormat::new(48_000, 1));
        let params = params(-1.0);
        let mut samples = vec![0.1f32; 8_192];
        processor.process(&mut samples, &params);
        assert!(samples[256..]
            .iter()
            .all(|sample| (*sample - 0.1).abs() < 1e-5));
    }

    #[test]
    fn reset_clears_the_lookahead_tail() {
        let mut processor = LimiterProcessor::new();
        processor.prepare(StreamFormat::new(48_000, 1));
        let mut samples = vec![0.9f32; 1_024];
        processor.process(&mut samples, &params(-6.0));
        processor.reset();
        let mut after = vec![0.0f32; 1_024];
        processor.process(&mut after, &params(-6.0));
        assert!(after[..256].iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn reenabling_restarts_with_clean_state() {
        let mut processor = LimiterProcessor::new();
        processor.prepare(StreamFormat::new(48_000, 1));
        let mut samples = vec![0.9f32; 1_024];
        processor.process(&mut samples, &params(-6.0));
        processor.process(&mut samples, &DspParams::flat());
        processor.process(&mut samples, &params(-6.0));
        assert!(samples.iter().all(|sample| sample.is_finite()));
    }
}
