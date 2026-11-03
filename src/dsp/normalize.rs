//! Aplicación de la ganancia de normalización (ReplayGain/LUFS).

use super::smooth::{db_to_linear, GainSmoother};
use super::{AudioProcessor, DspParams, StreamFormat};

const NORMALIZATION_SMOOTH_MS: f32 = 30.0;

pub struct NormalizationProcessor {
    gain: GainSmoother,
}

impl NormalizationProcessor {
    pub fn new() -> Self {
        Self {
            gain: GainSmoother::new(),
        }
    }
}

impl Default for NormalizationProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioProcessor for NormalizationProcessor {
    fn name(&self) -> &'static str {
        "normalization"
    }

    fn prepare(&mut self, format: StreamFormat) {
        self.gain
            .prepare(format.sample_rate, NORMALIZATION_SMOOTH_MS);
    }

    fn process(&mut self, samples: &mut [f32], params: &DspParams) {
        self.gain
            .set_target(db_to_linear(params.normalization.effective_gain_db()));
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
    use crate::dsp::NormalizationMode;

    fn params(mode: NormalizationMode) -> DspParams {
        let mut params = DspParams::flat();
        params.normalization.mode = mode;
        params
    }

    #[test]
    fn off_mode_is_bit_exact() {
        let mut processor = NormalizationProcessor::new();
        processor.prepare(StreamFormat::new(48_000, 2));
        let input = [0.25f32, -0.5, 0.75, -0.125];
        let mut output = input;
        processor.process(&mut output, &params(NormalizationMode::Off));
        assert_eq!(output, input);
    }

    #[test]
    fn replaygain_track_applies_its_gain() {
        let mut processor = NormalizationProcessor::new();
        processor.prepare(StreamFormat::new(48_000, 1));
        let mut params = params(NormalizationMode::ReplayGainTrack);
        params.normalization.gain_db = 6.020_6;
        let mut samples = [0.25f32; 48_000];
        processor.process(&mut samples, &params);
        assert!((samples[47_999] - 0.5).abs() < 1e-4);
    }

    #[test]
    fn lufs_mode_uses_target_minus_source() {
        let mut processor = NormalizationProcessor::new();
        processor.prepare(StreamFormat::new(48_000, 1));
        let mut params = params(NormalizationMode::Lufs);
        params.normalization.target_lufs = -14.0;
        params.normalization.source_lufs = -20.0;
        let mut samples = [0.25f32; 48_000];
        processor.process(&mut samples, &params);
        let expected = 0.25 * 10.0f32.powf(6.0 / 20.0);
        assert!((samples[47_999] - expected).abs() < 1e-4);
    }

    #[test]
    fn clipping_protection_limits_positive_gain() {
        let mut processor = NormalizationProcessor::new();
        processor.prepare(StreamFormat::new(48_000, 1));
        let mut params = params(NormalizationMode::ReplayGainTrack);
        params.normalization.gain_db = 12.0;
        params.normalization.source_peak = 0.9;
        params.normalization.prevent_clipping = true;
        let mut samples = [0.5f32; 48_000];
        processor.process(&mut samples, &params);
        let peak_gain = 0.999 / 0.9;
        let expected = 0.5 * peak_gain;
        assert!((samples[47_999] - expected).abs() < 1e-4);
    }

    #[test]
    fn negative_gain_is_allowed() {
        let mut processor = NormalizationProcessor::new();
        processor.prepare(StreamFormat::new(48_000, 1));
        let mut params = params(NormalizationMode::ReplayGainAlbum);
        params.normalization.gain_db = -6.020_6;
        let mut samples = [0.5f32; 48_000];
        processor.process(&mut samples, &params);
        assert!((samples[47_999] - 0.25).abs() < 1e-4);
    }
}
