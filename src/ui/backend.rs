//! Backend de la UI: tarea que ejecuta las operaciones de la capa de Aplicación
//! en segundo plano (para no bloquear el renderizado) y devuelve eventos.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use crate::app::aggregator::MetadataAggregator;
use crate::app::audio::{EventBus, PlaybackEvent, PlaybackState, PlaybackStatus};
use crate::app::history::History;
use crate::app::playback::PlaybackRouter;
use crate::app::search::SearchEngine;
use crate::app::thumbnail::{ThumbnailService, ThumbnailState};
use crate::domain::track::Track;
use crate::infrastructure::config::{Config, ConfigForm};
use crate::infrastructure::db::Db;
use crate::infrastructure::playback;
use crate::playback::{decide_recovery, QueueManager, RecoveryAction, RecoveryBudget, RepeatMode};
use crate::recommendation::acoustic_aggregator::AcousticAggregator;
use crate::recommendation::signals::{PlayContext, SignalKind};
use crate::recommendation::types::UserProfile;
use crate::recommendation::{
    aggregate_signals, metadata_similarity, negative_penalty, user_affinity,
};

use super::event::BackendEvent;

/// Comandos que la UI envía al backend.
#[derive(Debug)]
pub enum BackendCommand {
    Search(String),
    SaveTrack(Box<Track>),
    SaveSettings(Box<ConfigForm>),
    LoadHistory,
    LoadListeningStats,
    LoadSources,
    LoadSettings,
    /// Reproduce un track (resuelve el stream y lo delega al motor adecuado).
    Play(Box<Track>),
    Pause,
    Resume,
    Toggle,
    Stop,
    /// Busca una posición (en segundos). Lleva el identificador del track
    /// sobre el que se emitió: si al ejecutarse el track en curso cambió (el
    /// usuario reprodujo otra canción), el seek se Descarta para no saltar
    /// dentro de la canción nueva.
    Seek(u64, String),
    /// Cambia el volumen (0-100).
    Volume(u8),
    /// Pide letra sincronizada (LRCLIB) y recomendados de un video.
    /// Pide recomendaciones + letra para `track`. `generation` identifica la
    /// sesión en vuelo de la UI: la respuesta la repite y la UI la exige para
    /// no aplicar contenido de una sesión anterior.
    LoadRelated(Box<Track>, u64),
    /// Resuelve y cachea la miniatura de un track (para mostrarla en la TUI).
    Thumbnail(Box<Track>),
    /// Activa/desactiva la reproducción automática de recomendaciones.
    SetAutoplay(bool),
    /// REEMPLAZA toda la cola por la lista dada (acción explícita del usuario
    /// desde la vista de recomendaciones): a diferencia del crecimiento
    /// automático, descarta lo acumulado y empieza de cero desde esta lista.
    ReplaceQueue(Vec<Track>),
    /// REEMPLAZA SOLO el segmento de autoplay de la cola (tecla `R`): las
    /// elecciones explícitas del usuario (current, Play Next, Add to Queue,
    /// playlist/search) sobreviven; solo se renueva el fondo automático.
    ReplaceAutoplay(Vec<Track>),
    /// Activa/desactiva el shuffle de la cola (al activarlo, el track actual
    /// queda primero en la permutación).
    SetShuffle(bool),
    /// Cambia el modo de repetición de la cola (Off/All/One).
    SetRepeat(RepeatMode),
    /// Salta a la siguiente recomendación de la cola (Shift+D) y la reproduce.
    NextTrack,
    /// Salta a la anterior recomendación de la cola (Shift+A) y la reproduce.
    PrevTrack,
    // ----------------------------------------------------------- playlists
    ListPlaylists,
    CreatePlaylist(String),
    RenamePlaylist(i64, String),
    DeletePlaylist(i64),
    PlaylistTracks(i64),
    AddToPlaylist(i64, i64),
    RemoveFromPlaylist(i64, i64),
    MovePlaylistTrack(i64, i64, usize),
    SetArtworkOverride(i64, String),
    // ------------------------------------------------------------- L1K3D
    /// Pide el estado L1K3D completo (tracks "liked") para pintar corazones.
    LoadLiked,
    /// Conmuta "liked" de un track (añade/quita de L1K3D) y registra la señal
    /// `Like`/`Unlike` correspondiente.
    ToggleLiked(Box<Track>),
    /// Reproduce una playlist COMPLETA como cola EXPLÍCITA (origen Playlist, no
    /// autoplay): la lista queda encolada y arranca por su primera canción.
    PlayPlaylist(i64),
}

/// Servicios de la capa de Aplicación usados por la TUI.
///
/// Es `Clone` (todo el estado mutable vive en `Arc`): así los comandos pesados
/// (búsquedas, red, `Play`, playlists...) se ejecutan en tareas propias con un
/// clon y el loop principal queda libre para los controles de reproducción,
/// que se procesan en línea y de forma inmediata.
#[derive(Clone)]
pub struct Backend {
    db: Db,
    search: SearchEngine,
    history: History,
    config: Config,
    http: reqwest::Client,
    router: Arc<PlaybackRouter>,
    /// Resolución de streams: caché → router de proveedores → política de
    /// fallos. La lógica que antes vivía en `play_track*` vive ahora aquí.
    resolver: Arc<crate::media::StreamResolver>,
    /// Resuelve/cachea miniaturas de los tracks (descarga + decodificación).
    thumbnails: Arc<ThumbnailService>,
    /// Autoplay de recomendaciones: cuando un track termina, se reproduce la
    /// siguiente de la cola.
    autoplay: Arc<tokio::sync::Mutex<bool>>,
    /// Cola formal de reproducción (navegación, shuffle, repeat). El backend
    /// la mantiene estable entre canciones.
    queue: Arc<tokio::sync::Mutex<QueueManager>>,
    /// Track en curso (para recuperación en caliente).
    current: Arc<tokio::sync::Mutex<Option<Track>>>,
    /// Contexto en que se inició el track en curso (Manual/Queue/Autoplay/
    /// Recommendation). Permite registrar `skip`/`completed` con la semántica
    /// correcta al cambiar de canción (FASE 10/11).
    current_context: Arc<tokio::sync::Mutex<Option<PlayContext>>>,
    /// Acumulador acústico del track en curso: reduce los frames en vivo a un
    /// perfil que se persiste al terminar la reproducción (FASE 8).
    acoustic_since: Arc<tokio::sync::Mutex<Option<AcousticAggregator>>>,
    /// Presupuesto de auto-recuperación: UN refresco de stream por track.
    recovery_budget: Arc<tokio::sync::Mutex<RecoveryBudget>>,
    /// Preparación anticipada del SIGUIENTE track (warm de caché).
    preload: Arc<crate::playback::PreloadManager>,
    /// Bus de features del análisis de audio (`None` si el flag está OFF).
    features: Option<crate::analysis::FeatureBus>,
    /// Bus de envolvente de forma de onda del mismo análisis.
    waveform: Option<crate::analysis::WaveformBus>,
}

impl Backend {
    /// `true` si hay que emitir eventos de features hacia la UI.
    fn visuals_enabled(&self) -> bool {
        self.config.flags.advanced_visualization && self.features.is_some()
    }

