//! Backend de reproducción sobre el dispositivo de audio local (rodio/cpal).
//!
//! Descarga el stream HTTP de forma **progresiva** vía la capa de transporte
//! ([`HttpRangeStream`], ventanas Range encadenadas con validación): el
//! decodificador symphonia consume un stream lógico continuo mientras los
//! bytes siguen llegando, de modo que la reproducción arranca en cuanto hay
//! un buffer inicial suficiente.
//!
//! # Buffer adaptativo
//!
//! Un monitor pausa y reanuda el [`Player`] de rodio para mantener un margen de
//! audio ya descargado frente a las fluctuaciones de la red:
//!
//! - `Player::pause()` hace que rodio produzca silencio **sin consumir** el
//!   decodificador, así que la descarga se acumula en el buffer; `play()`
//!   retoma el consumo. Es la base del "prebuffer".
//! - El arranque inicial es adaptativo: si la red es lenta frente al bitrate,
//!   se espera más margen antes de empezar.
//! - Durante la reproducción se rellena por debajo de un umbral crítico y se
//!   reanuda por encima de uno de recuperación (histéresis: sin oscilaciones).
//! - Cada `play()`/`stop()` incrementa una **generación**: las tareas de
//!   descarga y monitor viejas salen antes de emitir eventos, evitando
//!   `Buffering`/`Playing`/`Finished`/`Error` atrasados de una canción previa.
//! - El fin natural solo se notifica cuando el buffer llegó a EOF sin errores;
//!   un stream cortado no dispara `Finished` (y por tanto no salta el autoplay).
//!
//! Los errores en caliente (corte de red, decode fallido, buffer del
//! dispositivo) se enrutan al bus agrupado como eventos clasificados, no a
//! `stderr`. La semántica HTTP (rangos, retries, restricciones) vive en la
//! capa de transporte; este backend solo decide qué hacer con cada clase.

use std::fmt;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use rodio::{source::Source as RodioSource, Decoder, Player};

use crate::analysis::AnalysisRuntime;
#[cfg(feature = "rodio")]
use crate::analysis::TapSource;
use crate::app::audio::{
    EventBus, PlaybackEngine, PlaybackError, PlaybackEvent, PlaybackState, PlaybackStatus,
};
use crate::domain::source::Source;
use crate::domain::stream::MediaSource;
use crate::domain::track::Track;
use crate::dsp::{DspControl, DspSource};
use crate::infrastructure::playback::is_http_source;
use crate::infrastructure::playback::output::SharedOutput;
use crate::media::transport::{HttpRangeStream, RangePolicy, TransportFailure};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// Frecuencia de sondeo del monitor de buffering.
const MONITOR_INTERVAL: Duration = Duration::from_millis(100);
/// Tiempo máximo para llenar el buffer inicial antes de declarar el stream
/// inviable (red demasiado lenta o cortada).
const INITIAL_FILL_TIMEOUT: Duration = Duration::from_secs(20);
/// Tiempo máximo para descargar la región de un seek antes de intentar el
/// salto igualmente (red demasiado lenta o cortada).
const SEEK_FILL_TIMEOUT: Duration = Duration::from_secs(40);
/// Bitrate por defecto cuando el contenedor no expone tamaño ni duración.
const FALLBACK_BITRATE: f64 = 128_000.0;
/// Tamaño máximo de cada tirada al buffer compartido (troceado de ventanas).
const CHUNK_PULL: usize = 256 * 1024;

/// Política del buffer adaptativo, en segundos de audio ya descargado.
///
/// Es pura (sin I/O) para poder testearla: el monitor de buffering la usa para
/// decidir cuándo pausar, avisar y reanudar la reproducción.
#[derive(Debug, Clone, Copy)]
pub struct BufferPolicy {
    /// Umbral de arranque inicial (2–6 s), ajustado a la velocidad de red.
    pub start_seconds: f64,
    /// Por debajo de esto se pausa para rellenar el buffer.
    pub critical_seconds: f64,
    /// Por encima de esto se reanuda tras un rellenado.
    pub resume_seconds: f64,
    /// Margen bajo: aviso de "stream lento" antes de llegar a crítico.
    pub low_seconds: f64,
}

impl Default for BufferPolicy {
    fn default() -> Self {
        Self {
            start_seconds: 4.0,
            critical_seconds: 1.5,
            resume_seconds: 3.5,
            low_seconds: 3.0,
        }
    }
}

impl BufferPolicy {
    /// Umbral de arranque adaptativo: cuanto más lenta sea la descarga frente
    /// al bitrate de reproducción, más margen se espera antes de empezar. La
    /// cota inferior es baja (1.5 s) para que una red sana arranque cuanto
    /// antes: el monitor de buffering rellena durante la reproducción si hace
    /// falta.
    pub fn adaptive_start(speed_bps: f64, playback_bps: f64) -> f64 {
        let ratio = if playback_bps > 0.0 {
            speed_bps / playback_bps
        } else {
            f64::INFINITY
        };
        let base: f64 = if ratio >= 2.0 {
            1.5
        } else if ratio >= 1.0 {
            3.0
        } else {
            4.5
        };
        base.clamp(1.5_f64, 4.5_f64)
    }

    /// Decisión del monitor a partir de si se está reproduciendo y del margen
    /// actual de audio descargado (en segundos).
    pub fn decide(&self, playing: bool, buffered_secs: f64, eof: bool) -> BufferAction {
        if eof {
            // El stream terminó: no tiene sentido pausar por bajo margen.
            return BufferAction::None;
        }
        match playing {
            true => {
                if buffered_secs < self.critical_seconds {
                    BufferAction::PauseAndRefill
                } else if buffered_secs < self.low_seconds {
                    BufferAction::Warn
                } else {
                    BufferAction::None
                }
            }
            false => {
                if buffered_secs >= self.resume_seconds {
                    BufferAction::Resume
                } else {
                    BufferAction::None
                }
            }
        }
    }
}

