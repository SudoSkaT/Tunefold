//! Modelos de features: RAW → SMOOTHED ([`AudioFeatures`]) y el bus de
//! publicación para consumidores (visualización Fase 7, métricas).
//!
//! [`AudioFeatures`] es el snapshot INMUTABLE del estado musical de un frame
//! (spec §21): todos los campos continuos están normalizados 0..1 salvo `bpm`
//! (0 = desconocido). `timestamp` marca tiempo de STREAM procesado — el reloj
//! musical real sigue siendo el PositionClock del playback.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use super::bands::BandRatios;

/// Features crudas de UN frame (salida directa del DSP, sin suavizar).
#[derive(Debug, Clone, Copy, Default)]
pub struct RawFeatures {
    pub timestamp: Duration,
    pub rms: f32,
    pub amplitude: f32,
    pub bands: BandRatios,
    /// Centroid espectral normalizado por Nyquist (0..1).
    pub centroid_norm: f32,
    pub flux: f32,
    /// Cromática del frame (12 clases de pica, suma 1; 0 sin contenido).
    pub chroma: [f32; 12],
}

/// Snapshot suavizado publicado a los consumidores.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioFeatures {
    pub timestamp: Duration,
    pub rms: f32,
    /// Amplitud pico normalizada.
    pub amplitude: f32,
    pub bass: f32,
    pub low_mid: f32,
    pub mid: f32,
    pub high_mid: f32,
    pub high: f32,
    /// Centroide espectral 0..1 (0=graves, 1=agudos).
    pub spectral_centroid: f32,
    /// Flujo espectral suavizado 0..1.
    pub spectral_flux: f32,
    /// Fuerza del onset en ESTE frame (pico crudo, sin retardo).
    pub onset: f32,
    /// Pulso de beat discreto en ESTE frame.
    pub beat: bool,
    /// Confianza del tempo estimado 0..1.
    pub beat_confidence: f32,
    /// BPM estimado (0 = desconocido; se mantiene el último estable).
    pub bpm: f32,
    /// Cromática del frame: 12 clases de pica normalizadas a suma 1
    /// (0 si no hay contenido en el rango musical). CRUDA, sin suavizar:
    /// el agregador acústico la promedia por track y el detector de cambios
    /// armónicos necesita el valor por frame.
    pub chroma: [f32; 12],
}

impl AudioFeatures {
    /// Frame silencioso con timestamp dado (arranques/cortes).
    pub fn silent(timestamp: Duration) -> Self {
        Self {
            timestamp,
            rms: 0.0,
            amplitude: 0.0,
            bass: 0.0,
            low_mid: 0.0,
            mid: 0.0,
            high_mid: 0.0,
            high: 0.0,
            spectral_centroid: 0.0,
            spectral_flux: 0.0,
            onset: 0.0,
            beat: false,
            beat_confidence: 0.0,
            bpm: 0.0,
            chroma: [0.0; 12],
        }
    }
}

/// Bus de publicación/lectura del último snapshot.
///
/// Escritor único (hilo de análisis) ~90 Hz; lectores múltiples (UI/render)
/// a ≤30 Hz clonando el `Arc` bajo lock de lectura — contención despreciable
/// y cero allocations en la ruta del audio.
#[derive(Clone)]
pub struct FeatureBus {
    slot: Arc<RwLock<Option<Arc<AudioFeatures>>>>,
}

impl Default for FeatureBus {
    fn default() -> Self {
        Self::new()
    }
}

impl FeatureBus {
    pub fn new() -> Self {
        Self {
            slot: Arc::new(RwLock::new(None)),
        }
    }

    /// Publica un nuevo snapshot como último disponible.
    pub fn publish(&self, features: AudioFeatures) -> Arc<AudioFeatures> {
        let arc = Arc::new(features);
        *self.slot.write().unwrap() = Some(Arc::clone(&arc));
        arc
    }

    /// Último snapshot publicado (`None` hasta que el análisis arranque).
    pub fn latest(&self) -> Option<Arc<AudioFeatures>> {
        self.slot.read().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bus_publishes_and_serves_latest_snapshot() {
        let bus = FeatureBus::new();
        assert!(bus.latest().is_none(), "sin publicar aún");

        let first = bus.publish(AudioFeatures::silent(Duration::from_millis(10)));
        assert_eq!(bus.latest().unwrap().timestamp, Duration::from_millis(10));
        assert_eq!(Arc::strong_count(&first), 2, "bus + lector local");

        // Un segundo publish reemplaza al anterior (los lectores viejos
        // conservan SU snapshot: inmutabilidad compartida).
        bus.publish(AudioFeatures::silent(Duration::from_millis(20)));
        assert_eq!(bus.latest().unwrap().timestamp, Duration::from_millis(20));
        assert_eq!(first.timestamp, Duration::from_millis(10));
    }

    #[test]
    fn bus_is_clonable_and_shared() {
        let bus = FeatureBus::new();
        let clone = bus.clone();
        clone.publish(AudioFeatures::silent(Duration::ZERO));
        assert!(bus.latest().is_some(), "clon comparte el mismo slot");
    }

    #[test]
    fn silent_features_are_all_zero_except_timestamp() {
        let f = AudioFeatures::silent(Duration::from_secs(3));
        assert_eq!(f.timestamp, Duration::from_secs(3));
        for v in [f.rms, f.amplitude, f.bass, f.high, f.spectral_centroid] {
            assert_eq!(v, 0.0);
        }
        assert!(!f.beat);
        assert_eq!(f.bpm, 0.0);
    }
}