    /// Guarda de sesión (FASE 4): un seek es obsoleto si el track sobre el que
    /// se emitió ya no es el que está en curso (o no hay ninguno). Evita que un
    /// seek asíncrono de la canción A salte dentro de la canción B.
    fn seek_is_stale(current: Option<&Track>, for_track: &str) -> bool {
        current.is_none_or(|t| t.identifier() != *for_track)
    }
}

impl Backend {
    pub fn new(db: Db, config: Config) -> Self {
        // Composición del subsistema media: un solo adaptador de YouTube
        // compartido por catálogo, streaming y verificación; resolver con
        // caché memoria+SQLite y política de fallos acotada. Los feature
        // flags deciden si YouTube se registra (apagado ⇒ registros vacíos).
        let composed = crate::api::compose_media(db.clone(), &config.flags);
        let aggregator = Arc::new(MetadataAggregator::new(composed.catalog));
        let http = config
            .apply_proxy_policy(
                reqwest::Client::builder()
                    .user_agent("Tunefold/1.5.3")
                    .timeout(Duration::from_secs(30)),
            )
            .build()
            .expect("cliente HTTP válido");

        // Bus de eventos agrupado: todos los motores notifican aquí. El
        // análisis de audio (si está habilitado) entrega además sus buses de
        // features y forma de onda a los consumidores (visualización).
        let (bus, joined) = EventBus::channel();
        let (engine_config, features, waveform) =
            playback::build_engines(&config, bus, http.clone());
        let router = Arc::new(PlaybackRouter::new(engine_config, joined));

        let preload = Arc::new(crate::playback::PreloadManager::new(
            composed.stream_resolver.clone(),
            crate::playback::PreloadConfig::default(),
        ));

        Self {
            search: SearchEngine::new(db.clone(), aggregator.clone()),
            history: History::new(db.clone()),
            thumbnails: Arc::new(ThumbnailService::new(http.clone(), aggregator)),
            http,
            db,
            config,
            router,
            resolver: composed.stream_resolver,
            autoplay: Arc::new(tokio::sync::Mutex::new(true)),
            queue: Arc::new(tokio::sync::Mutex::new(QueueManager::default())),
            current: Arc::new(tokio::sync::Mutex::new(None)),
            current_context: Arc::new(tokio::sync::Mutex::new(None)),
            acoustic_since: Arc::new(tokio::sync::Mutex::new(None)),
            recovery_budget: Arc::new(tokio::sync::Mutex::new(RecoveryBudget::default())),
            preload,
            features,
            waveform,
        }
    }

    /// Último snapshot de features de audio, si el análisis está activo
    /// (consumidores: visualización, métricas).
    pub fn features(&self) -> Option<&crate::analysis::FeatureBus> {
        self.features.as_ref()
    }

    /// Última envolvente de forma de onda, si el análisis está activo.
    pub fn waveform(&self) -> Option<&crate::analysis::WaveformBus> {
        self.waveform.as_ref()
    }

    /// Reconstruye el agregador de proveedores a partir de la configuración actual.
    fn rebuild_providers(&mut self) {
        let composed = crate::api::compose_media(self.db.clone(), &self.config.flags);
        let aggregator = Arc::new(MetadataAggregator::new(composed.catalog));
        self.search = SearchEngine::new(self.db.clone(), aggregator.clone());
        // El servicio de miniaturas delega en el agregador para las URLs
        // candidatas: se recrea (el caché en disco persiste).
        self.thumbnails = Arc::new(ThumbnailService::new(self.http.clone(), aggregator));
    }

    /// Resuelve el stream de un track y lo entrega al motor de reproducción.
    ///
    /// Toda la estrategia (caché caliente, caché fría SQLite con verificación,
    /// reintentos acotados, fallback entre proveedores, expiración) vive en el
    /// [`crate::media::StreamResolver`]; aquí solo queda la entrega al motor.
    async fn play_track(&self, track: &Track) -> Result<PlaybackStatus, String> {
        let resolution = self
            .resolver
            .resolve(track)
            .await
            .map_err(|e| e.to_string())?;
        let media_source = resolution
            .media_source()
            .ok_or_else(|| format!("resolución no reproducible para {}", track.source))?;

        self.router
            .play(track, Some(media_source))
            .await
            .map_err(|e| e.to_string())
    }

    /// Arranca una reproducción y solo después la hace visible como escucha
    /// persistente. El estado devuelto usa la duración que confirmó el
    /// decodificador cuando los metadatos del listado no la traían.
    ///
    /// `context` es la semántica de la interacción (Manual/Queue/Autoplay/
    /// Recommendation): se registra como señal `Play` y se recuerda para
    /// clasificar el `completed`/`skip` posterior (FASE 10/11).
    async fn start_and_record(
        &self,
        track: Track,
        context: PlayContext,
    ) -> Result<
        (
            PlaybackStatus,
            Vec<crate::infrastructure::storage::TrackListeningStats>,
        ),
        String,
    > {
        // Ancla de la cola, track en curso y presupuesto de recuperación se
        // actualizan ANTES de intentar reproducir (semántica histórica).
        let key = track.identifier();
        self.queue.lock().await.mark_played(&key);
        *self.current.lock().await = Some(track.clone());
        *self.current_context.lock().await = Some(context);
        self.recovery_budget.lock().await.arm(&key);

        let mut status = self.play_track(&track).await?;
        let mut persisted = status.track.clone().unwrap_or(track);
        if persisted.duration.is_none() {
            persisted.duration = status.duration;
        }
        status.track = Some(persisted.clone());

        let mut ids = HashMap::new();
        if let Some(external_id) = persisted.external_id.clone() {
            ids.insert(persisted.source, external_id);
        }
        let internal_id = self
            .search
            .save_track(&persisted, &ids)
            .await
            .map_err(|e| format!("guardar escucha: {e}"))?;

        // Señal real de interacción (FASE 11): un `Play` por contexto. La
        // duración total del track se conserva para normalizar completed/skip.
        let _ = self
            .db
            .record_signal(
                internal_id,
                SignalKind::Play,
                context,
                None,
                None,
                persisted.duration.map(|d| d.as_millis() as i64),
            )
            .await;

        // Nuevo acumulador acústico para esta reproducción (FASE 8): los
        // frames que lleguen vía el bus de features se agregan aquí y, al
        // terminar, se persisten como perfil del track.
        *self.acoustic_since.lock().await = Some(AcousticAggregator::new(internal_id));

        self.history
            .record(internal_id, persisted.source, persisted.duration)
            .await
            .map_err(|e| format!("historial: {e}"))?;
        // El track en curso conserva su id interno: otros consumidores (p. ej.
        // la clasificación de un `skip`) leen un `track_id` estable.
        if let Some(t) = self.current.lock().await.as_mut() {
            t.id = internal_id;
            if t.duration.is_none() {
                t.duration = persisted.duration;
            }
        }
        let stats = self
            .history
            .stats()
            .await
            .map_err(|e| format!("estadísticas: {e}"))?;
        Ok((status, stats))
    }

