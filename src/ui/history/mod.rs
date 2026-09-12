//! Vista de historial de reproducción.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem};
use ratatui::Frame;

use crate::infrastructure::storage::HistoryEntry;
use crate::ui::liked::Liked;
use crate::ui::widgets::format_duration;

pub fn render(frame: &mut Frame, area: Rect, entries: &[HistoryEntry], liked: &Liked) {
    let items: Vec<ListItem> = if entries.is_empty() {
        vec![ListItem::new(Line::from("Sin reproducciones todavía."))]
    } else {
        entries
            .iter()
            .map(|e| {
                let artist = e.artist_name.as_deref().unwrap_or("?");
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{}  ", e.played_at),
                        Style::new().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!("[{}] ", e.source.label()),
                        Style::new().fg(Color::Cyan),
                    ),
                    Span::raw(format!("{artist} - {}", e.title)),
                    if liked.contains_id(e.track_id) {
                        Span::styled(
                            format!("   {}", crate::ui::glyphs::GLYPHS.heart_liked()),
                            Style::new()
                                .fg(Color::LightRed)
                                .add_modifier(Modifier::BOLD),
                        )
                    } else {
                        Span::raw("")
                    },
                    Span::styled(
                        e.duration
                            .and_then(|ms| u64::try_from(ms).ok())
                            .map(|ms| {
                                format!(
                                    "  ({})",
                                    format_duration(std::time::Duration::from_millis(ms))
                                )
                            })
                            .unwrap_or_else(|| "  (duración pendiente)".to_string()),
                        Style::new().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!("  {}×", e.play_count),
                        Style::new().fg(Color::Green),
                    ),
                ]))
            })
            .collect()
    };

    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title(" Historial ")),
        area,
    );
}