/// Acción que el monitor debe aplicar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferAction {
    /// Sin cambios.
    None,
    /// Margen bajo: señala "stream lento" sin pausar.
    Warn,
    /// Pausa la reproducción y rellena el buffer.
    PauseAndRefill,
    /// Reanuda la reproducción.
    Resume,
}

/// Motor rodio: reproduce audio del stream HTTP descargado en streaming.
pub struct RodioBackend {
    http: reqwest::Client,
    bus: EventBus,
    player: Arc<Player>,
    /// Mantiene viva la salida de audio local: el `MixerDeviceSink` posee el
    /// `cpal::Stream` físico, así que si esta cae se para el audio.
    _output: Arc<SharedOutput>,
    /// Generación de reproducción: se incrementa en cada `play()`/`stop()`.
    /// Las tareas de descarga y monitor capturan su valor y salen en cuanto
    /// detectan un valor distinto, antes de emitir cualquier evento.
    generation: Arc<AtomicU64>,
    /// Análisis de audio opcional (flag AUDIO_ANALYSIS_ENABLED): runtime con
    /// hilo+anillo+bus; `None` ⇒ el decoder va directo al player.
    analysis: Option<AnalysisRuntime>,
    /// Pausa pedida por el usuario mientras un `play` todavía está
    /// prebuffereando. Sin esto, el `play()` final reanudaba el audio y perdía
    /// la pausa que el usuario acababa de pulsar.
    pause_requested: AtomicBool,
    /// Parámetros del pipeline DSP. Cada fuente nueva lee la configuración
    /// vigente sin reconstruir el motor; los cambios posteriores se publican
    /// de forma no bloqueante y se aplican entre bloques.
    dsp: DspControl,
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    track: Option<Track>,
    duration: Option<Duration>,
    state: PlaybackState,
    /// Stream en curso (si lo hay): se cancela al cambiar de canción o parar
    /// para que un decodificador bloqueado por underrun no atasque `clear()`.
    buffer: Option<Arc<StreamBuffer>>,
}

impl RodioBackend {
    /// Comparte la salida de audio con el resto de backends locales.
    ///
    /// El cliente HTTP externo se ignora: la descarga de streams necesita
    /// margen de tiempo amplio (YouTube limita la velocidad en bastantes redes)
    /// y tiempo de conexión corto, así que se construye uno propio. Con
    /// `disable_env_proxy` el cliente ignora las variables de proxy del
    /// entorno (feature flag `PROXY_ENABLED=false`).
    pub fn new(
        _http: reqwest::Client,
        output: Arc<SharedOutput>,
        bus: EventBus,
        disable_env_proxy: bool,
        analysis: Option<AnalysisRuntime>,
        dsp: DspControl,
    ) -> Self {
        let mut builder = reqwest::Client::builder().connect_timeout(CONNECT_TIMEOUT);
        if disable_env_proxy {
            builder = builder.no_proxy();
        }
        let download = builder.build().expect("cliente de descarga válido");

        Self {
            http: download,
            bus,
            player: output.player(),
            _output: output,
            generation: Arc::new(AtomicU64::new(0)),
            analysis,
            pause_requested: AtomicBool::new(false),
            dsp,
            inner: Arc::new(Mutex::new(Inner {
                track: None,
                duration: None,
                state: PlaybackState::Stopped,
                buffer: None,
            })),
        }
    }

    /// Handle para publicar parámetros DSP sin reconstruir el motor.
    ///
    /// El audio lee la configuración con `try_lock`; ninguna escritura de la
    /// UI puede bloquear el callback.
    pub fn dsp_control(&self) -> DspControl {
        self.dsp.clone()
    }

    fn status_locked(player: &Player, inner: &Inner) -> PlaybackStatus {
        PlaybackStatus {
            track: inner.track.clone(),
            state: inner.state,
            position: player.get_pos(),
            duration: inner.duration,
            stalled: inner
                .buffer
                .as_ref()
                .map(|b| b.is_stalled())
                .unwrap_or(false),
        }
    }

    /// Inicia una generación nueva de forma atómica respecto al monitor y a
    /// `status()`: el incremento ocurre bajo el mismo mutex que las escrituras
    /// de estado. Cancela el buffer anterior y publica el track/estado inicial.
    fn begin_generation(&self, track: Option<Track>, state: PlaybackState) -> u64 {
        let mut inner = self.inner.lock().unwrap();
        let gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(prev) = inner.buffer.take() {
            prev.cancel();
        }
        inner.track = track;
        inner.duration = None;
        inner.state = state;
        self.bus.emit(PlaybackEvent::Buffering);
        gen
    }

    /// Restaura el estado interno tras un fallo de `play()` antes de arrancar.
    ///
    /// Solo toca el estado si la generación sigue siendo la de este `play()`:
    /// si mientras fallaba llegó otra orden (canción nueva, stop), el estado
    /// ya pertenece a esa orden y no debe limpiarse.
    fn reset_after_failed_play(&self, buffer: Option<&Arc<StreamBuffer>>, gen: u64) {
        if let Some(b) = buffer {
            b.cancel();
        }
        // Re-comprobar DENTRO del lock: entre la comprobación y la escritura
        // una orden nueva puede haber cambiado de generación.
        let mut inner = self.inner.lock().unwrap();
        if self.generation.load(Ordering::SeqCst) != gen {
            return;
        }
        // Este play era la orden vigente y falló: nada debe seguir sonando.
        // Detiene cualquier fuente que quedara en el player (p. ej. la canción
        // de un autoplay que ganó la carrera contra este play).
        self.player.stop();
        inner.state = PlaybackState::Stopped;
        inner.track = None;
        inner.duration = None;
        inner.buffer = None;
    }

