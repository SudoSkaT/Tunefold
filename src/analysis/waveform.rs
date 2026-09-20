//! Envolvente ESTÉREO de forma de onda (trace temporal + min/max por canal)
//! extraída de las ventanas L/R del análisis, publicada junto a las features
//! al consumidor visual (spec §20).
//!
//! La escena ambiental es un osciloscopio REAL: la forma de onda muestreada de
//! cada canal del PCM. Por cada hop (~86 Hz) se reduce la ventana deslizante
//! de cada canal (2048 muestras ≈ 46 ms) a `WAVEFORM_BUCKETS` tríos por canal:
//!
//! - `trace`: muestra temporal representativa del bucket (su muestra central).
//!   Es el TRAZO principal: conserva la evolución temporal (subidas, bajadas,
//!   cruces por cero) para que el ojo pueda seguir una trayectoria.
//! - `min`/`max`: envolvente del bucket. Es información AUXILIAR: protege
//!   transitorios y picos que el trace puntual podría no tocar, y el renderer
//!   la pinta como detalle secundario.
//!
//! El UI decima esos buckets al ancho de terminal sin re-rankear y pinta el
//! trace columna a columna, con los extremos como acentos cuando aportan
//! novedad sobre el trace.

use std::sync::{Arc, RwLock};

/// Nº de buckets (tríos trace/min/max por canal). Fijo para que el consumidor
/// decime sin realojar: 128 cubren pantallas típicas de extremo a extremo.
pub const WAVEFORM_BUCKETS: usize = 128;

/// Envolvente de UN canal: trace temporal + envolvente min/max (valores de
/// amplitud sin normalizar, ~-1..1).
///
/// `Copy`, tamaño fijo en stack, sin allocations: barata de publicar (~86 Hz)
/// y de copiar al estado visual.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaveformEnvelope {
    /// Muestra temporal representativa por bucket (la central de su tramo).
    /// Es la forma de onda reducida; NUNCA es `(min+max)/2`.
    pub trace: [f32; WAVEFORM_BUCKETS],
    /// Mínimo (valle) por bucket.
    pub min: [f32; WAVEFORM_BUCKETS],
    /// Máximo (pico) por bucket.
    pub max: [f32; WAVEFORM_BUCKETS],
}

impl WaveformEnvelope {
    /// Reduce la ventana mono `window` a `WAVEFORM_BUCKETS` tríos trace/min/max.
    ///
    /// Cada bucket absorbe un tramo consecutivo: `len/128` muestras, con las
    /// primeras `len%128` tomando una extra (distribución uniforme, sin
    /// superponer ventanas). El trace es la muestra CENTRAL del tramo
    /// (temporalmente representativa, O(1) por bucket); min/max barren el
    /// tramo para no perder transitorios.
    pub fn from_window(window: &[f32]) -> Self {
        let mut trace = [0.0f32; WAVEFORM_BUCKETS];
        let mut min = [0.0f32; WAVEFORM_BUCKETS];
        let mut max = [0.0f32; WAVEFORM_BUCKETS];
        if window.is_empty() {
            return Self { trace, min, max };
        }
        let per = window.len() / WAVEFORM_BUCKETS;
        let rem = window.len() % WAVEFORM_BUCKETS;
        let mut idx = 0usize;
        for bucket in 0..WAVEFORM_BUCKETS {
            let len = per + usize::from(bucket < rem);
            if len == 0 {
                continue;
            }
            let hi = idx + len;
            trace[bucket] = window[idx + len / 2];
            min[bucket] = window[idx..hi]
                .iter()
                .copied()
                .fold(f32::INFINITY, f32::min);
            max[bucket] = window[idx..hi]
                .iter()
                .copied()
                .fold(f32::NEG_INFINITY, f32::max);
            idx = hi;
        }
        Self { trace, min, max }
    }

    /// Envolvente plana para cortes/arranques (trazo en línea base).
    pub fn silent() -> Self {
        Self {
            trace: [0.0; WAVEFORM_BUCKETS],
            min: [0.0; WAVEFORM_BUCKETS],
            max: [0.0; WAVEFORM_BUCKETS],
        }
    }

    /// Amplitud pico absoluta de la ventana (alimenta el auto-gain del trazo).
    pub fn peak(&self) -> f32 {
        let mut p = 0.0f32;
        for bucket in 0..WAVEFORM_BUCKETS {
            p = p.max(self.max[bucket].abs()).max(self.min[bucket].abs());
        }
        p
    }
}

