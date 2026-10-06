//! Modelo de dominio: Artista.

use std::time::Duration;

/// Qué representa el nombre que un proveedor adjuntó al track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtistRole {
    #[default]
    Artist,
    Channel,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Artist {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub role: ArtistRole,
    pub country: Option<String>,
    pub biography: Option<String>,
    pub image: Option<String>,
    pub genres: Vec<String>,
    /// ID externo en la plataforma de origen (ej. MBID, channel id, user id).
    pub external_id: Option<String>,
    /// Duración agregada de todas sus obras conocidas (opcional, para la UI).
    pub total_duration: Option<Duration>,
}

impl Artist {
    /// Constructor sin id de base de datos (para resultados provenientes de API).
    pub fn new(
        name: String,
        country: Option<String>,
        biography: Option<String>,
        image: Option<String>,
    ) -> Self {
        Self {
            id: 0,
            name,
            role: ArtistRole::Artist,
            country,
            biography,
            image,
            genres: Vec::new(),
            external_id: None,
            total_duration: None,
        }
    }

    pub fn channel(name: String, external_id: Option<String>, image: Option<String>) -> Self {
        let mut channel = Self::new(name, None, None, image);
        channel.role = ArtistRole::Channel;
        channel.external_id = external_id;
        channel
    }
}
