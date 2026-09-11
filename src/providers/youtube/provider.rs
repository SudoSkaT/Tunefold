//! Cliente de YouTube / YouTube Music (rustypipe) — núcleo del adaptador.
//!
//! Responsabilidades (capa PROVIDER, nunca importada fuera de `providers/`):
//! - metadata vía search/details/artist/album/related;
//! - letras sincronizadas vía LRCLIB (ver [`super::lyrics`]);
//! - resolución de stream de audio con clientes directos Android/iOS + PO
//!   tokens, verificación por rangos y cachés (memoria + dedupe en vuelo);
//! - mapeo a modelos de dominio vía [`super::mapper`].
//!
//! Los errores de la ruta de streaming salen tipados como [`ResolveFail`]
//! para que el adaptador los traduzca a la taxonomía estructural.
//!
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use rustypipe::client::{ClientType, RustyPipe};
use rustypipe::model::{MusicAlbum, MusicArtist};

use super::mapper::{best_thumbnail, map_track, THUMB_FALLBACK};
use crate::catalog::{CatalogError as ProviderError, CatalogProvider};
use crate::domain::source::Source;
use crate::domain::{album::Album, artist::Artist, track::Track};
use crate::media::FailureCategory;
use crate::providers::lyrics::fetch_lrclib_lyrics;

/// Directorio donde rustypipe guarda su caché (`rustypipe_cache.json`,
/// `bg_snapshot.bin` y reportes): `~/.cache/tunefold/rustypipe` (XDG).
fn cache_dir() -> std::path::PathBuf {
    crate::infrastructure::dirs::cache_dir().join("rustypipe")
}

/// Número máximo de intentos de resolución de stream. Cada intento fuerza un
/// visitor_data nuevo (YouTube marca como bloqueado el del pool cacheado y sus
/// streams responden 403 al descargar).
const RESOLVE_ATTEMPTS: usize = 3;
/// Pausa entre intentos de resolución: da margen a que YouTube "desbloquee" y
/// evita disparar el anti-bot con peticiones encadenadas.
const RESOLVE_BACKOFF: Duration = Duration::from_secs(1);

/// TTL de la caché en memoria de streams resueltos. Las URLs de googlevideo
/// viven horas, así que reutilizar la resolución de un video ya pedido (replay,
/// retry tras un 403 puntual) evita toda la ronda de visitor/player/verificación.
pub const STREAM_CACHE_TTL: Duration = Duration::from_secs(20 * 60);
/// Timeout de la verificación de un candidato (GET plano de cabeceras). Un
/// stream muerto responde lento o nunca: acotar aquí convierte el peor caso de
/// una verificación en segundos en vez de los 15s del cliente general.
const VERIFY_TIMEOUT: Duration = Duration::from_secs(4);
/// Cuántos candidatos se verifican a la vez en paralelo.
const VERIFY_CONCURRENCY: usize = 4;
/// Tiempo máximo de espera cuando otro hilo ya está resolviendo el mismo video.
const INFLIGHT_TIMEOUT: Duration = Duration::from_secs(30);
/// Periodo de sondeo de un resolución concurrente en curso.
const INFLIGHT_POLL: Duration = Duration::from_millis(50);

/// Cabeceras de contexto para descargar streams de googlevideo. El CDN no
/// valida el contexto en los GET por rangos (los probes `probe_boundary`
/// sirven 206 sin ninguna cabecera); se adjuntan por higiene, coherentes con
/// el cliente primario (VISIONOS).
const STREAM_HEADERS: &[(&str, &str)] = &[
    ("User-Agent", rustypipe::client::VISIONOS_UA),
    ("Referer", "https://www.youtube.com/"),
    ("Origin", "https://www.youtube.com"),
];

