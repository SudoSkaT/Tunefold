//! StreamResolver: orquesta caché → router → proveedores → política →
//! validación → persistencia (spec §8).
//!
//! Garantías del flujo:
//!
//! - **Caché primero**: un hit vigente (y que pasa el validador si existe) no
//!   toca la red.
//! - **Fallback acotado**: el plan de proveedores es finito y los reintentos
//!   por proveedor y el total global los limita [`FailurePolicy`].
//! - **Causa raíz preservada**: si todo falla, el error conserva el PRIMER
//!   motivo y la cadena completa de intentos; nunca se degrada a un string.
//! - **Validación antes de servir**: toda resolución pasa
//!   [`StreamResolution::validate`]; las cacheadas pueden además verificarse
//!   en vivo con un [`StreamValidator`] (sondeo barato de la URI).
//!
//! El resolver es agnóstico del motor: entrega una [`StreamResolution`] lista;
//! convertir a fuente reproducible es cosa del consumidor.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::domain::stream::StreamResolution;
use crate::domain::track::Track;

use crate::media::cache::ResolutionCache;
use crate::media::failure::{FailureCategory, ResolutionError};
use crate::media::policy::FailurePolicy;
use crate::media::policy::PolicyAction;
use crate::media::provider::ResolveContext;
use crate::media::registry::StreamRegistry;
use crate::media::router;

/// Verificación en vivo de una resolución cacheada.
///
/// Las URIs remotas mueren antes de su `expires_at` declarado; este puerto
/// permite al compositor inyectar un sondeo barato (GET de cabeceras) SIN que
/// el resolver conozca CDNs ni cabeceras concretas. Un validador puede REPARAR
/// la resolución en sitio (p. ej. completar cabeceras de contexto).
#[async_trait]
pub trait StreamValidator: Send + Sync {
    /// `true` si la resolución sigue utilizable (tras posible reparación).
    async fn check(&self, resolution: &mut StreamResolution) -> bool;
}

/// Configuración del resolver.
#[derive(Debug, Clone)]
pub struct ResolverConfig {
    pub policy: FailurePolicy,
    /// Timeout aplicado a CADA intento individual de proveedor.
    pub attempt_timeout: Duration,
}

impl Default for ResolverConfig {
    fn default() -> Self {
        Self {
            policy: FailurePolicy::default(),
            attempt_timeout: Duration::from_secs(20),
        }
    }
}

/// Registro de UN intento (observabilidad estructurada).
#[derive(Debug, Clone)]
pub struct AttemptRecord {
    pub provider_id: &'static str,
    /// `None` = no se llegó a intentar (circuito abierto).
    pub category: Option<FailureCategory>,
    pub message: String,
    pub latency: Duration,
}

impl AttemptRecord {
    fn skipped_circuit(provider_id: &'static str) -> Self {
        Self {
            provider_id,
            category: None,
            message: "circuito abierto".to_string(),
            latency: Duration::ZERO,
        }
    }
}

/// Fallo final de una `resolve()`.
///
/// `root` conserva SIEMPRE el primer error real (spec §8: nunca ocultar el
/// motivo original); `attempts` traza la cadena completa para métricas/UI.
#[derive(Debug, Clone)]
pub struct ResolveError {
    pub key: String,
    pub attempts: Vec<AttemptRecord>,
    /// Primer fallo real encontrado. Si ni siquiera hubo candidatos, lleva la
    /// categoría `ProviderUnavailable` con el motivo exacto.
    pub root: ResolutionError,
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "no se pudo resolver «{}»", self.key)?;
        if !self.attempts.is_empty() {
            write!(f, " tras {} intento(s)", self.attempts.len())?;
            let chain: Vec<String> = self
                .attempts
                .iter()
                .map(|a| a.provider_id.to_string())
                .collect();
            write!(f, " ({})", chain.join(" → "))?;
        }
        write!(f, ": {}", self.root)
    }
}

impl std::error::Error for ResolveError {}

