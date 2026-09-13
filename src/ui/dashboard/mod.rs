//! Vista Now Playing: tarjeta de canción + barra de progreso + panel de
//! recomendaciones (scrollable) + panel de controles.
//!
//! El reparto vertical lo decide [`crate::ui::layout`] según el perfil del
//! terminal: son SIEMPRE las mismas secciones, solo cambia su tamaño (nunca
//! desaparecen, ni en terminales pequeños).

use ratatui::layout::{Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::audio::{PlaybackState, PlaybackStatus};
use crate::app::thumbnail::ThumbnailState;

use super::glyphs::GLYPHS;
use super::layout::{dashboard_layout, TerminalProfile};
use crate::ui::related::RelatedState;
use crate::ui::widgets::{progress_bar, song_card};
use crate::visualization::palette::VisualPalette;
use crate::visualization::render as visualizer;
use crate::visualization::VisualState;

#[allow(clippy::too_many_arguments)] // el frame de animación es interno a la vista
pub fn render(
    frame: &mut Frame,
    area: Rect,
    playback: &PlaybackStatus,
    related: &mut RelatedState,
    autoplay: bool,
    shuffle: bool,
    repeat: crate::playback::queue::RepeatMode,
    mouse: &Option<(u16, u16)>,
    click: &mut bool,
    thumbnails: &std::collections::HashMap<String, ThumbnailState>,
    frame_anim: u64,
    stats: &std::collections::HashMap<String, crate::infrastructure::storage::TrackListeningStats>,
    visual: &VisualState,
    liked: bool,
    liked_state: &crate::ui::liked::Liked,
) {
    // Reparto adaptativo (todas las secciones presentes, solo cambia el
    // tamaño). El panel de recomendaciones se clampa para que la lista pueda
    // crecer sin oprimir a la barra/controles.
    let profile = TerminalProfile::from_rect(area);
    let split = dashboard_layout(area, profile);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(split.constraints())
        .split(area);

    let state = playback
        .track
        .as_ref()
        .and_then(|t| thumbnails.get(&t.identifier()));
    song_card::render(frame, chunks[0], playback.track.as_ref(), state, frame_anim);
    // La banda del visual es COMPOSICIÓN completa (osciloscopio + barras);
    // la paleta de la portada ya la fundió el motor en `visual.scene.palette`.
    // Con el análisis inactivo el renderer pinta la banda apagada.
    visualizer::render(frame, chunks[1], visual, playback.position.as_secs_f32());
    progress_bar::render(frame, chunks[2], playback, frame_anim);

    let queue_txt = format!(
        "{} · a: autoplay {}",
        related.tracks.len(),
        if autoplay { "ON" } else { "OFF" }
    );
    let title = format!(" Recomendaciones · {queue_txt} ",);
    crate::ui::related::render_tracks_list(
        frame,
        chunks[3],
        related,
        title,
        "Sin recomendaciones todavía. Reproduce una canción o pulsa Enter.",
        playback.track.as_ref().map(|t| t.identifier()).as_deref(),
        mouse,
        click,
        stats,
        liked_state,
    );

    render_controls(
        frame,
        chunks[4],
        playback,
        related,
        autoplay,
        shuffle,
        repeat,
        frame_anim,
        liked,
        &visual.scene.palette,
    );
}

/// Panel de estado de reproducción bajo el panel de recomendaciones (antes
/// "Controles" — tabla de atajos; estos viven SOLO en la ayuda Shift+H). Muestra
/// el fotograma actual de la sesión: estado, posiciones, modo de cola y L1K3D.
///
/// En perfiles bajos (Tiny) condensa en una sola línea; solo cambia la forma,
/// nunca la información.
#[allow(clippy::too_many_arguments)] // peak, estado, modo de cola y animación son datos del panel
fn render_controls(
    frame: &mut Frame,
    area: Rect,
    playback: &PlaybackStatus,
    related: &RelatedState,
    autoplay: bool,
    shuffle: bool,
    repeat: crate::playback::queue::RepeatMode,
    frame_anim: u64,
    liked: bool,
    palette: &VisualPalette,
) {
    let state = match playback.state {
        PlaybackState::Playing => format!(
            "{} reproduciendo {}",
            GLYPHS.play(),
            GLYPHS.activity(frame_anim)
        ),
        PlaybackState::Paused => format!("{} pausado", GLYPHS.pause()),
        PlaybackState::Stopped => format!("{} detenido", GLYPHS.stop()),
        PlaybackState::Buffering => format!("{} preparando", GLYPHS.spinner(frame_anim)),
        PlaybackState::Seeking => format!("{} buscando", GLYPHS.seeking()),
    };
    let stall = if playback.stalled {
        format!(
            " · {} red lenta: rellenando buffer…",
            GLYPHS.spinner(frame_anim)
        )
    } else {
        String::new()
    };
    // Indicadores SIEMPRE visibles del modo de cola (Nivel 1): reflejan el
    // estado real confirmado por el backend (y el optimista de la UI al
    // teclear f/t). Sin estos no queda claro por qué el siguiente/anterior
    // salta.
    let shuffle_txt = if shuffle { "SHUFFLE" } else { "orden" };
    let repeat_txt = match repeat {
        crate::playback::queue::RepeatMode::All => "REPETIR todo",
        crate::playback::queue::RepeatMode::Off => "repetir off",
        crate::playback::queue::RepeatMode::One => "REPETIR una",
    };
    // Corazón de L1K3D: coloreado con la paleta de la portada (primary con
    // pulso mientras está "liked"; secondary/oscuro cuando no). El estado lo
    // marca el App (set `liked`), alimentado por `L1K3D`/`L1K3DChanged`.
    let pulse = (frame_anim / 12).is_multiple_of(2);
    let heart_style = if liked {
        Style::new().fg(Color::Rgb(
            if pulse {
                palette.primary[0]
            } else {
                palette.accent[0]
            },
            if pulse {
                palette.primary[1]
            } else {
                palette.accent[1]
            },
            if pulse {
                palette.primary[2]
            } else {
                palette.accent[2]
            },
        ))
    } else {
        Style::new().fg(Color::Rgb(
            palette.secondary[0],
            palette.secondary[1],
            palette.secondary[2],
        ))
    };
    let heart = Span::styled(
        if liked {
            GLYPHS.heart_liked()
        } else {
            GLYPHS.heart_empty()
        },
        heart_style,
    );
    // Posición de reproducción real del motor (la misma que ve la barra de
    // progreso): la extrapolación del karaoke no aplica aquí.
    let elapsed = super::widgets::format_duration(playback.position);
    let total = playback
        .duration
        .map(super::widgets::format_duration)
        .unwrap_or_else(|| "--:--".to_string());
    let autoplay_txt = if autoplay { "ON" } else { "OFF" };
    if area.height < 5 {
        // Fila compacta para terminales bajos: misma información, una sola
        // línea (sin decoración, para no robarle filas a la lista).
        frame.render_widget(
            Paragraph::new(vec![Line::from(vec![
                Span::raw(format!(
                    "{state}{stall}  ·  {elapsed} / {total}  ·  {shuffle_txt} · {repeat_txt} · autoplay {autoplay_txt}"
                )),
                Span::raw("  "),
                Span::styled("l", Style::new().fg(Color::Cyan)),
                Span::raw(" L1K3D "),
                heart,
            ])]),
            area,
        );
        return;
    }
    let lines = vec![
        Line::from(vec![Span::raw(format!(
            "{state}{stall}  ·  {elapsed} / {total}"
        ))]),
        Line::from(vec![
            Span::styled(
                format!(
                    "{shuffle_txt} · {repeat_txt} · {} en cola",
                    related.tracks.len()
                ),
                Style::new().fg(Color::White),
            ),
            Span::raw(format!(" · autoplay {autoplay_txt}")),
            Span::raw(" · "),
            Span::styled("l", Style::new().fg(Color::Cyan)),
            Span::raw(" L1K3D "),
            heart,
        ]),
        Line::styled(
            "Shift+H: todos los atajos",
            Style::new().fg(Color::DarkGray),
        ),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Reproducción "),
        ),
        area,
    );
}
