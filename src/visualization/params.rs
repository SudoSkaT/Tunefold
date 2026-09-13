//! ParameterMapper: de [`AudioFeatures`] a parámetros visuales con curvas
//! configurables (spec §23/§24).
//!
//! Nada aquí toca el renderer ni conoce píxeles: produce valores semánticos
//! (nivel, intensidad, turbulencia, pulso, envolvente de barras, tasa de
//! fase) que el motor visual compone con la posición de reproducción.

use crate::analysis::AudioFeatures;
use crate::visualization::VISUAL_BARS;

/// Configuración del mapeo (editable en el futuro: presets, UI).
#[derive(Debug, Clone, Copy)]
pub struct MapperConfig {
    /// Multiplicador global de reactividad.
    pub sensitivity: f32,
    /// Puerta de ruido: por debajo de este nivel todo vale 0.
    pub noise_floor: f32,
    /// Curva no lineal post-normalización (>1 comprime silencios, realza
    /// picos).
    pub gamma: f32,
    /// Peso extra del bass en el nivel global.
    pub bass_boost: f32,
    /// Cuánto flujo espectral se convierte en turbulencia (jitter).
    pub turbulence_gain: f32,
    /// Cuánto golpea un beat al pulso.
    pub beat_gain: f32,
    /// Tasa de fase (ciclos/s) cuando no hay BPM conocido.
    pub fallback_phase_rate: f32,
}

impl Default for MapperConfig {
    fn default() -> Self {
        Self {
            sensitivity: 1.6,
            noise_floor: 0.02,
            gamma: 1.35,
            bass_boost: 0.8,
            turbulence_gain: 0.9,
            beat_gain: 1.0,
            fallback_phase_rate: 0.5,
        }
    }
}

/// Parámetros visuales semánticos de un frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VisualParameters {
    /// Envolvente de barras ya mapeada (0..1).
    pub bars: [f32; VISUAL_BARS],
    /// Energía global 0..1 (bass ponderado).
    pub level: f32,
    /// Brillo/color 0..1 (agudos + flujo).
    pub intensity: f32,
    /// Turbulencia 0..1 (flujo → jitter espacial).
    pub turbulence: f32,
    /// Golpe de pulso instantáneo 0..1 (onset/beat crudos).
    pub pulse_kick: f32,
    /// Ciclos por segundo para la fase determinista.
    pub phase_rate: f32,
    /// Energía continua 0..1 (RMS): infla la escena sin depender del beat.
    pub energy: f32,
    /// Brillo 0..1 (agudos + flujo): intensidad de color/resplandor.
    pub brightness: f32,
}

fn shaped(v: f32, cfg: &MapperConfig) -> f32 {
    let gated = ((v - cfg.noise_floor).max(0.0)) * cfg.sensitivity;
    // Gamma sobre el valor normalizado post-gate (clamp implícito vía powf de
    // un número ≤ ~1; clamp explícito para sensibilidad agresiva).
    gated.clamp(0.0, 1.0).powf(cfg.gamma)
}