    /// Descarga (esperando de forma asíncrona) la región del stream que cubre
    /// `pos`, con un margen de seguridad para cabeceras del contenedor y el
    /// refinado de symphonia (lee paquetes por delante del punto de salto).
    ///
    /// Devuelve `false` si el stream fue cancelado mientras se esperaba
    /// (cambio de canción / stop): el seek queda obsoleto y no debe ejecutarse,
    /// o saltaría dentro de la canción nueva.
    async fn prefetch_seek_region(&self, pos: Duration) -> bool {
        let buffer = {
            let inner = self.inner.lock().unwrap();
            inner.buffer.clone()
        };
        let Some(buffer) = buffer else {
            return true;
        };
        if buffer.failed() {
            return false;
        }
        let bitrate = buffer.bitrate();
        if bitrate == 0 {
            // Sin bitrate conocido no se puede estimar la región: se intenta
            // el salto directamente (comportamiento previo).
            return true;
        }
        let need = pos.as_secs_f64() * bitrate as f64 / 8.0;
        let target = need * 1.1 + 128.0 * 1024.0;
        if buffer.downloaded() as f64 >= target {
            return true;
        }
        let started = Instant::now();
        loop {
            if buffer.failed() {
                return false;
            }
            if buffer.eof() || buffer.downloaded() as f64 >= target {
                // Lista, o el stream terminó: rodio satura el salto al final.
                return true;
            }
            if started.elapsed() > SEEK_FILL_TIMEOUT {
                // La red no alcanzó: se abandona el salto en vez de bloquear el
                // hilo de audio esperando bytes (lo congelaría todo). El audio
                // sigue sonando desde la posición actual.
                return false;
            }
            tokio::time::sleep(MONITOR_INTERVAL).await;
        }
    }

    /// Tarea del monitor de buffering: pausa/reanuda el player para mantener
    /// un margen de audio descargado y notifica transiciones por el bus.
    fn spawn_monitor(&self, buffer: Arc<StreamBuffer>, my_gen: u64) {
        let player = self.player.clone();
        let bus = self.bus.clone();
        let generation = self.generation.clone();
        let inner = self.inner.clone();
        tokio::spawn(async move {
            // El monitor arranca justo después del buffer inicial: reproduciendo.
            let mut playing = true;
            loop {
                let state = {
                    // La generación se incrementa bajo este mismo lock: leer
                    // estado y generación juntos evita que un monitor viejo
                    // observe la canción nueva y escriba sobre ella.
                    let inner = inner.lock().unwrap();
                    if generation.load(Ordering::SeqCst) != my_gen {
                        break;
                    }
                    inner.state
                };
                // El usuario pausó: no tocar la reproducción.
                if state == PlaybackState::Paused {
                    tokio::time::sleep(MONITOR_INTERVAL).await;
                    continue;
                }
                // Durante un seek el estado es transitorio: el monitor no debe
                // pausar por bajo margen mientras se re-ancla (eso pausaría la
                // canción en la posición antigua durante el salto).
                if state == PlaybackState::Seeking {
                    tokio::time::sleep(MONITOR_INTERVAL).await;
                    continue;
                }
                if state == PlaybackState::Stopped {
                    break;
                }
                // El estado real manda: si el usuario reanudó (o un seek/otro
                // camino volvió a `Playing`) mientras el monitor estaba
                // rellenando, vuelve a considerar que se está reproduciendo.
                if state == PlaybackState::Playing && !playing {
                    playing = true;
                }
                // Error del stream en caliente (corte, decode...). Las
                // cancelaciones por cambio de canción se filtran arriba por la
                // generación, así que aquí solo llegan errores reales.
                if let Some(err) = buffer.error() {
                    let mut inner = inner.lock().unwrap();
                    if generation.load(Ordering::SeqCst) != my_gen {
                        break;
                    }
                    inner.state = PlaybackState::Stopped;
                    inner.track = None;
                    inner.duration = None;
                    inner.buffer = None;
                    bus.emit(PlaybackEvent::Error(err));
                    break;
                }
                // Corte por límite del CDN: el prefijo descargado sigue
                // sonando hasta agotarse y entonces se notifica con el mensaje
                // (sin emitir `Finished`, que dispararía el autoplay a la
                // siguiente SIN aviso). El `Cut` lo recoge el backend para
                // continuar la reproducción (siguiente o repetir) y avisar.
                if let Some(msg) = buffer.cut() {
                    if player.empty() {
                        let mut inner = inner.lock().unwrap();
                        if generation.load(Ordering::SeqCst) != my_gen {
                            break;
                        }
                        inner.state = PlaybackState::Stopped;
                        inner.track = None;
                        inner.duration = None;
                        inner.buffer = None;
                        bus.emit(PlaybackEvent::Cut(msg));
                        break;
                    }
                }
                let buffered_secs = buffer.buffered_secs();
                let eof = buffer.eof();
                // Fin natural: el `status()` se encarga de emitir `Finished`
                // (con el buffer a EOF y sin errores). Aquí solo se termina.
                if eof && player.empty() {
                    break;
                }
                match policy_decide(playing, buffered_secs, eof) {
                    BufferAction::PauseAndRefill => {
                        {
                            let mut inner = inner.lock().unwrap();
                            if generation.load(Ordering::SeqCst) != my_gen {
                                break;
                            }
                            player.pause();
                            inner.state = PlaybackState::Buffering;
                            bus.emit(PlaybackEvent::Buffering);
                        }
                        buffer.set_stalled(true);
                        playing = false;
                    }
                    BufferAction::Warn => {
                        buffer.set_stalled(true);
                    }
                    BufferAction::Resume => {
                        {
                            let mut inner = inner.lock().unwrap();
                            if generation.load(Ordering::SeqCst) != my_gen {
                                break;
                            }
                            player.play();
                            inner.state = PlaybackState::Playing;
                            bus.emit(PlaybackEvent::Playing);
                        }
                        buffer.set_stalled(false);
                        playing = true;
                    }
                    BufferAction::None => {
                        buffer.set_stalled(false);
                    }
                }
                tokio::time::sleep(MONITOR_INTERVAL).await;
            }
        });
    }
}

fn policy_decide(playing: bool, buffered_secs: f64, eof: bool) -> BufferAction {
    BufferPolicy::default().decide(playing, buffered_secs, eof)
}

impl fmt::Debug for RodioBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RodioBackend").finish()
    }
}

