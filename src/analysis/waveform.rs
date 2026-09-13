//! Envolvente de forma de onda (min/max) extraída de la ventana mono del
//! análisis, publicada junto a las features al consumidor visual (spec §20).
//!
//! La escena ambiental abandona la "lava" sintética (metaballs) por un
//! osciloscopio REAL: la forma de onda muestreada del PCM, no un FFT. Por cada
//! hop (~86 Hz) se reduce la ventana deslizante (2048 muestras ≈ 46 ms) a
//! `WAVEFORM_BUCKETS` pares min/max. El UI decima esos buckets al ancho de
//! terminal sin re-rankear y pinta el trazo/tinte vertical columna a columna.

use std::sync::{Arc, RwLock};

/// Nº de buckets (pares min/max). Fijo para que el consumidor decime sin
/// realojar: 128 cubren pantallas típicas de extremo a extremo.
pub const WAVEFORM_BUCKETS: usize = 128;

/// Envolvente de UNA ventana (valores de amplitud sin normalizar, ~-1..1).
#[derive(Debug, Clone, PartialEq)]
pub struct WaveformEnvelope {
    /// Mínimo (valle) por bucket.
    pub min: [f32; WAVEFORM_BUCKETS],
    /// Máximo (pico) por bucket.
    pub max: [f32; WAVEFORM_BUCKETS],
}

impl WaveformEnvelope {
    /// Reduce la ventana mono `window` a `WAVEFORM_BUCKETS` pares min/max.
    ///
    /// Cada bucket absorbe un tramo consecutivo: `len/128` muestras, con las
    /// primeras `len%128` tomando una extra (distribución uniforme, sin
    /// superponer ventanas).
    pub fn from_window(window: &[f32]) -> Self {
        let mut min = [0.0f32; WAVEFORM_BUCKETS];
        let mut max = [0.0f32; WAVEFORM_BUCKETS];
        if window.is_empty() {
            return Self { min, max };
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
        Self { min, max }
    }

    /// Envolvente plana para cortes/arranques (trazo en línea base).
    pub fn silent() -> Self {
        Self {
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

/// Bus de publicación/lectura del último snapshot de envolvente.
///
/// Mismo patrón que [`super::features::FeatureBus`]: escritor único (hilo de
/// análisis) ~86 Hz, lectores múltiples a ≤15 Hz clonando el `Arc` bajo lock
/// de lectura — contención despreciable y cero allocs en la ruta del audio.
#[derive(Clone)]
pub struct WaveformBus {
    slot: Arc<RwLock<Option<Arc<WaveformEnvelope>>>>,
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
    pub fn publish(&self, envelope: WaveformEnvelope) -> Arc<WaveformEnvelope> {
        let arc = Arc::new(envelope);
        *self.slot.write().unwrap() = Some(Arc::clone(&arc));
        arc
    }

    /// Última envolvente publicada (`None` hasta que el análisis arranque).
    pub fn latest(&self) -> Option<Arc<WaveformEnvelope>> {
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

        let loud = WaveformEnvelope::from_window(&[0.8; 2048]);
        let got = bus.publish(loud);
        assert_eq!(bus.latest().unwrap().peak(), 0.8);
        assert_eq!(Arc::strong_count(&got), 2, "bus + lector local");

        bus.publish(WaveformEnvelope::silent());
        assert_eq!(bus.latest().unwrap().peak(), 0.0);
        assert_eq!(got.peak(), 0.8, "el snapshot viejo sigue inmutable");
    }
}
