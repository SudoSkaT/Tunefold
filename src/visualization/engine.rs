//! VisualEngine: estado visual puro a partir de features + envolvente de forma
//! de onda + POSICIÓN de reproducción (spec §22).
//!
//! - Sin reloj propio: la fase se calcula `fract(posición × tasa)` — la misma
//!   posición produce SIEMPRE la misma fase (determinista, spec §43).
//! - Suavizado temporal de barras: subida rápida, caída lenta (picos que
//!   respiran); el jitter de turbulencia es función de fase+barras, nunca del
//!   wall-clock.
//! - El pulso decae por EVENTO recibido (~15 Hz de features): determinista
//!   frente al flujo de eventos.
//! - Osciloscopio real: la escena es el trace temporal + envolvente min/max
//!   ESTÉREO del PCM ([`StereoWaveform`], ~86 Hz) decimados a
//!   `WAVEFORM_BUCKETS` por canal; la ganancia se auto-ajusta por frame
//!   (1/peak) con suavizado para que el trazo nunca "salte" entre ventanas.

use std::sync::Arc;
use std::time::Duration;

use crate::analysis::{AudioFeatures, StereoWaveform, WaveformEnvelope};
use crate::visualization::palette::VisualTheme;
use crate::visualization::params::{ParameterMapper, VisualParameters};
use crate::visualization::VISUAL_BARS;

/// Vista de forma de onda ESTÉREO lista para el renderer (por valor, `Copy`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaveformView {
    /// Trace + envolvente del canal izquierdo (ch0), sin normalizar (~-1..1).
    pub left: WaveformEnvelope,
    /// Trace + envolvente del canal derecho (ch1).
    pub right: WaveformEnvelope,
    /// Ganancia automática suavizada (1/peak acotada): el renderer multiplica
    /// la amplitud por este factor en AMBOS canales.
    pub gain: f32,
}

impl WaveformView {
    /// Trazo en línea base (sin señal / escena dormida).
    pub fn baseline() -> Self {
        Self {
            left: WaveformEnvelope::silent(),
            right: WaveformEnvelope::silent(),
            gain: 1.0,
        }
    }
}

/// Escena ambiental (osciloscopio estéreo de forma de onda) lista para el renderer.
///
/// El App NO conoce envelopes ni buckets: consume `VisualState` y recibe la
/// escena ya calculada por el motor (spec §10).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneState {
    /// Trace + envolvente decimados listos para el trazo (+ auto-gain).
    pub waveform: WaveformView,
    /// Energía continua 0..1 (suavizada, RMS).
    pub energy: f32,
    /// Brillo 0..1 (agudos, con aporte del pulso de beat).
    pub brightness: f32,
    /// Tema fundido del track actual (el renderer solo lo consume).
    pub theme: VisualTheme,
    /// `true` cuando hay features frescas (análisis activo y sonando).
    pub active: bool,
}

/// Estado listo para el renderer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VisualState {
    /// Alturas finales 0..1 (post jitter y suavizado temporal).
    pub bars: [f32; VISUAL_BARS],
    /// Nivel global 0..1.
    pub level: f32,
    /// Brillo 0..1.
    pub intensity: f32,
    /// Pulso de beat actual 0..1 (decae entre beats).
    pub pulse: f32,
    /// Fase determinista derivada de la posición (0..1).
    pub phase: f32,
    /// `true` cuando hay features frescas (análisis activo y sonando).
    pub active: bool,
    /// Escena ambiental (osciloscopio) producida por el mismo motor.
    pub scene: SceneState,
}

impl VisualState {
    pub fn inactive() -> Self {
        Self {
            bars: [0.0; VISUAL_BARS],
            level: 0.0,
            intensity: 0.0,
            pulse: 0.0,
            phase: 0.0,
            active: false,
            scene: SceneState {
                waveform: WaveformView::baseline(),
                energy: 0.0,
                brightness: 0.0,
                theme: VisualTheme::fallback(),
                active: false,
            },
        }
    }
}

pub struct VisualEngine {
    mapper: ParameterMapper,
    prev_bars: [f32; VISUAL_BARS],
    prev_pulse: f32,
    /// Última posición vista (para detectar seeks: salto brusco ⇒ sin suavizar).
    last_position: Option<Duration>,
    /// Tema fundido actual (se acerca al del track en cada frame).
    theme: VisualTheme,
    /// Vista de forma de onda actual: se mantiene hasta que llegue una envolvente
    /// nueva, así el trazo "respira" sin parpadear entre frames.
    waveform_view: WaveformView,
    /// Última envolvente estéreo procesada (dedupe por identidad del `Arc`).
    last_envelope: Option<Arc<StereoWaveform>>,
    /// Envolventes suavizadas de la escena (para que no "tiemblen").
    smooth_energy: f32,
    smooth_brightness: f32,
}