/// Offset del sondeo anti-techo: desde el diagnóstico del límite de ~1 MiB,
/// las URLs resueltas con clientes directos capados (ANDROID_VR/iOS) responden
/// 403 en cualquier rango cuyo fin supere ~1 MiB. Un stream SANO responde 206
/// ahí; uno capado, 403. Verificación obligatoria antes de comprometer la
/// reproducción.
const FAR_PROBE_START: u64 = 1536 * 1024;
/// Longitud del sondeo lejano.
const FAR_PROBE_LEN: u64 = 64 * 1024;

/// Opciones de construcción del proveedor.
#[derive(Debug, Clone, Copy, Default)]
pub struct YoutubeOptions {
    /// Ignorar los proxies del entorno (`PROXY_ENABLED=false`): todos los
    /// clientes HTTP internos se construyen con `.no_proxy()`.
    pub disable_env_proxy: bool,
}

pub struct YoutubeProvider {
    client: RustyPipe,
    /// Cliente pre-signed para verificar que una URL de stream responde antes
    /// de comprometer la reproducción (ver [`YoutubeProvider::stream_url_ok`]).
    http: reqwest::Client,
    /// Cliente dedicado a la verificación de streams: timeouts cortos para que
    /// un candidato muerto falle en segundos, no en los 15s del cliente general.
    verify: reqwest::Client,
    /// Cliente preferido para la siguiente resolución, alternando entre
    /// Android (ANDROID_VR) e iOS. YouTube bloquea streams por cliente de
    /// forma intermitente, así que rotar primario reparte los aciertos.
    next_primary: std::sync::atomic::AtomicU8,
    /// Caché en memoria de streams ya resueltos (`video_id` → stream + TTL).
    /// El stream se re-verifica con el GET rápido antes de reutilizarse: una
    /// URL muerta se descarta sola y cae a la resolución completa.
    stream_cache: Arc<tokio::sync::Mutex<HashMap<String, CachedStream>>>,
    /// Resoluciones en curso por `video_id`: un segundo `Play` del mismo video
    /// espera el resultado del primero en vez de duplicar toda la ronda (y
    /// arriesgar el anti-bot con peticiones encadenadas).
    inflight: Arc<InflightRegistry>,
}

/// Resultado de una resolución de stream compartido entre hilos concurrentes
/// (`Ok(None)` = video sin audio; `Err(msg)` = error de resolución).
type ResolveOutcome = Result<Option<String>, CategorizedFail>;

/// Registro de resoluciones en curso (`video_id` → estado compartido): cada
/// entrada es el `Arc<Mutex<Option<ResolveOutcome>>>` que rellena el hilo
/// "resolver" y que esperan los hilos concurrentes del mismo video.
type InflightRegistry =
    tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<Option<ResolveOutcome>>>>>;

/// Entrada de la caché de streams.
struct CachedStream {
    url: String,
    expires_at: Instant,
}

impl std::fmt::Debug for YoutubeProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("YoutubeProvider").finish_non_exhaustive()
    }
}

impl YoutubeProvider {
    pub fn new() -> Self {
        Self::with_options(YoutubeOptions::default())
    }

