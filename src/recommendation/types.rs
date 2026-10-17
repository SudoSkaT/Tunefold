//! Tipos centrales del sistema de recomendaciones (FASE 9–10).
//!
//! `FeatureVector` — representación numérica de un track para comparación acústica.
//! `UserProfile` — resumen del gusto del usuario derivado del historial.
//! `Candidate` — track candidato con su perfil acústico para scoring.
//! `RecommendationScore` — puntuación final con componentes desglosados.
//! `ScoreComponents` — desglose de cada componente del scoring.

use std::collections::{HashMap, HashSet};

use crate::domain::source::Source;
use crate::domain::track::Track;

/// Vector de features acústicas de un track (promedio de frames, normalizado).
///
/// Se usa para calcular la distancia coseno entre tracks en `acoustic_similarity`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FeatureVector {
    pub rms: f32,
    pub bass: f32,
    pub low_mid: f32,
    pub mid: f32,
    pub high_mid: f32,
    pub high: f32,
    pub spectral_centroid: f32,
    pub bpm_norm: f32,
    pub onset: f32,
}

impl FeatureVector {
    /// Convierte un `TrackAcousticProfile` en un `FeatureVector` normalizado.
    pub fn from_profile(p: &TrackAcousticProfile) -> Self {
        Self {
            rms: p.rms_mean,
            bass: p.bass_mean,
            low_mid: p.low_mid_mean,
            mid: p.mid_mean,
            high_mid: p.high_mid_mean,
            high: p.high_mean,
            spectral_centroid: p.spectral_centroid_mean,
            bpm_norm: (p.bpm_mean / 200.0).clamp(0.0, 1.0),
            onset: p.onset_mean,
        }
    }

    /// Producto punto con otro vector (asumiendo normalizados).
    pub fn dot(&self, other: &Self) -> f32 {
        self.rms * other.rms
            + self.bass * other.bass
            + self.low_mid * other.low_mid
            + self.mid * other.mid
            + self.high_mid * other.high_mid
            + self.high * other.high
            + self.spectral_centroid * other.spectral_centroid
            + self.bpm_norm * other.bpm_norm
            + self.onset * other.onset
    }

    /// Magnitud del vector (asumiendo valores 0..1).
    pub fn magnitude(&self) -> f32 {
        (self.rms * self.rms
            + self.bass * self.bass
            + self.low_mid * self.low_mid
            + self.mid * self.mid
            + self.high_mid * self.high_mid
            + self.high * self.high
            + self.spectral_centroid * self.spectral_centroid
            + self.bpm_norm * self.bpm_norm
            + self.onset * self.onset)
            .sqrt()
    }
}

/// Buckets temporales de la firma estructural (sectores iguales del tramo
/// analizado: intro/build/… proxy de la arquitectura de la pista).
pub const STRUCT_BUCKETS: usize = 8;
/// Métricas por bucket de estructura: [flux, rms, centroid, onset].
pub const STRUCT_METRICS: usize = 4;
/// Longitud total de la firma estructural (`STRUCT_BUCKETS × STRUCT_METRICS`).
pub const STRUCT_LEN: usize = STRUCT_BUCKETS * STRUCT_METRICS;

/// Perfil acústico agregado de un track (promedio ponderado de frames).
///
/// `Serialize`/`Deserialize` porque el ranking compartido por JNI de Android
/// lo recibe y devuelve dentro del envelope JSON (`platform::android_rank`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TrackAcousticProfile {
    pub track_id: i64,
    pub rms_mean: f32,
    pub bass_mean: f32,
    pub low_mid_mean: f32,
    pub mid_mean: f32,
    pub high_mid_mean: f32,
    pub high_mean: f32,
    pub spectral_centroid_mean: f32,
    pub bpm_mean: f32,
    pub bpm_variance: f32,
    pub onset_mean: f32,
    pub band_profile: [f32; 5],
    pub frame_count: i64,
    /// Cromática media: 12 clases de pica normalizadas a suma 1.
    pub chroma_profile: [f32; 12],
    /// Tonalidad estimada: -1 desconocida; 0..11 mayor (C..B); 12..23 menor.
    pub key: i16,
    /// Confianza de la estimación de tonalidad 0..1.
    pub key_confidence: f32,
    /// Tasa de cambios armónicos sobre el tramo activo (cambios/s; 0 = sin
    /// datos suficientes o armonía estática).
    pub chord_change_rate: f32,
    /// Firma estructural: `STRUCT_BUCKETS` sectores × [flux, rms, centroid,
    /// onset] (0 si no hubo ventana conocida para bucketizar).
    pub structure_profile: [f32; STRUCT_LEN],
}

