//! Vista de búsqueda: entrada de consulta + resultados.
//!
//! El editor guarda el texto como `Vec<char>` para que el índice del cursor
//! siempre caiga en un límite de carácter (UTF-8 seguro: `String::insert`/
//! `remove` exigen índices de byte, lo que rompía con `ñ`/`á`/emojis).

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::domain::track::Track;
use crate::infrastructure::storage::TrackListeningStats;
use crate::ui::liked::Liked;
use crate::ui::widgets::search_results;

use super::navigation::ListSelection;

#[derive(Debug, Default)]
pub struct SearchState {
    pub input: Vec<char>,
    pub cursor: usize,
    /// `true` si el usuario ha editado la consulta tras la última búsqueda.
    pub editing: bool,
    pub results: Vec<Track>,
    /// Índice en `results` donde empiezan las recomendaciones relacionadas.
    pub related_from: usize,
    pub list_state: ratatui::widgets::ListState,
    pub searching: bool,
    pub last_query: Option<String>,
}

impl SearchState {
    pub fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor, c);
        self.cursor += 1;
        self.editing = true;
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.input.remove(self.cursor - 1);
            self.cursor -= 1;
            self.editing = true;
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.input.len() {
            self.input.remove(self.cursor);
            self.editing = true;
        }
    }

    pub fn clear(&mut self) {
        self.input.clear();
        self.cursor = 0;
        self.editing = true;
    }

    pub fn move_cursor_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_cursor_right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.input.len());
    }

    pub fn move_cursor_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_cursor_end(&mut self) {
        self.cursor = self.input.len();
    }

    /// Texto visible del campo.
    pub fn text(&self) -> String {
        self.input.iter().collect()
    }

    pub fn selected(&self) -> Option<&Track> {
        self.list_state.selected().and_then(|i| self.results.get(i))
    }

    pub fn select_next(&mut self) {
        self.step(true);
    }

    pub fn select_prev(&mut self) {
        self.step(false);
    }
}

impl super::navigation::ListSelection for SearchState {
    fn list_len(&self) -> usize {
        self.results.len()
    }

    fn cursor(&self) -> Option<usize> {
        self.list_state.selected()
    }

    fn set_cursor(&mut self, index: Option<usize>) {
        self.list_state.select(index);
    }
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &mut SearchState,
    mouse: &Option<(u16, u16)>,
    click: &mut bool,
    stats: &std::collections::HashMap<String, TrackListeningStats>,
    liked: &Liked,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    // Entrada de consulta
    let input_block = Block::default().borders(Borders::ALL).title(format!(
        " Consulta {}",
        if state.last_query.is_some() {
            " (Enter selecciona el resultado)"
        } else {
            ""
        }
    ));
    let query = state.text();
    let input_line = Line::from(vec![
        Span::styled("> ", Style::new().fg(Color::Cyan)),
        Span::raw(query.clone()),
    ]);
    frame.render_widget(Paragraph::new(input_line).block(input_block), chunks[0]);

    // Cuadro visible del cursor: estándar + prefijo "> " de ancho 2.
    let width = query
        .chars()
        .take(state.cursor)
        .map(uniseg_width)
        .sum::<u16>();
    frame.set_cursor_position((
        (chunks[0].x + 1 + 2 + width).min(chunks[0].right().saturating_sub(1)),
        chunks[0].y + 1,
    ));

    // Resultados
    if state.searching {
        frame.render_widget(
            Paragraph::new(Line::from("Buscando..."))
                .style(Style::new().add_modifier(Modifier::ITALIC)),
            chunks[1],
        );
    } else if state.results.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(
                "Escribe una consulta y pulsa Enter. Los resultados se guardan al seleccionarlos.",
            ))
            .block(Block::default().borders(Borders::ALL).title(" Resultados ")),
            chunks[1],
        );
    } else {
        let hovered = super::widgets::hover_index(*mouse, chunks[1]);
        if *click {
            if let Some(i) = hovered {
                state.list_state.select(Some(i));
            }
            *click = false;
        }
        search_results::render(
            frame,
            chunks[1],
            &state.results,
            &mut state.list_state,
            hovered,
            state.related_from,
            stats,
            liked,
        );
    }
}

/// Ancho de celda de un carácter (East Asian wide = 2). Asume que la celda
/// base no se usa para antes de llamar al widget; sufiente para ANSI+emoji.
fn uniseg_width(c: char) -> u16 {
    if c as u32 >= 0x1100
        && matches!(c as u32,
        0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6 | 0x1F300..=0x1F64F | 0x1F900..=0x1F9FF | 0x20000..=0x2FFFD
        | 0x30000..=0x3FFFD)
    {
        2
    } else {
        1
    }
}
