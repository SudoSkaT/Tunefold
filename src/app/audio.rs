//! Contrato de reproducción de audio en la capa de Aplicación.
//!
//! Este módulo define el trait [`PlaybackEngine`] que los backends de
//! Infraestructura implementan (solo rodio local). Ningún detalle de esos
//! backends se filtra aquí: la capa de Aplicación solo conoce el contrato y el
//! canal de eventos [`PlaybackEvent`].

use std::fmt;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::domain::source::Source;
use crate::domain::stream::MediaSource;
use crate::domain::track::Track;

/// Estado de reproducción expuesto a la UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Buffering,
    Playing,
    Paused,
    /// Se está ejecutando un seek en el backend. El audio real puede seguir
    /// sonando desde la posición anterior mientras se pre-descarga la región
    /// objetivo; el estado se considera transitorio hasta que llega
    /// `SeekCompleted` o `SeekFailed`.
    Seeking,
}

/// Snapshot del estado actual de reproducción para la UI.
#[derive(Debug, Clone)]
pub struct PlaybackStatus {
    pub track: Option<Track>,
    pub state: PlaybackState,
    pub position: Duration,
    pub duration: Option<Duration>,
    /// El decodificador está bloqueado esperando datos (buffer underrun):
    /// el stream va lento o se cortó y la reproducción tiene que rellenarse.
    pub stalled: bool,
}

impl PlaybackStatus {
    pub fn idle() -> Self {
        Self {
            track: None,
            state: PlaybackState::Stopped,
            position: Duration::ZERO,
            duration: None,
            stalled: false,
        }
    }
}

/// Eventos asíncronos emitidos por un motor de reproducción.
#[derive(Debug, Clone)]
pub enum PlaybackEvent {
    /// Se está descargando / preparando el stream.
    Buffering,
    /// La reproducción empezó o reanudó.
    Playing,
    /// La reproducción está en pausa.
    Paused,
    /// Comenzó a ejecutarse un seek (el backend está buscando la región).
    SeekStarted,
    /// Un seek se completó con éxito: el audio real quedó en la posición
    /// objetivo.
    SeekCompleted,
    /// Un seek falló: el audio real NO cambió de posición.
    SeekFailed,
    /// La canción terminó.
    Finished,
    /// El servidor restringió/cerró el stream en caliente (p. ej. techo
    /// posicional por contexto de resolución): la reproducción debe
    /// CONTINUAR con la siguiente canción (o repetir la misma desde el
    /// inicio) sin borrar la metadata. Lleva el mensaje.
    Cut(String),
    /// La reproducción se detuvo.
    Stopped,
    /// Error de reproducción (descripción amigable).
    Error(String),
}

/// Error de reproducción con clasificación para la UI.
#[derive(Debug, thiserror::Error)]
pub enum PlaybackError {
    /// El backend no está disponible (sin dispositivo, daemon caído, etc.).
    #[error("{0}")]
    Unavailable(String),
    /// Error al resolver/descargar el stream.
    #[error("{0}")]
    Transport(String),
    /// El stream no se pudo decodificar.
    #[error("{0}")]
    Decode(String),
    /// El backend no soporta el origen de la canción.
    #[error("el backend no soporta {0}")]
    Unsupported(Source),
}

/// Bus de eventos compartido que los backends usan para emitir
/// [`PlaybackEvent`]. Un solo receptor agrupa a todos los motores.
#[derive(Clone, Debug)]
pub struct EventBus {
    tx: UnboundedSender<PlaybackEvent>,
}

impl EventBus {
    /// Crea el par (bus, receptor agrupado).
    pub fn channel() -> (Self, UnboundedReceiver<PlaybackEvent>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (Self { tx }, rx)
    }

    /// Emite un evento; si no hay receptores se descarta silenciosamente.
    pub fn emit(&self, event: PlaybackEvent) {
        let _ = self.tx.send(event);
    }
}

/// Contrato de un motor de reproducción de audio.
///
/// Implementado por el backend local en Infraestructura (rodio).
#[async_trait]
pub trait PlaybackEngine: fmt::Debug + Send + Sync {
    /// Identificador estable del backend (p. ej. `"rodio"`).
    fn id(&self) -> &'static str;

    /// `true` si este motor puede reproducir canciones de `source`.
    fn supports(&self, source: Source) -> bool;

    /// Reproduce una canción. `source` es la fuente resuelta por el
    /// [`crate::media::StreamResolver`] (URI + contexto de descarga).
    async fn play(
        &self,
        track: &Track,
        source: Option<MediaSource>,
    ) -> Result<PlaybackStatus, PlaybackError>;

    /// Pausa la reproducción.
    async fn pause(&self) -> Result<PlaybackStatus, PlaybackError>;

    /// Reanuda la reproducción.
    async fn resume(&self) -> Result<PlaybackStatus, PlaybackError>;

    /// Detiene la reproducción.
    async fn stop(&self) -> Result<PlaybackStatus, PlaybackError>;

    /// Busca una posición.
    async fn seek(&self, pos: Duration) -> Result<PlaybackStatus, PlaybackError>;

    /// Establece el volumen (0-100).
    async fn set_volume(&self, volume: u8) -> Result<PlaybackStatus, PlaybackError>;

    /// Snapshot actual del estado.
    fn status(&self) -> PlaybackStatus;

    /// `true` si el motor está llenando su buffer inicial. Se usa para
    /// cancelar un prebuffer viejo al aceptar una orden nueva sin provocar los
    /// efectos laterales de `status()` (p. ej. publicar `Finished`).
    fn is_buffering(&self) -> bool {
        self.status().state == PlaybackState::Buffering
    }
}

/// Política de selección de backend.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PlaybackPolicy {
    /// Por fuente: cada canción se enruta al backend que la soporta.
    #[default]
    Auto,
    /// Todo se reproduce con el backend de este id (solo `"rodio"`).
    Global(String),
}