/// Resolvedor central. Componible: registro/caché/validador entran por `Arc`.
pub struct StreamResolver {
    registry: Arc<StreamRegistry>,
    cache: Arc<dyn ResolutionCache>,
    validator: Option<Arc<dyn StreamValidator>>,
    config: ResolverConfig,
    cache_hits: std::sync::atomic::AtomicU64,
    cache_misses: std::sync::atomic::AtomicU64,
}

impl std::fmt::Debug for StreamResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamResolver")
            .field("config", &self.config)
            .field("validator", &self.validator.is_some())
            .finish_non_exhaustive()
    }
}

impl StreamResolver {
    pub fn new(
        registry: Arc<StreamRegistry>,
        cache: Arc<dyn ResolutionCache>,
        config: ResolverConfig,
    ) -> Self {
        Self {
            registry,
            cache,
            validator: None,
            config,
            cache_hits: std::sync::atomic::AtomicU64::new(0),
            cache_misses: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Instala el verificador en vivo para hits de caché.
    pub fn with_validator(mut self, validator: Arc<dyn StreamValidator>) -> Self {
        self.validator = Some(validator);
        self
    }

    pub fn registry(&self) -> &Arc<StreamRegistry> {
        &self.registry
    }

    pub fn cache(&self) -> &Arc<dyn ResolutionCache> {
        &self.cache
    }

    /// Cumulative cache decisions; useful to report per-play deltas at adapters.
    pub fn cache_stats(&self) -> (u64, u64) {
        use std::sync::atomic::Ordering;
        (
            self.cache_hits.load(Ordering::Relaxed),
            self.cache_misses.load(Ordering::Relaxed),
        )
    }

    /// Invalida la resolución cacheada de un track (p. ej. fallo en caliente
    /// reportado por el motor de reproducción).
    pub async fn invalidate(&self, track: &Track) {
        self.cache.invalidate(&track.identifier()).await;
    }

    /// Invalida todas las resoluciones de un origen.
    pub async fn invalidate_provider(&self, source: crate::domain::source::Source) {
        self.cache.clear_provider(source).await;
    }

    /// Resolución forzada: ignora la lectura de caché y resuelve en red
    /// (refresh tras expiración o fallo). El resultado fresco sobreescribe la
    /// caché.
    pub async fn refresh(&self, track: &Track) -> Result<StreamResolution, ResolveError> {
        self.cache.invalidate(&track.identifier()).await;
        self.resolve_inner(track).await
    }

    /// Resuelve el stream del track: caché primero, red después.
    pub async fn resolve(&self, track: &Track) -> Result<StreamResolution, ResolveError> {
        let key = track.identifier();

        // ------------------------------------------------------- caché
        if let Some(mut cached) = self.cache.get(&key).await {
            if cached.is_expired() {
                self.cache_misses
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                tracing::debug!(key = %key, "stream_expired: entrada de caché caducada");
                self.cache.invalidate(&key).await;
            } else {
                // Sondeo en vivo SOLO para hits (el camino fresco ya verificó
                // dentro del proveedor). El validador puede reparar en sitio.
                let alive = match &self.validator {
                    Some(v) => v.check(&mut cached).await,
                    None => true,
                };
                if alive && cached.validate().is_ok() {
                    tracing::debug!(
                        key = %key,
                        provider = %cached.provider,
                        "resolution_cache_hit"
                    );
                    self.cache_hits
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    return Ok(cached);
                }
                self.cache_misses
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                tracing::debug!(key = %key, "resolution_cache_hit_stale");
                self.cache.invalidate(&key).await;
            }
        } else {
            self.cache_misses
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

        tracing::debug!(key = %key, "resolution_started");
        self.resolve_inner(track).await
    }

    /// Camino de red completo (sin leer caché): router → intentos → política.
    async fn resolve_inner(&self, track: &Track) -> Result<StreamResolution, ResolveError> {
        let key = track.identifier();
        let now = Instant::now();
        let snapshots = self.registry.snapshot(track, now);
        let plan = router::order(track, snapshots);

        if plan.is_empty() {
            return Err(ResolveError {
                key,
                attempts: Vec::new(),
                root: ResolutionError::new(
                    FailureCategory::ProviderUnavailable,
                    track.source,
                    "sin proveedores disponibles para este track",
                ),
            });
        }

        let mut attempts: Vec<AttemptRecord> = Vec::new();
        // La causa raíz es el PRIMER fallo real de toda la cadena.
        let mut root: Option<ResolutionError> = None;
        let mut total_attempts: u32 = 0;

        for candidate in plan {
            if !self.registry.allow_attempt(candidate.id, Instant::now()) {
                attempts.push(AttemptRecord::skipped_circuit(candidate.id));
                continue;
            }

            let ctx = ResolveContext::with_deadline(Instant::now() + self.config.attempt_timeout);
            let mut attempt: u32 = 0;
            loop {
                total_attempts += 1;
                let started = Instant::now();
                let outcome = tokio::time::timeout(
                    self.config.attempt_timeout,
                    candidate.provider.resolve(track, &ctx),
                )
                .await;
                let latency = started.elapsed();

                let result = match outcome {
                    Ok(r) => r,
                    Err(_elapsed) => Err(ResolutionError::new(
                        FailureCategory::Timeout,
                        candidate.source,
                        format!(
                            "{} no respondió en {:?}",
                            candidate.id, self.config.attempt_timeout
                        ),
                    )),
                };

                match result {
                    Ok(resolution) => {
                        if let Err(validation) = resolution.validate() {
                            // Estructura inválida: tratar como respuesta
                            // inválida del proveedor (determinista → fallback).
                            let err = ResolutionError::new(
                                FailureCategory::InvalidResponse,
                                resolution.provider,
                                validation.to_string(),
                            );
                            self.registry.record_failure(candidate.id);
                            if root.is_none() {
                                root = Some(err.clone());
                            }
                            attempts.push(AttemptRecord {
                                provider_id: candidate.id,
                                category: Some(err.category),
                                message: err.message.clone(),
                                latency,
                            });
                            break; // sin retry: siguiente proveedor
                        }
                        self.registry.record_success(candidate.id);
                        self.cache.put(&key, resolution.clone()).await;
                        tracing::info!(
                            key = %key,
                            provider = candidate.id,
                            latency_ms = latency.as_millis() as u64,
                            "resolution_success"
                        );
                        return Ok(resolution);
                    }
                    Err(err) => {
                        self.registry.record_failure(candidate.id);
                        if root.is_none() {
                            root = Some(err.clone());
                        }
                        attempts.push(AttemptRecord {
                            provider_id: candidate.id,
                            category: Some(err.category),
                            message: err.message.clone(),
                            latency,
                        });
                        tracing::debug!(
                            provider = candidate.id,
                            category = %err.category,
                            attempt,
                            "resolution_provider_failed"
                        );
                        if self.config.policy.total_exhausted(total_attempts) {
                            break;
                        }
                        match self.config.policy.decide(&err, attempt) {
                            PolicyAction::Retry { delay } => {
                                tokio::time::sleep(delay).await;
                                attempt += 1;
                            }
                            PolicyAction::Fallback => break,
                        }
                    }
                }
            }
        }

        let root = root.unwrap_or_else(|| {
            ResolutionError::new(
                FailureCategory::ProviderUnavailable,
                track.source,
                "ningún proveedor pudo ser intentado",
            )
        });
        tracing::info!(
            key = %key,
            attempts = attempts.len(),
            root_category = %root.category,
            "resolution_failed"
        );
        Err(ResolveError {
            key,
            attempts,
            root,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::source::Source;
    use crate::media::cache::MemoryResolutionCache;
    use crate::media::circuit::CircuitConfig;
    use crate::media::policy::FailurePolicy;
    use crate::media::provider::{ResolveContext, StreamProvider};
    use crate::media::registry::StreamRegistry;
    use crate::media::test_support::{FakeStreamProvider, Step};

    use chrono::TimeZone;

    fn yt_track(vid: &str) -> Track {
        let mut t = Track::new("T".into(), Vec::new(), Source::YouTube);
        t.external_id = Some(vid.into());
        t
    }

    fn fast_policy() -> FailurePolicy {
        FailurePolicy {
            base_backoff: Duration::from_millis(5),
            ..FailurePolicy::default()
        }
    }

    fn resolver(
        registry: Arc<StreamRegistry>,
        cache: Arc<MemoryResolutionCache>,
        policy: FailurePolicy,
    ) -> StreamResolver {
        StreamResolver::new(
            registry,
            cache,
            ResolverConfig {
                policy,
                attempt_timeout: Duration::from_secs(2),
            },
        )
    }

    struct BoolValidator {
        alive: std::sync::atomic::AtomicBool,
    }
    impl BoolValidator {
        fn live() -> Self {
            Self {
                alive: std::sync::atomic::AtomicBool::new(true),
            }
        }
        fn kill(&self) {
            self.alive.store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }
    #[async_trait]
    impl StreamValidator for BoolValidator {
        async fn check(&self, _r: &mut StreamResolution) -> bool {
            self.alive.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[tokio::test]
    async fn cache_hit_does_not_touch_providers() {
        let a = Arc::new(FakeStreamProvider::ok("a", Source::YouTube, "one"));
        let reg = Arc::new(StreamRegistry::new());
        reg.register(a.clone());
        let cache = Arc::new(MemoryResolutionCache::default());
        let r = resolver(reg, cache, fast_policy());

        let first = r.resolve(&yt_track("v1")).await.unwrap();
        assert_eq!(first.uri, "https://cdn.fake/one");
        assert_eq!(r.cache_stats(), (0, 1));

        // Segunda resolución: servida de caché, cero llamadas nuevas.
        let second = r.resolve(&yt_track("v1")).await.unwrap();
        assert_eq!(second.uri, "https://cdn.fake/one");
        assert_eq!(a.call_count(), 1);
        assert_eq!(r.cache_stats(), (1, 1));
    }

    #[tokio::test]
    async fn expired_cache_entry_forces_network_resolution() {
        let a = Arc::new(FakeStreamProvider::ok("a", Source::YouTube, "fresh"));
        let reg = Arc::new(StreamRegistry::new());
        reg.register(a.clone());
        let cache = Arc::new(MemoryResolutionCache::default());

        // Entrada ya caducada sembrada a mano.
        let mut stale = StreamResolution::new(Source::YouTube, "https://cdn.fake/stale");
        stale.expires_at = Some(chrono::Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap());
        cache.put("v1", stale).await;

        let r = resolver(reg, cache.clone(), fast_policy());
        let res = r.resolve(&yt_track("v1")).await.unwrap();
        assert_eq!(res.uri, "https://cdn.fake/fresh");
        assert_eq!(r.cache_stats(), (0, 1));
        assert_eq!(a.call_count(), 1);
        assert_eq!(
            cache.get("v1").await.unwrap().uri,
            "https://cdn.fake/fresh",
            "el resultado fresco reemplaza al caducado"
        );
    }

    #[tokio::test]
    async fn dead_validator_discards_hit_and_resolves_fresh() {
        let a = Arc::new(FakeStreamProvider::ok("a", Source::YouTube, "fresh"));
        let reg = Arc::new(StreamRegistry::new());
        reg.register(a.clone());
        let cache = Arc::new(MemoryResolutionCache::default());
        let validator = Arc::new(BoolValidator::live());

        let r = resolver(reg, cache, fast_policy()).with_validator(validator.clone());
        r.resolve(&yt_track("v1")).await.unwrap();

        // El validador declara muerta la URI cacheada: se re-resuelve.
        validator.kill();
        let res = r.resolve(&yt_track("v1")).await.unwrap();
        assert_eq!(res.uri, "https://cdn.fake/fresh");
        assert_eq!(a.call_count(), 2);
    }

    #[tokio::test]
    async fn falls_back_to_next_provider_and_serves_result() {
        let a = Arc::new(FakeStreamProvider::failing(
            "a",
            Source::YouTube,
            FailureCategory::NetworkFailure,
            "dns roto",
        ));
        let b = Arc::new(FakeStreamProvider::ok("b", Source::YouTube, "de-b"));
        let reg = Arc::new(StreamRegistry::new());
        reg.register_with_circuit(a.clone(), CircuitConfig::default());
        reg.register_with_circuit(b.clone(), CircuitConfig::default());

        let r = resolver(
            reg,
            Arc::new(MemoryResolutionCache::default()),
            fast_policy(),
        );
        let res = r.resolve(&yt_track("v1")).await.unwrap();
        assert_eq!(res.uri, "https://cdn.fake/de-b");
        assert_eq!(res.provider, Source::YouTube);
        assert_eq!(b.call_count(), 1);
    }

    #[tokio::test]
    async fn unsupported_falls_back_without_retrying() {
        let a = Arc::new(FakeStreamProvider::failing(
            "a",
            Source::YouTube,
            FailureCategory::Unsupported,
            "sin audio",
        ));
        let b = Arc::new(FakeStreamProvider::ok("b", Source::YouTube, "de-b"));
        let reg = Arc::new(StreamRegistry::new());
        reg.register(a.clone());
        reg.register(b.clone());

        let r = resolver(
            reg,
            Arc::new(MemoryResolutionCache::default()),
            fast_policy(),
        );
        assert!(r.resolve(&yt_track("v1")).await.is_ok());
        assert_eq!(a.call_count(), 1, "determinista: sin reintento");
    }

    #[tokio::test]
    async fn retryable_errors_retry_within_cap_then_fallback() {
        let a = Arc::new(FakeStreamProvider::failing(
            "a",
            Source::YouTube,
            FailureCategory::NetworkFailure,
            "intermitente",
        ));
        let b = Arc::new(FakeStreamProvider::ok("b", Source::YouTube, "de-b"));
        let reg = Arc::new(StreamRegistry::new());
        reg.register(a.clone());
        reg.register(b.clone());

        // retries_per_provider=1 → A se intenta 2 veces y luego cae a B.
        let r = resolver(
            reg,
            Arc::new(MemoryResolutionCache::default()),
            fast_policy(),
        );
        let res = r.resolve(&yt_track("v1")).await.unwrap();
        assert_eq!(res.uri, "https://cdn.fake/de-b");
        assert_eq!(a.call_count(), 2, "primer intento + un reintento");
        assert_eq!(b.call_count(), 1);
    }

    #[tokio::test]
    async fn all_fail_preserves_first_root_cause() {
        let a = Arc::new(FakeStreamProvider::failing(
            "a",
            Source::YouTube,
            FailureCategory::NetworkFailure,
            "causa raíz A",
        ));
        let b = Arc::new(FakeStreamProvider::failing(
            "b",
            Source::YouTube,
            FailureCategory::RateLimited,
            "cuota agotada",
        ));
        let reg = Arc::new(StreamRegistry::new());
        reg.register(a.clone());
        reg.register(b.clone());

        let policy = FailurePolicy {
            retries_per_provider: 0,
            ..fast_policy()
        };
        let r = resolver(reg, Arc::new(MemoryResolutionCache::default()), policy);
        let err = r.resolve(&yt_track("v1")).await.unwrap_err();

        assert_eq!(err.attempts.len(), 2);
        assert_eq!(err.root.message, "causa raíz A", "el primer motivo manda");
        assert_eq!(err.root.category, FailureCategory::NetworkFailure);
        assert!(
            err.to_string().contains("a"),
            "la cadena de intentos es visible"
        );
    }

    #[tokio::test]
    async fn no_candidates_reports_provider_unavailable() {
        let reg = Arc::new(StreamRegistry::new()); // vacío
        let r = resolver(
            reg,
            Arc::new(MemoryResolutionCache::default()),
            fast_policy(),
        );
        let err = r.resolve(&yt_track("v1")).await.unwrap_err();
        assert_eq!(err.root.category, FailureCategory::ProviderUnavailable);
        assert!(err.attempts.is_empty());
    }

    #[tokio::test]
    async fn disabled_provider_is_skipped() {
        let a = Arc::new(FakeStreamProvider::ok("a", Source::YouTube, "de-a"));
        let b = Arc::new(FakeStreamProvider::ok("b", Source::YouTube, "de-b"));
        let reg = Arc::new(StreamRegistry::new());
        reg.register(a.clone());
        reg.register(b);
        assert!(reg.set_enabled("a", false));

        let r = resolver(
            reg,
            Arc::new(MemoryResolutionCache::default()),
            fast_policy(),
        );
        let res = r.resolve(&yt_track("v1")).await.unwrap();
        assert_eq!(res.uri, "https://cdn.fake/de-b");
        assert_eq!(a.call_count(), 0, "apagado en caliente: ni una llamada");
    }

    #[tokio::test]
    async fn open_circuit_skips_provider_without_calling_it() {
        let a = Arc::new(FakeStreamProvider::ok("a", Source::YouTube, "de-a"));
        let b = Arc::new(FakeStreamProvider::ok("b", Source::YouTube, "de-b"));
        let reg = Arc::new(StreamRegistry::new());
        reg.register_with_circuit(
            a.clone(),
            CircuitConfig {
                failure_threshold: 1,
                cooldown: Duration::from_secs(30),
            },
        );
        reg.register(b.clone());
        reg.record_failure("a"); // umbral 1 → abierto

        let r = resolver(
            reg,
            Arc::new(MemoryResolutionCache::default()),
            fast_policy(),
        );
        let res = r.resolve(&yt_track("v1")).await.unwrap();
        assert_eq!(res.uri, "https://cdn.fake/de-b");
        assert_eq!(
            a.call_count(),
            0,
            "circuito abierto: el provider no se molesta"
        );
    }

    #[tokio::test]
    async fn hanging_provider_hits_attempt_timeout_and_falls_back() {
        let a = Arc::new(FakeStreamProvider::new(
            "a",
            Source::YouTube,
            200, // prioridad alta: primero en el plan
            vec![Step::SlowOk(Duration::from_secs(5))],
        ));
        let b = Arc::new(FakeStreamProvider::ok("b", Source::YouTube, "de-b"));
        let reg = Arc::new(StreamRegistry::new());
        reg.register(a);
        reg.register(b);

        let config = ResolverConfig {
            policy: FailurePolicy {
                retries_per_provider: 0,
                ..fast_policy()
            },
            attempt_timeout: Duration::from_millis(80),
        };
        let r = StreamResolver::new(reg, Arc::new(MemoryResolutionCache::default()), config);
        let res = r.resolve(&yt_track("v1")).await.unwrap();
        assert_eq!(res.uri, "https://cdn.fake/de-b");
        assert_eq!(res.provider, Source::YouTube);
    }

    #[tokio::test]
    async fn refresh_bypasses_cache_and_overwrites_it() {
        let a = Arc::new(FakeStreamProvider::ok("a", Source::YouTube, "one"));
        let reg = Arc::new(StreamRegistry::new());
        reg.register(a.clone());
        let cache = Arc::new(MemoryResolutionCache::default());
        let r = resolver(reg, cache, fast_policy());

        r.resolve(&yt_track("v1")).await.unwrap();
        assert_eq!(a.call_count(), 1); // desde caché ya no haría falta red

        r.refresh(&yt_track("v1")).await.unwrap();
        assert_eq!(a.call_count(), 2, "refresh ignora la caché");
    }

    /// Proveedor que devuelve resoluciones estructuralmente inválidas.
    #[derive(Debug)]
    struct InvalidOutputProvider(Source);
    #[async_trait]
    impl StreamProvider for InvalidOutputProvider {
        fn id(&self) -> &'static str {
            "broken"
        }
        fn source(&self) -> Source {
            self.0
        }
        async fn resolve(
            &self,
            _t: &Track,
            _c: &ResolveContext,
        ) -> Result<StreamResolution, ResolutionError> {
            Ok(StreamResolution::new(self.0, "")) // URI vacía: inválida
        }
    }

    #[tokio::test]
    async fn structurally_invalid_resolution_is_rejected_and_fallback_runs() {
        let bad = Arc::new(InvalidOutputProvider(Source::YouTube));
        let good = Arc::new(FakeStreamProvider::ok("good", Source::YouTube, "de-good"));
        let reg = Arc::new(StreamRegistry::new());
        reg.register(bad);
        reg.register(good.clone());

        let r = resolver(
            reg,
            Arc::new(MemoryResolutionCache::default()),
            fast_policy(),
        );
        let res = r.resolve(&yt_track("v1")).await.unwrap();
        assert_eq!(res.uri, "https://cdn.fake/de-good");
        assert_eq!(good.call_count(), 1);
    }
}