    /// Comandos ligeros, mutaciones del estado del backend o raros: se ejecutan
    /// en línea en el loop principal (los controles de reproducción no esperan).
    async fn handle(&mut self, cmd: BackendCommand) -> Option<BackendEvent> {
        match cmd {
            BackendCommand::SaveSettings(form) => {
                let form = *form;
                if let Err(e) = Config::persist(&form) {
                    return Some(BackendEvent::Error(format!("guardar ajustes: {e}")));
                }
                self.config.apply_form(&form);
                self.rebuild_providers();
                // Reconstruye router y motores con la nueva política/config
                // (el Drop del AnalysisRuntime viejo detiene su hilo).
                let (bus, joined) = EventBus::channel();
                let (engine_config, features, waveform) =
                    playback::build_engines(&self.config, bus, self.http.clone());
                self.router = Arc::new(PlaybackRouter::new(engine_config, joined));
                self.features = features;
                self.waveform = waveform;
                Some(BackendEvent::Settings(self.config.form()))
            }
            BackendCommand::LoadHistory => match self.history.recent(30).await {
                Ok(entries) => Some(BackendEvent::History(entries)),
                Err(e) => Some(BackendEvent::Error(format!("historial: {e}"))),
            },
            BackendCommand::LoadListeningStats => match self.history.stats().await {
                Ok(stats) => Some(BackendEvent::ListeningStats(stats)),
                Err(e) => Some(BackendEvent::Error(format!("estadísticas: {e}"))),
            },
            BackendCommand::LoadSources => {
                Some(BackendEvent::Sources(self.config.available_sources()))
            }
            BackendCommand::LoadSettings => Some(BackendEvent::Settings(self.config.form())),
            BackendCommand::Pause => match self.router.pause().await {
                Ok(status) => Some(BackendEvent::Playback(status)),
                Err(e) => Some(BackendEvent::PlaybackError(e.to_string())),
            },
            BackendCommand::Resume => match self.router.resume().await {
                Ok(status) => Some(BackendEvent::Playback(status)),
                Err(e) => Some(BackendEvent::PlaybackError(e.to_string())),
            },
            BackendCommand::Toggle => {
                let status = self.router.status().await;
                if status.state == PlaybackState::Playing {
                    self.router
                        .pause()
                        .await
                        .map(BackendEvent::Playback)
                        .ok()
                        .or(Some(BackendEvent::Playback(status)))
                } else if status.state == PlaybackState::Paused {
                    self.router
                        .resume()
                        .await
                        .map(BackendEvent::Playback)
                        .ok()
                        .or(Some(BackendEvent::Playback(status)))
                } else {
                    Some(BackendEvent::Playback(status))
                }
            }
            BackendCommand::Stop => match self.router.stop().await {
                Ok(status) => Some(BackendEvent::Playback(status)),
                Err(e) => Some(BackendEvent::PlaybackError(e.to_string())),
            },
            BackendCommand::Volume(vol) => match self.router.set_volume(vol).await {
                Ok(status) => Some(BackendEvent::Playback(status)),
                Err(e) => Some(BackendEvent::PlaybackError(e.to_string())),
            },
            // ---------------------------------------------------- autoplay
            BackendCommand::SetAutoplay(enabled) => {
                *self.autoplay.lock().await = enabled;
                None
            }
            BackendCommand::ReplaceQueue(tracks) => {
                let state = {
                    let mut guard = self.queue.lock().await;
                    guard.set_tracks(tracks);
                    BackendEvent::QueueState {
                        shuffle: guard.shuffle_active(),
                        repeat: guard.repeat(),
                        len: guard.len(),
                    }
                };
                Some(state)
            }
            BackendCommand::ReplaceAutoplay(tracks) => {
                // Tecla `R`: renueva SOLO el fondo autoplay. Las elecciones
                // explícitas (current + Play Next + playlist/search) sobreviven
                // y conservan su orden; solo se redibuja el segmento automático.
                let (queue, origins) = {
                    let mut guard = self.queue.lock().await;
                    guard.replace_autoplay(&tracks);
                    (guard.tracks().to_vec(), guard.origins().to_vec())
                };
                Some(BackendEvent::Queue { queue, origins })
            }
            BackendCommand::SetShuffle(on) => {
                let state = {
                    let mut guard = self.queue.lock().await;
                    guard.set_shuffle(on);
                    BackendEvent::QueueState {
                        shuffle: guard.shuffle_active(),
                        repeat: guard.repeat(),
                        len: guard.len(),
                    }
                };
                Some(state)
            }
            BackendCommand::SetRepeat(mode) => {
                let state = {
                    let mut guard = self.queue.lock().await;
                    guard.set_repeat(mode);
                    BackendEvent::QueueState {
                        shuffle: guard.shuffle_active(),
                        repeat: guard.repeat(),
                        len: guard.len(),
                    }
                };
                Some(state)
            }
            // Comandos pesados: se procesan en tareas propias (ver `spawn_backend`).
            _ => unreachable!("comando pesado atendido por `handle_heavy`"),
        }
    }

