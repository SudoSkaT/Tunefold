//! Balance, anchura estéreo y crossfeed con estado continuo.

use super::smooth::GainSmoother;
use super::{AudioProcessor, DspParams, StreamFormat};

const SMOOTH_MS: f32 = 10.0;
const CROSSFEED_DELAY_CAP: usize = 64;
const CROSSFEED_DELAY_MASK: usize = CROSSFEED_DELAY_CAP - 1;
const CROSSFEED_DELAY_SECONDS: f32 = 0.000_3;
const CROSSFEED_CUTOFF_HZ: f32 = 700.0;

pub struct StereoProcessor {
    format: Option<StreamFormat>,
    balance_left: GainSmoother,
    balance_right: GainSmoother,
    width: GainSmoother,
    crossfeed: GainSmoother,
    delay_left: [f32; CROSSFEED_DELAY_CAP],
    delay_right: [f32; CROSSFEED_DELAY_CAP],
    write: usize,
    delay_frames: usize,
    lowpass_left: f32,
    lowpass_right: f32,
    lowpass_coeff: f32,
}

impl StereoProcessor {
    pub fn new() -> Self {
        Self {
            format: None,
            balance_left: GainSmoother::new(),
            balance_right: GainSmoother::new(),
            width: GainSmoother::new(),
            crossfeed: GainSmoother::new(),
            delay_left: [0.0; CROSSFEED_DELAY_CAP],
            delay_right: [0.0; CROSSFEED_DELAY_CAP],
            write: 0,
            delay_frames: 1,
            lowpass_left: 0.0,
            lowpass_right: 0.0,
            lowpass_coeff: 0.0,
        }
    }

    fn prepare_smoothers(&mut self, sample_rate: u32) {
        self.balance_left.prepare(sample_rate, SMOOTH_MS);
        self.balance_right.prepare(sample_rate, SMOOTH_MS);
        self.width.prepare(sample_rate, SMOOTH_MS);
        self.crossfeed.prepare(sample_rate, SMOOTH_MS);
    }
}