impl Default for TrackAcousticProfile {
    fn default() -> Self {
        Self {
            track_id: 0,
            rms_mean: 0.0,
            bass_mean: 0.0,
            low_mid_mean: 0.0,
            mid_mean: 0.0,
            high_mid_mean: 0.0,
            high_mean: 0.0,
            spectral_centroid_mean: 0.0,
            bpm_mean: 0.0,
            bpm_variance: 0.0,
            onset_mean: 0.0,
            band_profile: [0.0; 5],
            frame_count: 0,
            chroma_profile: [0.0; 12],
            key: -1,
            key_confidence: 0.0,
            chord_change_rate: 0.0,
            structure_profile: [0.0; STRUCT_LEN],
        }
    }
}

/// Candidato a recomendación con su perfil acústico para scoring.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub track: Track,
    pub acoustic_profile: Option<TrackAcousticProfile>,
}

impl Candidate {
    pub fn new(track: Track) -> Self {
        Self {
            acoustic_profile: None,
            track,
        }
    }

    pub fn with_acoustic(mut self, profile: TrackAcousticProfile) -> Self {
        self.acoustic_profile = Some(profile);
        self
    }
}

/// Score final de una recomendación con desglose de componentes.
#[derive(Debug, Clone)]
pub struct RecommendationScore {
    pub track_id: i64,
    pub final_score: f64,
    pub components: ScoreComponents,
}

/// Componentes individuales del scoring.
#[derive(Debug, Clone, Default)]
pub struct ScoreComponents {
    pub metadata: f64,
    pub acoustic: f64,
    pub harmonic: f64,
    pub structure: f64,
    pub affinity: f64,
    pub recency: f64,
    pub popularity: f64,
    pub negative: f64,
}

impl ScoreComponents {
    #[allow(clippy::too_many_arguments)]
    pub fn weighted_sum(
        &self,
        w_meta: f64,
        w_acoustic: f64,
        w_harmonic: f64,
        w_structure: f64,
        w_affinity: f64,
        w_recency: f64,
        w_popularity: f64,
    ) -> f64 {
        w_meta * self.metadata
            + w_acoustic * self.acoustic
            + w_harmonic * self.harmonic
            + w_structure * self.structure
            + w_affinity * self.affinity
            + w_recency * self.recency
            + w_popularity * self.popularity
    }
}

/// Perfil musical del usuario derivado de los eventos de interacción (FASE 10).
#[derive(Debug, Clone, Default)]
pub struct UserProfile {
    // ── Metadata (pesado por la calidad de la señal) ──
    pub favorite_artists: Vec<String>,
    pub favorite_genres: Vec<String>,
    pub favorite_albums: Vec<i64>,
    pub favorite_decades: Vec<i64>,
    pub favorite_tags: Vec<String>,

    // ── Features acústicos (promedio ponderado por el peso de la señal) ──
    pub acoustic_profile: AcousticProfile,

    // ── Historial de señales (diferenciadas, no mezcladas) ──
    pub total_plays: u64,
    pub total_completions: u64,
    pub total_skips: u64,
    pub total_replays: u64,
    pub total_likes: u64,
    /// Peso acumulado de todas las señales (para normalizar afinidades).
    pub total_weight: f32,
    pub tracks_played: HashSet<i64>,
    pub tracks_completed: HashSet<i64>,
    pub tracks_skipped: HashSet<i64>,
    pub tracks_replayed: HashSet<i64>,

