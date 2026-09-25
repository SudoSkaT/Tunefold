//! Adaptador de **YouTube / YouTube Music** para las capas catálogo y media.
//!
//! Este módulo es la ÚNICA frontera entre Tunefold y YouTube: implementa
//! [`StreamProvider`], [`StreamValidator`] y [`CatalogProvider`] delegando en
//! el cliente de [`provider`] (rustypipe) con mapeo de [`mapper`]. Si YouTube
//! deja de funcionar, se apaga aquí — el resto del sistema ni se entera.
//!
//! Mapeo de responsabilidades:
//!
//! | Trait             | Delegación                                             |
//! |-------------------|--------------------------------------------------------|
//! | `CatalogProvider` | búsqueda/detalles/letras/miniaturas (metadata pura)    |
//! | `StreamProvider`  | resolución → [`StreamResolution`] con vigencia honesta |
//! | `StreamValidator` | sondeo en vivo de URIs cacheadas + reparación cabeceras|

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;

pub mod link;
pub mod mapper;
pub mod provider;

pub use provider::{
    classify_rp_error, context_headers, CategorizedFail, YoutubeOptions, YoutubeProvider,
};

use crate::catalog::CatalogProvider;
use crate::domain::{
    album::Album, artist::Artist, source::Source, stream::StreamResolution, track::Track,
};
use crate::media::failure::{FailureCategory, ResolutionError};
use crate::media::provider::{ResolveContext, StreamProvider};
use crate::media::resolver::StreamValidator;

/// TTL de vigencia que declara el adaptador en cada resolución: el TTL
/// conservador del propio cliente ([`provider::STREAM_CACHE_TTL`] vía las
/// constantes internas). Las URLs de googlevideo viven horas; aquí se prefiere
/// el límite prudente y la verificación en vivo hace el resto.
fn resolution_ttl() -> chrono::Duration {
    chrono::Duration::from_std(Duration::from_secs(20 * 60)).unwrap_or(chrono::Duration::MAX)
}

/// Adaptador completo de YouTube: un solo tipo que sirve catálogo, streaming
/// y verificación sobre UNA instancia compartida del cliente.
///
/// Clonable de forma barata (`Arc` interno) para que el punto de composición
/// pueda registrar el MISMO cliente como catálogo, stream provider y
/// validador.
#[derive(Clone)]
pub struct YouTubeAdapter {
    inner: Arc<YoutubeProvider>,
}

impl std::fmt::Debug for YouTubeAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("YouTubeAdapter").finish_non_exhaustive()
    }
}

impl YouTubeAdapter {
    /// Crea el adaptador con su propia instancia de cliente.
    pub fn new() -> Self {
        Self::from_inner(Arc::new(YoutubeProvider::new()))
    }

    /// Crea el adaptador compartiendo una instancia ya existente (composición
    /// con un solo cliente: una sola caché de disco, un solo pool visitor).
    pub fn from_inner(inner: Arc<YoutubeProvider>) -> Self {
        Self { inner }
    }

    /// Cliente subyacente (herramientas de desarrollo/probes).
    pub fn inner(&self) -> &Arc<YoutubeProvider> {
        &self.inner
    }
}

impl Default for YouTubeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------- streaming

#[async_trait]
impl StreamProvider for YouTubeAdapter {
    fn id(&self) -> &'static str {
        "youtube"
    }

    fn source(&self) -> Source {
        Source::YouTube
    }

    async fn resolve(
        &self,
        track: &Track,
        _ctx: &ResolveContext,
    ) -> Result<StreamResolution, ResolutionError> {
        match self.inner.resolve_audio_url(track).await {
            Ok(Some(url)) => {
                let mut resolution = StreamResolution::new(Source::YouTube, url);
                resolution.headers = context_headers();
                // Vigencia honesta conocida: TTL conservador del cliente; la
                // verificación en vivo cubre muertes anticipadas.
                resolution.expires_at = Utc::now().checked_add_signed(resolution_ttl());
                Ok(resolution)
            }
            // El player respondió pero no hay audio utilizable.
            Ok(None) => Err(ResolutionError::new(
                FailureCategory::Unsupported,
                Source::YouTube,
                format!("{} no ofrece stream de audio", track.identifier()),
            )),
            Err(fail) => Err(ResolutionError::new(
                fail.category,
                Source::YouTube,
                fail.message,
            )),
        }
    }
}

#[async_trait]
impl StreamValidator for YouTubeAdapter {
    /// Verificación en vivo de una URI cacheada: GET sondeo por rangos con el
    /// contexto que exige el CDN. Repara en sitio las cabeceras de contexto
    /// (las entradas frías de SQLite llegan sin ellas).
    async fn check(&self, resolution: &mut StreamResolution) -> bool {
        if !self.inner.verify_audio_url(&resolution.uri).await {
            return false;
        }
        let headers = context_headers();
        if !headers.is_empty() {
            resolution.headers = headers;
        }
        true
    }
}