impl Default for StereoProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioProcessor for StereoProcessor {
    fn name(&self) -> &'static str {
        "stereo"
    }

    fn prepare(&mut self, format: StreamFormat) {
        self.format = Some(format);
        self.prepare_smoothers(format.sample_rate);
        self.delay_frames = ((format.sample_rate as f32 * CROSSFEED_DELAY_SECONDS).round()
            as usize)
            .clamp(1, CROSSFEED_DELAY_MASK);
        self.lowpass_coeff = 1.0
            - (-2.0 * std::f32::consts::PI * CROSSFEED_CUTOFF_HZ
                / format.sample_rate.max(1) as f32)
                .exp();
        self.delay_left = [0.0; CROSSFEED_DELAY_CAP];
        self.delay_right = [0.0; CROSSFEED_DELAY_CAP];
        self.write = 0;
        self.lowpass_left = 0.0;
        self.lowpass_right = 0.0;
    }

    fn process(&mut self, samples: &mut [f32], params: &DspParams) {
        let Some(format) = self.format else {
            return;
        };
        if format.channels != 2 {
            return;
        }
        if params.stereo.is_flat() {
            self.reset();
            return;
        }
        let stereo = &params.stereo;
        let left_balance = if stereo.balance <= 0.0 {
            1.0
        } else {
            1.0 - stereo.balance
        };
        let right_balance = if stereo.balance >= 0.0 {
            1.0
        } else {
            1.0 + stereo.balance
        };
        self.balance_left.set_target(left_balance);
        self.balance_right.set_target(right_balance);
        self.width.set_target(stereo.width);
        self.crossfeed.set_target(stereo.crossfeed);

        for frame in samples.chunks_exact_mut(2) {
            let left = frame[0];
            let right = frame[1];

            let read =
                (self.write + CROSSFEED_DELAY_CAP - self.delay_frames) & CROSSFEED_DELAY_MASK;
            let delayed_left = self.delay_left[read];
            let delayed_right = self.delay_right[read];
            self.delay_left[self.write] = left;
            self.delay_right[self.write] = right;
            self.write = (self.write + 1) & CROSSFEED_DELAY_MASK;

            self.lowpass_left += self.lowpass_coeff * (delayed_left - self.lowpass_left);
            self.lowpass_right += self.lowpass_coeff * (delayed_right - self.lowpass_right);

            let mid = (left + right) * 0.5;
            let side = (left - right) * 0.5;
            let width = self.width.next();
            let wide_left = mid + side * width;
            let wide_right = mid - side * width;

            let balanced_left = wide_left * self.balance_left.next();
            let balanced_right = wide_right * self.balance_right.next();

            let crossfeed = self.crossfeed.next().clamp(0.0, 1.0);
            frame[0] = balanced_left * (1.0 - crossfeed) + self.lowpass_right * crossfeed;
            frame[1] = balanced_right * (1.0 - crossfeed) + self.lowpass_left * crossfeed;
        }
    }

    fn reset(&mut self) {
        self.balance_left.reset();
        self.balance_right.reset();
        self.width.reset();
        self.crossfeed.reset();
        self.delay_left = [0.0; CROSSFEED_DELAY_CAP];
        self.delay_right = [0.0; CROSSFEED_DELAY_CAP];
        self.write = 0;
        self.lowpass_left = 0.0;
        self.lowpass_right = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(balance: f32, width: f32, crossfeed: f32) -> DspParams {
        let mut params = DspParams::flat();
        params.stereo.balance = balance;
        params.stereo.width = width;
        params.stereo.crossfeed = crossfeed;
        params
    }

    #[test]
    fn neutral_stereo_is_bit_exact() {
        let mut processor = StereoProcessor::new();
        processor.prepare(StreamFormat::new(48_000, 2));
        let input = [0.25f32, -0.5, 0.75, 0.125];
        let mut output = input;
        processor.process(&mut output, &DspParams::flat());
        assert_eq!(output, input);
    }

    #[test]
    fn zero_width_collapses_to_mono() {
        let mut processor = StereoProcessor::new();
        processor.prepare(StreamFormat::new(48_000, 2));
        let mut samples = vec![0.5f32, -0.5, 0.25, 0.75];
        processor.process(&mut samples, &params(0.0, 0.0, 0.0));
        for frame in samples.chunks_exact(2) {
            assert!((frame[0] - frame[1]).abs() < 1e-6);
        }
    }

    #[test]
    fn double_width_keeps_mid_and_expands_side() {
        let mut processor = StereoProcessor::new();
        processor.prepare(StreamFormat::new(48_000, 2));
        let mut samples = vec![0.75f32, 0.25];
        processor.process(&mut samples, &params(0.0, 2.0, 0.0));
        assert!((samples[0] - 1.0).abs() < 1e-6);
        assert!((samples[1] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn full_left_balance_mutes_right() {
        let mut processor = StereoProcessor::new();
        processor.prepare(StreamFormat::new(48_000, 2));
        let mut samples = vec![0.5f32, 0.5];
        processor.process(&mut samples, &params(-1.0, 1.0, 0.0));
        assert!((samples[0] - 0.5).abs() < 1e-6);
        assert!(samples[1].abs() < 1e-6);
    }

    #[test]
    fn crossfeed_uses_the_opposite_channel() {
        let mut processor = StereoProcessor::new();
        processor.prepare(StreamFormat::new(48_000, 2));
        let mut samples = vec![0.0f32; 4_800];
        for frame in samples.chunks_exact_mut(2) {
            frame[0] = 0.5;
        }
        processor.process(&mut samples, &params(0.0, 1.0, 1.0));
        let last = &samples[samples.len() - 2..];
        assert!(
            last[1] > 0.01,
            "la señal izquierda debe llegar por crossfeed"
        );
    }

    #[test]
    fn mono_is_left_untouched() {
        let mut processor = StereoProcessor::new();
        processor.prepare(StreamFormat::new(48_000, 1));
        let input = [0.25f32, -0.5];
        let mut output = input;
        processor.process(&mut output, &params(1.0, 0.0, 1.0));
        assert_eq!(output, input);
    }
}
