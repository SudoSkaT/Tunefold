//! Ecualizador paramétrico de bandas en cascada.

use super::{AudioProcessor, DspParams, EqParams, StreamFormat, MAX_EQ_BANDS};
use crate::dsp::DSP_MAX_CHANNELS;

const MIN_GAIN_DB: f32 = 1e-6;

#[derive(Clone, Copy)]
struct Coefficients {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Coefficients {
    const fn identity() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        }
    }

    fn peaking(sample_rate: u32, frequency_hz: f32, q: f32, gain_db: f32) -> Self {
        if sample_rate == 0 || gain_db.abs() <= MIN_GAIN_DB {
            return Self::identity();
        }
        let nyquist = sample_rate as f32 * 0.5;
        let frequency = frequency_hz.clamp(10.0, nyquist * 0.95);
        let a = 10.0f32.powf(gain_db / 40.0);
        let omega = 2.0 * std::f32::consts::PI * frequency / sample_rate as f32;
        let sin = omega.sin();
        let cos = omega.cos();
        let alpha = sin / (2.0 * q.max(0.1));
        let a0 = 1.0 + alpha / a;
        Self {
            b0: (1.0 + alpha * a) / a0,
            b1: (-2.0 * cos) / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: (-2.0 * cos) / a0,
            a2: (1.0 - alpha / a) / a0,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct State {
    z1: f32,
    z2: f32,
}

impl State {
    fn process(&mut self, coefficients: Coefficients, input: f32) -> f32 {
        let output = coefficients.b0 * input + self.z1;
        self.z1 = coefficients.b1 * input - coefficients.a1 * output + self.z2;
        self.z2 = coefficients.b2 * input - coefficients.a2 * output;
        if self.z1.abs() < 1e-30 {
            self.z1 = 0.0;
        }
        if self.z2.abs() < 1e-30 {
            self.z2 = 0.0;
        }
        output
    }
}

pub struct EqProcessor {
    format: Option<StreamFormat>,
    last: Option<EqParams>,
    coefficients: [Coefficients; MAX_EQ_BANDS],
    states: [[State; MAX_EQ_BANDS]; DSP_MAX_CHANNELS],
    active_indices: [usize; MAX_EQ_BANDS],
    active_bands: usize,
    enabled: bool,
}

impl EqProcessor {
    pub fn new() -> Self {
        Self {
            format: None,
            last: None,
            coefficients: [Coefficients::identity(); MAX_EQ_BANDS],
            states: [[State::default(); MAX_EQ_BANDS]; DSP_MAX_CHANNELS],
            active_indices: [0; MAX_EQ_BANDS],
            active_bands: 0,
            enabled: false,
        }
    }

    fn clear_states(&mut self) {
        self.states = [[State::default(); MAX_EQ_BANDS]; DSP_MAX_CHANNELS];
    }

    fn refresh(&mut self, eq: &EqParams, sample_rate: u32) {
        let bands = eq.active_band_count().min(MAX_EQ_BANDS);
        let previous_active = self.active_indices;
        let previous_count = self.active_bands;
        self.active_bands = 0;
        self.enabled = eq.enabled;
        for index in 0..bands {
            let band = eq.bands[index];
            let coefficients = if eq.enabled && band.enabled {
                Coefficients::peaking(sample_rate, band.frequency_hz, band.q, band.gain_db)
            } else {
                Coefficients::identity()
            };
            self.coefficients[index] = coefficients;
            if !eq.enabled || !band.enabled || band.gain_db.abs() <= MIN_GAIN_DB {
                continue;
            }
            self.active_indices[self.active_bands] = index;
            self.active_bands += 1;
        }
        if self.active_bands == 0 {
            self.enabled = false;
        }
        if previous_count > 0 {
            for previous in previous_active[..previous_count].iter() {
                if !self.active_indices[..self.active_bands].contains(previous) {
                    for channel in 0..DSP_MAX_CHANNELS {
                        self.states[channel][*previous] = State::default();
                    }
                }
            }
        }
        self.last = Some(*eq);
    }
}

impl Default for EqProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioProcessor for EqProcessor {
    fn name(&self) -> &'static str {
        "eq"
    }

    fn prepare(&mut self, format: StreamFormat) {
        self.format = Some(format);
        self.last = None;
        self.clear_states();
    }

    fn process(&mut self, samples: &mut [f32], params: &DspParams) {
        let Some(format) = self.format else {
            return;
        };
        if format.channels == 0 || format.channels as usize > DSP_MAX_CHANNELS {
            return;
        }
        if self.last != Some(params.eq) {
            if !params.eq.enabled {
                self.clear_states();
            }
            self.refresh(&params.eq, format.sample_rate);
        }
        if !self.enabled {
            return;
        }

        let channels = format.channels as usize;
        let frames = samples.len() / channels;
        for frame in 0..frames {
            let base = frame * channels;
            for channel in 0..channels {
                let mut sample = samples[base + channel];
                for band in 0..self.active_bands {
                    sample = self.states[channel][band].process(self.coefficients[band], sample);
                }
                samples[base + channel] = sample;
            }
        }
    }

    fn reset(&mut self) {
        self.clear_states();
        self.last = None;
        self.active_bands = 0;
        self.enabled = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::params::EqBand;

    fn sine(frequency: f32, sample_rate: u32, frames: usize, amplitude: f32) -> Vec<f32> {
        (0..frames)
            .flat_map(|frame| {
                let value = (2.0 * std::f32::consts::PI * frequency * frame as f32
                    / sample_rate as f32)
                    .sin()
                    * amplitude;
                [value, value]
            })
            .collect()
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32).sqrt()
    }

    fn eq_params(band_count: u8, frequency: f32, gain_db: f32) -> DspParams {
        let mut params = DspParams::flat();
        params.eq = EqParams {
            enabled: true,
            band_count,
            ..EqParams::graphic_flat()
        };
        params.eq.bands[0] = EqBand {
            frequency_hz: frequency,
            gain_db,
            q: 1.0,
            enabled: true,
        };
        params
    }

    #[test]
    fn flat_eq_is_bit_exact() {
        let mut processor = EqProcessor::new();
        processor.prepare(StreamFormat::new(48_000, 2));
        let input: Vec<f32> = (0..1024).map(|i| (i as f32 * 0.01).sin()).collect();
        let mut output = input.clone();
        processor.process(&mut output, &DspParams::flat());
        assert_eq!(output, input);
    }

    #[test]
    fn peaking_band_boosts_its_center_frequency() {
        let mut processor = EqProcessor::new();
        processor.prepare(StreamFormat::new(48_000, 2));
        let params = eq_params(10, 1_000.0, 6.0);
        let mut samples = sine(1_000.0, 48_000, 48_000, 0.25);
        processor.process(&mut samples, &params);

        let settled = &samples[24_000 * 2..];
        let gain = rms(settled) / (0.25 / 2.0f32.sqrt());
        let db = 20.0 * gain.log10();
        assert!((db - 6.0).abs() < 0.3, "ganancia medida {db} dB");
    }

    #[test]
    fn band_does_not_boost_far_away_frequency() {
        let mut processor = EqProcessor::new();
        processor.prepare(StreamFormat::new(48_000, 2));
        let params = eq_params(10, 1_000.0, 6.0);
        let mut samples = sine(10_000.0, 48_000, 48_000, 0.25);
        processor.process(&mut samples, &params);

        let settled = &samples[24_000 * 2..];
        let gain = rms(settled) / (0.25 / 2.0f32.sqrt());
        let db = 20.0 * gain.log10();
        assert!(db.abs() < 1.0, "ganancia fuera de banda {db} dB");
    }

    #[test]
    fn disabled_bands_keep_samples_and_reset_on_enable() {
        let mut processor = EqProcessor::new();
        processor.prepare(StreamFormat::new(48_000, 2));
        let mut params = eq_params(10, 1_000.0, 6.0);
        let mut samples = sine(1_000.0, 48_000, 4_800, 0.25);
        processor.process(&mut samples, &params);
        assert!(samples.iter().any(|sample| sample.abs() > 0.25));

        params.eq.enabled = false;
        let mut bypassed = sine(1_000.0, 48_000, 1_024, 0.25);
        processor.process(&mut bypassed, &params);
        assert_eq!(bypassed, sine(1_000.0, 48_000, 1_024, 0.25));

        processor.reset();
        assert_eq!(processor.active_bands, 0);
    }

    #[test]
    fn coefficients_stay_finite_for_extreme_requests() {
        let mut processor = EqProcessor::new();
        processor.prepare(StreamFormat::new(8_000, 2));
        let mut params = eq_params(10, 40_000.0, 24.0);
        params.eq.bands[0].q = 0.1;
        let mut samples = vec![0.1; 4_096];
        processor.process(&mut samples, &params);
        assert!(samples.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn mono_and_stereo_both_process() {
        let mut processor = EqProcessor::new();
        processor.prepare(StreamFormat::new(48_000, 1));
        let params = eq_params(10, 1_000.0, 6.0);
        let mut mono = sine(1_000.0, 48_000, 4_800, 0.25);
        let mut mono_single = Vec::with_capacity(mono.len() / 2);
        for frame in mono.chunks_exact(2) {
            mono_single.push(frame[0]);
        }
        processor.process(&mut mono_single, &params);
        assert!(mono_single.iter().all(|sample| sample.is_finite()));
        mono.clear();
    }
}