#[async_trait]
impl PlaybackEngine for RodioBackend {
    fn id(&self) -> &'static str {
        "rodio"
    }

    fn supports(&self, source: Source) -> bool {
        is_http_source(source)
    }

    async fn play(
        &self,
        track: &Track,
        source: Option<MediaSource>,
    ) -> Result<PlaybackStatus, PlaybackError> {
        // Nueva generación: invalida cualquier tarea de la canción anterior.
        // Una pausa pedida DURANTE el prebuffer se conserva (se resetea aquí,
        // antes de que el nuevo play empiece a aceptar órdenes).
        self.pause_requested.store(false, Ordering::Release);
        let gen = self.begin_generation(Some(track.clone()), PlaybackState::Buffering);
        tracing::info!(
            session_id = gen,
            source = ?track.source,
            key = %track.identifier(),
            "playback_session_start"
        );

        let Some(MediaSource::Remote(stream)) = source else {
            let msg = if source.is_some() {
                format!("fuente no remota para {}", track.source)
            } else {
                format!("{} no ofrece stream", track.source)
            };
            self.reset_after_failed_play(None, gen);
            tracing::warn!(session_id = gen, error = %msg, "playback_session_failed");
            return Err(PlaybackError::Transport(msg));
        };

        // Apertura del TRANSPORTE (capa Media): la primera petición con rango
        // cerrado valida la respuesta y descubre el tamaño total vía
        // Content-Range. La semántica HTTP (ventanas, retries, restricciones)
        // vive en HttpRangeStream; aquí solo se decide qué hacer con cada
        // clase de fallo.
        let mut transport = HttpRangeStream::open(
            self.http.clone(),
            stream.url.clone(),
            stream.headers.clone(),
            RangePolicy::from_env(),
        )
        .await
        .map_err(|f| {
            self.reset_after_failed_play(None, gen);
            tracing::warn!(session_id = gen, category = %f.category(), error = %f, "playback_transport_open_failed");
            PlaybackError::Transport(format!("abrir stream: {f}"))
        })?;
        let content_length = transport.total() as f64;
        if content_length <= 0.0 {
            self.reset_after_failed_play(None, gen);
            return Err(PlaybackError::Transport("stream vacío".to_string()));
        }

        // Buffer compartido: la tarea de descarga lo rellena y el decodificador
        // (hilo del player rodio) lo consume en cuanto arranca la reproducción.
        // El stream lógico es continuo: los límites de ventana son invisibles.
        let buffer = Arc::new(StreamBuffer::new());
        let writer = buffer.clone();
        tokio::spawn(async move {
            let mut last_instant = Instant::now();
            let mut ema: f64 = 0.0;
            loop {
                if writer.failed() {
                    // Cancelación por cambio de canción/stop: salir sin ruido.
                    break;
                }
                match transport.next_chunk(CHUNK_PULL).await {
                    Ok(Some(chunk)) => {
                        writer.append(&chunk);
                        // Velocidad de descarga (EMA) para el umbral adaptativo.
                        let now = Instant::now();
                        let dt = (now - last_instant).as_secs_f64();
                        if dt > 0.0 {
                            let inst = chunk.len() as f64 / dt;
                            ema = if ema == 0.0 {
                                inst
                            } else {
                                0.5 * ema + 0.5 * inst
                            };
                            writer.set_bps_ema((ema * 8.0) as u64);
                        }
                        last_instant = now;
                    }
                    Ok(None) => {
                        writer.finish();
                        break;
                    }
                    Err(f) => {
                        // Clasificación honesta:
                        // - Restricción posicional con prefijo ya servido =>
                        //   fin limpio con aviso (`Cut`, sin autoplay ciego).
                        // - Resto => error de transporte; PlaybackRecovery
                        //   decide invalidar/re-resolver (un intento por track).
                        if matches!(f, TransportFailure::Restricted { .. }) {
                            let msg = format!(
                                "el servidor restringe este stream a partir del byte {}: \
                                 la resolución actual solo entrega un prefijo ({f})",
                                transport.position()
                            );
                            tracing::warn!(category = %f.category(), "{msg}");
                            writer.set_cut(msg);
                            writer.finish();
                        } else if !writer.failed() {
                            tracing::warn!(category = %f.category(), error = %f, "transport_error");
                            writer.fail(f.to_string());
                        }
                        break;
                    }
                }
            }
        });

        // La inicialización del decodificador lee la cabecera del contenedor y
        // bloquea hasta que lleguen los primeros bytes; se hace en un hilo de
        // bloqueo para no ocupar un worker de tokio mientras bufferea.
        let reader = StreamReader::new(buffer.clone());
        let bus_decode = self.bus.clone();
        let generation_decode = self.generation.clone();
        let source = match tokio::task::spawn_blocking(move || {
            Decoder::new(reader).map_err(|e| {
                // Si la generación cambió mientras symphonia inicializaba el
                // decoder, este error pertenece a una sesión vieja: no debe
                // publicarse en el bus (la UI lo asociaría al track nuevo).
                if generation_decode.load(Ordering::SeqCst) == gen {
                    bus_decode.emit(PlaybackEvent::Error(format!("decodificar stream: {e}")));
                    PlaybackError::Decode(format!("decodificar stream: {e}"))
                } else {
                    PlaybackError::Transport("reproducción reemplazada por otra orden".to_string())
                }
            })
        })
        .await
        {
            Ok(Ok(source)) => source,
            Ok(Err(error)) => {
                self.reset_after_failed_play(Some(&buffer), gen);
                tracing::warn!(session_id = gen, error = %error, "playback_decode_init_failed");
                return Err(error);
            }
            Err(join_error) => {
                self.reset_after_failed_play(Some(&buffer), gen);
                tracing::warn!(session_id = gen, error = %join_error, "playback_decode_init_join_failed");
                return Err(PlaybackError::Transport(format!(
                    "decodificar stream: {join_error}"
                )));
            }
        };

        // Duración del contenedor ANTES de envolver (el tap la reporta None).
        let decoded_duration = source.total_duration();

        // Análisis opcional: envuelve la fuente decodificada para copiar PCM
        // al anillo SPSC (coste por muestra trivial, jamás bloquea el audio).
        #[cfg(feature = "rodio")]
        let source: Box<dyn rodio::Source<Item = f32> + Send> = match self.analysis.as_ref() {
            Some(rt) => Box::new(TapSource::new(source, rt.tap())),
            None => Box::new(source),
        };
        #[cfg(not(feature = "rodio"))]
        let source: Box<dyn rodio::Source<Item = f32> + Send> = Box::new(source);

        // El análisis queda ANTES del DSP: las recomendaciones y la
        // visualización describen la señal de origen, no la ecualización del
        // usuario. El bloque procesado es lo que realmente llega al player.
        #[cfg(feature = "rodio")]
        let source: Box<dyn rodio::Source<Item = f32> + Send> =
            Box::new(DspSource::new(source, self.dsp.clone()));

        // Duration del contenedor si está disponible; si no, la de metadatos.
        let total = decoded_duration.or(track.duration);
        let total_secs = total.map(|d| d.as_secs_f64()).unwrap_or(1.0).max(1.0);
        let playback_bps = if content_length > 0.0 {
            (content_length * 8.0 / total_secs).max(FALLBACK_BITRATE)
        } else {
            FALLBACK_BITRATE
        };
        buffer.set_bitrate(playback_bps as u64);

        {
            let mut inner = self.inner.lock().unwrap();
            if self.generation.load(Ordering::SeqCst) != gen {
                drop(inner);
                buffer.cancel();
                return Err(PlaybackError::Transport(
                    "reproducción reemplazada por otra orden".to_string(),
                ));
            }
            inner.duration = total;
            inner.buffer = Some(buffer.clone());
        }

        // Espera de buffer inicial: el player todavía no consume el decoder,
        // así que la descarga se acumula hasta alcanzar el umbral (adaptativo
        // según la velocidad de red) o hasta que llegue el final del stream.
        let mut start_target = BufferPolicy::default().start_seconds;
        let mut targeted = false;
        let started = Instant::now();
        loop {
            if buffer.error().is_some() {
                let msg = buffer
                    .error()
                    .unwrap_or_else(|| "stream interrumpido".to_string());
                self.reset_after_failed_play(Some(&buffer), gen);
                tracing::warn!(session_id = gen, error = %msg, "playback_initial_fill_failed");
                return Err(PlaybackError::Transport(msg));
            }
            if buffer.eof() {
                break;
            }
            let speed = buffer.bps_ema() as f64;
            if !targeted && speed > 0.0 {
                start_target = BufferPolicy::adaptive_start(speed, playback_bps);
                targeted = true;
            }
            if buffer.buffered_secs() >= start_target {
                break;
            }
            if started.elapsed() > INITIAL_FILL_TIMEOUT {
                let msg = "el stream no llena el buffer inicial (red demasiado lenta o cortada)"
                    .to_string();
                self.reset_after_failed_play(Some(&buffer), gen);
                tracing::warn!(session_id = gen, error = %msg, "playback_initial_fill_timeout");
                return Err(PlaybackError::Transport(msg));
            }
            tokio::time::sleep(MONITOR_INTERVAL).await;
        }
        if let Some(err) = buffer.error() {
            self.reset_after_failed_play(Some(&buffer), gen);
            tracing::warn!(session_id = gen, error = %err, "playback_initial_fill_aborted");
            return Err(PlaybackError::Transport(err));
        }

        // El commit (comprobar generación + limpiar/añadir/arrancar) ocurre
        // bajo el lock de estado. Si una orden nueva incrementó la generación
        // mientras se llenaba el buffer, este play no debe tocar el player ni
        // el estado de la orden vigente.
        let paused_at_commit = self.pause_requested.load(Ordering::Acquire);
        {
            let mut inner = self.inner.lock().unwrap();
            if self.generation.load(Ordering::SeqCst) != gen {
                drop(inner);
                buffer.cancel();
                return Err(PlaybackError::Transport(
                    "reproducción reemplazada por otra orden".to_string(),
                ));
            }
            self.player.clear();
            self.player.append(source);
            if paused_at_commit {
                self.player.pause();
                inner.state = PlaybackState::Paused;
                self.bus.emit(PlaybackEvent::Paused);
            } else {
                self.player.play();
                inner.state = PlaybackState::Playing;
                self.bus.emit(PlaybackEvent::Playing);
            }
        }
        tracing::info!(
            session_id = gen,
            paused = paused_at_commit,
            "playback_audio_started"
        );
        self.spawn_monitor(buffer.clone(), gen);

        let inner = self.inner.lock().unwrap();
        Ok(Self::status_locked(&self.player, &inner))
    }

    async fn pause(&self) -> Result<PlaybackStatus, PlaybackError> {
        self.pause_requested.store(true, Ordering::Release);
        let mut inner = self.inner.lock().unwrap();
        self.player.pause();
        inner.state = PlaybackState::Paused;
        self.bus.emit(PlaybackEvent::Paused);
        Ok(Self::status_locked(&self.player, &inner))
    }

    async fn resume(&self) -> Result<PlaybackStatus, PlaybackError> {
        self.pause_requested.store(false, Ordering::Release);
        let mut inner = self.inner.lock().unwrap();
        self.player.play();
        inner.state = PlaybackState::Playing;
        self.bus.emit(PlaybackEvent::Playing);
        Ok(Self::status_locked(&self.player, &inner))
    }

    async fn stop(&self) -> Result<PlaybackStatus, PlaybackError> {
        self.pause_requested.store(false, Ordering::Release);
        let mut inner = self.inner.lock().unwrap();
        let gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(buf) = inner.buffer.take() {
            buf.cancel();
        }
        self.player.stop();
        inner.track = None;
        inner.duration = None;
        inner.state = PlaybackState::Stopped;
        tracing::info!(session_id = gen, "playback_session_stopped");
        self.bus.emit(PlaybackEvent::Stopped);
        Ok(Self::status_locked(&self.player, &inner))
    }

    async fn seek(&self, pos: Duration) -> Result<PlaybackStatus, PlaybackError> {
        // No mantener el lock mientras `prefetch_seek_region` espera (puede
        // tardar hasta que la región objetivo esté descargada).
        let (had_play, has_buffer, gen) = {
            let inner = self.inner.lock().unwrap();
            (
                matches!(
                    inner.state,
                    PlaybackState::Playing | PlaybackState::Buffering | PlaybackState::Seeking
                ),
                inner.buffer.is_some(),
                self.generation.load(Ordering::SeqCst),
            )
        };
        if !has_buffer {
            // Sin stream activo: un seek no tiene adónde ir.
            let inner = self.inner.lock().unwrap();
            return Ok(Self::status_locked(&self.player, &inner));
        }
        // Estado transitorio "buscando": el audio real NUNCA se mueve todavía.
        self.bus.emit(PlaybackEvent::SeekStarted);
        {
            let mut inner = self.inner.lock().unwrap();
            if self.generation.load(Ordering::SeqCst) != gen {
                return Ok(Self::status_locked(&self.player, &inner));
            }
            inner.state = PlaybackState::Seeking;
        }
        // Antes de pedirle a rodio el salto hay que asegurar la región
        // objetivo en el buffer: `Player::try_seek` ejecuta el seek en el
        // hilo de audio y el lector bloquea hasta tener los bytes, lo que
        // congela TODA la salida durante el rellenado. Esperar aquí (async)
        // deja la canción sonando desde la posición actual hasta que el salto
        // pueda ejecutarse al instante.
        //
        // Separamos el SEEK REAL del audio (try_seek + confirmación) de la
        // actualización optimista del reloj: el estado solo avanza si el
        // backend confirma el salto.
        let seek_result: Result<(), rodio::source::SeekError> =
            if self.prefetch_seek_region(pos).await {
                let mut inner = self.inner.lock().unwrap();
                // Una canción nueva pudo empezar durante el prefetch: el seek es
                // obsoleto y no debe tocar el player ni el estado de la nueva.
                if self.generation.load(Ordering::SeqCst) != gen {
                    return Ok(Self::status_locked(&self.player, &inner));
                }
                let paused = inner.state == PlaybackState::Paused;
                let buffering = inner.state == PlaybackState::Buffering;
                let result = self.player.try_seek(pos);
                // Restaura la pausa/refill: el seek no debe reanudar audio que el
                // usuario (o el monitor) tenía detenido.
                if result.is_ok() && (paused || buffering) {
                    self.player.pause();
                }
                match result {
                    Ok(()) => {
                        inner.state = if paused {
                            PlaybackState::Paused
                        } else if buffering {
                            PlaybackState::Buffering
                        } else {
                            PlaybackState::Playing
                        };
                        self.bus.emit(PlaybackEvent::SeekCompleted);
                        return Ok(Self::status_locked(&self.player, &inner));
                    }
                    Err(e) => Err(e),
                }
            } else {
                Err(rodio::source::SeekError::NotSupported {
                    underlying_source: "stream",
                })
            };

        let mut inner = self.inner.lock().unwrap();
        if self.generation.load(Ordering::SeqCst) != gen {
            return Ok(Self::status_locked(&self.player, &inner));
        }
        // El seek falló: el audio NO cambió de posición. Recuperamos el estado
        // anterior, sin fingir un salto.
        inner.state = if inner.state == PlaybackState::Paused {
            PlaybackState::Paused
        } else if had_play {
            PlaybackState::Playing
        } else {
            PlaybackState::Stopped
        };
        self.bus.emit(PlaybackEvent::SeekFailed);
        Err(PlaybackError::Transport(format!(
            "no se pudo buscar: {seek_result:?}"
        )))
    }

    async fn set_volume(&self, volume: u8) -> Result<PlaybackStatus, PlaybackError> {
        let inner = self.inner.lock().unwrap();
        self.player.set_volume(volume as f32 / 100.0);
        Ok(Self::status_locked(&self.player, &inner))
    }

    fn is_buffering(&self) -> bool {
        self.inner.lock().unwrap().state == PlaybackState::Buffering
    }

    fn status(&self) -> PlaybackStatus {
        let mut inner = self.inner.lock().unwrap();
        let mut status = Self::status_locked(&self.player, &inner);
        // Fin natural: el buffer llegó a EOF sin errores y el player ya no
        // tiene fuentes. Un stream cortado (`err` en el buffer) NO emite
        // `Finished`: evita que el autoplay salte con una canción incompleta.
        // Un corte por límite del CDN (`cut`) tampoco: el monitor notifica el
        // Error cuando el prefijo se agota.
        if matches!(
            inner.state,
            PlaybackState::Playing | PlaybackState::Paused | PlaybackState::Seeking
        ) && self.player.empty()
        {
            let natural = inner
                .buffer
                .as_ref()
                .map(|b| b.eof() && !b.failed() && b.cut().is_none())
                .unwrap_or(true);
            if natural {
                status.state = PlaybackState::Stopped;
                inner.state = PlaybackState::Stopped;
                inner.track = None;
                inner.duration = None;
                self.bus.emit(PlaybackEvent::Finished);
            }
        }
        status
    }
}