    // Acumuladores internos para ordenar favoritos por peso.
    pub(crate) _artist_w: HashMap<String, f32>,
    pub(crate) _genre_w: HashMap<String, f32>,
    pub(crate) _album_w: HashMap<i64, f32>,
    pub(crate) _decade_w: HashMap<i64, f32>,
    pub(crate) _tag_w: HashMap<String, f32>,
}

#[derive(Debug, Clone, Default)]
pub struct AcousticProfile {
    pub rms: f32,
    pub bass: f32,
    pub low_mid: f32,
    pub mid: f32,
    pub high_mid: f32,
    pub high: f32,
    pub spectral_centroid: f32,
    pub bpm_mean: f32,
    pub bpm_variance: f32,
    pub onset_mean: f32,
    pub band_profile: [f32; 5],
    /// Tendencia armónica del usuario: media ponderada de las cromáticas.
    pub chroma: [f32; 12],
    pub weight_sum: f32,
}

impl AcousticProfile {
    pub fn add(&mut self, features: &TrackAcousticProfile, weight: f32) {
        self.rms += weight * features.rms_mean;
        self.bass += weight * features.bass_mean;
        self.low_mid += weight * features.low_mid_mean;
        self.mid += weight * features.mid_mean;
        self.high_mid += weight * features.high_mid_mean;
        self.high += weight * features.high_mean;
        self.spectral_centroid += weight * features.spectral_centroid_mean;
        self.bpm_mean += weight * features.bpm_mean;
        self.bpm_variance += weight * features.bpm_variance;
        self.onset_mean += weight * features.onset_mean;
        for i in 0..5 {
            self.band_profile[i] += weight * features.band_profile[i];
        }
        for i in 0..12 {
            self.chroma[i] += weight * features.chroma_profile[i];
        }
        self.weight_sum += weight;
    }

    pub fn finalize(&mut self) {
        let w = self.weight_sum.max(1.0);
        self.rms /= w;
        self.bass /= w;
        self.low_mid /= w;
        self.mid /= w;
        self.high_mid /= w;
        self.high /= w;
        self.spectral_centroid /= w;
        self.bpm_mean /= w;
        self.bpm_variance /= w;
        self.onset_mean /= w;
        for i in 0..5 {
            self.band_profile[i] /= w;
        }
        // La media ponderada de cromáticas normalizadas sigue normalizada
        // (cada una suma 1 ⇒ la combinación lineal también), pero se
        // re-normaliza por seguridad ante perfiles parciales/corruptos.
        let chroma_total: f32 = self.chroma.iter().map(|v| v.max(0.0)).sum();
        if chroma_total > f32::EPSILON {
            for v in self.chroma.iter_mut() {
                *v /= chroma_total;
            }
        }
    }

    pub fn to_vector(&self) -> FeatureVector {
        FeatureVector {
            rms: self.rms,
            bass: self.bass,
            low_mid: self.low_mid,
            mid: self.mid,
            high_mid: self.high_mid,
            high: self.high,
            spectral_centroid: self.spectral_centroid,
            bpm_norm: (self.bpm_mean / 200.0).clamp(0.0, 1.0),
            onset: self.onset_mean,
        }
    }
}

/// Evento de reproducción con contexto de duración del track para calcular completion/skip.
#[derive(Debug, Clone)]
pub struct HistoryPlayEvent {
    pub track_id: i64,
    pub played_at: String,
    pub source: Source,
    pub duration: Option<i64>,
    pub track_duration: Option<i64>,
    pub artist_name: Option<String>,
}

/// Agregado de señales por track para la penalización negativa (FASE 9/11).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TrackSignals {
    /// Intentos de reproducción (plays + rec_clicks) de este track.
    pub plays: i64,
    /// Señales negativas significativas (skips manuales + unlikes).
    pub negative: i64,
}
