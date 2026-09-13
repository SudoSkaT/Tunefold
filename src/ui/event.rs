//! Tipos de eventos entre la UI (entrada) y el backend de aplicación.

use crossterm::event::{KeyEvent, MouseEvent};

use std::sync::Arc;

use crate::analysis::{AudioFeatures, WaveformEnvelope};
use crate::app::aggregator::SearchOutcome;
use crate::app::audio::PlaybackStatus;
use crate::app::thumbnail::ThumbnailState;
use crate::domain::{source::Source, track::Track};
use crate::infrastructure::config::ConfigForm;
use crate::infrastructure::storage::{HistoryEntry, PlaylistRow, TrackListeningStats};
use crate::playback::queue::{QueueItemOrigin, RepeatMode};

/// Eventos de entrada del terminal hacia el loop de la UI.
#[derive(Debug)]
pub enum UiEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
}

/// Eventos producidos por el backend (capa de aplicación) hacia la UI.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)] // HTTP al cuadrado de variantes de tamaño desigual
pub enum BackendEvent {
    SearchResults {
        query: String,
        outcome: Box<SearchOutcome<Track>>,
    },
    TrackSaved {
        track: Box<Track>,
        internal_id: i64,
    },
    History(Vec<HistoryEntry>),
    ListeningStats(Vec<TrackListeningStats>),
    Sources(Vec<Source>),
    Settings(ConfigForm),
    Playback(PlaybackStatus),
    /// Recuperación en caliente del MISMO track (re-resolución del stream):
    /// el motor reinició la canción desde el prefijo servible, así que la UI
    /// debe rebobinar el reloj del karaoke con ella. Sin este evento el tick
    /// vendría disfrazado de muestra normal y el guard monótono dejaría las
    /// letras clavadas en la posición previa.
    RecoveryResumed(PlaybackStatus),
    /// Inicio confirmado: la escucha ya fue persistida y sus estadísticas
    /// acompañan al snapshot para que todas las vistas se actualicen juntas.
    PlaybackStarted {
        status: PlaybackStatus,
        stats: Vec<TrackListeningStats>,
    },
    PlaybackError(String),
    /// Comenzó a ejecutarse un seek en el backend (estado transitorio).
    SeekStarted,
    /// Un seek se confirmó como salto REAL del audio.
    SeekCompleted,
    /// Un seek falló: el audio no cambió de posición.
    SeekFailed,
    /// Error **en caliente** del stream de audio (buffer overrun/underrun,
    /// corte de red, error de decodificación en mitad de la canción). No es un
    /// fallo terminal de reproducción: la UI lo muestra como pie de página
    /// discreto (abajo a la derecha) y sigue reproduciendo.
    StreamError(String),
    Related {
        track: Box<Track>,
        /// Recomendaciones FRESCAS de la canción (las que "Reemplazar" puede
        /// imponer como nueva cola).
        related: Vec<Track>,
        /// Cola completa de autoplay YA actualizada con `related` (dedupe):
        /// la vista de recomendaciones muestra lo que realmente va a sonar.
        queue: Vec<Track>,
        /// Origen de cada elemento de `queue` (paralelo, índice a índice):
        /// la UI lo usa para etiquetar qué añadió el autoplay y qué pidió el
        /// usuario, sin mantener la cola duplicada en el frontend.
        origins: Vec<QueueItemOrigin>,
        /// Letra sincronizada en formato LRC (LRCLIB): única fuente del karaoke.
        synced: Option<String>,
        /// Generación de la sesión (devuelta por `BackendCommand::LoadRelated`):
        /// la UI solo aplica la respuesta si sigue siendo la carga en vuelo.
        generation: u64,
    },
    /// Estado global de la cola (shuffle/repeat/tamaño). Se emite cuando cambia
    /// cualquiera de esos atributos (al alternar shuffle/repeat y al reemplazar
    /// la cola) para que la UI refleje de inmediato los mandos, sin depender de
    /// cuándo llega el siguiente tick.
    QueueState {
        shuffle: bool,
        repeat: RepeatMode,
        len: usize,
    },
    /// Miniatura resuelta de un track (`key` = identificador estable).
    /// El estado es `Loaded`/`Failed`/`None`; la UI muestra `Loading` desde
    /// que pide la miniatura hasta que llega este evento.
    Thumbnail {
        key: String,
        state: ThumbnailState,
    },
    /// Último snapshot de análisis de audio (~15 Hz mientras suena). Una SOLA
    /// estructura con las características musicales y la envolvente de forma
    /// de onda del MISMO frame (spec): el consumidor visual las consume juntas
    /// y un solo evento ⇒ un solo redraw.
    VisualFrame {
        features: Arc<AudioFeatures>,
        waveform: Option<Arc<WaveformEnvelope>>,
    },
    Playlists(Vec<PlaylistRow>),
    PlaylistTracks {
        playlist_id: i64,
        tracks: Vec<Track>,
    },
    /// Estado L1K3D completo (tracks actualmente "liked"). Se envía al arrancar
    /// para pintar los corazones de cada vista: cada track trae su id interno y
    /// su id externo, las dos claves por las que la UI resuelve la pertenencia.
    L1K3D(Vec<Track>),
    /// Una canción entró/salió de L1K3D (toggle inmediato: la UI ya puede
    /// pintar el corazón lleno/vacío sin volver a consultar la playlist).
    L1K3DChanged {
        track: Box<Track>,
        liked: bool,
    },
    /// Cola completa (tracks + orígenes en paralelo) tras una mutación que la
    /// UI debe reflejar de inmediato (p. ej. `R` = reemplazar solo autoplay).
    Queue {
        queue: Vec<Track>,
        origins: Vec<QueueItemOrigin>,
    },
    Message(String),
    Error(String),
}
