//! Punto de COMPOSICIÓN de proveedores y del subsistema media.
//!
//! Este módulo no implementa nada de red ni conoce detalles de YouTube: su
//! única responsabilidad es CONSTRUIR los registros de catálogo y el
//! [`crate::media::StreamResolver`] con los adaptadores habilitados por los
//! [`crate::infrastructure::config::FeatureFlags`].
//!
//! Tras la Fase 5, TODO el código específico de YouTube vive en
//! `crate::providers::youtube`; ningún otro módulo lo referencia.
//!
//! El adaptador de YouTube solo se construye con la feature `youtube`
//! (excluida del binario oficial). Sin ella, catálogo y registros quedan
//! vacíos y la app degrada con errores estructurados.

use crate::catalog::CatalogRegistry;
use crate::media::{
    MemoryResolutionCache, ResolverConfig, StreamRegistry, StreamResolver, TwoTierCache,
};
use std::sync::Arc;

/// Conjunto compuesto: catálogo + resolvedor de streams listos para usarse.
pub struct ComposedMedia {
    pub catalog: CatalogRegistry,
    pub stream_resolver: Arc<StreamResolver>,
}

/// Monta el resolver con su caché a dos niveles y el validador opcional.
fn assemble(
    catalog: CatalogRegistry,
    stream_registry: Arc<StreamRegistry>,
    cache: Arc<TwoTierCache>,
    validator: Option<Arc<dyn crate::media::StreamValidator>>,
) -> ComposedMedia {
    let mut resolver = StreamResolver::new(stream_registry, cache, ResolverConfig::default());
    if let Some(v) = validator {
        resolver = resolver.with_validator(v);
    }
    ComposedMedia {
        catalog,
        stream_resolver: Arc::new(resolver),
    }
}

fn cache_for(db: crate::infrastructure::db::Db) -> Arc<TwoTierCache> {
    Arc::new(TwoTierCache::new(
        Arc::new(MemoryResolutionCache::default()),
        Arc::new(crate::infrastructure::storage::DbResolutionCache::new(db)),
    ))
}

/// Registra los proveedores de catálogo activos: únicamente YouTube.
///
/// Ruta legacy usada por el CLI (`main.rs`); la TUI pasa por
/// [`compose_media`], que comparte una sola instancia entre catálogo y
/// streaming.
#[cfg(feature = "youtube")]
pub fn build_providers() -> CatalogRegistry {
    let mut registry = CatalogRegistry::new();
    registry.register(Box::new(crate::providers::youtube::YouTubeAdapter::new()));
    registry
}

/// Sin la feature `youtube` el catálogo queda vacío.
#[cfg(not(feature = "youtube"))]
pub fn build_providers() -> CatalogRegistry {
    CatalogRegistry::new()
}

/// Punto de composición del subsistema media (única referencia a
/// `providers/*` permitida fuera del propio provider).
///
/// Construye UNA instancia del adaptador de YouTube compartida por catálogo,
/// streaming y verificación, la registra en el registry de streams y monta el
/// [`crate::media::StreamResolver`] con caché a dos niveles (memoria + SQLite).
///
/// Los [`crate::infrastructure::config::FeatureFlags`] deciden QUÉ se registra:
/// con `youtube_provider=false` (o sin la feature `youtube`) los registros
/// quedan VACÍOS y no se construye ni un cliente de YouTube — la app arranca
/// sana y las resoluciones devuelven el error estructurado "sin proveedores
/// disponibles" (spec §32).
#[cfg(feature = "youtube")]
pub fn compose_media(
    db: crate::infrastructure::db::Db,
    flags: &crate::infrastructure::config::FeatureFlags,
) -> ComposedMedia {
    use crate::providers::youtube::{YouTubeAdapter, YoutubeOptions, YoutubeProvider};

    // El adaptador solo existe si su flag lo habilita: apagado ⇒ cero
    // construcción de clientes, cero cachés de disco, cero registro.
    let adapter = flags.youtube_provider.then(|| {
        Arc::new(YouTubeAdapter::from_inner(Arc::new(
            YoutubeProvider::with_options(YoutubeOptions {
                disable_env_proxy: !flags.proxy,
            }),
        )))
    });

    let mut catalog = CatalogRegistry::new();
    if let Some(adapter) = &adapter {
        // El mismo adaptador sirve catálogo y streaming (una sola instancia
        // del cliente: una sola caché de disco, un solo pool de visitor data).
        catalog.register(Box::new((**adapter).clone()));
    }

    let stream_registry = Arc::new(StreamRegistry::new());
    if let Some(adapter) = adapter.clone() {
        stream_registry.register(adapter.clone());
    }

    let validator: Option<Arc<dyn crate::media::StreamValidator>> =
        adapter.map(|a| a as Arc<dyn crate::media::StreamValidator>);

    assemble(catalog, stream_registry, cache_for(db), validator)
}