/// Buffer de bytes compartido entre la tarea de descarga y el decodificador.
///
/// La descarga rellena el buffer y avisa a los lectores vía `Condvar`; la
/// lectura bloquea hasta que hay datos (o EOF). Mantiene todo el stream en
/// memoria: para una canción son unos pocos MB y symphonia puede rebobinar
/// durante la inicialización.
///
/// Además de los bytes, acumula las métricas que usa el monitor de buffering:
/// bytes descargados/consumidos, velocidad estimada (EMA) y bitrate de
/// reproducción, para convertir bytes en segundos de audio.
pub(crate) struct StreamBuffer {
    inner: Mutex<State>,
    cv: Condvar,
    /// `true` mientras un lector espera datos (buffer underrun): el stream
    /// va lento o quedó cortado y la reproducción está "en seco".
    stalled: AtomicBool,
    /// Bytes descargados hasta ahora (solo crece).
    downloaded: AtomicU64,
    /// Posición más lejana consumida por el decodificador.
    consumed: AtomicU64,
    /// Velocidad de descarga estimada (bits/s), media móvil del downloader.
    bps_ema: AtomicU64,
    /// Bitrate de reproducción (bits/s) para convertir bytes→segundos.
    bitrate: AtomicU64,
}

#[derive(Default)]
struct State {
    buf: Vec<u8>,
    eof: bool,
    err: Option<String>,
    /// Corte por límite del CDN: fin de datos con mensaje (a diferencia de
    /// `err`, el buffer sigue sonando hasta agotarse).
    cut: Option<String>,
}

