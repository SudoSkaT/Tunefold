//! Pipeline de generación de candidatos → scoring → ranking.
//!
//! Es el ÚNICO camino de scoring del sistema (la TUI lo invoca vía
//! `reorder_by_local_taste` y Android vía JNI — ver `Documentacion/docs/recommendations.md`).
//! Los candidatos se comparan por el SONIDO real (timbre, armonía, ritmo
//! armónico, estructura) y por la relación con el usuario; la popularidad es
//! solo un desempate residual.

use std::collections::HashMap;

use crate::domain::track::Track;
use crate::infrastructure::storage::TrackListeningStats;
use crate::recommendation::scoring::acoustic::acoustic_similarity_to_profile;
use crate::recommendation::scoring::harmonic::{
    harmonic_similarity, harmonic_similarity_to_profile, structure_similarity,
};
use crate::recommendation::scoring::recency::days_since;
use crate::recommendation::types::{
    RecommendationScore, ScoreComponents, TrackAcousticProfile, TrackSignals, UserProfile,
};
use crate::recommendation::{
    metadata_similarity, negative_penalty, popularity_factor, recency_bonus, user_affinity,
};

/// Pesos del pipeline (suman 1.0).
///
/// Justificación (rediseño v0.20 — ver `Documentacion/docs/recommendations.md`):
/// - `acoustic` (0.30): similitud de TIMBRE/ENERGÍA (bandas, centroid, BPM,
///   onset) contra el gusto del usuario y la pista que suena ahora (semilla).
/// - `harmonic` (0.20): cromática, tonalidad y tasa de cambios armónicos —
///   el "acorde" del usuario y de la semilla. NUEVO: es lo que desplaza a la
///   popularidad como criterio principal.
/// - `affinity` (0.20): relación DIRECTA del usuario con el track (plays
///   recientes × frecuencia + refuerzo acústico).
/// - `structure` (0.10): firma de 8 sectores temporales vs la semilla — la
///   arquitectura de la pista (intro/build/estribillo proxy). NUEVO.
/// - `meta` (0.10): artista/género/álbum/década; baja de 0.25 porque la
///   metadata del provider está casi vacía y no debe dominar.
/// - `recency` (0.05) y `popularity` (0.05): señales débiles de contexto;
///   popularidad (play-count local) CONSCIENTEMENTE residual.
///
/// El SONIDO (acoustic + harmonic + structure) suma 0.60 — es el eje del
/// ranking; la popularidad local, 0.05.
pub const WEIGHTS: ScoringWeights = ScoringWeights {
    meta: 0.10,
    acoustic: 0.30,
    harmonic: 0.20,
    structure: 0.10,
    affinity: 0.20,
    recency: 0.05,
    popularity: 0.05,
};

#[derive(Debug, Clone, Copy)]
pub struct ScoringWeights {
    pub meta: f64,
    pub acoustic: f64,
    pub harmonic: f64,
    pub structure: f64,
    pub affinity: f64,
    pub recency: f64,
    pub popularity: f64,
}

impl ScoringWeights {
    #[allow(clippy::too_many_arguments)]
    pub fn weighted_sum(
        &self,
        meta: f64,
        acoustic: f64,
        harmonic: f64,
        structure: f64,
        affinity: f64,
        recency: f64,
        popularity: f64,
    ) -> f64 {
        self.meta * meta
            + self.acoustic * acoustic
            + self.harmonic * harmonic
            + self.structure * structure
            + self.affinity * affinity
            + self.recency * recency
            + self.popularity * popularity
    }
}

/// Blend de dos similitudes opcionales (0.6 usuario / 0.4 semilla): si solo
/// existe una fuente, se usa tal cual; si ninguna, 0.
fn blend_user_seed(user: Option<f64>, seed: Option<f64>) -> f64 {
    match (user, seed) {
        (Some(u), Some(s)) => 0.6 * u + 0.4 * s,
        (Some(u), None) => u,
        (None, Some(s)) => s,
        (None, None) => 0.0,
    }
}

/// Genera recomendaciones para un usuario a partir de un catálogo.
///
/// `signals`: agregados por track (plays / negativos) para la penalización
/// negativa basada en señales reales, no en play-count.
/// `seed`: perfil acústico de la pista de la que cuelgan las recomendaciones
/// ("relacionadas con X"): permite similitud acústica/armónica/estructural
/// candidato↔semilla incluso con perfil de usuario escaso.
pub async fn rank(
    candidates: &[Track],
    profile: &UserProfile,
    history: &[TrackListeningStats],
    acoustic_profiles: &HashMap<i64, TrackAcousticProfile>,
    signals: &HashMap<i64, TrackSignals>,
    seed: Option<&TrackAcousticProfile>,
) -> Vec<RecommendationScore> {
    rank_sync(
        candidates,
        profile,
        history,
        acoustic_profiles,
        signals,
        seed,
    )
}