    /// Construye el proveedor con opciones (política de proxy).
    pub fn with_options(options: YoutubeOptions) -> Self {
        // Asegura el directorio de caché de rustypipe (usage en write).
        let cache_dir = cache_dir();
        let _ = std::fs::create_dir_all(&cache_dir);
        let client = RustyPipe::builder()
            .storage_dir(cache_dir.to_str().unwrap_or("data/youtube"))
            .build()
            .expect("cliente rustypipe válido");
        let apply_proxy = |b: reqwest::ClientBuilder| {
            if options.disable_env_proxy {
                b.no_proxy()
            } else {
                b
            }
        };
        let http = apply_proxy(
            reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(15)),
        )
        .build()
        .expect("cliente http válido");
        let verify = apply_proxy(
            reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(3))
                .timeout(VERIFY_TIMEOUT),
        )
        .build()
        .expect("cliente de verificación válido");
        Self {
            client,
            http,
            verify,
            next_primary: std::sync::atomic::AtomicU8::new(0),
            stream_cache: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            inflight: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Pide los recomendados de un video (fetch puntual desde YTM). Las letras
    /// ya no vienen de YouTube Music: el karaoke usa solo LRCLIB (`syncedLyrics`).
    async fn fetch_related(&self, video_id: &str) -> Vec<Track> {
        let details = match self.client.query().music_details(video_id).await {
            Ok(d) => d,
            Err(_) => return Vec::new(),
        };

        let related = match details.related_id {
            Some(id) => match self.client.query().music_related(id).await {
                Ok(related) => related.tracks.into_iter().map(|t| map_track(&t)).collect(),
                Err(_) => Vec::new(),
            },
            None => Vec::new(),
        };

        // Algunas canciones vienen sin tab Related (o el browse devuelve
        // vacío): se cae a la radio automática del video (RDAMVM<id>), que
        // siempre responde con canciones afines.
        if related.is_empty() {
            if let Ok(radio) = self.client.query().music_radio_track(video_id).await {
                return radio.items.into_iter().map(|t| map_track(&t)).collect();
            }
        }

        related
    }

    /// Comprueba que la URL del stream responde a la MISMA descarga que hará
    /// el motor de reproducción: contexto de cabeceras + `Range` cerrado.
    ///
    /// Tres sondas:
    ///   1. `bytes=0-1023`: el stream vive y sirve rangos (206).
    ///   2. `bytes=512K..1M-1`: el prefijo del primer MiB está servible.
    ///   3. `bytes=1.5M..+64K` (solo si el archivo llega): detecta el techo
    ///      posicional (~1 MiB) que YouTube aplica a las URLs resueltas con
    ///      clientes capados; un stream sano responde 206 ahí. Sin esta sonda,
    ///      una URL capada pasaría la verificación y cortaría a ~65 s.
    async fn stream_url_ok(&self, url: &str) -> bool {
        if self.range_probe(url, 0, 1023).await != Some(206) {
            return false;
        }
        let clen = url
            .split("clen=")
            .nth(1)
            .and_then(|v| v.split('&').next())
            .and_then(|v| v.parse::<u64>().ok());
        let Some(clen) = clen else {
            return true;
        };
        let start = 512 * 1024;
        let end = clen.saturating_sub(1).min(1024 * 1024 - 1);
        if start < end && !matches!(self.range_probe(url, start, end).await, Some(206 | 416)) {
            return false;
        }
        // Sonda lejana: solo tiene sentido si el archivo supera la frontera.
        if clen > FAR_PROBE_START + FAR_PROBE_LEN {
            return matches!(
                self.range_probe(url, FAR_PROBE_START, FAR_PROBE_START + FAR_PROBE_LEN - 1)
                    .await,
                Some(206)
            );
        }
        true
    }

    /// GET con `Range` cerrado y contexto; devuelve el status HTTP o `None`
    /// si la petición falló (timeout, conexión).
    async fn range_probe(&self, url: &str, start: u64, end: u64) -> Option<u16> {
        let mut req = self.verify.get(url);
        for (k, v) in STREAM_HEADERS {
            req = req.header(*k, *v);
        }
        req.header("Range", format!("bytes={start}-{end}"))
            .send()
            .await
            .map(|r| r.status().as_u16())
            .ok()
    }

    /// Stream en caché válido para `video_id`, si existe.
    ///
    /// El stream se re-verifica con el GET rápido antes de reutilizarse: una
    /// URL muerta (caducada o bloqueada) se descarta y el siguiente intento cae
    /// a la resolución completa, que rota visitor_data/cliente.
    async fn cached_stream(&self, video_id: &str) -> Option<String> {
        let mut cache = self.stream_cache.lock().await;
        let entry = match cache.get(video_id) {
            Some(e) if e.expires_at >= Instant::now() => e,
            _ => {
                cache.remove(video_id);
                return None;
            }
        };
        if self.stream_url_ok(&entry.url).await {
            Some(entry.url.clone())
        } else {
            cache.remove(video_id);
            None
        }
    }

    /// Resuelve el stream de `video_id` en segundo plano y comparte el
    /// resultado con los demás hilos que pidieran el mismo video.
    ///
    /// El primer hilo que llega es el "resolver" y rellena `shared`; los que
    /// llegan después esperan (con timeout) su resultado en vez de repetir toda
    /// la ronda de visitor/player/verificación, que encadena peticiones y
    /// arriesga el anti-bot.
    async fn shared_resolve(&self, video_id: &str) -> Result<Option<String>, CategorizedFail> {
        // Si ya hay un resolver en marcha para este video, esperar su
        // resultado (con timeout) en vez de repetir toda la ronda. Si el
        // resolver colgó (red muerta), se toma el relevo registrándose.
        {
            let inflight = self.inflight.lock().await;
            if let Some(existing) = inflight.get(video_id).cloned() {
                drop(inflight);
                let deadline = Instant::now() + INFLIGHT_TIMEOUT;
                loop {
                    let done = existing.lock().await;
                    if let Some(outcome) = done.as_ref() {
                        return Self::outcome_to_result(outcome);
                    }
                    drop(done);
                    if Instant::now() >= deadline {
                        break;
                    }
                    tokio::time::sleep(INFLIGHT_POLL).await;
                }
            }
        }
        let shared = Arc::new(tokio::sync::Mutex::new(None));
        self.inflight
            .lock()
            .await
            .insert(video_id.to_string(), shared.clone());

        let outcome = self.resolve_stream_inner(video_id).await;
        if let Ok(Some(url)) = &outcome {
            self.stream_cache.lock().await.insert(
                video_id.to_string(),
                CachedStream {
                    url: url.clone(),
                    expires_at: Instant::now() + STREAM_CACHE_TTL,
                },
            );
        }
        {
            let mut inflight = self.inflight.lock().await;
            if inflight
                .get(video_id)
                .is_some_and(|s| Arc::ptr_eq(s, &shared))
            {
                inflight.remove(video_id);
            }
        }
        *shared.lock().await = Some(outcome);
        let done = shared.lock().await;
        match done.as_ref() {
            Some(outcome) => Self::outcome_to_result(outcome),
            None => Ok(None),
        }
    }

    /// Clona el resultado compartido (la causa ya viaja clasificada).
    fn outcome_to_result(outcome: &ResolveOutcome) -> Result<Option<String>, CategorizedFail> {
        match outcome {
            Ok(url) => Ok(url.clone()),
            Err(fail) => Err(fail.clone()),
        }
    }

    /// Ronda completa de resolución (sin caché ni dedupe concurrente): recoge
    /// candidatos de los clientes directos en paralelo y acepta el primero que
    /// responde 2xx al GET de descarga.
    ///
    /// Orden: VISIONOS SIEMPRE primero — es el único cliente directo actual
    /// cuyas URLs no tienen el techo posicional de ~1 MiB (diagnóstico
    /// `probe_frontier`: ANDROID_VR/iOS sirven solo el prefijo; verificación
    /// lejana en [`Self::stream_url_ok`] lo descarta). Android/iOS quedan como
    /// respaldo rotando cuál va segundo.
    async fn resolve_stream_inner(
        &self,
        video_id: &str,
    ) -> Result<Option<String>, CategorizedFail> {
        let second = if self
            .next_primary
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .is_multiple_of(2)
        {
            ClientType::Android
        } else {
            ClientType::Ios
        };
        let order: [ClientType; 2] = [ClientType::Visionos, second];

        let is_mp4 = |s: &rustypipe::model::AudioStream| {
            s.url.contains("mime=audio%2Fmp4")
                || s.url.contains("mime=audio/mp4")
                || s.url.contains("mime%3Daudio%2Fmp4")
        };

        for attempt in 0..RESOLVE_ATTEMPTS {
            // Cada intento parte de un visitor_data nuevo y lo fija en la
            // consulta: YouTube bloquea los del pool cacheado (el primer
            // intento con visitor del pool responde 403 en todos los streams),
            // y entre intentos se espera un poco porque pedir visitantes/PO
            // tokens nuevos de forma encadenada dispara el anti-bot.
            if attempt > 0 {
                tokio::time::sleep(RESOLVE_BACKOFF).await;
            }
            // Si la adquisición de visitor data falla, es sistémico (YouTube
            // bloqueando, consent, red): repetir la ronda entera N veces solo
            // dispara más peticiones inútiles. Se aborta al primer fallo y el
            // reintento lo decide el loop de reproducción (con backoff).
            let vd = self
                .client
                .query()
                .get_visitor_data(true)
                .await
                .map_err(CategorizedFail::from_rp)?;
            let query = self.client.query().visitor_data(vd);

            // Recoge los streams de audio de los clientes directos en
            // paralelo: media petición por intento. No se corta en el primero
            // que responde: sus streams pueden estar muertos aun pasando el
            // player.
            let mut candidates: Vec<(String, u32)> = Vec::new();
            let mut any_player = false;
            let first = [order[0]];
            let second = [order[1]];
            let (left, right) = tokio::join!(
                query.player_from_clients(video_id, &first),
                query.player_from_clients(video_id, &second),
            );
            for p in [left, right].into_iter().flatten() {
                if !p.audio_streams.is_empty() {
                    any_player = true;
                    for s in &p.audio_streams {
                        let pref = (is_mp4(s) as u32 * 1_000_000) + s.bitrate;
                        candidates.push((s.url.clone(), pref));
                    }
                }
            }
            // Último recurso: player conjunto, que resuelve videos que un solo
            // cliente rechaza (sin Desktop: la deofuscación de player.js no
            // funciona con la versión actual de rustypipe).
            if !any_player {
                if let Ok(p) = query
                    .player_from_clients(
                        video_id,
                        &[ClientType::Visionos, ClientType::Android, ClientType::Ios],
                    )
                    .await
                {
                    for s in &p.audio_streams {
                        let pref = (is_mp4(s) as u32 * 1_000_000) + s.bitrate;
                        candidates.push((s.url.clone(), pref));
                    }
                }
            }

            // Prefiere MP4/AAC (rodio/symphonia decodifica AAC pero no Opus)
            // y luego la mayor tasa de bits. Se verifican en PARALELO con
            // timeout corto y se acepta el primero que responda 2xx: un
            // candidato muerto deja de bloquear a los demás (antes cada uno
            // podía consumir los 15s del cliente general, en serie).
            candidates.sort_by_key(|c| std::cmp::Reverse(c.1));
            let mut seen = std::collections::HashSet::new();
            let candidates: Vec<String> = candidates
                .into_iter()
                .filter_map(|(url, _)| seen.insert(url.clone()).then_some(url))
                .collect();
            // Verificación por LOTES en orden de prioridad: el primer lote
            // contiene los mejores candidatos y dentro de él se acepta el
            // primero que responda bien. Un buffer_unordered global elegía
            // por orden de COMPLETACIÓN (una variante de baja calidad podía
            // ganar la carrera a la preferida).
            let mut chosen: Option<String> = None;
            for batch in candidates.chunks(VERIFY_CONCURRENCY) {
                let checks: Vec<_> = batch
                    .iter()
                    .map(|url| {
                        let url = url.clone();
                        async move { (url.clone(), self.stream_url_ok(&url).await) }
                    })
                    .collect();
                let results = futures_util::future::join_all(checks).await;
                if let Some((url, _)) = results.into_iter().find(|(_, ok)| *ok) {
                    chosen = Some(url);
                    break;
                }
            }
            if let Some(url) = chosen {
                return Ok(Some(url));
            }
        }

        Ok(None)
    }
}