impl StreamBuffer {
    pub(crate) fn new() -> Self {
        Self {
            inner: Mutex::new(State::default()),
            cv: Condvar::new(),
            stalled: AtomicBool::new(false),
            downloaded: AtomicU64::new(0),
            consumed: AtomicU64::new(0),
            bps_ema: AtomicU64::new(0),
            bitrate: AtomicU64::new(0),
        }
    }

    /// Marca que un lector va a quedarse esperando datos.
    fn mark_stalled(&self) {
        self.stalled.store(true, Ordering::Relaxed);
    }

    /// Desmarca el estado de espera (el lector obtuvo datos, EOF o error).
    fn clear_stalled(&self) {
        self.stalled.store(false, Ordering::Relaxed);
    }

    /// `true` si el decodificador está bloqueado esperando que lleguen datos.
    fn is_stalled(&self) -> bool {
        self.stalled.load(Ordering::Relaxed)
    }

    /// Pone/limpia la señal de "rellenando buffer" desde el monitor.
    fn set_stalled(&self, stalled: bool) {
        self.stalled.store(stalled, Ordering::Relaxed);
    }

    pub(crate) fn append(&self, bytes: &[u8]) {
        let mut s = self.inner.lock().unwrap();
        s.buf.extend_from_slice(bytes);
        self.downloaded
            .fetch_add(bytes.len() as u64, Ordering::Relaxed);
        self.cv.notify_all();
    }

