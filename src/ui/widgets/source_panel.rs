//! Panel de fuentes: lista de proveedores activos.

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem};
use ratatui::Frame;

use crate::domain::source::Source;

pub fn render(frame: &mut Frame, area: Rect, sources: &[Source]) {
    let items: Vec<ListItem> = if sources.is_empty() {
        vec![ListItem::new(Line::from("Sin fuentes configuradas"))]
    } else {
        sources
            .iter()
            .map(|s| {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        crate::ui::glyphs::GLYPHS.active_source(),
                        Style::new().fg(Color::Green),
                    ),
                    Span::raw(s.label()),
                ]))
            })
            .collect()
    };

    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Fuentes activas "),
        ),
        area,
    );
}
