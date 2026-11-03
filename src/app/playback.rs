//! Enrutador de reproducción: selecciona y arbitra el motor de audio
//! adecuado para cada canción según la política configurada.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast::{self, Receiver, Sender};
use tokio::sync::Mutex;

use crate::domain::source::Source;
use crate::domain::stream::MediaSource;
use crate::domain::track::Track;

use super::audio::{PlaybackEngine, PlaybackError, PlaybackEvent, PlaybackPolicy, PlaybackStatus};

/// Lista de backends disponibles en tiempo de ejecución.
///
/// Se construye una vez en Infraestructura y se entrega al enrutador.
pub struct RouterConfig {
    /// Motores registrados (ni la capa de Aplicación sabe cuál es cuál).
    pub engines: Vec<Arc<dyn PlaybackEngine>>,
    /// Política de selección.
    pub policy: PlaybackPolicy,
}

/// Enrutador de reproducción: delega en el motor adecuado según la fuente y
/// reenvía los eventos de todos los motores a un canal compartido.
pub struct PlaybackRouter {
    engines: Vec<Arc<dyn PlaybackEngine>>,
    /// Índice del motor global si la política es `Global`.
    global: Option<usize>,
    /// Último origen reproducido (para operaciones pause/resume/seek).
    current_source: Mutex<Option<Source>>,
    event_tx: Sender<PlaybackEvent>,
}

impl PlaybackRouter {
    /// Crea el enrutador. Los motores notifican a un único bus agrupado
    /// (`EventBus::channel`); `joined` es el receptor de ese bus que este
    /// método re-emite como canal `broadcast` accesible con
    /// [`PlaybackRouter::subscribe`].
    pub fn new(
        cfg: RouterConfig,
        joined: tokio::sync::mpsc::UnboundedReceiver<PlaybackEvent>,
    ) -> Self {
        let global = match cfg.policy {
            PlaybackPolicy::Global(id) => cfg.engines.iter().position(|e| e.id() == id),
            PlaybackPolicy::Auto => None,
        };
        let (event_tx, _) = broadcast::channel::<PlaybackEvent>(64);
        let fanout_tx = event_tx.clone();

        tokio::spawn(async move {
            let mut joined = joined;
            while let Some(event) = joined.recv().await {
                let _ = fanout_tx.send(event);
            }
        });

        Self {
            engines: cfg.engines,
            global,
            current_source: Mutex::new(None),
            event_tx,
        }
    }

    /// Receptor `broadcast` con los eventos fusionados de todos los motores.
    pub fn subscribe(&self) -> Receiver<PlaybackEvent> {
        self.event_tx.subscribe()
    }

    fn engine_for(&self, source: Option<Source>) -> Option<&Arc<dyn PlaybackEngine>> {
        if let Some(idx) = self.global {
            return self.engines.get(idx);
        }
        let source = source?;
        self.engines
            .iter()
            .find(|e| e.supports(source))
            .or_else(|| self.engines.first())
    }

    /// Reproduce una canción en el motor adecuado.
    pub async fn play(
        &self,
        track: &Track,
        source: Option<MediaSource>,
    ) -> Result<PlaybackStatus, PlaybackError> {
        let engine = self.engine_for(Some(track.source)).ok_or_else(|| {
            PlaybackError::Unavailable("no hay ningún motor de reproducción".to_string())
        })?;
        *self.current_source.lock().await = Some(track.source);
        engine.play(track, source).await
    }

    /// Pausa en el motor de la canción actual.
    pub async fn pause(&self) -> Result<PlaybackStatus, PlaybackError> {
        let engine = self.current_engine().await;
        engine.pause().await
    }

    pub async fn resume(&self) -> Result<PlaybackStatus, PlaybackError> {
        let engine = self.current_engine().await;
        engine.resume().await
    }

    pub async fn stop(&self) -> Result<PlaybackStatus, PlaybackError> {
        let engine = self.current_engine().await;
        engine.stop().await
    }

    pub async fn seek(&self, pos: Duration) -> Result<PlaybackStatus, PlaybackError> {
        let engine = self.current_engine().await;
        engine.seek(pos).await
    }

    pub async fn set_volume(&self, vol: u8) -> Result<PlaybackStatus, PlaybackError> {
        let engine = self.current_engine().await;
        engine.set_volume(vol).await
    }

    pub async fn status(&self) -> PlaybackStatus {
        let source = *self.current_source.lock().await;
        self.engine_for(source)
            .map(|e| e.status())
            .unwrap_or_else(PlaybackStatus::idle)
    }

    /// Estado de prebuffer sin efectos laterales (no publica `Finished`).
    pub async fn is_buffering(&self) -> bool {
        let source = *self.current_source.lock().await;
        self.engine_for(source)
            .map(|e| e.is_buffering())
            .unwrap_or(false)
    }

    /// Devuelve `&dyn` no es viable con async_trait; clonamos el Arc del motor.
    async fn current_engine(&self) -> Arc<dyn PlaybackEngine> {
        let source = *self.current_source.lock().await;
        self.engine_for(source)
            .cloned()
            .unwrap_or_else(|| self.engines[0].clone())
    }

    /// Emite un evento a todos los suscriptores.
    pub fn emit(&self, event: PlaybackEvent) {
        let _ = self.event_tx.send(event);
    }
}