    pub(crate) fn finish(&self) {
        let mut s = self.inner.lock().unwrap();
        s.eof = true;
        self.cv.notify_all();
    }

    /// Marca el fin de datos por corte del CDN: deja el mensaje y señaliza EOF.
    /// A diferencia de [`Self::fail`], los bytes ya descargados siguen
    /// sonando hasta agotarse (el lector ve EOF normal y no un error).
    fn set_cut(&self, msg: String) {
        let mut s = self.inner.lock().unwrap();
        s.cut = Some(msg);
        s.eof = true;
        self.cv.notify_all();
    }

    fn cut(&self) -> Option<String> {
        self.inner.lock().unwrap().cut.clone()
    }

    pub(crate) fn fail(&self, err: String) {
        let mut s = self.inner.lock().unwrap();
        s.err = Some(err);
        self.cv.notify_all();
    }

    /// Cancela la lectura en curso (la usa el reproductor al cambiar de
    /// canción o parar): un decodificador bloqueado en `read` sale con error
    /// e inmediatamente termina, sin esperar a que llegue más red.
    fn cancel(&self) {
        self.fail("reproducción cancelada".to_string());
    }

    /// `true` si la lectura ya terminó con un error (la descarga debe parar).
    fn failed(&self) -> bool {
        self.inner.lock().unwrap().err.is_some()
    }

    fn error(&self) -> Option<String> {
        self.inner.lock().unwrap().err.clone()
    }

    fn eof(&self) -> bool {
        self.inner.lock().unwrap().eof
    }

    fn downloaded(&self) -> u64 {
        self.downloaded.load(Ordering::Relaxed)
    }

    fn consumed(&self) -> u64 {
        self.consumed.load(Ordering::Relaxed)
    }

    /// Registra la posición más lejana leída por un `StreamReader`.
    fn note_consumed(&self, pos: u64) {
        self.consumed.fetch_max(pos, Ordering::Relaxed);
    }

    fn set_bps_ema(&self, bps: u64) {
        self.bps_ema.store(bps, Ordering::Relaxed);
    }

    fn bps_ema(&self) -> u64 {
        self.bps_ema.load(Ordering::Relaxed)
    }

    fn set_bitrate(&self, bps: u64) {
        self.bitrate.store(bps, Ordering::Relaxed);
    }

    /// Bitrate de reproducción (bits/s) para convertir bytes↔segundos.
    fn bitrate(&self) -> u64 {
        self.bitrate.load(Ordering::Relaxed)
    }

    /// Margen de audio descargado aún no consumido, en segundos.
    fn buffered_secs(&self) -> f64 {
        let bps = self.bitrate.load(Ordering::Relaxed);
        if bps == 0 {
            return 0.0;
        }
        let buffered = self.downloaded().saturating_sub(self.consumed());
        buffered as f64 * 8.0 / bps as f64
    }
}

/// Vista `Read + Seek` (bloqueante) sobre un [`StreamBuffer`].
///
/// Mientras el stream no llega al final, `read` espera a que haya bytes y
/// `seek` espera a que la posición solicitada haya sido descargada.
pub(crate) struct StreamReader {
    shared: Arc<StreamBuffer>,
    pos: u64,
}

impl StreamReader {
    pub(crate) fn new(shared: Arc<StreamBuffer>) -> Self {
        Self { shared, pos: 0 }
    }

    fn wait_data(&self, need: usize) -> io::Result<()> {
        let mut s = self.shared.inner.lock().unwrap();
        loop {
            if let Some(e) = &s.err {
                return Err(io::Error::other(e.clone()));
            }
            if s.buf.len() >= need || s.eof {
                return Ok(());
            }
            self.shared.mark_stalled();
            s = self.shared.cv.wait(s).unwrap();
            self.shared.clear_stalled();
        }
    }
}