/// Núcleo síncrono de [`rank`] (no hay I/O: existe para que el ranking
/// compartido por JNI en Android pueda llamarlo sin bloquear sobre un
/// runtime tokio — ver `platform::android_rank`).
pub fn rank_sync(
    candidates: &[Track],
    profile: &UserProfile,
    history: &[TrackListeningStats],
    acoustic_profiles: &HashMap<i64, TrackAcousticProfile>,
    signals: &HashMap<i64, TrackSignals>,
    seed: Option<&TrackAcousticProfile>,
) -> Vec<RecommendationScore> {
    let max_play_count = history.iter().map(|h| h.play_count).max().unwrap_or(1);
    let user_has_chroma = profile.acoustic_profile.chroma.iter().sum::<f32>() > f32::EPSILON;

    let mut scores: Vec<RecommendationScore> = Vec::new();

    for track in candidates {
        let meta = metadata_similarity(track, profile);
        let track_profile = acoustic_profiles.get(&track.id);

        // Timbre/energía: al usuario (si hay perfil) y a la semilla (si la hay).
        let acoustic = blend_user_seed(
            track_profile
                .filter(|_| profile.acoustic_profile.weight_sum > f32::EPSILON)
                .map(|p| acoustic_similarity_to_profile(p, &profile.acoustic_profile)),
            seed.map(|s| {
                track_profile.map_or(0.0, |p| {
                    crate::recommendation::scoring::acoustic::acoustic_similarity(p, s)
                })
            }),
        );

        // Armonía: cromática + tonalidad + ritmo de cambios, al usuario y a la
        // semilla con el mismo blend.
        let harmonic = blend_user_seed(
            track_profile
                .filter(|_| user_has_chroma)
                .map(|p| harmonic_similarity_to_profile(p, &profile.acoustic_profile)),
            seed.map(|s| track_profile.map_or(0.0, |p| harmonic_similarity(p, s))),
        );

        // Estructura: solo tiene sentido contra la semilla (la del usuario no
        // es "una" estructura). Sin semilla el componente no aporta (0).
        let structure = match (seed, track_profile) {
            (Some(s), Some(p)) => structure_similarity(p, s),
            _ => 0.0,
        };

        let affinity = user_affinity(track, history, &profile.acoustic_profile, acoustic_profiles);

        let track_history: Vec<&TrackListeningStats> =
            history.iter().filter(|h| h.track_id == track.id).collect();
        let recency = track_history
            .iter()
            .map(|h| {
                let days = days_since(&h.last_played);
                recency_bonus(days)
            })
            .sum::<f64>()
            / track_history.len().max(1) as f64;

        let popularity = popularity_factor(
            track_history.iter().map(|h| h.play_count).sum::<i64>(),
            max_play_count,
        );
        // Señales negativas REALES (skips detectados / unlikes) sobre intentos.
        let sig = signals.get(&track.id).copied().unwrap_or_default();
        let negative = negative_penalty(sig.negative, sig.plays);

        let raw = WEIGHTS.weighted_sum(
            meta, acoustic, harmonic, structure, affinity, recency, popularity,
        );
        let final_score = raw * negative;

        scores.push(RecommendationScore {
            track_id: track.id,
            final_score,
            components: ScoreComponents {
                metadata: meta,
                acoustic,
                harmonic,
                structure,
                affinity,
                recency,
                popularity,
                negative,
            },
        });
    }

    // Ordenación ESTABLE: los empates (p. ej. sin ningún dato) conservan el
    // orden del provider — el descubrimiento nunca se rompe por azar.
    scores.sort_by(|a, b| {
        b.final_score
            .partial_cmp(&a.final_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scores
}

/// Genera un vector de features para un track a partir de su perfil acústico.
pub fn track_to_feature_vector(
    track_id: i64,
    profiles: &HashMap<i64, TrackAcousticProfile>,
) -> Option<crate::recommendation::types::FeatureVector> {
    profiles
        .get(&track_id)
        .map(crate::recommendation::types::FeatureVector::from_profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_sum_to_one() {
        let w = WEIGHTS;
        let total =
            w.meta + w.acoustic + w.harmonic + w.structure + w.affinity + w.recency + w.popularity;
        assert!(
            (total - 1.0).abs() < 1e-10,
            "pesos deben sumar 1.0: {total}"
        );
    }

    #[test]
    fn blend_prefers_user_when_both_present() {
        assert!((blend_user_seed(Some(1.0), Some(0.0)) - 0.6).abs() < 1e-9);
        assert!((blend_user_seed(Some(0.0), Some(1.0)) - 0.4).abs() < 1e-9);
        assert_eq!(blend_user_seed(Some(0.8), None), 0.8);
        assert_eq!(blend_user_seed(None, Some(0.7)), 0.7);
        assert_eq!(blend_user_seed(None, None), 0.0);
    }
}