// -------------------------------------------------------------- catálogo

#[async_trait]
impl CatalogProvider for YouTubeAdapter {
    fn source(&self) -> Source {
        Source::YouTube
    }

    async fn search_tracks(
        &self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<Track>, crate::catalog::CatalogError> {
        self.inner.search_tracks(query, limit).await
    }

    async fn search_artists(
        &self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<Artist>, crate::catalog::CatalogError> {
        self.inner.search_artists(query, limit).await
    }

    async fn search_albums(
        &self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<Album>, crate::catalog::CatalogError> {
        self.inner.search_albums(query, limit).await
    }

    async fn get_track(&self, external_id: &str) -> Result<Track, crate::catalog::CatalogError> {
        self.inner.get_track(external_id).await
    }

    async fn get_artist(&self, external_id: &str) -> Result<Artist, crate::catalog::CatalogError> {
        self.inner.get_artist(external_id).await
    }

    async fn get_album(&self, external_id: &str) -> Result<Album, crate::catalog::CatalogError> {
        self.inner.get_album(external_id).await
    }

    async fn get_album_tracks(
        &self,
        external_id: &str,
    ) -> Result<Vec<Track>, crate::catalog::CatalogError> {
        self.inner.get_album_tracks(external_id).await
    }

    async fn related(&self, video_id: &str) -> Result<Vec<Track>, crate::catalog::CatalogError> {
        self.inner.related(video_id).await
    }

    async fn synced_lyrics(
        &self,
        track: &Track,
    ) -> Result<Option<String>, crate::catalog::CatalogError> {
        self.inner.synced_lyrics(track).await
    }

    fn thumbnail_candidates(&self, track: &Track) -> Vec<String> {
        self.inner.thumbnail_candidates(track)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::failure::FailureCategory as C;

    /// El clasificador fino usa los tipos REALES de rustypipe (vendored).
    #[test]
    fn rustypipe_errors_map_precisely() {
        use rustypipe::error::{
            Error as Rp, ExtractionError as Ex, UnavailabilityReason as Reason,
        };

        // Red pura vs timeout detectado en el mensaje del cliente HTTP.
        assert_eq!(
            classify_rp_error(&Rp::Http("connection reset".into())),
            C::NetworkFailure
        );
        assert_eq!(
            classify_rp_error(&Rp::Http("operation timed out".into())),
            C::Timeout
        );

        // Status codes explícitos.
        let status = |c| classify_rp_error(&Rp::HttpStatus(c, "msg".into()));
        assert_eq!(status(403), C::AuthenticationRequired);
        assert_eq!(status(404), C::Unsupported);
        assert_eq!(status(429), C::RateLimited);
        assert_eq!(status(503), C::ProviderUnavailable);
        assert_eq!(status(418), C::Unknown);

        // Auth de Innertube (consent/PO token rechazado).
        assert_eq!(
            classify_rp_error(&Rp::Auth(rustypipe::error::AuthError::NoLogin)),
            C::AuthenticationRequired
        );

        // Extracción: contenido no disponible ≠ protocolo roto.
        let unavail = Ex::Unavailable {
            reason: Reason::AgeRestricted,
            msg: "sign in to confirm".into(),
        };
        assert_eq!(classify_rp_error(&Rp::Extraction(unavail)), C::Unsupported);
        let not_found = Ex::NotFound {
            id: "abc".into(),
            msg: "no existe".into(),
        };
        assert_eq!(
            classify_rp_error(&Rp::Extraction(not_found)),
            C::Unsupported
        );

        // Datos inválidos / firma no deobfuscable / botguard caído.
        assert_eq!(
            classify_rp_error(&Rp::Extraction(Ex::InvalidData("json raro".into()))),
            C::InvalidResponse
        );
        assert_eq!(
            classify_rp_error(&Rp::Extraction(Ex::Deobfuscation("player.js".into()))),
            C::InvalidResponse
        );
        assert_eq!(
            classify_rp_error(&Rp::Extraction(Ex::Botguard("bg caído".into()))),
            C::AuthenticationRequired
        );

        // Desconocido.
        assert_eq!(classify_rp_error(&Rp::Other("??".into())), C::Unknown);
    }

    /// La causa raíz viaja íntegra en el fallo clasificado (spec §8).
    #[test]
    fn categorized_fail_preserves_original_message() {
        use rustypipe::error::Error as Rp;
        let e = Rp::HttpStatus(403, "contexto inválido".into());
        let fail = CategorizedFail::from_rp(e);
        assert_eq!(fail.category, C::AuthenticationRequired);
        assert!(fail.message.contains("403"), "{}", fail.message);
        assert!(fail.to_string().contains("contexto"), "Display intacto");
    }
}
