//! Widgets reutilizables de la TUI (solo renderizado, sin lógica).

pub mod karaoke;
pub mod metadata_table;
pub mod progress_bar;
pub mod search_results;
pub mod song_card;
pub mod source_panel;
pub mod thumb;

use ratatui::layout::Rect;
use std::time::Duration;

use super::glyphs::GLYPHS;

/// Fase del spinner correspondiente al frame actual de animación (delegada a
/// [`crate::ui::glyphs`]: la secuencia y su respaldo ASCII viven en el sistema
/// de glifos, que es la única fuente).
pub(crate) fn spinner_phase(frame: u64) -> char {
    GLYPHS.spinner(frame)
}

/// Formatea una duración como `m:ss` (o `h:mm:ss` si supera la hora).
pub(crate) fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Índice del item bajo el cursor del ratón en una lista que ocupa `area`,
/// asumiendo el hash con la columna de bordes de ratatui (primera fila = `area.y`).
///
/// Las listas de esta TUI no superan la ventana, así que se devuelve el índice
/// directo sin considerar `ListState::offset`.
pub(crate) fn hover_index(mouse: Option<(u16, u16)>, area: Rect) -> Option<usize> {
    let (_, row) = mouse?;
    let inner_top = area.y + 1;
    let inner_bottom = area.bottom().saturating_sub(1);
    if row >= inner_top && row < inner_bottom {
        Some((row - inner_top) as usize)
    } else {
        None
    }
}