impl Read for StreamReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let mut s = self.shared.inner.lock().unwrap();
        loop {
            let start = self.pos as usize;
            if start < s.buf.len() {
                let n = (s.buf.len() - start).min(out.len());
                out[..n].copy_from_slice(&s.buf[start..start + n]);
                self.pos += n as u64;
                self.shared.note_consumed(self.pos);
                return Ok(n);
            }
            if let Some(e) = &s.err {
                return Err(io::Error::other(e.clone()));
            }
            if s.eof {
                return Ok(0);
            }
            self.shared.mark_stalled();
            s = self.shared.cv.wait(s).unwrap();
            self.shared.clear_stalled();
        }
    }
}

impl Seek for StreamReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let target = match pos {
            SeekFrom::Start(off) => off as i64,
            SeekFrom::Current(delta) => self.pos as i64 + delta,
            SeekFrom::End(off) => {
                let mut s = self.shared.inner.lock().unwrap();
                while !s.eof && s.err.is_none() {
                    self.shared.mark_stalled();
                    s = self.shared.cv.wait(s).unwrap();
                    self.shared.clear_stalled();
                }
                s.buf.len() as i64 + off
            }
        };

        if target < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "posición negativa",
            ));
        }
        // Espera a que la posición haya sido descargada (nos permite saltar a
        // medio stream aunque todavía no haya llegado).
        let need = target as usize;
        self.wait_data(need)?;
        {
            let s = self.shared.inner.lock().unwrap();
            if s.buf.len() < need {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "seek más allá del final",
                ));
            }
        }
        self.pos = need as u64;
        self.shared.note_consumed(self.pos);
        Ok(self.pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decide_holds_steady_with_good_margin() {
        let p = BufferPolicy::default();
        assert_eq!(p.decide(true, 5.0, false), BufferAction::None);
        assert_eq!(p.decide(true, 3.5, false), BufferAction::None);
    }

    #[test]
    fn decide_warns_between_low_and_critical() {
        let p = BufferPolicy::default();
        assert_eq!(p.decide(true, 2.0, false), BufferAction::Warn);
        assert_eq!(p.decide(true, 2.9, false), BufferAction::Warn);
    }

    #[test]
    fn decide_refills_below_critical() {
        let p = BufferPolicy::default();
        assert_eq!(p.decide(true, 1.4, false), BufferAction::PauseAndRefill);
        assert_eq!(p.decide(true, 0.0, false), BufferAction::PauseAndRefill);
    }

    #[test]
    fn decide_resumes_only_after_recovery_margin() {
        let p = BufferPolicy::default();
        // Histéresis: reanuda por encima de `resume`, no al recuperar crítico.
        assert_eq!(p.decide(false, 3.4, false), BufferAction::None);
        assert_eq!(p.decide(false, 3.5, false), BufferAction::Resume);
        assert_eq!(p.decide(false, 8.0, false), BufferAction::Resume);
    }

    #[test]
    fn decide_never_refills_after_eof() {
        let p = BufferPolicy::default();
        assert_eq!(p.decide(true, 0.0, true), BufferAction::None);
        assert_eq!(p.decide(false, 0.0, true), BufferAction::None);
    }

    #[test]
    fn adaptive_start_rewards_fast_network() {
        // A 2× el bitrate de reproducción, basta con 1.5 s de margen.
        assert_eq!(BufferPolicy::adaptive_start(256_000.0, 128_000.0), 1.5);
    }

    #[test]
    fn adaptive_start_matches_slow_network() {
        // A la misma velocidad que el bitrate, 3 s.
        assert_eq!(BufferPolicy::adaptive_start(128_000.0, 128_000.0), 3.0);
        // La mitad de velocidad: margen máximo (4.5 s).
        assert_eq!(BufferPolicy::adaptive_start(64_000.0, 128_000.0), 4.5);
    }

    #[test]
    fn adaptive_start_clamps_and_handles_unknown_bitrate() {
        assert!(BufferPolicy::adaptive_start(1_000_000.0, 1.0) >= 1.5);
        assert!(BufferPolicy::adaptive_start(1.0, 1_000_000.0) <= 4.5);
        // Sin bitrate de reproducción conocido: velocidad infinita → 1.5 s.
        assert_eq!(BufferPolicy::adaptive_start(500_000.0, 0.0), 1.5);
    }

    #[test]
    fn slow_producer_streams_chunks_until_eof() {
        use std::io::Read;
        // Simula un productor lento: chunk → pausa → chunk → pausa → EOF. El
        // lector nunca ve un final prematuro: consume todo y `eof` llega limpio.
        let buffer = Arc::new(StreamBuffer::new());
        let writer = buffer.clone();
        let producer = std::thread::spawn(move || {
            for _ in 0..3 {
                writer.append(&vec![0xABu8; 1024]);
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
            writer.finish();
        });

        let mut reader = StreamReader::new(buffer.clone());
        let mut total = 0usize;
        let mut out = [0u8; 512];
        loop {
            let n = reader.read(&mut out).unwrap();
            if n == 0 {
                break;
            }
            total += n;
        }
        producer.join().unwrap();

        assert_eq!(total, 3 * 1024, "el lector consume el stream completo");
        assert_eq!(buffer.downloaded(), 3 * 1024);
        assert!(buffer.eof(), "EOF señalizado por el productor");
        assert!(!buffer.failed(), "sin errores");
    }

    #[test]
    fn cancel_unblocks_a_waiting_reader() {
        use std::io::Read;
        // Un lector esperando datos debe salir al instante cuando se cancela
        // (cambio de canción / stop): no queda colgado esperando más red.
        let buffer = Arc::new(StreamBuffer::new());
        let reader_buf = buffer.clone();
        let waiter = std::thread::spawn(move || {
            let mut reader = StreamReader::new(reader_buf);
            let mut out = [0u8; 64];
            reader.read(&mut out)
        });
        std::thread::sleep(std::time::Duration::from_millis(20));
        buffer.cancel();
        let res = waiter.join().unwrap();
        assert!(
            res.is_err(),
            "la cancelación desbloquea al lector con error"
        );
        assert!(buffer.failed());
        assert!(!buffer.eof());
    }
}
