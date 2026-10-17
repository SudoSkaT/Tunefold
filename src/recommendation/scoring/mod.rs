//! Componentes de scoring del sistema de recomendaciones.

pub mod acoustic;
pub mod affinity;
pub mod harmonic;
pub mod metadata;
pub mod negative;
pub mod popularity;
pub mod recency;

pub use acoustic::{acoustic_similarity, acoustic_similarity_to_profile};
pub use affinity::user_affinity;
pub use harmonic::{
    chord_rate_similarity, chroma_similarity, harmonic_similarity, harmonic_similarity_to_profile,
    key_similarity, structure_similarity,
};
pub use metadata::metadata_similarity;
pub use negative::negative_penalty;
pub use popularity::popularity_factor;
pub use recency::recency_bonus;