/// Sin la feature `youtube` no se registra ni se construye ningún cliente.
#[cfg(not(feature = "youtube"))]
pub fn compose_media(
    db: crate::infrastructure::db::Db,
    _flags: &crate::infrastructure::config::FeatureFlags,
) -> ComposedMedia {
    assemble(
        CatalogRegistry::new(),
        Arc::new(StreamRegistry::new()),
        cache_for(db),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{source::Source, track::Track};
    use crate::infrastructure::config::FeatureFlags;
    use crate::infrastructure::db::Db;
    use crate::media::FailureCategory;

    fn yt_track() -> Track {
        let mut t = Track::new("T".into(), Vec::new(), Source::YouTube);
        t.external_id = Some("vid-1".into());
        t
    }

    async fn temp_db() -> Db {
        let dir = tempfile::tempdir().unwrap();
        Db::connect(dir.path().join("music.db").to_str().unwrap())
            .await
            .unwrap()
    }

    /// CRITERIO DE REEMPLAZABILIDAD (spec §3/§46): con el flag de YouTube en
    /// off NO se registra ningún proveedor y el resolver responde con el error
    /// ESTRUCTURADO limpio — sin pánico, sin tocar red, sin romper la app.
    #[tokio::test]
    async fn youtube_disabled_yields_empty_registry_and_structured_error() {
        let db = temp_db().await;
        let flags = FeatureFlags {
            youtube_provider: false,
            ..FeatureFlags::default()
        };
        let composed = compose_media(db, &flags);

        assert!(composed.catalog.is_empty(), "catálogo vacío sin providers");

        let err = composed
            .stream_resolver
            .resolve(&yt_track())
            .await
            .unwrap_err();
        assert_eq!(err.root.category, FailureCategory::ProviderUnavailable);
        assert!(
            err.root.message.contains("sin proveedores"),
            "el motivo es explícito: {}",
            err.root
        );
        assert!(err.attempts.is_empty(), "nada que intentar sin providers");
    }

    /// Con la feature y el flag por defecto el adaptador se registra en
    /// catálogo y streams (construcción OFFLINE: aquí no se hace ninguna
    /// petición).
    #[cfg(feature = "youtube")]
    #[tokio::test]
    async fn youtube_enabled_registers_catalog_and_stream_providers() {
        let db = temp_db().await;
        let composed = compose_media(db, &FeatureFlags::default());

        assert!(!composed.catalog.is_empty());
        assert_eq!(
            composed.catalog.get(Source::YouTube).map(|p| p.source()),
            Some(Source::YouTube)
        );
        // El snapshot del registro contiene a YouTube como candidato soportado.
        let reg = composed.stream_resolver.registry();
        let snaps = reg.snapshot(&yt_track(), std::time::Instant::now());
        assert!(
            snaps.iter().any(|s| s.id == "youtube"),
            "YouTube registrado y habilitado"
        );
    }
}
