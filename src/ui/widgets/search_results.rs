//! Lista de resultados de búsqueda con resaltado de la selección y del hover.
//! Las recomendaciones relacionadas con la búsqueda (a partir de
//! `related_from`) se marcan con un prefijo para distinguirlas de los aciertos.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

use crate::domain::track::Track;
use crate::infrastructure::storage::TrackListeningStats;
use crate::ui::liked::Liked;

/// Renderiza los resultados. `hovered` es el índice bajo el cursor (si lo hay)
/// y se resalta con `fg(Gray)` para no confundirlo con la selección activa.
#[allow(clippy::too_many_arguments)] // resultados, estado, hover y metadatos son datos del render
pub fn render(
    frame: &mut Frame,
    area: Rect,
    results: &[Track],
    state: &mut ListState,
    hovered: Option<usize>,
    related_from: usize,
    stats: &std::collections::HashMap<String, TrackListeningStats>,
    liked: &Liked,
) {
    let items: Vec<ListItem> = results
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let is_rec = i >= related_from && related_from > 0;
            let artist = t.primary_artist_name().unwrap_or("Desconocido");
            let duration = t
                .duration
                .map(super::format_duration)
                .unwrap_or_else(|| "duración pendiente".to_string());
            let listened = stats.get(&t.identifier());
            let base = Line::from(vec![
                Span::styled(
                    format!("[{}] ", t.source.label()),
                    if is_rec {
                        Style::new().fg(Color::Yellow)
                    } else {
                        Style::new().fg(Color::Cyan)
                    },
                ),
                if is_rec {
                    Span::styled(
                        crate::ui::glyphs::GLYPHS.recommended(),
                        Style::new().fg(Color::Yellow),
                    )
                } else {
                    Span::raw("")
                },
                Span::raw(format!("{artist} - {}", t.title)),
                if liked.contains(t) {
                    Span::styled(
                        format!("   {}", crate::ui::glyphs::GLYPHS.heart_liked()),
                        Style::new()
                            .fg(Color::LightRed)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::raw("")
                },
                Span::styled(format!("  ({duration})"), Style::new().fg(Color::DarkGray)),
                listened
                    .map(|s| {
                        Span::styled(
                            if s.recently_played {
                                format!(
                                    "  {} reciente · {}×",
                                    crate::ui::glyphs::GLYPHS.recent(),
                                    s.play_count
                                )
                            } else {
                                format!(
                                    "  {} {}×",
                                    crate::ui::glyphs::GLYPHS.listens(),
                                    s.play_count
                                )
                            },
                            Style::new().fg(Color::Green),
                        )
                    })
                    .unwrap_or_else(|| Span::raw("")),
            ]);
            let style = if Some(i) == hovered {
                Style::new().fg(Color::Gray)
            } else {
                Style::new()
            };
            ListItem::new(base).style(style)
        })
        .collect();

    let title = if related_from > 0 {
        " Resultados + Recomendadas "
    } else {
        " Resultados "
    };
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(
            Style::new()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, area, state);
}