impl Default for YoutubeProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Mapea un error de rustypipe a [`ProviderError`] (ruta de CATÁLOGO: aquí la
/// taxonomía fina no aporta; el texto conserva la operación y la causa).
fn map_error(e: rustypipe::error::Error, op: &str) -> ProviderError {
    ProviderError::Other(format!("{op}: {e}"))
}

/// Clasifica un error de rustypipe en la taxonomía estructural (ruta de
/// STREAMING). Mapeo FINO contra los tipos reales del vendored 0.11.4:
///
/// | rustypipe                              | categoría                  |
/// |----------------------------------------|----------------------------|
/// | `Http` (reqwest) con "timed out"       | Timeout                    |
/// | `Http`                                 | NetworkFailure             |
/// | `HttpStatus(401|403)`                  | AuthenticationRequired     |
/// | `HttpStatus(404)`                      | Unsupported                |
/// | `HttpStatus(429)`                      | RateLimited                |
/// | `HttpStatus(5xx)`                      | ProviderUnavailable        |
/// | `Auth(_)` (PO token / consent)         | AuthenticationRequired     |
/// | `Extraction(Unavailable{..})`          | Unsupported                |
/// | `Extraction(NotFound{..})`             | Unsupported                |
/// | `Extraction(Botguard(_))`              | AuthenticationRequired     |
/// | resto de `Extraction(_)`               | InvalidResponse            |
/// | `Other(_)`                             | Unknown                    |
pub fn classify_rp_error(e: &rustypipe::error::Error) -> FailureCategory {
    use rustypipe::error::{Error as Rp, ExtractionError as Ex};
    match e {
        Rp::Http(msg) => {
            let m = msg.to_ascii_lowercase();
            if m.contains("timed out") || m.contains("timeout") {
                FailureCategory::Timeout
            } else {
                FailureCategory::NetworkFailure
            }
        }
        Rp::HttpStatus(code, _) => match code {
            401 | 403 => FailureCategory::AuthenticationRequired,
            404 => FailureCategory::Unsupported,
            429 => FailureCategory::RateLimited,
            500..=599 => FailureCategory::ProviderUnavailable,
            _ => FailureCategory::Unknown,
        },
        Rp::Auth(_) => FailureCategory::AuthenticationRequired,
        Rp::Extraction(ex) => match ex {
            Ex::Unavailable { .. } | Ex::NotFound { .. } => FailureCategory::Unsupported,
            Ex::Botguard(_) => FailureCategory::AuthenticationRequired,
            _ => FailureCategory::InvalidResponse,
        },
        Rp::Other(_) => FailureCategory::Unknown,
    }
}

/// Fallo de resolución YA clasificado; el mensaje conserva SIEMPRE el Display
/// del error original (causa raíz preservada, spec §8).
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct CategorizedFail {
    pub category: FailureCategory,
    pub message: String,
}

impl CategorizedFail {
    pub fn from_rp(e: rustypipe::error::Error) -> Self {
        let category = classify_rp_error(&e);
        Self {
            category,
            message: e.to_string(),
        }
    }
}

/// Cabeceras de contexto que toda descarga de googlevideo debe llevar (el
/// motor reproduce con el MISMO contexto que validó la verificación).
pub fn context_headers() -> Vec<(String, String)> {
    STREAM_HEADERS
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

impl YoutubeProvider {
    /// Resuelve el mejor stream de audio del video vía clientes directos
    /// (Android/iOS) con PO tokens, verificando que el stream responde al GET
    /// real de descarga antes de devolver la URL (y rotando visitor_data si
    /// YouTube entrega streams muertos).
    ///
    /// La resolución NO se conforma con el primer cliente que responde: un
    /// cliente puede devolver un player con streams que luego dan 403 al
    /// descargar (throttling por `n` sin descifrar, típico de iOS). Se recogen
    /// candidatos de todos los clientes directos y se acepta el primero que
    /// supera la verificación [`Self::stream_url_ok`].
    pub async fn resolve_audio_url(
        &self,
        track: &Track,
    ) -> Result<Option<String>, CategorizedFail> {
        let video_id = track.external_id.as_deref().unwrap_or_default();
        if video_id.is_empty() {
            return Ok(None);
        }

        // Caché de una resolución previa válida: reproducir de nuevo el mismo
        // video (o el retry tras un 403 puntual) no repite toda la ronda.
        if let Some(url) = self.cached_stream(video_id).await {
            return Ok(Some(url));
        }
        // Resolución concurrente del mismo video: esperar al que ya va en
        // marcha en vez de duplicar las peticiones.
        self.shared_resolve(video_id).await
    }

    /// Verificación en vivo de una URL cacheada (GET sondeo con contexto):
    /// camino barato para reutilizar streams ya conocidos.
    pub async fn verify_audio_url(&self, url: &str) -> bool {
        self.stream_url_ok(url).await
    }
}

#[async_trait]
impl CatalogProvider for YoutubeProvider {
    fn source(&self) -> Source {
        Source::YouTube
    }

    async fn search_tracks(&self, query: &str, limit: u32) -> Result<Vec<Track>, ProviderError> {
        let result = self
            .client
            .query()
            .music_search_tracks(query)
            .await
            .map_err(|e| map_error(e, "search_tracks"))?;
        Ok(result
            .items
            .items
            .into_iter()
            .take(limit as usize)
            .map(|t| map_track(&t))
            .collect())
    }

    async fn search_artists(&self, query: &str, limit: u32) -> Result<Vec<Artist>, ProviderError> {
        let result = self
            .client
            .query()
            .music_search_artists(query)
            .await
            .map_err(|e| map_error(e, "search_artists"))?;
        Ok(result
            .items
            .items
            .into_iter()
            .take(limit as usize)
            .map(|a| {
                let mut artist = Artist::new(
                    if a.name.is_empty() {
                        "Desconocido".to_string()
                    } else {
                        a.name.clone()
                    },
                    None,
                    None,
                    best_thumbnail(&a.avatar),
                );
                artist.external_id = Some(a.id);
                artist
            })
            .collect())
    }

    async fn search_albums(&self, query: &str, limit: u32) -> Result<Vec<Album>, ProviderError> {
        let result = self
            .client
            .query()
            .music_search_albums(query)
            .await
            .map_err(|e| map_error(e, "search_albums"))?;
        Ok(result
            .items
            .items
            .into_iter()
            .take(limit as usize)
            .map(|a| {
                Album::new(
                    a.name.clone(),
                    a.year
                        .and_then(|y| chrono::NaiveDate::from_ymd_opt(y as i32, 1, 1)),
                    best_thumbnail(&a.cover),
                    None,
                )
            })
            .collect())
    }

    async fn get_track(&self, external_id: &str) -> Result<Track, ProviderError> {
        let details = self
            .client
            .query()
            .music_details(external_id)
            .await
            .map_err(|e| map_error(e, "music_details"))?;
        Ok(map_track(&details.track))
    }

    async fn get_artist(&self, external_id: &str) -> Result<Artist, ProviderError> {
        let artist: MusicArtist = self
            .client
            .query()
            .music_artist(external_id, false)
            .await
            .map_err(|e| map_error(e, "music_artist"))?;
        let mut domain = Artist::new(
            if artist.name.is_empty() {
                "Desconocido".to_string()
            } else {
                artist.name.clone()
            },
            None,
            artist.description.clone(),
            best_thumbnail(&artist.header_image),
        );
        domain.external_id = Some(artist.id);
        Ok(domain)
    }

    async fn get_album(&self, external_id: &str) -> Result<Album, ProviderError> {
        let album: MusicAlbum = self
            .client
            .query()
            .music_album(external_id)
            .await
            .map_err(|e| map_error(e, "music_album"))?;
        Ok(Album::new(
            album.name.clone(),
            album
                .year
                .and_then(|y| chrono::NaiveDate::from_ymd_opt(y as i32, 1, 1)),
            best_thumbnail(&album.cover),
            None,
        ))
    }

    async fn get_album_tracks(&self, external_id: &str) -> Result<Vec<Track>, ProviderError> {
        let album: MusicAlbum = self
            .client
            .query()
            .music_album(external_id)
            .await
            .map_err(|e| map_error(e, "music_album"))?;
        Ok(album.tracks.iter().map(map_track).collect())
    }

    async fn related(&self, video_id: &str) -> Result<Vec<Track>, ProviderError> {
        Ok(self.fetch_related(video_id).await)
    }

    /// Letra sincronizada (LRC) vía LRCLIB, por firma del track (título y
    /// artista, con la duración como desempate). Es la única fuente del
    /// karaoke: sin `syncedLyrics` no hay LRC. Devuelve `None` sin
    /// considerarse error cuando no hay letras o no se encuentra coincidencia.
    async fn synced_lyrics(&self, track: &Track) -> Result<Option<String>, ProviderError> {
        Ok(fetch_lrclib_lyrics(&self.http, track).await)
    }

    /// Candidatas de miniatura, de mejor a peor: primero la portada de mayor
    /// resolución que ya devuelve la API (normalmente `maxresdefault` o una
    /// imagen mayor) y después la cadena reconstruida desde el `video_id`.
    ///
    /// Reconstruir con `i.ytimg.com` cubre los tracks guardados en la base de
    /// datos (tienen `video_id` pero no portada persistida) y sirve de fallback
    /// si la URL de la API deja de responder.
    fn thumbnail_candidates(&self, track: &Track) -> Vec<String> {
        let mut urls = Vec::new();
        if let Some(t) = &track.thumbnail {
            urls.push(t.url.clone());
        }
        if let Some(id) = track.external_id.as_deref().filter(|s| !s.is_empty()) {
            for quality in THUMB_FALLBACK {
                urls.push(format!("https://i.ytimg.com/vi/{id}/{quality}.jpg"));
            }
        }
        // Deduplica conservando el primer (mejor) candidato.
        let mut seen = std::collections::HashSet::new();
        urls.retain(|u| seen.insert(u.clone()));
        urls
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::track::Thumbnail as TrackThumbnail;

    #[test]
    fn thumbnail_candidates_prefer_api_then_rebuild() {
        let mut track = sample_track();
        track.external_id = Some("dQw4w9WgXcQ".to_string());
        let candidates = YoutubeProvider::new().thumbnail_candidates(&track);
        assert_eq!(
            candidates,
            vec![
                "cover.jpg".to_string(),
                "https://i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg".to_string(),
                "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg".to_string(),
                "https://i.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg".to_string(),
                "https://i.ytimg.com/vi/dQw4w9WgXcQ/default.jpg".to_string(),
            ]
        );
    }

    #[test]
    fn thumbnail_candidates_fallback_without_api_cover() {
        let mut track = sample_track();
        track.thumbnail = None;
        track.external_id = Some("abc123".to_string());
        let candidates = YoutubeProvider::new().thumbnail_candidates(&track);
        assert_eq!(candidates.len(), 4);
        assert!(candidates[0].ends_with("maxresdefault.jpg"));
        assert!(candidates[3].ends_with("default.jpg"));
    }

    fn sample_track() -> Track {
        let mut track = Track::new(
            "Título".to_string(),
            vec![crate::domain::artist::Artist::new(
                "Artista".to_string(),
                None,
                None,
                None,
            )],
            Source::YouTube,
        );
        track.thumbnail = Some(TrackThumbnail {
            url: "cover.jpg".to_string(),
        });
        track
    }
}
