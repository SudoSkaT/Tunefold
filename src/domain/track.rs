//! Modelo de dominio: Track (canción).

use std::time::Duration;

use crate::domain::{album::Album, artist::Artist, genre::Genre, source::Source};

/// Recurso de imagen asociado a una canción (portada / miniatura).
///
/// Es agnóstico del proveedor: solo transporta la URL. La generación de URLs
/// específicas (p. ej. `i.ytimg.com` vía `video_id`) vive en la capa del
/// proveedor, nunca en el modelo de dominio ni en la interfaz.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Thumbnail {
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Track {
    pub id: i64,
    pub title: String,
    pub artists: Vec<Artist>,
    pub album: Option<Album>,
    pub duration: Option<Duration>,
    pub genres: Vec<Genre>,
    /// Plataforma donde se encontró / se reproducirá este resultado.
    pub source: Source,
    /// ID externo en la plataforma (p. ej. el `video_id` de YouTube).
    pub external_id: Option<String>,
    /// ISRC de la grabación, cuando el proveedor lo expone.
    pub isrc: Option<String>,
    /// URL de reproducción o de la página del track.
    pub url: Option<String>,
    /// Miniatura/portada asociada al track (URL, resuelta por el proveedor).
    pub thumbnail: Option<Thumbnail>,
}

impl Track {
    /// Constructor sin id de base de datos (para resultados provenientes de API).
    pub fn new(title: String, artists: Vec<Artist>, source: Source) -> Self {
        Self {
            id: 0,
            title,
            artists,
            album: None,
            duration: None,
            genres: Vec::new(),
            source,
            external_id: None,
            isrc: None,
            url: None,
            thumbnail: None,
        }
    }

    /// Nombre principal del artista, útil para listados y búsqueda.
    pub fn primary_artist_name(&self) -> Option<&str> {
        self.artists.first().map(|a| a.name.as_str())
    }

    /// Representación corta "Artista - Título".
    pub fn display_title(&self) -> String {
        match self.primary_artist_name() {
            Some(name) => format!("{name} - {}", self.title),
            None => self.title.clone(),
        }
    }

    /// Identificador estable para comparar copias del mismo track
    /// (recomendaciones, autoplay). Usa el id externo si existe y no está
    /// vacío; si no, una firma de título + primer artista, y como último
    /// recurso `desconocido:<id>` para no producir claves vacías.
    pub fn identifier(&self) -> String {
        if let Some(external_id) = self
            .external_id
            .as_deref()
            .map(str::trim)
            .filter(|external_id| !external_id.is_empty())
        {
            return external_id.to_string();
        }

        let title = self.title.trim();
        let artist = self.primary_artist_name().unwrap_or("").trim();
        if title.is_empty() && artist.is_empty() {
            format!("desconocido:{}", self.id)
        } else {
            format!("{title}|{artist}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::artist::Artist;

    #[test]
    fn identifier_rejects_an_empty_external_id_and_falls_back_to_title() {
        let mut track = Track::new(
            "Canción".to_string(),
            vec![Artist::new("Artista".to_string(), None, None, None)],
            Source::YouTube,
        );
        track.external_id = Some("   ".to_string());

        assert_eq!(track.identifier(), "Canción|Artista");
    }

    #[test]
    fn identifier_without_any_metadata_is_not_empty() {
        let track = Track::new(String::new(), Vec::new(), Source::YouTube);
        assert_eq!(track.identifier(), "desconocido:0");
    }

    #[test]
    fn identifier_keeps_the_provider_external_id() {
        let mut track = Track::new(String::new(), Vec::new(), Source::YouTube);
        track.external_id = Some("abc123".to_string());
        assert_eq!(track.identifier(), "abc123");
    }
}
