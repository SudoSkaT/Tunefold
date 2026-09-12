//! Vista de metadatos de la canción seleccionada.

use ratatui::layout::Rect;
use ratatui::Frame;

use crate::domain::track::Track;
use crate::ui::liked::Liked;

use crate::ui::widgets::metadata_table;

pub fn render(frame: &mut Frame, area: Rect, track: Option<&Track>, liked: &Liked) {
    match track {
        Some(t) => metadata_table::render(frame, area, t, liked),
        None => metadata_table::placeholder(
            frame,
            area,
            "Sin canción seleccionada. Selecciona un resultado en Search (Shift+3).",
        ),
    }
}