// Constantes de dinámica (por llamada ≈ cada frame de features ~15 Hz):
const BAR_RISE: f32 = 0.55;
const BAR_FALL: f32 = 0.18;
const PULSE_DECAY: f32 = 0.86;
/// Cuánto se funde la paleta por frame (consigue la transición de track).
const PALETTE_RATE: f32 = 0.35;
/// Suavizado temporal de los niveles de escena por frame.
const SCENE_SMOOTH: f32 = 0.45;
/// Ajuste por frame del auto-gain (≈ cmd a ~0.3 s a otro nivel).
const WAVEFORM_GAIN_SMOOTH: f32 = 0.30;
/// Límites del auto-gain (1/peak): ni disparar a ruido de fondo ni aplastar.
const WAVEFORM_GAIN_MIN: f32 = 0.5;
const WAVEFORM_GAIN_MAX: f32 = 8.0;

impl VisualEngine {
    pub fn new(mapper: ParameterMapper) -> Self {
        Self {
            mapper,
            prev_bars: [0.0; VISUAL_BARS],
            prev_pulse: 0.0,
            last_position: None,
            theme: VisualTheme::fallback(),
            waveform_view: WaveformView::baseline(),
            last_envelope: None,
            smooth_energy: 0.0,
            smooth_brightness: 0.0,
        }
    }

