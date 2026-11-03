//! Preamplificación global con transiciones suaves.

use super::smooth::{db_to_linear, GainSmoother};
use super::{AudioProcessor, DspParams, StreamFormat};

const PREAMP_SMOOTH_MS: f32 = 8.0;

pub struct GainProcessor {
    gain: GainSmoother,
}

impl GainProcessor {
    pub fn new() -> Self {
        Self {
            gain: GainSmoother::new(),
        }
    }
}

impl Default for GainProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioProcessor for GainProcessor {
    fn name(&self) -> &'static str {
        "preamp"
    }

    fn prepare(&mut self, format: StreamFormat) {
        self.gain.prepare(format.sample_rate, PREAMP_SMOOTH_MS);
    }

    fn process(&mut self, samples: &mut [f32], params: &DspParams) {
        self.gain.set_target(db_to_linear(params.preamp_db));
        for sample in samples {
            *sample *= self.gain.next();
        }
    }

    fn reset(&mut self) {
        self.gain.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_db_is_bit_exact() {
        let mut processor = GainProcessor::new();
        processor.prepare(StreamFormat::new(48_000, 2));
        let input = [0.25f32, -0.5, 1.0, -1.0];
        let mut output = input;
        processor.process(&mut output, &DspParams::flat());
        assert_eq!(output, input);
    }

    #[test]
    fn positive_and_negative_gain_apply() {
        let mut processor = GainProcessor::new();
        processor.prepare(StreamFormat::new(48_000, 1));

        let mut params = DspParams::flat();
        params.preamp_db = 6.020_6;
        let mut boosted = [0.25f32; 48_000];
        processor.process(&mut boosted, &params);
        assert!((boosted[47_999] - 0.5).abs() < 1e-4);

        params.preamp_db = -6.020_6;
        let mut cut = [0.5f32; 48_000];
        processor.process(&mut cut, &params);
        assert!((cut[47_999] - 0.25).abs() < 1e-4);
    }

    #[test]
    fn gain_changes_are_smoothed() {
        let mut processor = GainProcessor::new();
        processor.prepare(StreamFormat::new(48_000, 1));
        processor.process(&mut [0.0f32; 32], &DspParams::flat());

        let mut params = DspParams::flat();
        params.preamp_db = 12.0;
        let mut first = [0.25f32; 1];
        processor.process(&mut first, &params);
        assert!(first[0] < 0.5, "no debe saltar a x4 en una muestra");

        let mut settled = [0.25f32; 48_000];
        processor.process(&mut settled, &params);
        let expected = 0.25 * 10.0f32.powf(12.0 / 20.0);
        assert!((settled[47_999] - expected).abs() < 1e-3);
    }
}
