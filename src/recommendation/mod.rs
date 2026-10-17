//! Sistema de recomendaciones local (FASE 8–10).
//!
//! Pipeline: candidatos → scoring (metadata + acoustic + affinity + recency + popularity - negative) → ranking.

pub mod acoustic_aggregator;
pub mod greeting;
pub mod profile;
pub mod ranker;
pub mod scoring;
pub mod session;
pub mod signals;
pub mod types;

pub use session::RecommendationSession;

#[cfg(test)]
pub mod behavior_tests;

pub use ranker::{rank, rank_sync};
pub use scoring::{
    acoustic_similarity, harmonic_similarity, harmonic_similarity_to_profile, metadata_similarity,
    negative_penalty, popularity_factor, recency_bonus, structure_similarity, user_affinity,
};
pub use signals::{
    aggregate_signals, is_meaningful_negative, signal_weight, PlayContext, PlaySignal, SignalKind,
};
pub use types::{
    AcousticProfile, Candidate, FeatureVector, RecommendationScore, ScoreComponents,
    TrackAcousticProfile, TrackSignals, UserProfile,
};
