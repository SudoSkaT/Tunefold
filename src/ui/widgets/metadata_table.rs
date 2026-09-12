//! Tabla de metadatos de una canción: campo → valor.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Cell, Row, Table, TableState};
use ratatui::Frame;

use crate::domain::track::Track;
use crate::ui::liked::Liked;

use super::format_duration;

pub fn render(frame: &mut Frame, area: Rect, track: &Track, liked: &Liked) {
    let artists = track
        .artists
        .iter()
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let genres = track
        .genres
        .iter()
        .map(|g| g.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let duration = track
        .duration
        .map(format_duration)
        .unwrap_or_else(|| "-".to_string());
    let album = track
        .album
        .as_ref()
        .map(|a| {
            let release = a
                .release_date
                .map(|d| format!(" ({d})"))
                .unwrap_or_default();
            format!("{}{release}", a.title)
        })
        .unwrap_or_else(|| "-".to_string());
    // L1K3D: el corazón de la canción en curso, visible aunque sea otra
    // dependencia del track.
    let liked_cell = if liked.contains(track) {
        format!("{} En L1K3D", crate::ui::glyphs::GLYPHS.heart_liked())
    } else {
        "No en L1K3D · l para marcarla".to_string()
    };
    let liked_style = if liked.contains(track) {
        Style::new()
            .fg(Color::LightRed)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(Color::DarkGray)
    };

    let rows = [
        Row::new(vec![Cell::from("Título"), Cell::from(track.title.clone())]),
        Row::new(vec![Cell::from("Artistas"), Cell::from(artists)]),
        Row::new(vec![Cell::from("Álbum"), Cell::from(album)]),
        Row::new(vec![Cell::from("Duración"), Cell::from(duration)]),
        Row::new(vec![Cell::from("Fuente"), Cell::from(track.source.label())]),
        Row::new(vec![
            Cell::from("L1K3D"),
            Cell::from(liked_cell).style(liked_style),
        ]),
        Row::new(vec![
            Cell::from("ISRC"),
            Cell::from(track.isrc.clone().unwrap_or_else(|| "-".to_string())),
        ]),
        Row::new(vec![
            Cell::from("ID externo"),
            Cell::from(track.external_id.clone().unwrap_or_else(|| "-".to_string())),
        ]),
        Row::new(vec![
            Cell::from("URL"),
            Cell::from(track.url.clone().unwrap_or_else(|| "-".to_string())),
        ]),
        Row::new(vec![
            Cell::from("Géneros"),
            Cell::from(if genres.is_empty() {
                "-".to_string()
            } else {
                genres
            }),
        ]),
    ];

    let widths = [
        ratatui::layout::Constraint::Percentage(25),
        ratatui::layout::Constraint::Percentage(75),
    ];
    let table = Table::new(rows, widths)
        .header(
            Row::new(vec![Cell::from("Campo"), Cell::from("Valor")])
                .style(Style::new().add_modifier(Modifier::BOLD).fg(Color::Cyan)),
        )
        .block(Block::default().borders(Borders::ALL).title(" Metadatos "));

    let mut state = TableState::default();
    frame.render_stateful_widget(table, area, &mut state);
}

pub fn placeholder(frame: &mut Frame, area: Rect, msg: &str) {
    let block = Block::default().borders(Borders::ALL).title(" Metadatos ");
    let text = ratatui::widgets::Paragraph::new(Line::from(msg)).block(block);
    frame.render_widget(text, area);
}