    /// Comandos pesados (red, caché de letras/miniaturas, playlists, `Play`):
    /// se ejecutan sobre un clon del backend, en una tarea propia, para no
    /// bloquear los controles de reproducción.
    ///
    /// Devuelve una lista de eventos (una mutación puede emitir varios, p. ej.
    /// L1K3DChanged + refresco del listado): el loop de la UI los aplica en
    /// orden sin necesitar re-consultas por cada efecto colateral.
    async fn handle_heavy(&self, cmd: BackendCommand) -> Vec<BackendEvent> {
        match cmd {
            BackendCommand::Search(query) => {
                if self.search.aggregator().is_empty() {
                    vec![BackendEvent::Message(
                        "No hay fuentes de catálogo activas (revisa los feature flags en .env)."
                            .to_string(),
                    )]
                } else {
                    let outcome = self.search.search_tracks(&query, 10).await;
                    match outcome {
                        Ok(outcome) => vec![BackendEvent::SearchResults {
                            query,
                            outcome: Box::new(outcome),
                        }],
                        Err(e) => vec![BackendEvent::Error(format!("búsqueda: {e}"))],
                    }
                }
            }
            BackendCommand::SaveTrack(track) => {
                let track = *track;
                let mut ids = HashMap::new();
                if let Some(ext) = track.external_id.clone() {
                    ids.insert(track.source, ext);
                }
                match self.search.save_track(&track, &ids).await {
                    Ok(internal_id) => vec![BackendEvent::TrackSaved {
                        track: Box::new(track),
                        internal_id,
                    }],
                    Err(e) => vec![BackendEvent::Error(format!("guardar: {e}"))],
                }
            }
            BackendCommand::Play(track) => {
                // El usuario eligió explícitamente reproducir: contexto Manual.
                // (Los contextos Queue/Autoplay salen de `skip_track` y
                // `autoplay_next`.)
                match self.start_and_record(*track, PlayContext::Manual).await {
                    Ok((status, stats)) => vec![BackendEvent::PlaybackStarted { status, stats }],
                    Err(e) => vec![BackendEvent::PlaybackError(e)],
                }
            }
            BackendCommand::NextTrack => match self.skip_track(true).await {
                Ok(Some((status, stats))) => vec![BackendEvent::PlaybackStarted { status, stats }],
                Ok(None) => Vec::new(),
                Err(e) => vec![BackendEvent::Message(e)],
            },
            BackendCommand::PrevTrack => match self.skip_track(false).await {
                Ok(Some((status, stats))) => vec![BackendEvent::PlaybackStarted { status, stats }],
                Ok(None) => Vec::new(),
                Err(e) => vec![BackendEvent::Message(e)],
            },
            BackendCommand::LoadRelated(track, generation) => {
                let track = *track;
                let video_id = track.external_id.clone().unwrap_or_default();
                // Recomendados desde YouTube Music (solo esa parte).
                let mut related = self.search.aggregator().related(&video_id).await;
                self.search
                    .aggregator()
                    .hydrate_missing_durations(&mut related)
                    .await;
                // Letra sincronizada desde LRCLIB: única fuente del karaoke.
                let mut synced = self.search.aggregator().synced_lyrics(&track).await;

                // Id canónico del track (el ya guardado en la BD): permite leer
                // y escribir la caché de letras aunque este track llegara sin
                // `id` interno (p. ej. recién buscado y reproducido en vivo).
                let cache_id = if track.id > 0 {
                    Some(track.id)
                } else if !video_id.is_empty() {
                    self.db
                        .internal_id_for_external(&video_id)
                        .await
                        .ok()
                        .flatten()
                } else {
                    None
                };
                if let Some(id) = cache_id {
                    // Caché local como respaldo cuando LRCLIB no responde
                    // (offline, rate-limit 429...): se recupera el LRC de una
                    // sesión anterior. La caché solo guarda sincronizadas.
                    if synced.is_none() {
                        synced = self.db.get_synced_lyrics(id).await.ok().flatten();
                    }
                    if let Some(s) = &synced {
                        let _ = self.db.cache_synced_lyrics(id, s).await;
                    }
                }
                // Reordena/filtra los recomendados por el gusto local del
                // usuario (FASE 9/10): lo que encaja con su perfil sube y lo
                // que rechazó claramente baja o se descarta.
                self.reorder_by_local_taste(&mut related).await;
                // La cola de autoplay CRECE con las recomendaciones de cada
                // canción (dedupe, sin reemplazar lo ya encolado): así nunca se
                // pierde la lista que el usuario estaba construyendo y el
                // autoplay avanza por todo el fondo acumulado en vez de dar
                // vueltas sobre ~7 canciones.
                let (queue, origins) = {
                    let mut queue_guard = self.queue.lock().await;
                    queue_guard.append_unique_with_origin(
                        &related,
                        crate::playback::QueueItemOrigin::Recommendation,
                    );
                    (
                        queue_guard.tracks().to_vec(),
                        queue_guard.origins().to_vec(),
                    )
                };
                vec![BackendEvent::Related {
                    track: Box::new(track),
                    related,
                    queue,
                    origins,
                    synced,
                    generation,
                }]
            }
            BackendCommand::Thumbnail(track) => {
                let track = *track;
                let key = track.identifier();
                let state = self.thumbnails.prepare(&track).await;
                // La paleta del artwork se persiste UNA vez, al decodificar
                // (nunca durante el render): queda como caché reutilizable y
                // el karaoke/visual pueden leerla aunque la miniatura aún no
                // estuviera decodificada en memoria.
                if let (ThumbnailState::Loaded(img), true) = (&state, track.id > 0) {
                    if let Some(palette) = img.palette {
                        let _ = self.db.set_track_palette(track.id, palette).await;
                    }
                }
                vec![BackendEvent::Thumbnail { key, state }]
            }
            // `Seek` puede bloquear hasta que la región objetivo del stream
            // quede descargada (streams HTTP progresivos): fuera del loop para
            // no congelar los controles durante un salto hacia delante.
            // Guarda de sesión (FASE 4): si mientras tanto arrancó otra
            // canción (el track en curso ya no coincide con este seek) la
            // operación es obsoleta y debe descartarse — de otro modo saltaría
            // dentro de la canción nueva.
            BackendCommand::Seek(secs, for_track) => {
                let stale = Self::seek_is_stale(self.current.lock().await.as_ref(), &for_track);
                if stale {
                    return Vec::new();
                }
                match self.router.seek(Duration::from_secs(secs)).await {
                    Ok(status) => vec![BackendEvent::Playback(status)],
                    Err(e) => vec![BackendEvent::PlaybackError(e.to_string())],
                }
            }
            // ------------------------------------------------------ playlists
            BackendCommand::ListPlaylists => match self.db.list_playlists().await {
                Ok(pls) => vec![BackendEvent::Playlists(pls)],
                Err(e) => vec![BackendEvent::Error(format!("playlists: {e}"))],
            },
            BackendCommand::CreatePlaylist(name) => match self.db.create_playlist(&name).await {
                Ok(_) => self.refresh_playlists().await,
                Err(e) => vec![BackendEvent::Error(format!("crear playlist: {e}"))],
            },
            BackendCommand::RenamePlaylist(id, name) => {
                match self.db.rename_playlist(id, &name).await {
                    Ok(()) => self.refresh_playlists().await,
                    Err(e) => vec![BackendEvent::Error(format!("renombrar playlist: {e}"))],
                }
            }
            BackendCommand::DeletePlaylist(id) => match self.db.delete_playlist(id).await {
                Ok(()) => self.refresh_playlists().await,
                Err(e) => vec![BackendEvent::Error(format!("borrar playlist: {e}"))],
            },
            BackendCommand::PlaylistTracks(id) => match self.db.playlist_tracks(id).await {
                Ok(tracks) => vec![BackendEvent::PlaylistTracks {
                    playlist_id: id,
                    tracks,
                }],
                Err(e) => vec![BackendEvent::Error(format!("tracks playlist: {e}"))],
            },
            BackendCommand::AddToPlaylist(pid, tid) => {
                match self.db.add_to_playlist(pid, tid).await {
                    Ok(()) => self.refresh_playlists().await,
                    Err(e) => vec![BackendEvent::Error(format!("añadir: {e}"))],
                }
            }
            BackendCommand::RemoveFromPlaylist(pid, tid) => {
                match self.db.remove_from_playlist(pid, tid).await {
                    Ok(()) => self.refresh_playlists().await,
                    Err(e) => vec![BackendEvent::Error(format!("quitar: {e}"))],
                }
            }
            BackendCommand::MovePlaylistTrack(pid, tid, to) => {
                match self.db.move_playlist_track(pid, tid, to).await {
                    Ok(()) => match self.db.playlist_tracks(pid).await {
                        Ok(tracks) => vec![
                            BackendEvent::PlaylistTracks {
                                playlist_id: pid,
                                tracks,
                            },
                            BackendEvent::Message("Playlist reordenada.".to_string()),
                        ],
                        Err(e) => vec![BackendEvent::Error(format!("tracks playlist: {e}"))],
                    },
                    Err(e) => vec![BackendEvent::Error(format!("reordenar: {e}"))],
                }
            }
            BackendCommand::SetArtworkOverride(tid, image) => {
                match self.db.set_artwork_override(tid, &image).await {
                    Ok(()) => vec![BackendEvent::Message("Portada actualizada.".to_string())],
                    Err(e) => vec![BackendEvent::Error(format!("portada: {e}"))],
                }
            }
            // ------------------------------------------------------------- L1K3D
            BackendCommand::LoadLiked => match self.db.liked_tracks().await {
                Ok(tracks) => vec![BackendEvent::L1K3D(tracks)],
                Err(e) => vec![BackendEvent::Error(format!("L1K3D: {e}"))],
            },
            BackendCommand::ToggleLiked(track) => {
                let track = *track;
                // Primero se persiste el track (id interno canónico). La señal
                // Like/Unlike queda ligada a ese id, igual que todo lo demás.
                let mut ids = HashMap::new();
                if let Some(ext) = track.external_id.clone() {
                    ids.insert(track.source, ext);
                }
                let internal_id = match self.search.save_track(&track, &ids).await {
                    Ok(id) => id,
                    Err(e) => return vec![BackendEvent::Error(format!("guardar: {e}"))],
                };
                let liked = match self.db.is_liked(internal_id).await {
                    Ok(l) => l,
                    Err(e) => return vec![BackendEvent::Error(format!("L1K3D: {e}"))],
                };
                let toggled_on = !liked;
                if let Err(e) = self.db.set_liked(internal_id, toggled_on).await {
                    return vec![BackendEvent::Error(format!("L1K3D: {e}"))];
                }
                // Señal de interacción real del usuario: `Like`/`Unlike` (jamás
                // `PlaylistAdd` — me gusta ≠ meter en una lista normal).
                let _ = self
                    .db
                    .record_signal(
                        internal_id,
                        if toggled_on {
                            SignalKind::Like
                        } else {
                            SignalKind::Unlike
                        },
                        PlayContext::Manual,
                        None,
                        None,
                        None,
                    )
                    .await;
                let mut ev = vec![BackendEvent::L1K3DChanged {
                    track: Box::new(track),
                    liked: toggled_on,
                }];
                // El contador de L1K3D en el listado debe reflejar el cambio.
                ev.extend(self.refresh_playlists().await);
                ev
            }
            BackendCommand::PlayPlaylist(playlist_id) => {
                match self.db.playlist_tracks(playlist_id).await {
                    Ok(playlist_tracks) if !playlist_tracks.is_empty() => {
                        // La playlist se impone como COLA EXPLÍCITA (origen
                        // Playlist, no autoplay): reproduce justo lo que el
                        // usuario eligió, en su orden persistente.
                        let (len, shuffle, repeat) = {
                            let mut queue_guard = self.queue.lock().await;
                            queue_guard.set_tracks_with_origin(
                                playlist_tracks.clone(),
                                crate::playback::QueueItemOrigin::Playlist,
                            );
                            (
                                queue_guard.len(),
                                queue_guard.shuffle_active(),
                                queue_guard.repeat(),
                            )
                        };
                        let order = {
                            let q = self.queue.lock().await;
                            (q.tracks().to_vec(), q.origins().to_vec())
                        };
                        let mut ev = vec![
                            BackendEvent::Queue {
                                queue: order.0,
                                origins: order.1,
                            },
                            BackendEvent::QueueState {
                                shuffle,
                                repeat,
                                len,
                            },
                        ];
                        let first = playlist_tracks[0].clone();
                        match self.start_and_record(first, PlayContext::Manual).await {
                            Ok((status, stats)) => {
                                ev.push(BackendEvent::PlaybackStarted { status, stats })
                            }
                            Err(e) => ev.push(BackendEvent::PlaybackError(e)),
                        }
                        ev
                    }
                    Ok(_) => vec![BackendEvent::Message(
                        "La playlist no tiene canciones.".to_string(),
                    )],
                    Err(e) => vec![BackendEvent::Error(format!("playlist: {e}"))],
                }
            }
            // Comandos ligeros: se procesan en línea en `handle`.
            _ => unreachable!("comando ligero atendido por `handle`"),
        }
    }