/// Forma de onda ESTÉREO: trace temporal + envolventes min/max
/// independientes por canal.
///
/// Es el payload del [`WaveformBus`]. El renderer sigue el trace de cada
/// canal como trazo principal y conserva pico Y valle como acentos; dos
/// canales distintos pueden pintarse en sus planos sin perderse.
#[derive(Debug, Clone, PartialEq)]
pub struct StereoWaveform {
    /// Canal izquierdo (ch0).
    pub left: WaveformEnvelope,
    /// Canal derecho (ch1).
    pub right: WaveformEnvelope,
}

impl StereoWaveform {
    /// Construye el snapshot desde las ventanas de canal ya de-intercaladas
    /// (ruta caliente del motor de análisis, sin reordenar muestras).
    pub fn from_windows(left: &[f32], right: &[f32]) -> Self {
        Self {
            left: WaveformEnvelope::from_window(left),
            right: WaveformEnvelope::from_window(right),
        }
    }

    /// Construye el snapshot desde muestras intercaladas (`L0 R0 L1 R1 …`).
    ///
    /// Política de canales: mono (1) duplica la señal a ambos canales; estéreo
    /// y multicanal usan ch0 → L y ch1 → R (el resto se ignora). Pensado para
    /// bancos de prueba/arranques; el motor caliente usa [`Self::from_windows`].
    pub fn from_interleaved(samples: &[f32], channels: u16) -> Self {
        if samples.is_empty() {
            return Self {
                left: WaveformEnvelope::silent(),
                right: WaveformEnvelope::silent(),
            };
        }
        let ch = usize::from(channels.clamp(1, 2));
        if ch == 1 {
            let e = WaveformEnvelope::from_window(samples);
            return Self { left: e, right: e };
        }
        let frames = samples.len() / ch;
        let mut left = Vec::with_capacity(frames);
        let mut right = Vec::with_capacity(frames);
        for f in 0..frames {
            left.push(samples[f * ch]);
            right.push(samples[f * ch + 1]);
        }
        Self::from_windows(&left, &right)
    }

    /// Snapshot plano para cortes/arranques/pausas (trazo en línea base).
    pub fn silent() -> Self {
        Self {
            left: WaveformEnvelope::silent(),
            right: WaveformEnvelope::silent(),
        }
    }

    /// Amplitud pico absoluta a través de ambos canales (auto-gain del trazo).
    pub fn peak(&self) -> f32 {
        self.left.peak().max(self.right.peak())
    }
}

/// Bus de publicación/lectura del último snapshot de envolvente ESTÉREO.
///
/// Mismo patrón que [`super::features::FeatureBus`]: escritor único (hilo de
/// análisis) ~86 Hz, lectores múltiples a ≤15 Hz clonando el `Arc` bajo lock
/// de lectura — contención despreciable y cero allocs en la ruta del audio.
#[derive(Clone)]
pub struct WaveformBus {
    slot: Arc<RwLock<Option<Arc<StereoWaveform>>>>,
}

impl Default for WaveformBus {
    fn default() -> Self {
        Self::new()
    }
}

impl WaveformBus {
    pub fn new() -> Self {
        Self {
            slot: Arc::new(RwLock::new(None)),
        }
    }

    /// Publica la envolvente del último hop como la disponible.
    pub fn publish(&self, envelope: StereoWaveform) -> Arc<StereoWaveform> {
        let arc = Arc::new(envelope);
        *self.slot.write().unwrap() = Some(Arc::clone(&arc));
        arc
    }

