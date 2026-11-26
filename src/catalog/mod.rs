//! Capa de Catálogo: contratos de metadata independientes del proveedor.
//!
//! [`CatalogProvider`] es la abstracción de fuente de metadatos (búsqueda,
//! fichas, letras, miniaturas). NO incluye streaming: un proveedor puede ser
//! solo catálogo, solo streaming ([`crate::media::StreamProvider`]) o ambos;
//! esa decisión vive en cada adaptador concreto, nunca en el contrato.
//!
//! Los modelos de APIs externas y los clientes concretos jamás se referencian
//! desde aquí ni desde los consumidores del catálogo.

use async_trait::async_trait;

use crate::domain::{
    album::Album, artist::Artist, external_playlist::ExternalPlaylist, source::Source, track::Track,
};

/// Error unificado devuelto por cualquier proveedor de metadatos.
#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("error de red: {0}")]
    Http(#[from] reqwest::Error),
    #[error("{provider} respondió con estado {status}")]
    Api { provider: String, status: u16 },
    #[error("no se encontró «{query}» en {provider}")]
    NotFound { provider: String, query: String },
    #[error("respuesta inválida de {provider}: {message}")]
    Invalid { provider: String, message: String },
    #[error("configuración incompleta: {0}")]
    Config(String),
    #[error("{0}")]
    Other(String),
}

/// Contrato de una fuente de metadatos.
///
/// Responsabilidades: buscar y describir contenido (metadata, artwork,
/// identificadores, duración). NUNCA controla reproducción ni resuelve
/// streams.
#[async_trait]
pub trait CatalogProvider: std::fmt::Debug + Send + Sync {
    /// Origen de los datos que produce este proveedor (clave para el
    /// agregador, la BD y el enrutado de streams).
    fn source(&self) -> Source;

    async fn search_tracks(&self, query: &str, limit: u32) -> Result<Vec<Track>, CatalogError>;
    async fn search_artists(&self, query: &str, limit: u32) -> Result<Vec<Artist>, CatalogError>;
    async fn search_albums(&self, query: &str, limit: u32) -> Result<Vec<Album>, CatalogError>;

    async fn get_track(&self, external_id: &str) -> Result<Track, CatalogError>;
    async fn get_artist(&self, external_id: &str) -> Result<Artist, CatalogError>;
    async fn get_album(&self, external_id: &str) -> Result<Album, CatalogError>;

    /// Contenido de un artista: perfil, canciones y álbumes.
    async fn artist_content(
        &self,
        external_id: &str,
    ) -> Result<(Artist, Vec<Track>, Vec<Album>), CatalogError> {
        let _ = external_id;
        Err(CatalogError::Other(
            "artist content not supported".to_string(),
        ))
    }
    async fn get_album_tracks(&self, external_id: &str) -> Result<Vec<Track>, CatalogError>;

    /// External playlist metadata and entries, if the provider supports them.
    async fn playlist(&self, external_id: &str) -> Result<ExternalPlaylist, CatalogError> {
        let _ = external_id;
        Err(CatalogError::Other("playlist not supported".to_string()))
    }

    /// Recomendados de un video. `Ok(vec![])` si la fuente no los expone.
    async fn related(&self, _video_id: &str) -> Result<Vec<Track>, CatalogError> {
        Ok(Vec::new())
    }

    /// Letra sincronizada (formato LRC) del track, si la fuente la tiene.
    ///
    /// Es la única fuente del modo karaoke; una implementación por defecto
    /// devuelve `Ok(None)` cuando la fuente no ofrece LRC. La letra plana no es
    /// una fuente legítima del karaoke.
    async fn synced_lyrics(&self, _track: &Track) -> Result<Option<String>, CatalogError> {
        Ok(None)
    }

    /// URLs candidatas de miniatura para un track, en orden de preferencia.
    ///
    /// El servicio de thumbnails prueba cada una en orden hasta conseguir una
    /// que se pueda descargar y decodificar. La implementación por defecto usa
    /// la miniatura que el propio proveedor adjuntó al track; un proveedor
    /// concreto (p. ej. YouTube) puede reconstruir URLs específicas a partir de
    /// sus identificadores y añadir una cadena de fallback.
    fn thumbnail_candidates(&self, track: &Track) -> Vec<String> {
        track
            .thumbnail
            .as_ref()
            .map(|t| vec![t.url.clone()])
            .unwrap_or_default()
    }
}

/// Registro de proveedores de catálogo activos, consultado por el agregador.
///
/// La aplicación no construye proveedores ad hoc desde la UI: pasan por este
/// registro (`api::build_providers` es el punto de composición).
#[derive(Debug, Default)]
pub struct CatalogRegistry {
    providers: Vec<Box<dyn CatalogProvider>>,
}

impl CatalogRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, provider: Box<dyn CatalogProvider>) {
        self.providers.push(provider);
    }

    pub fn providers(&self) -> &[Box<dyn CatalogProvider>] {
        &self.providers
    }

    pub fn get(&self, source: Source) -> Option<&dyn CatalogProvider> {
        self.providers
            .iter()
            .find(|p| p.source() == source)
            .map(|p| p.as_ref())
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}