    /// Eventos de refresco tras una mutación de playlists: el listado y —si la
    /// mutación tocó L1K3D— el estado de "likes" se re-emiten para que la UI
    /// nunca se quede con contadores o corazones obsoletos.
    async fn refresh_playlists(&self) -> Vec<BackendEvent> {
        match self.db.list_playlists().await {
            Ok(pls) => vec![BackendEvent::Playlists(pls)],
            Err(e) => vec![BackendEvent::Error(format!("playlists: {e}"))],
        }
    }

    /// Track de la cola en la dirección pedida, relativo al último reproducido
    /// (la navegación vive en [`QueueManager`]; el backend solo decide si hay
    /// cola que recorrer).
    async fn skip_track(
        &self,
        forward: bool,
    ) -> Result<
        Option<(
            PlaybackStatus,
            Vec<crate::infrastructure::storage::TrackListeningStats>,
        )>,
        String,
    > {
        let next = {
            let mut queue = self.queue.lock().await;
            queue.pick(forward, None)
        };
        let Some(next) = next else {
            return Err(
                "Sin cola de recomendaciones. Reproduce una canción para cargarlas.".to_string(),
            );
        };
        // El usuario saltó la canción que estaba sonando: se registra como señal
        // `Skip` (con el contexto en que se reprodujo) ANTES de pisar el track
        // en curso con la siguiente (FASE 11). El autoplay no genera aversión:
        // lo filtra `is_meaningful_negative`.
        self.record_skip().await;
        match self.start_and_record(next, PlayContext::Queue).await {
            Ok(result) => Ok(Some(result)),
            Err(e) => Err(format!("salto: {e}")),
        }
    }

    /// Registra una señal `Skip` para el track en curso, si hay uno y su id
    /// interno es conocido. Se usa al saltar manualmente la canción.
    async fn record_skip(&self) {
        let (track_id, ctx) = {
            let track = self.current.lock().await.clone();
            let ctx = *self.current_context.lock().await;
            (track.and_then(|t| (t.id > 0).then_some(t.id)), ctx)
        };
        let Some(track_id) = track_id else { return };
        let _ = self
            .db
            .record_signal(
                track_id,
                SignalKind::Skip,
                ctx.unwrap_or(PlayContext::Manual),
                None,
                None,
                None,
            )
            .await;
    }

    /// Al terminar una canción: registra la señal `Completed` (se escuchó
    /// completa) y persiste el perfil acústico acumulado durante la
    /// reproducción (FASE 8). Ambos fallos son no-críticos.
    async fn record_completed_and_persist_acoustic(&self) {
        let (track_id, ctx, duration_ms) = {
            let track = self.current.lock().await.clone();
            let ctx = *self.current_context.lock().await;
            (
                track.as_ref().and_then(|t| (t.id > 0).then_some(t.id)),
                ctx,
                track
                    .as_ref()
                    .and_then(|t| t.duration)
                    .map(|d| d.as_millis() as i64),
            )
        };
        if let Some(track_id) = track_id {
            // `Completed` implica duración completa: duration == track_duration
            // para que `is_completion()` sea cierto y la señal pese positivo.
            if let Some(ms) = duration_ms {
                let _ = self
                    .db
                    .record_signal(
                        track_id,
                        SignalKind::Completed,
                        ctx.unwrap_or(PlayContext::Manual),
                        Some(ms),
                        None,
                        Some(ms),
                    )
                    .await;
            }
            // Perfil acústico: si hubo suficientes frames, se persiste para
            // que el ranking local pueda comparar por sonido sin re-analizar.
            let profile = self
                .acoustic_since
                .lock()
                .await
                .take()
                .and_then(|a| a.into_profile());
            if let Some(p) = profile {
                let _ = self.db.save_acoustic_profile(&p).await;
            }
        }
    }