/// Anclas de banda → envolvente de `BARS` barras por interpolación suave.
///
/// Distribución log-ish: las 5 bandas cubren más barras en graves (donde está
/// la energía musical) y menos en agudos.
fn band_anchor_positions() -> [usize; 5] {
    let n = VISUAL_BARS as f32;
    [
        (n * 0.10) as usize,
        (n * 0.30) as usize,
        (n * 0.52) as usize,
        (n * 0.74) as usize,
        (n * 0.95).min(n - 1.0) as usize,
    ]
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub struct ParameterMapper {
    cfg: MapperConfig,
    anchors: [usize; 5],
}

impl Default for ParameterMapper {
    fn default() -> Self {
        Self::new(MapperConfig::default())
    }
}

impl ParameterMapper {
    pub fn new(cfg: MapperConfig) -> Self {
        Self {
            cfg,
            anchors: band_anchor_positions(),
        }
    }

    pub fn config(&self) -> &MapperConfig {
        &self.cfg
    }

    pub fn set_config(&mut self, cfg: MapperConfig) {
        self.cfg = cfg;
    }

    /// Mapeo PURO y determinista: mismas features → mismos parámetros.
    pub fn map(&self, f: &AudioFeatures) -> VisualParameters {
        // Anclas de banda con curva individual.
        let anchors_v = [
            shaped(f.bass, &self.cfg),
            shaped(f.low_mid, &self.cfg),
            shaped(f.mid, &self.cfg),
            shaped(f.high_mid, &self.cfg),
            shaped(f.high, &self.cfg),
        ];

        // Relleno entre anclas: interpolación smoothstep (sin allocs).
        let mut bars = [0.0f32; VISUAL_BARS];
        for (i, bar) in bars.iter_mut().take(self.anchors[0]).enumerate() {
            *bar = anchors_v[0] * smoothstep(i as f32 / self.anchors[0].max(1) as f32);
        }
        for seg in 0..4 {
            let a = self.anchors[seg];
            let b = self.anchors[seg + 1];
            for (i, bar) in bars[a..b].iter_mut().enumerate() {
                let i = a + i;
                let t = (i - a) as f32 / (b - a).max(1) as f32;
                *bar = anchors_v[seg] + (anchors_v[seg + 1] - anchors_v[seg]) * smoothstep(t);
            }
        }
        for (i, bar) in bars.iter_mut().enumerate().skip(self.anchors[4]) {
            // Cauda de agudos: decae hacia el borde derecho.
            let t =
                (i - self.anchors[4] + 1) as f32 / (VISUAL_BARS - self.anchors[4]).max(1) as f32;
            *bar = anchors_v[4] * (1.0 - smoothstep(t));
        }

        // Nivel global con boost de graves.
        let level_raw = (f.bass * (1.0 + self.cfg.bass_boost)
            + f.low_mid * 0.7
            + f.mid * 0.6
            + f.high_mid * 0.5
            + f.high * 0.5)
            / 3.1;
        let level = shaped(level_raw, &self.cfg);

        let intensity = shaped(f.high * 0.8 + f.spectral_flux * 0.4, &self.cfg);
        let turbulence = (f.spectral_flux * self.cfg.turbulence_gain).clamp(0.0, 1.0);
        let pulse_kick =
            (f.onset.max(if f.beat { 1.0 } else { 0.0 }) * self.cfg.beat_gain).clamp(0.0, 1.0);
        let phase_rate = if f.bpm > 0.0 {
            f.bpm / 60.0
        } else {
            self.cfg.fallback_phase_rate
        };

        // Compañeros de escena: continúan cuando el beat está ausente (alcance
        // de "energía visual continua", spec §6).
        let energy = shaped(f.rms, &self.cfg);
        let brightness = shaped(
            f.high * 0.8 + f.high_mid * 0.4 + f.spectral_flux * 0.3,
            &self.cfg,
        );

        VisualParameters {
            bars,
            level,
            intensity,
            turbulence,
            pulse_kick,
            phase_rate,
            energy,
            brightness,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn features(bands: [f32; 5], flux: f32, onset: f32, beat: bool, bpm: f32) -> AudioFeatures {
        AudioFeatures {
            timestamp: Duration::from_secs(1),
            rms: bands.iter().sum::<f32>() / 5.0,
            amplitude: 0.5,
            bass: bands[0],
            low_mid: bands[1],
            mid: bands[2],
            high_mid: bands[3],
            high: bands[4],
            spectral_centroid: 0.3,
            spectral_flux: flux,
            onset,
            beat,
            beat_confidence: 0.8,
            bpm,
        }
    }

    #[test]
    fn deterministic_mapping_same_input_same_output() {
        let mapper = ParameterMapper::default();
        let f = features([0.8, 0.4, 0.2, 0.1, 0.05], 0.3, 0.9, true, 120.0);
        let a = mapper.map(&f);
        let b = ParameterMapper::default().map(&f);
        assert_eq!(a, b, "el mapeo es una función pura");
    }

    #[test]
    fn all_outputs_stay_in_unit_range() {
        let mapper = ParameterMapper::new(MapperConfig {
            sensitivity: 4.0,
            ..Default::default()
        });
        let extremes = features([1.0; 5], 1.0, 1.0, true, 200.0);
        let p = mapper.map(&extremes);
        for v in p.bars.iter() {
            assert!((0.0..=1.0).contains(v), "bar fuera de rango: {v}");
        }
        for v in [p.level, p.intensity, p.turbulence, p.pulse_kick] {
            assert!((0.0..=1.0).contains(&v), "{v} fuera de rango");
        }
        for v in [p.energy, p.brightness] {
            assert!((0.0..=1.0).contains(&v), "{v} fuera de rango");
        }
    }

    #[test]
    fn silence_below_noise_floor_is_flat_zero() {
        let mapper = ParameterMapper::default();
        let quiet = features([0.01; 5], 0.0, 0.0, false, 0.0);
        let p = mapper.map(&quiet);
        assert!(p.bars.iter().all(|v| *v == 0.0));
        assert_eq!(p.level, 0.0);
    }

    #[test]
    fn bass_heavy_song_lifts_low_bars_more_than_high_ones() {
        let mapper = ParameterMapper::default();
        let bassy = features([0.9, 0.2, 0.05, 0.02, 0.01], 0.0, 0.0, false, 0.0);
        let p = mapper.map(&bassy);
        let low_avg: f32 = p.bars[..8].iter().sum::<f32>() / 8.0;
        let high_avg: f32 = p.bars[16..].iter().sum::<f32>() / 8.0;
        assert!(low_avg > high_avg * 2.0, "graves mandan a la izquierda");
    }

    #[test]
    fn bpm_drives_phase_rate_and_fallback_applies_without_it() {
        let mapper = ParameterMapper::default();
        let with_bpm = mapper.map(&features([0.5; 5], 0.0, 0.0, false, 120.0));
        assert!(
            (with_bpm.phase_rate - 2.0).abs() < 1e-4,
            "120 BPM = 2 ciclos/s"
        );

        let without = mapper.map(&features([0.5; 5], 0.0, 0.0, false, 0.0));
        assert!((without.phase_rate - MapperConfig::default().fallback_phase_rate).abs() < 1e-6);
    }

    #[test]
    fn beat_kicks_pulse_only_when_present() {
        let mapper = ParameterMapper::default();
        let hit = mapper.map(&features([0.5; 5], 0.1, 0.8, true, 0.0));
        let calm = mapper.map(&features([0.5; 5], 0.1, 0.0, false, 0.0));
        assert!(hit.pulse_kick > 0.5);
        assert_eq!(calm.pulse_kick, 0.0);
    }

    fn rms_of(bands: [f32; 5]) -> f32 {
        bands.iter().sum::<f32>() / 5.0
    }

    #[test]
    fn loud_rms_raises_energy_monotonically_beyond_floor() {
        let mapper = ParameterMapper::default();
        let soft = mapper.map(&features([0.1; 5], 0.0, 0.0, false, 0.0));
        let loud = mapper.map(&features([0.9; 5], 0.0, 0.0, false, 0.0));
        assert!(
            loud.energy > soft.energy,
            "más RMS ⇒ más energía de escena (continuo, no binario)"
        );
        assert_eq!(
            mapper
                .map(&features([0.005; 5], 0.0, 0.0, false, 0.0))
                .energy,
            0.0
        );
    }

    #[test]
    fn treble_raises_brightness_above_bass_heavy_song() {
        let mapper = ParameterMapper::default();
        let treble = mapper.map(&features([0.1, 0.1, 0.1, 0.3, 0.9], 0.2, 0.0, false, 0.0));
        let bassy = mapper.map(&features([0.9, 0.5, 0.2, 0.05, 0.02], 0.0, 0.0, false, 0.0));
        assert!(treble.brightness > bassy.brightness, "agudos iluminan");
    }

    #[test]
    fn energy_brightness_do_not_touch_on_silence() {
        let mapper = ParameterMapper::default();
        let p = mapper.map(&features(
            [0.005, 0.005, 0.0, 0.005, 0.005],
            0.0,
            0.0,
            false,
            0.0,
        ));
        assert_eq!(p.energy, 0.0);
        assert_eq!(p.brightness, 0.0);
    }

    #[test]
    fn test_rms_helper_is_sane() {
        assert!((rms_of([0.0; 5]) - 0.0).abs() < 1e-6);
        assert!((rms_of([1.0; 5]) - 1.0).abs() < 1e-6);
    }
}