    /// Avanza el estado visual.
    ///
    /// `features` = último snapshot del bus (`None` ⇒ visual inactivo).
    /// `envelope` = envolvente del MISMO frame (`None` ⇒ mantener el trazo
    /// anterior). La fase usa EXCLUSIVAMENTE `position` — pasarla desde el
    /// PositionClock. `theme` es el tema del track en curso (de la
    /// `DecodedThumb`); el motor lo funde internamente con el anterior.
    pub fn update(
        &mut self,
        features: Option<&Arc<AudioFeatures>>,
        envelope: Option<&Arc<StereoWaveform>>,
        position: Duration,
        theme: &VisualTheme,
    ) -> VisualState {
        self.theme = self.theme.mix(theme, PALETTE_RATE);
        let Some(f) = features else {
            // Inactivo: resetear memoria para no arrastrar picos viejos y
            // dejar la escena en línea base lista para fluir al volver.
            self.prev_bars = [0.0; VISUAL_BARS];
            self.prev_pulse = 0.0;
            self.smooth_energy = 0.0;
            self.smooth_brightness = 0.0;
            self.last_position = None;
            self.waveform_view = WaveformView::baseline();
            self.last_envelope = None;
            return VisualState {
                bars: [0.0; VISUAL_BARS],
                level: 0.0,
                intensity: 0.0,
                pulse: 0.0,
                phase: 0.0,
                active: false,
                scene: SceneState {
                    waveform: WaveformView::baseline(),
                    energy: 0.0,
                    brightness: 0.0,
                    theme: self.theme,
                    active: false,
                },
            };
        };

        let params: VisualParameters = self.mapper.map(f);

        // Seek detection: retroceso grande o salto >2 s reinicia la inercia de
        // barras (la escena del osciloscopio converge sola por auto-gain).
        if let Some(prev) = self.last_position {
            let jumped = position < prev || position.saturating_sub(prev) > Duration::from_secs(2);
            if jumped {
                self.prev_bars = params.bars;
                self.prev_pulse = params.pulse_kick;
            }
        }
        self.last_position = Some(position);

        // Fase determinista SOLO desde la posición musical.
        let rate = params.phase_rate.max(0.01);
        let phase = (position.as_secs_f32() * rate).fract().clamp(0.0, 1.0);

        // Barras: objetivo con jitter de turbulencia ligado a la fase.
        let mut bars = [0.0f32; VISUAL_BARS];
        for (i, slot) in bars.iter_mut().enumerate() {
            let wobble = ((phase * std::f32::consts::TAU * 3.0) + i as f32 * 0.7).sin();
            let target =
                (params.bars[i] * (1.0 + wobble * 0.18 * params.turbulence)).clamp(0.0, 1.0);
            let k = if target > self.prev_bars[i] {
                BAR_RISE
            } else {
                BAR_FALL
            };
            *slot = self.prev_bars[i] + (target - self.prev_bars[i]) * k;
        }
        self.prev_bars = bars;

        // Pulso: golpe instantáneo + decaimiento exponencial por evento.
        let pulse = (params.pulse_kick).max(self.prev_pulse * PULSE_DECAY);
        self.prev_pulse = pulse;

        // Osciloscopio: absorbe la envolvente estéreo nueva (dedupe por
        // identidad) y reajusta la ganancia automática suavemente hacia 1/peak.
        if let Some(env) = envelope {
            if !self
                .last_envelope
                .as_ref()
                .is_some_and(|last| Arc::ptr_eq(last, env))
            {
                let peak = env.peak().max(1e-4);
                let target = (1.0 / peak).clamp(WAVEFORM_GAIN_MIN, WAVEFORM_GAIN_MAX);
                self.waveform_view.gain +=
                    (target - self.waveform_view.gain) * WAVEFORM_GAIN_SMOOTH;
                self.waveform_view.left = env.left;
                self.waveform_view.right = env.right;
                self.last_envelope = Some(Arc::clone(env));
            }
        }

        // Envolventes continuas (no binarias): suben/bajan suaves.
        self.smooth_energy += (params.energy - self.smooth_energy) * SCENE_SMOOTH;
        self.smooth_brightness += (params.brightness - self.smooth_brightness) * SCENE_SMOOTH;

        VisualState {
            bars,
            level: params.level,
            intensity: params.intensity,
            pulse,
            phase,
            active: true,
            scene: SceneState {
                waveform: self.waveform_view,
                energy: self.smooth_energy.clamp(0.0, 1.0),
                brightness: (self.smooth_brightness + pulse * 0.3).clamp(0.0, 1.0),
                theme: self.theme,
                active: true,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::bands::BandRatios;
    use crate::visualization::params::{MapperConfig, ParameterMapper};

    fn features(bass: f32, flux: f32, onset: f32, beat: bool, bpm: f32) -> AudioFeatures {
        AudioFeatures {
            timestamp: Duration::from_secs(1),
            rms: bass * 0.5,
            amplitude: bass,
            bass,
            low_mid: bass * 0.5,
            mid: bass * 0.25,
            high_mid: 0.05,
            high: 0.02,
            spectral_centroid: 0.2,
            spectral_flux: flux,
            onset,
            beat,
            beat_confidence: 0.9,
            bpm,
        }
    }

    /// Envolvente estéreo de amplitud constante por bucket (auto-gain).
    fn envelope(amp: f32) -> Arc<StereoWaveform> {
        Arc::new(StereoWaveform::from_windows(&[amp; 2048], &[amp; 2048]))
    }

    /// Envolvente estéreo con canales ASIMÉTRICOS.
    fn stereo_envelope(left: f32, right: f32) -> Arc<StereoWaveform> {
        Arc::new(StereoWaveform::from_windows(&[left; 2048], &[right; 2048]))
    }

    fn engine() -> VisualEngine {
        VisualEngine::new(ParameterMapper::new(MapperConfig {
            noise_floor: 0.01,
            ..Default::default()
        }))
    }

    const FALLBACK: VisualTheme = VisualTheme::fallback();

    fn pal(cover: Option<[[u8; 3]; 3]>) -> VisualTheme {
        VisualTheme::from_cover(cover)
    }

    #[test]
    fn same_inputs_same_state_deterministic() {
        let mut a = engine();
        let mut b = engine();
        let f = Arc::new(features(0.7, 0.4, 0.5, true, 120.0));
        let env = envelope(0.4);
        let pos = Duration::from_secs(42);
        assert_eq!(
            a.update(Some(&f), Some(&env), pos, &FALLBACK),
            b.update(Some(&f), Some(&env), pos, &FALLBACK)
        );
    }

    #[test]
    fn phase_depends_only_on_playback_position_not_wall_clock() {
        let mut e = engine();
        let f = Arc::new(features(0.5, 0.0, 0.0, false, 120.0)); // 2 ciclos/s
        let s1 = e.update(Some(&f), None, Duration::from_millis(250), &FALLBACK);
        // "Mucho después" en wall-clock pero MISMA posición musical:
        std::thread::sleep(std::time::Duration::from_millis(30));
        let s2 = e.update(Some(&f), None, Duration::from_millis(250), &FALLBACK);
        assert_eq!(s1.phase, s2.phase, "cero reloj visual independiente");
        assert!(
            (s1.phase - 0.5).abs() < 1e-4,
            "250 ms × 2 ciclos/s → fase ½"
        );
    }

    #[test]
    fn outputs_always_clamped_to_unit_range() {
        let mut e = VisualEngine::new(ParameterMapper::new(MapperConfig {
            sensitivity: 5.0,
            turbulence_gain: 2.0,
            ..Default::default()
        }));
        let loud = Arc::new(features(1.0, 1.0, 1.0, true, 200.0));
        for step in 0..60 {
            let s = e.update(
                Some(&loud),
                Some(&envelope(0.9)),
                Duration::from_millis(step * 66),
                &FALLBACK,
            );
            for v in s.bars.iter() {
                assert!((0.0..=1.0).contains(v), "bar {v} fuera de rango en t{step}");
            }
            for v in [
                s.level,
                s.intensity,
                s.pulse,
                s.scene.energy,
                s.scene.brightness,
            ] {
                assert!((0.0..=1.0).contains(&v), "{v} fuera de rango en t{step}");
            }
            assert!((0.0..=1.0).contains(&s.phase));
            assert!(
                (WAVEFORM_GAIN_MIN..=WAVEFORM_GAIN_MAX).contains(&s.scene.waveform.gain),
                "gain acotada: {}",
                s.scene.waveform.gain
            );
        }
    }

    #[test]
    fn beat_spikes_pulse_then_decays_monotonically() {
        let mut e = engine();
        let hit = Arc::new(features(0.6, 0.1, 1.0, true, 0.0));
        let rest = Arc::new(features(0.6, 0.1, 0.0, false, 0.0));

        let s0 = e.update(Some(&hit), None, Duration::ZERO, &FALLBACK);
        assert!(s0.pulse > 0.8, "el beat pega fuerte: {}", s0.pulse);

        let mut prev = s0.pulse;
        for i in 1..12 {
            let s = e.update(Some(&rest), None, Duration::from_millis(i * 66), &FALLBACK);
            assert!(s.pulse <= prev, "el pulso decae sin nuevos beats");
            prev = s.pulse;
        }
        assert!(prev < 0.25, "y llega casi a cero: {prev}");
    }

    #[test]
    fn silence_is_active_but_low() {
        let mut e = engine();
        let quiet = Arc::new(features(0.005, 0.0, 0.0, false, 0.0));
        let s = e.update(Some(&quiet), None, Duration::ZERO, &FALLBACK);
        assert!(s.active);
        assert!(s.level < 0.05);
        assert!(s.bars.iter().all(|v| *v < 0.08));
        assert!(s.scene.energy < 0.05);
    }

    #[test]
    fn none_features_resets_to_inactive() {
        let mut e = engine();
        let f = Arc::new(features(0.9, 0.5, 0.5, true, 120.0));
        let env = envelope(0.6);
        let _ = e.update(Some(&f), Some(&env), Duration::ZERO, &FALLBACK);
        let off = e.update(None, None, Duration::ZERO, &FALLBACK);
        assert!(!off.active);
        assert!(off.bars.iter().all(|v| *v == 0.0));
        assert!(!off.scene.active);
        assert_eq!(
            off.scene.waveform,
            WaveformView::baseline(),
            "sin features la escena reposa en línea base"
        );
        assert_eq!(off.scene.energy, 0.0);

        // Reactivar equivale a un motor FRESCO: sin arrastrar picos viejos.
        let mut virgin = engine();
        let again = e.update(Some(&f), Some(&env), Duration::ZERO, &FALLBACK);
        let baseline = virgin.update(Some(&f), Some(&env), Duration::ZERO, &FALLBACK);
        assert_eq!(again.bars, baseline.bars, "sin memoria de la sesión previa");
    }

    #[test]
    fn seek_reinitializes_bar_inertia_without_crash() {
        let mut e = engine();
        let f = Arc::new(features(0.8, 0.3, 0.0, false, 0.0));
        let _ = e.update(Some(&f), None, Duration::from_secs(100), &FALLBACK);
        // Retroceso de 90 s: debe comportarse sano (sin pánicos ni NaN).
        let s = e.update(Some(&f), None, Duration::from_secs(10), &FALLBACK);
        assert!(s.bars.iter().all(|v| v.is_finite()));
        assert!(s
            .scene
            .waveform
            .left
            .min
            .iter()
            .chain(s.scene.waveform.left.max.iter())
            .chain(s.scene.waveform.right.min.iter())
            .chain(s.scene.waveform.right.max.iter())
            .all(|v| v.is_finite()));
    }

    #[test]
    fn envelope_absorbs_and_auto_gains_smoothly() {
        let mut e = engine();
        let f = Arc::new(features(0.6, 0.2, 0.0, false, 0.0));

        // Una envolvente nueva se copia y mueve la ganancia hacia 1/peak.
        let loud = envelope(1.0);
        let s = e.update(Some(&f), Some(&loud), Duration::ZERO, &FALLBACK);
        assert_eq!(s.scene.waveform.left.max[0], 1.0, "absorbe el pico máximo");
        assert_eq!(
            s.scene.waveform.left.min[0], 1.0,
            "absorbe el valle (constante)"
        );
        let target = (1.0_f32 / 1.0_f32).clamp(WAVEFORM_GAIN_MIN, WAVEFORM_GAIN_MAX);
        assert!(
            (s.scene.waveform.gain - target).abs() < 0.4,
            "primer frame ya se acerca a 1/peak: {}",
            s.scene.waveform.gain
        );

        // El MISMO Arc repetido no recalcula pero conserva el estado: el trazo
        // "respira" a la misma ganancia sin parpadeo.
        let again = e.update(Some(&f), Some(&loud), Duration::from_millis(66), &FALLBACK);
        assert_eq!(again.scene.waveform, s.scene.waveform);
    }

    #[test]
    fn stereo_channels_absorbed_independently() {
        let mut e = engine();
        let f = Arc::new(features(0.6, 0.2, 0.0, false, 0.0));

        // L fuerte, R débil: ambas curvas deben llegar sin mezclarse.
        let asym = stereo_envelope(0.9, 0.15);
        let s = e.update(Some(&f), Some(&asym), Duration::ZERO, &FALLBACK);
        assert_eq!(s.scene.waveform.left.max[0], 0.9, "L conserva su pico");
        assert_eq!(s.scene.waveform.right.max[0], 0.15, "R conserva el suyo");
        assert!(
            (s.scene.waveform.gain - (1.0 / 0.9)).abs() < 0.4,
            "el auto-gain mira el pico del canal más fuerte: {}",
            s.scene.waveform.gain
        );

        // Luego R llega con un pico mayor: la ganancia baja hacia su 1/peak.
        let flip = stereo_envelope(0.2, 1.0);
        let s2 = e.update(Some(&f), Some(&flip), Duration::from_millis(66), &FALLBACK);
        assert_eq!(s2.scene.waveform.left.max[0], 0.2);
        assert_eq!(s2.scene.waveform.right.max[0], 1.0);
    }

    #[test]
    fn quieter_audio_raises_gain_toward_ceiling() {
        let mut e = engine();
        let f = Arc::new(features(0.6, 0.2, 0.0, false, 0.0));

        // Audio muy suave, ventanas NUEVAS por frame (como llegan los hops):
        // el auto-gain debe crecer hacia el límite superior (para que el trazo
        // siga siendo visible, sin disparar del todo).
        let mut prev = WAVEFORM_GAIN_MIN;
        let mut rose = false;
        for step in 0..24 {
            let soft = envelope(0.02);
            let s = e.update(
                Some(&f),
                Some(&soft),
                Duration::from_millis(step * 66),
                &FALLBACK,
            );
            assert!(
                s.scene.waveform.gain >= prev - 1e-4,
                "ganancia monótona a 1/peak: {}",
                s.scene.waveform.gain
            );
            rose |= s.scene.waveform.gain > prev;
            prev = s.scene.waveform.gain;
        }
        assert!(rose, "la ganancia sube al bajar el volumen");
        assert!(
            (prev - WAVEFORM_GAIN_MAX).abs() < 1.5,
            "y converge hacia el techo: {prev}"
        );
    }

    #[test]
    fn palette_blends_toward_target_track() {
        let mut e = engine();
        let f = Arc::new(features(0.6, 0.2, 0.0, false, 0.0));
        let warm = pal(Some([[200u8, 40, 40], [40, 200, 60], [30, 60, 220]]));
        // Primeros frames hacia "warm": se acerca, nunca salta.
        let first = e.update(Some(&f), None, Duration::ZERO, &warm).scene.theme;
        assert_eq!(first, e.theme);
        assert_ne!(first, warm, "la fusión es gradual, no teleport");
        for _ in 0..40 {
            e.update(Some(&f), None, Duration::from_millis(66), &warm);
        }
        let near_warm = e.theme;
        let d = |a: &VisualTheme, b: &VisualTheme| {
            a.primary
                .iter()
                .zip(b.primary.iter())
                .map(|(x, y)| (*x as i32 - *y as i32).abs())
                .sum::<i32>()
        };
        assert!(d(&near_warm, &warm) <= 3, "converge al track nuevo");
        // Y la escena expone ESA paleta al renderer.
        let s = e.update(Some(&f), None, Duration::ZERO, &warm);
        assert_eq!(s.scene.theme, e.theme);
    }

    // BandRatios referenciado para mantener import vivo si evoluciona.
    #[allow(dead_code)]
    fn _touch(_: BandRatios) {}
}