    /// Reordena y filtra una lista de recomendados por el gusto LOCAL del
    /// usuario (FASE 9/10). Sin datos de interacción la lista se deja tal cual
    /// (mantiene el orden de YouTube). Con datos, lo que encaja con el perfil
    /// sube y lo que el usuario rechazó claramente se descarta.
    ///
    /// Falla silencioso: si algo no se puede cargar (red de señales, acústica)
    /// se conserva la lista original — el descubrimiento siempre funciona.
    async fn reorder_by_local_taste(&self, related: &mut Vec<Track>) {
        let Ok(signals) = self.db.all_signals(20_000).await else {
            return;
        };
        if signals.is_empty() {
            return;
        }
        let (Ok(all_tracks), Ok(acoustic_profiles)) = (
            self.db.all_tracks().await,
            self.db.all_acoustic_profiles().await,
        ) else {
            return;
        };
        let mut tracks_map = HashMap::new();
        for t in &all_tracks {
            tracks_map.insert(t.id, t.clone());
        }
        let profile = UserProfile::from_signals(&signals, &tracks_map, &acoustic_profiles);
        let signals_by_track = aggregate_signals(&signals);
        let history = self.history.stats().await.ok();

        let mut scored: Vec<(Track, f64, crate::recommendation::types::TrackSignals)> = Vec::new();
        for mut t in related.drain(..) {
            // Resuelve el id interno del candidato para consultar su señal y
            // su perfil acústico (los candidatos frescos de YouTube llegan con
            // `id == 0`).
            let internal = match t.external_id.as_deref() {
                Some(ext) => self.db.internal_id_for_external(ext).await.ok().flatten(),
                None => None,
            };
            if let Some(id) = internal {
                t.id = id;
            }
            let meta = metadata_similarity(&t, &profile);
            let affinity = match &history {
                Some(h) => user_affinity(&t, h, &profile.acoustic_profile, &acoustic_profiles),
                None => 0.0,
            };
            let sig = signals_by_track.get(&t.id).copied().unwrap_or_default();
            let negative = negative_penalty(sig.negative, sig.plays);
            let local = (0.5 * meta + 0.5 * affinity) * negative;
            scored.push((t, local, sig));
        }
        // Solo se descarta lo que el usuario rechazó CLARAMENTE: al menos dos
        // negativas y en número >= los intentos. Un único skip (o un skip de
        // autoplay, que ya filtra `aggregate_signals`) no elimina un track.
        scored.retain(|(_, _, sig)| !(sig.negative >= 2 && sig.negative >= sig.plays.max(1)));
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        related.extend(scored.into_iter().map(|(t, _, _)| t));
    }

    /// Reproduce la siguiente recomendación si el autoplay está activo.
    /// Devuelve `Ok(None)` cuando no hay autoplay o cola que seguir.
    ///
    /// `finished_after` es la canción que acababa de terminar cuando se disparó
    /// el `Finished`: si mientras esta tarea resolvía la cola el usuario lanzó
    /// otra reproducción (el ancla cambió), la cola quedó obsoleta y no debe
    /// pisar la canción que el usuario pidió.
    async fn autoplay_next(
        &self,
        finished_after: Option<String>,
    ) -> Result<
        Option<(
            PlaybackStatus,
            Vec<crate::infrastructure::storage::TrackListeningStats>,
        )>,
        String,
    > {
        let autoplay = *self.autoplay.lock().await;
        if !autoplay || self.queue.lock().await.is_empty() {
            return Ok(None);
        }
        if self.queue.lock().await.last_played() != finished_after.as_deref() {
            return Ok(None);
        }
        let next = self
            .queue
            .lock()
            .await
            .pick(true, finished_after.as_deref())
            .ok_or_else(|| "sin siguiente recomendación".to_string())?;
        match self.start_and_record(next, PlayContext::Autoplay).await {
            Ok(result) => Ok(Some(result)),
            Err(e) => Err(format!("autoplay: {e}")),
        }
    }

    /// Recuperación en caliente: re-resuelve el stream del track en curso con
    /// una resolución FRESCA y reintenta la reproducción (spec §37).
    ///
    /// Solo se llama cuando el presupuesto lo permite y la clasificación dijo
    /// `RefreshAndResume`; un fallo aquí NO reinicia la app ni rompe la cola:
    /// degrada al aviso original.
    async fn recover_current(&self, original_notice: String) -> RecoveryOutcome {
        let Some(track) = self.current.lock().await.clone() else {
            return RecoveryOutcome::Failed(original_notice);
        };
        tracing::warn!(key = %track.identifier(), "playback_recovery_started");
        let fresh = async { self.resolver.refresh(&track).await.ok()?.media_source() };
        match fresh.await {
            Some(media_source) => match self.router.play(&track, Some(media_source)).await {
                Ok(status) => {
                    tracing::info!(key = %track.identifier(), "playback_recovery_success");
                    RecoveryOutcome::Resumed(status)
                }
                Err(_) => RecoveryOutcome::Failed(original_notice),
            },
            None => {
                tracing::warn!(key = %track.identifier(), "playback_recovery_failed");
                RecoveryOutcome::Failed(original_notice)
            }
        }
    }
}

/// Resultado de una recuperación en caliente.
#[allow(clippy::large_enum_variant)] // mismo patrón que BackendEvent
enum RecoveryOutcome {
    Resumed(PlaybackStatus),
    /// No se pudo: se informa con el aviso ORIGINAL (nunca ocultar la causa).
    Failed(String),
}

/// ¿Un evento del bus de reproducción exige reenviar a la UI el estado REAL
/// del motor de inmediato, sin esperar al tick de 500 ms?
///
/// Son los cambios de estado: un corte por rellenado de buffer (`Buffering`),
/// la reanudación (`Playing`), una pausa (`Paused`) y la parada (`Stopped`).
/// El ticker los acabaría reenviando igualmente, pero cada ~500 ms de retraso
/// deja al reloj de letras extrapolando por delante de un audio congelado. Los
/// demás eventos tienen reenvío propio (seek/error/fin).
fn forwards_real_state(ev: &PlaybackEvent) -> bool {
    matches!(
        ev,
        PlaybackEvent::Buffering
            | PlaybackEvent::Playing
            | PlaybackEvent::Paused
            | PlaybackEvent::Stopped
    )
}

