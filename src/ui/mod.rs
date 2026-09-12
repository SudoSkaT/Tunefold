//! Capa de Presentación (ratatui).
//!
//! Únicamente renderiza widgets y reenvía teclas; la lógica vive en la capa de
//! Aplicación, ejecutada por el backend de la UI ([`backend`]).

pub mod app;
pub mod backend;
pub mod dashboard;
pub mod event;
pub mod glyphs;
pub mod help;
pub mod history;
pub mod layout;
pub mod liked;
pub mod metadata;
pub mod navigation;
pub mod playlists;
pub mod related;
pub mod search;
pub mod settings;
pub mod sources;
pub mod view;
pub mod widgets;

use anyhow::Result;
use crossterm::event::{
    self as crossterm_event, DisableMouseCapture, EnableMouseCapture, Event as CrosstermEvent,
    KeyEventKind, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use tokio::sync::mpsc;

use crate::infrastructure::db::Db;

use self::app::App;
use self::backend::{spawn_backend, Backend};
use self::event::UiEvent;

/// Contenido de la banda superior de la vista Related (spec §16/§17).
///
/// Un subestado de la capa de presentación que conmuta con `v` sin regenerar
/// recomendaciones, sin tocar la reproducción y sin destruir el estado del
/// otro modo (las letras y el visual coexisten en la misma capa ambiental).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VisualContent {
    /// Letras si las hay; si no, el visual (recomendado por defecto).
    #[default]
    Auto,
    /// Siempre letras (karaoke ambiental).
    Lyrics,
    /// Siempre el visualizador.
    Visual,
}

impl VisualContent {
    /// Siguiente modo en el ciclo `Auto → Lyrics → Visual → Auto`.
    pub fn next(self) -> Self {
        match self {
            Self::Auto => Self::Lyrics,
            Self::Lyrics => Self::Visual,
            Self::Visual => Self::Auto,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "automático",
            Self::Lyrics => "letras",
            Self::Visual => "visual",
        }
    }
}

/// Inicializa la terminal y ejecuta el loop principal de la TUI.
pub async fn run() -> Result<()> {
    let mut terminal = ratatui::init();
    enable_keyboard_enhancements();
    let _ = crossterm::execute!(std::io::stdout(), EnableMouseCapture);

    let result = async {
        let db = Db::connect(
            crate::infrastructure::dirs::db_path()
                .to_str()
                .unwrap_or("data/music.db"),
        )
        .await?;
        let backend = Backend::new(db, crate::infrastructure::config::Config::load());
        let (backend_tx, backend_rx) = spawn_backend(backend);

        let (ui_tx, ui_rx) = mpsc::unbounded_channel::<UiEvent>();
        spawn_input_thread(ui_tx);

        let mut app = App::new(backend_tx);
        app.run(&mut terminal, ui_rx, backend_rx).await
    }
    .await;

    let _ = crossterm::execute!(std::io::stdout(), DisableMouseCapture);
    disable_keyboard_enhancements();
    ratatui::restore();
    result?;
    Ok(())
}

/// Activa el protocolo de teclado kitty (`modifyOtherKeys`/CSI-u).
///
/// La navegación de vistas usa `Shift+1..Shift+7` y no depende de este
/// protocolo: `Shift+dígito` escribe un símbolo imprimible (`!@#$%^&`) que la
/// UI mapea a la vista, y con el protocolo activo se reporta además como
/// `Char('1'..'7')` + SHIFT. El protocolo sirve para desambiguar Escape de las
/// secuencias de teclas especiales. En terminales que no lo soportan, la
/// secuencia se ignora sin efectos colaterales.
fn enable_keyboard_enhancements() {
    let flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
        | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES;
    let _ = crossterm::execute!(std::io::stdout(), PushKeyboardEnhancementFlags(flags));
}

fn disable_keyboard_enhancements() {
    let _ = crossterm::execute!(std::io::stdout(), PopKeyboardEnhancementFlags);
}

/// Lee eventos de teclado (bloqueante) en un hilo nativo y los reenvía al loop.
///
/// Se usa un hilo `std` y no un task de tokio para que la salida del programa no
/// quede bloqueada esperando a un `event::read()` que nunca retorna.
fn spawn_input_thread(tx: mpsc::UnboundedSender<UiEvent>) {
    std::thread::spawn(move || loop {
        match crossterm_event::read() {
            Ok(CrosstermEvent::Key(key)) => {
                if key.kind == KeyEventKind::Press {
                    let _ = tx.send(UiEvent::Key(key));
                }
            }
            Ok(CrosstermEvent::Mouse(mouse)) => {
                let _ = tx.send(UiEvent::Mouse(mouse));
            }
            Ok(CrosstermEvent::Resize(w, h)) => {
                let _ = tx.send(UiEvent::Resize(w, h));
            }
            Ok(_) => {}
            Err(_) => break,
        }
    });
}