    /// Última envolvente publicada (`None` hasta que el análisis arranque).
    pub fn latest(&self) -> Option<Arc<StereoWaveform>> {
        self.slot.read().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_window_reduces_preserving_global_peak() {
        let window: Vec<f32> = (0..2048).map(|i| (i as f32 / 2048.0) * 2.0 - 1.0).collect();
        let env = WaveformEnvelope::from_window(&window);
        assert!((env.peak() - 1.0).abs() < 1e-3, "peak ≈ 1: {}", env.peak());
        for b in 0..WAVEFORM_BUCKETS {
            assert!(env.min[b] <= env.max[b], "bucket {b} invertido");
        }
    }

    #[test]
    fn tiny_windows_do_not_overflow_bucket_bounds() {
        // 2 muestras < 128 buckets: las primeras 2 buckets toman 1 muestra
        // cada una (en orden), el resto queda a 0.
        let env = WaveformEnvelope::from_window(&[0.5, -0.5]);
        assert_eq!(env.min[0], 0.5, "bucket 0 toma la primera muestra");
        assert_eq!(env.max[0], 0.5);
        assert_eq!(env.min[1], -0.5, "bucket 1 toma la segunda muestra");
        assert_eq!(env.max[1], -0.5);
        assert!(env.min[2..].iter().all(|v| *v == 0.0), "sin muestras ⇒ 0");
    }

    #[test]
    fn empty_and_silent_envelopes_are_flat() {
        assert_eq!(
            WaveformEnvelope::from_window(&[]),
            WaveformEnvelope::silent()
        );
        assert_eq!(WaveformEnvelope::silent().peak(), 0.0);
    }

    #[test]
    fn bus_publishes_latest_like_feature_bus() {
        let bus = WaveformBus::new();
        assert!(bus.latest().is_none(), "sin publicar aún");

        let loud = StereoWaveform::from_windows(&[0.8; 2048], &[0.6; 2048]);
        let got = bus.publish(loud);
        assert_eq!(bus.latest().unwrap().peak(), 0.8);
        assert_eq!(Arc::strong_count(&got), 2, "bus + lector local");

        bus.publish(StereoWaveform::silent());
        assert_eq!(bus.latest().unwrap().peak(), 0.0);
        assert_eq!(got.peak(), 0.8, "el snapshot viejo sigue inmutable");
    }

    #[test]
    fn interleaved_mono_duplicates_signal_to_both_channels() {
        let stereo = StereoWaveform::from_interleaved(&[0.5, -0.25, 0.7, -0.9], 1);
        assert_eq!(stereo.left, stereo.right);
        for b in 0..WAVEFORM_BUCKETS {
            assert_eq!(stereo.left.min[b], stereo.right.min[b]);
            assert_eq!(stereo.left.max[b], stereo.right.max[b]);
        }
        assert!((stereo.peak() - 0.9).abs() < 1e-6);
    }

    #[test]
    fn interleaved_stereo_splits_channels() {
        // L es una rampa creciente, R una rampa decreciente (misma magnitud).
        let mut samples = Vec::with_capacity(1024);
        for f in 0..512 {
            let t = f as f32 / 511.0;
            samples.push(t); // L
            samples.push(1.0 - t); // R
        }
        let stereo = StereoWaveform::from_interleaved(&samples, 2);
        assert!(
            (stereo.left.peak() - 1.0).abs() < 1e-6,
            "L pico a 1: {}",
            stereo.left.peak()
        );
        assert!(
            (stereo.right.peak() - 1.0).abs() < 1e-6,
            "R pico a 1: {}",
            stereo.right.peak()
        );
        // En el rango donde la rampa creciente domina, L > R en esos buckets.
        let mut left_above = false;
        let mut right_above = false;
        for b in 0..WAVEFORM_BUCKETS {
            if stereo.left.max[b] > stereo.right.max[b] + 0.5 {
                left_above = true;
            }
            if stereo.right.max[b] > stereo.left.max[b] + 0.5 {
                right_above = true;
            }
        }
        assert!(left_above, "ramas L deben dominar hacia el final");
        assert!(right_above, "ramas R deben dominar hacia el inicio");
    }

    #[test]
    fn from_windows_buckets_forth_and_back() {
        // from_interleaved(2) ≡ from_windows(de-intercalado manual).
        let samples: Vec<f32> = (0..2048).map(|i| (i as f32 / 2047.0) * 2.0 - 1.0).collect();
        let mut left = Vec::with_capacity(1024);
        let mut right = Vec::with_capacity(1024);
        for f in 0..1024 {
            left.push(samples[2 * f]);
            right.push(samples[2 * f + 1]);
        }
        let from_i = StereoWaveform::from_interleaved(&samples, 2);
        let from_w = StereoWaveform::from_windows(&left, &right);
        assert_eq!(from_i.left, from_w.left);
        assert_eq!(from_i.right, from_w.right);
    }

    #[test]
    fn silent_channels_and_opposite_phases() {
        // Canal derecho mudo: solo L debe mover el trazo.
        let stereo = StereoWaveform::from_windows(&[0.6; 2048], &[0.0; 2048]);
        assert!((stereo.peak() - 0.6).abs() < 1e-6);
        assert_eq!(stereo.right.peak(), 0.0);

        // Fases opuestas (L=sin, R=-sin): picos iguales (no se cancelan).
        let opposite = StereoWaveform::from_windows(&[0.9; 2048], &[-0.9; 2048]);
        assert!((opposite.peak() - 0.9).abs() < 1e-6);

        assert_eq!(StereoWaveform::silent().peak(), 0.0);
    }

    #[test]
    fn interleaved_l0r0l1r1l2r2_splits_in_order() {
        // Caso canónico PCM interleaved `L0 R0 L1 R1 L2 R2` (3 frames con
        // valores distintos para detectar swaps o mezclas accidentales):
        // L = [L0, L1, L2], R = [R0, R1, R2], en orden, sin cruzarse.
        let (l0, l1, l2) = (0.1f32, 0.3, 0.5);
        let (r0, r1, r2) = (-0.1f32, -0.3, -0.5);
        let samples = [l0, r0, l1, r1, l2, r2];
        let stereo = StereoWaveform::from_interleaved(&samples, 2);
        // Ventana diminuta (3 muestras < 128 buckets): la muestra i cae en el
        // bucket i, en orden — permite verificar posición, no solo picos.
        for (i, &expected) in [l0, l1, l2].iter().enumerate() {
            assert_eq!(
                stereo.left.min[i], expected,
                "L[{i}] conserva su muestra en orden"
            );
            assert_eq!(stereo.left.max[i], expected);
        }
        for (i, &expected) in [r0, r1, r2].iter().enumerate() {
            assert_eq!(
                stereo.right.min[i], expected,
                "R[{i}] conserva su muestra en orden"
            );
            assert_eq!(stereo.right.max[i], expected);
        }
        // Ni mezclados (L0 con R0) ni cruzados (L con contenido de R).
        assert_ne!(stereo.left.min[0], stereo.right.min[0]);
        assert!((stereo.left.max[2] - l2).abs() < 1e-6);
        assert!((stereo.right.min[2] - r2).abs() < 1e-6);
    }

    #[test]
    fn stereo_channels_never_swapped_or_mixed() {
        // L fuerte / R débil con valores constantes distinguibles: cada canal
        // conserva SU amplitud; un swap accidental invertiría estos asserts.
        let stereo = StereoWaveform::from_windows(&[0.9; 2048], &[0.15; 2048]);
        assert!(
            (stereo.left.peak() - 0.9).abs() < 1e-6,
            "L conserva su pico: {}",
            stereo.left.peak()
        );
        assert!(
            (stereo.right.peak() - 0.15).abs() < 1e-6,
            "R conserva el suyo: {}",
            stereo.right.peak()
        );
        assert!(
            stereo.left.min.iter().all(|&v| (v - 0.9).abs() < 1e-6),
            "L no contiene muestras de R"
        );
        assert!(
            stereo.right.min.iter().all(|&v| (v - 0.15).abs() < 1e-6),
            "R no contiene muestras de L"
        );
    }

    #[test]
    fn trace_follows_sine_with_sign_changes_and_zero_crossings() {
        // TRACE CRÍTICO (§41): un seno debe producir un trace que sube, baja
        // y cruza cero — evolución temporal, no una envolvente abstracta.
        let sine: Vec<f32> = (0..2048)
            .map(|i| 0.8 * ((i as f32 / 2048.0) * std::f32::consts::TAU * 6.0).sin())
            .collect();
        let env = WaveformEnvelope::from_window(&sine);
        assert!(
            env.trace.iter().any(|&v| v > 0.5),
            "el trace alcanza picos +"
        );
        assert!(
            env.trace.iter().any(|&v| v < -0.5),
            "el trace alcanza valles -"
        );
        // Cruces por cero entre buckets consecutivos (cambios de signo).
        let crossings = env
            .trace
            .windows(2)
            .filter(|w| w[0].signum() != w[1].signum())
            .count();
        assert!(
            crossings >= 8,
            "el trace cruza cero como el seno: {crossings}"
        );
        // No es constante ni una envolvente centrada: varía de verdad.
        let distinct: std::collections::HashSet<u32> = env
            .trace
            .iter()
            .map(|v| (v * 100.0) as i32 as u32)
            .collect();
        assert!(
            distinct.len() > 20,
            "trace con resolución temporal: {}",
            distinct.len()
        );
    }

    #[test]
    fn trace_is_a_real_sample_not_the_envelope_midpoint() {
        // El trace es una muestra temporal REAL del tramo, no (min+max)/2:
        // en un seno el centro del tramo cae en el interior (min < trace <
        // max) en la mayoría de buckets, mientras que el midpoint de una
        // envolvente simétrica sería ~0.
        let sine: Vec<f32> = (0..2048)
            .map(|i| 0.8 * ((i as f32 / 2048.0) * std::f32::consts::TAU * 6.0).sin())
            .collect();
        let env = WaveformEnvelope::from_window(&sine);
        let interior = (0..WAVEFORM_BUCKETS)
            .filter(|&b| env.min[b] < env.trace[b] && env.trace[b] < env.max[b])
            .count();
        assert!(
            interior > WAVEFORM_BUCKETS / 2,
            "trace interior a la envolvente en {interior}/128 buckets"
        );
        // Y coincide con una muestra real de la ventana (la central del tramo).
        assert!(
            sine.contains(&env.trace[10]),
            "el trace es una muestra de la ventana"
        );
    }

    #[test]
    fn envelope_catches_what_trace_misses_impulse() {
        // Roles complementarios: un impulso fuera del centro del bucket NO lo
        // toca el trace (sigue en línea base) pero la envolvente lo conserva.
        let mut window = [0.0f32; 2048];
        window[1000] = 1.0;
        let env = WaveformEnvelope::from_window(&window);
        // Bucket 62 cubre 992..1008, centro = índice 1000 → ¡SÍ lo toca!
        // Usar índice 1001 (no central): trace en base, envolvente con pico.
        let mut window2 = [0.0f32; 2048];
        window2[1001] = 1.0;
        let env2 = WaveformEnvelope::from_window(&window2);
        let b = 1001 / 16;
        assert_eq!(env2.max[b], 1.0, "la envolvente conserva el impulso");
        assert!(
            env2.trace[b].abs() < 1e-6,
            "el trace puntual no lo toca: {}",
            env2.trace[b]
        );
        let _ = env;
    }

    #[test]
    fn trace_follows_ramp_monotonically() {
        // Rampa creciente: el trace debe ser (casi) monótono creciente.
        let ramp: Vec<f32> = (0..2048).map(|i| i as f32 / 2048.0 * 2.0 - 1.0).collect();
        let env = WaveformEnvelope::from_window(&ramp);
        let mut violations = 0;
        for w in env.trace.windows(2) {
            if w[1] < w[0] - 1e-6 {
                violations += 1;
            }
        }
        assert_eq!(
            violations, 0,
            "trace monótono en rampa: {violations} fallos"
        );
        assert!(env.trace[0] < -0.9 && env.trace[WAVEFORM_BUCKETS - 1] > 0.9);
    }

    #[test]
    fn trace_alternates_on_square_wave() {
        // Onda cuadrada con semiperiodo no alineado a buckets: el trace toma
        // ambos extremos siguiendo la forma, y min/max cubren ambos.
        let sq: Vec<f32> = (0..2048)
            .map(|i| if (i / 5) % 2 == 0 { 0.9 } else { -0.9 })
            .collect();
        let env = WaveformEnvelope::from_window(&sq);
        assert!(
            env.trace.iter().all(|&v| v.abs() > 0.8),
            "trace en extremos"
        );
        assert!(env.trace.iter().any(|&v| v > 0.0), "trace toca +");
        assert!(env.trace.iter().any(|&v| v < 0.0), "trace toca -");
        assert!(env.min.iter().all(|&v| (v + 0.9).abs() < 1e-6));
        assert!(env.max.iter().all(|&v| (v - 0.9).abs() < 1e-6));
    }

    #[test]
    fn trace_of_tiny_window_preserves_sample_order() {
        // Ventana diminuta: cada muestra cae en su bucket, también el trace.
        let env = WaveformEnvelope::from_window(&[0.5, -0.5]);
        assert_eq!(env.trace[0], 0.5);
        assert_eq!(env.trace[1], -0.5);
        assert!(env.trace[2..].iter().all(|v| *v == 0.0));
    }

    #[test]
    fn downsampling_preserves_transient_peak_instead_of_averaging() {
        // Transitorio de alta frecuencia: un único spike 1.0 entre ceros. Un
        // `chunks().average()` lo diluiría (~1/16 ≈ 0.06); la envolvente
        // min/max debe conservar el pico intacto en su bucket.
        let mut window = [0.0f32; 2048];
        window[1000] = 1.0;
        window[1001] = -1.0;
        let env = WaveformEnvelope::from_window(&window);
        assert!(
            (env.peak() - 1.0).abs() < 1e-6,
            "el transitorio sobrevive a la reducción: {}",
            env.peak()
        );
        assert!(
            env.max.iter().any(|&v| (v - 1.0).abs() < 1e-6),
            "el pico positivo queda en algún bucket"
        );
        assert!(
            env.min.iter().any(|&v| (v + 1.0).abs() < 1e-6),
            "el valle negativo queda en algún bucket"
        );
        // Y un promedio ingenuo lo habría escondido: sanity del test.
        let naive_avg: f32 = window.iter().sum::<f32>() / window.len() as f32;
        assert!(
            naive_avg.abs() < 0.01,
            "el promedio global esconde el transitorio ({naive_avg})"
        );
    }
}