/// Lanza la tarea del backend y devuelve (emisor de comandos, receptor de eventos).
pub fn spawn_backend(
    mut backend: Backend,
) -> (
    UnboundedSender<BackendCommand>,
    UnboundedReceiver<BackendEvent>,
) {
    let (cmd_tx, mut cmd_rx) = unbounded_channel::<BackendCommand>();
    let (event_tx, event_rx) = unbounded_channel::<BackendEvent>();
    let router = backend.router.clone();

    tokio::spawn(async move {
        // Receptor de eventos de reproducción del router.
        let mut playback_events = router.subscribe();
        // Informa del progreso de reproducción dos veces por segundo.
        let mut ticker = tokio::time::interval(Duration::from_millis(500));
        // Muestreo de features para la visualización (~15 Hz): cada evento
        // redibuja la TUI, así que el ritmo visual sigue al flujo real.
        let mut visual_ticker = tokio::time::interval(Duration::from_millis(66));
        visual_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last_sent_features: Option<std::time::Duration> = None;
        loop {
            tokio::select! {
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(cmd) => {
                            // Comandos pesados (red, `Play` con su prebuffer,
                            // playlists): se lanzan en una tarea propia con un
                            // clon, de modo que los controles de reproducción
                            // del siguiente comando se atienden de inmediato.
                            if is_heavy(&cmd) {
                                let backend = backend.clone();
                                let event_tx = event_tx.clone();
                                tokio::spawn(async move {
                                    for event in backend.handle_heavy(cmd).await {
                                        let _ = event_tx.send(event);
                                    }
                                });
                            } else if let Some(event) = backend.handle(cmd).await {
                                let _ = event_tx.send(event);
                            }
                        }
                        None => break,
                    }
                }
                ev = playback_events.recv() => {
                    match ev {
                        // Fin de canción: el corte por límite de red (Cut) se
                        // trata EXACTAMENTE igual — la canción termina en el
                        // prefijo servible, nunca se simula una continuación
                        // (repetir desde el inicio) que confunde al usuario.
                        Ok(ev @ (PlaybackEvent::Finished | PlaybackEvent::Cut(_))) => {
                            if let PlaybackEvent::Cut(msg) = ev {
                                // Aviso discreto del porqué del fin (pie de
                                // página con caducidad, no toca la línea de
                                // estado).
                                let _ = event_tx.send(BackendEvent::StreamError(msg));
                            }
                            // Informa del fin de inmediato: la UI deja de
                            // pintar la canción terminada (karaoke limpio,
                            // estado detenido) mientras el autoplay resuelve
                            // la siguiente.
                            let _ = event_tx.send(BackendEvent::Playback(
                                PlaybackStatus::idle(),
                            ));
                            // La canción terminó naturalmente: señal `Completed`
                            // y persistencia del perfil acústico acumulado.
                            backend.record_completed_and_persist_acoustic().await;
                            // El autoplay reproduce la siguiente canción (con
                            // su prebuffer): fuera del loop para no bloquear.
                            let backend = backend.clone();
                            let event_tx = event_tx.clone();
                            // La canción que acaba de terminar: si el usuario
                            // lanza otra mientras se resuelve la cola, el
                            // autoplay debe abortarse (ver `autoplay_next`).
                            let finished_after = backend
                                .queue
                                .lock()
                                .await
                                .last_played()
                                .map(str::to_string);
                            tokio::spawn(async move {
                                match backend.autoplay_next(finished_after.clone()).await {
                                    Ok(Some((status, stats))) => {
                                        let _ = event_tx.send(BackendEvent::PlaybackStarted { status, stats });
                                    }
                                    Ok(None) => {}
                                    Err(e) => {
                                        // Solo se reporta el fallo si no hubo
                                        // una reproducción nueva del usuario
                                        // mientras se resolvía la cola (ese
                                        // fallo de autoplay es irrelevante).
                                        if backend.queue.lock().await.last_played()
                                            == finished_after.as_deref()
                                        {
                                            let _ = event_tx.send(BackendEvent::PlaybackError(e));
                                        }
                                    }
                                }
                            });
                        }
                        Ok(PlaybackEvent::Error(msg)) => {
                            // Errores en caliente: primero se intenta la
                            // recuperación acotada (UN refresco por track);
                            // si no procede o falla, queda el aviso ORIGINAL
                            // como pie discreto.
                            //
                            // ANTES de nada, se reenvía el estado REAL del
                            // motor. Al llegar el error el router ya está
                            // `Stopped` (el monitor fija el estado antes de
                            // emitir), pero hasta aquí la UI seguía creyendo
                            // que estaba `Playing` y su reloj de letras seguía
                            // extrapolando indefinidamente (el ticker no
                            // reenvía estados `Stopped`). Enviar el snapshot
                            // cierra ese hueco: la UI se entera del corte
                            // aunque el error resulte recuperable.
                            let _ = event_tx.send(BackendEvent::Playback(
                                router.status().await,
                            ));
                            let event_for_decision = PlaybackEvent::Error(msg.clone());
                            let key = backend
                                .current
                                .lock()
                                .await
                                .as_ref()
                                .map(|t| t.identifier());
                            // Orden deliberado: clasificar ANTES de gastar el
                            // presupuesto (un fallo no recuperable no lo quema).
                            let mut allowed =
                                decide_recovery(&event_for_decision)
                                    == RecoveryAction::RefreshAndResume;
                            match key.as_deref() {
                                Some(k) if allowed => {
                                    allowed = backend
                                        .recovery_budget
                                        .lock()
                                        .await
                                        .try_consume(k);
                                }
                                _ => allowed = false,
                            }
                            if allowed {
                                let backend = backend.clone();
                                let event_tx = event_tx.clone();
                                tokio::spawn(async move {
                                    match backend.recover_current(msg.clone()).await {
                                        RecoveryOutcome::Resumed(status) => {
                                            let _ = event_tx.send(BackendEvent::Message(
                                                "stream renovado tras un fallo; seguimos donde estabas.".to_string(),
                                            ));
                                            // Evento DEDICADO: la UI debe tratar el
                                            // reinicio como replay del mismo track
                                            // (rebobinar el reloj de letras), jamás como
                                            // un tick normal — el guard monótono del
                                            // PositionClock lo dejaría clavado en la
                                            // posición previa.
                                            let _ = event_tx.send(BackendEvent::RecoveryResumed(status));
                                        }
                                        RecoveryOutcome::Failed(original) => {
                                            let _ = event_tx.send(BackendEvent::StreamError(original));
                                        }
                                    }
                                });
                            } else {
                                let _ = event_tx.send(BackendEvent::StreamError(msg));
                            }
                        }
                        // Estado de seek: lo reenviamos para que la UI confirme
                        // o cancele el reloj sin depender de heurísticas.
                        Ok(PlaybackEvent::SeekStarted) => {
                            let _ = event_tx.send(BackendEvent::SeekStarted);
                        }
                        Ok(PlaybackEvent::SeekCompleted) => {
                            let _ = event_tx.send(BackendEvent::SeekCompleted);
                        }
                        Ok(PlaybackEvent::SeekFailed) => {
                            let _ = event_tx.send(BackendEvent::SeekFailed);
                        }
                        // Cambios de estado del motor: NO se descartan. Aunque el
                        // ticker de 500 ms los reenvíe cada ciclo, la UI debe
                        // enterarse de un corte (buffer bajo el stream en
                        // `Buffering`, pausa, parada) EN CUANTO ocurre: hasta el
                        // tick siguiente su reloj de letras seguiría extrapolando
                        // — hasta ~500 ms por delante de un audio congelado — y
                        // las letras se adelantan y luego rebotan (el bug
                        // "se están acelerando"). Un snapshot aqui cierra el hueco.
                        Ok(ev) if forwards_real_state(&ev) => {
                            let _ = event_tx.send(BackendEvent::Playback(
                                backend.router.status().await,
                            ));
                        }
                        _ => {}
                    }
                }
                _ = visual_ticker.tick() => {
                    // Agregación acústica del track en curso: se alimenta
                    // SIEMPRE que haya features (independientemente del switcher
                    // de visualización) para poder persistir el perfil al
                    // terminar (FASE 8).
                    if let Some(bus) = backend.features() {
                        if let Some(f) = bus.latest() {
                            if let Some(agg) = backend.acoustic_since.lock().await.as_mut() {
                                agg.add(&f);
                            }
                        }
                    }
                    if backend.visuals_enabled() {
                        if let Some(bus) = backend.features() {
                            if let Some(f) = bus.latest() {
                                if last_sent_features != Some(f.timestamp) {
                                    last_sent_features = Some(f.timestamp);
                                    // La envolvente del MISMO frame viaja con
                                    // las features en un único evento (spec).
                                    let waveform = backend
                                        .waveform()
                                        .and_then(|wb| wb.latest());
                                    let _ = event_tx.send(BackendEvent::VisualFrame {
                                        features: std::sync::Arc::clone(&f),
                                        waveform,
                                    });
                                }
                            }
                        }
                    }
                }
                _ = ticker.tick() => {
                    let status = backend.router.status().await;
                    if status.state != PlaybackState::Stopped {
                        let _ = event_tx.send(BackendEvent::Playback(status.clone()));
                    }
                    // Preparación anticipada del SIGUIENTE track: solo cuando
                    // el actual entra en la ventana final (spec §36). Nunca se
                    // resuelve la cola entera; el warm va a la caché.
                    if status.state == PlaybackState::Playing {
                        let current_id = status.track.as_ref().map(|t| t.identifier());
                        if let Some(current_id) = current_id {
                            let next = backend
                                .queue
                                .lock()
                                .await
                                .pick(true, Some(current_id.as_str()));
                            if crate::playback::should_preload(
                                status.position,
                                status.duration,
                                next.is_some(),
                                backend.preload.config(),
                            ) {
                                backend.preload.consider(next).await;
                            }
                        }
                    }
                }
            }
        }
    });

    (cmd_tx, event_rx)
}

/// `true` para los comandos que pueden tardar (red, disco, `Play` con su
/// prebuffer). Se procesan en tareas propias para no bloquear los controles.
fn is_heavy(cmd: &BackendCommand) -> bool {
    matches!(
        cmd,
        BackendCommand::Search(_)
            | BackendCommand::SaveTrack(_)
            | BackendCommand::Play(_)
            | BackendCommand::NextTrack
            | BackendCommand::PrevTrack
            | BackendCommand::LoadRelated(..)
            | BackendCommand::Thumbnail(_)
            | BackendCommand::Seek(..)
            | BackendCommand::ListPlaylists
            | BackendCommand::CreatePlaylist(_)
            | BackendCommand::RenamePlaylist(..)
            | BackendCommand::DeletePlaylist(_)
            | BackendCommand::PlaylistTracks(_)
            | BackendCommand::AddToPlaylist(..)
            | BackendCommand::RemoveFromPlaylist(..)
            | BackendCommand::MovePlaylistTrack(..)
            | BackendCommand::SetArtworkOverride(..)
            | BackendCommand::LoadLiked
            | BackendCommand::ToggleLiked(_)
            | BackendCommand::PlayPlaylist(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec_track_generic(title: &str) -> Track {
        let mut t = Track::new(
            title.to_string(),
            vec![crate::domain::artist::Artist::new(
                "Banda".to_string(),
                None,
                None,
                None,
            )],
            crate::domain::source::Source::YouTube,
        );
        t.external_id = Some(title.to_string());
        t
    }

    // La navegación de la cola (avance, wrap, ancla desconocida, shuffle,
    // repeat) se cubre exhaustivamente en `playback::queue::tests`.

    #[test]
    fn identifier_falls_back_to_title_and_artist() {
        let mut t = Track::new(
            "Canción".to_string(),
            vec![crate::domain::artist::Artist::new(
                "Banda".to_string(),
                None,
                None,
                None,
            )],
            crate::domain::source::Source::YouTube,
        );
        t.external_id = Some("vid".to_string());
        assert_eq!(t.identifier(), "vid");
        t.external_id = None;
        assert_eq!(t.identifier(), "Canción|Banda");
    }

    /// Los cortes de estado del motor se reenvían a la UI INMEDIATAMENTE (para
    /// que el karaoke congele su extrapolación en cuanto el audio se para), no
    /// solo en el tick de 500 ms.
    #[test]
    fn state_changes_are_forwarded_immediately_but_others_are_not() {
        for ev in [
            PlaybackEvent::Buffering,
            PlaybackEvent::Playing,
            PlaybackEvent::Paused,
            PlaybackEvent::Stopped,
        ] {
            assert!(forwards_real_state(&ev), "se reenvía {ev:?}");
        }
        for ev in [
            PlaybackEvent::Finished,
            PlaybackEvent::Cut("fin".to_string()),
            PlaybackEvent::Error("boom".to_string()),
            PlaybackEvent::SeekStarted,
            PlaybackEvent::SeekCompleted,
            PlaybackEvent::SeekFailed,
        ] {
            assert!(!forwards_real_state(&ev), "tiene reenvío propio {ev:?}");
        }
    }

    /// La decisión de recuperación consume presupuesto EXACTAMENTE una vez por
    /// track y clasificar no gasta (orden deliberado en el loop de eventos).
    #[test]
    fn recovery_decision_and_budget_interplay() {
        let ev_transport = PlaybackEvent::Error("leer stream: roto".to_string());
        let ev_decode = PlaybackEvent::Error("decodificar stream: malo".to_string());

        assert_eq!(decide_recovery(&ev_decode), RecoveryAction::Report);
        assert_eq!(
            decide_recovery(&ev_transport),
            RecoveryAction::RefreshAndResume
        );

        let mut budget = RecoveryBudget::default();
        budget.arm("song");
        assert!(budget.try_consume("song"));
        assert!(!budget.try_consume("song"), "un solo refresco por track");
    }

    /// FASE 4: un seek es obsoleto si el track en curso ya no coincide con el
    /// que lo emitió. El caso `A → seek → B` (el seek llegó tarde, después de
    /// reproducir B) debe DESCARTAR el seek para no saltar dentro de B.
    #[test]
    fn stale_seek_after_track_change_is_discarded() {
        let a = rec_track_generic("song-a");
        let b = rec_track_generic("song-b");

        // Seek emitido mientras sonaba `song-a`... válido.
        assert!(!Backend::seek_is_stale(Some(&a), &a.identifier()));
        // ...pero llega cuando `song-b` ya está en curso: obsoleto.
        assert!(Backend::seek_is_stale(Some(&b), &a.identifier()));
        // Sin track en curso (p. ej. detenido): no hay dónde saltar.
        assert!(Backend::seek_is_stale(None, &a.identifier()));
        // El mismo track sigue siendo válido.
        assert!(!Backend::seek_is_stale(Some(&b), &b.identifier()));
    }
}
