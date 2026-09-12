//! Barra de progreso de reproducción.

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Gauge};
use ratatui::Frame;

use crate::app::audio::PlaybackStatus;
use crate::ui::widgets::{format_duration, spinner_phase};

pub fn render(frame: &mut Frame, area: Rect, playback: &PlaybackStatus, frame_anim: u64) {
    // En alturas reducidas (perfil Tiny) se dibuja la barra desnuda, sin
    // decoración, para no robarla de filas al resto de secciones.
    let block = if area.height >= 3 {
        Block::default().borders(Borders::ALL).title(" Progreso ")
    } else {
        Block::default()
    };

    let position = playback.position;
    let duration = playback.duration.unwrap_or_default();
    let ratio = if duration.is_zero() {
        0.0
    } else {
        (position.as_secs_f64() / duration.as_secs_f64()).clamp(0.0, 1.0)
    };

    // Buffer underrun: el stream va lento o se cortó. Se muestra un spinner
    // animado para que el usuario vea que la reproducción se está corrigiendo
    // y no que la UI se quedó colgada.
    let stalled = playback.stalled && playback.track.is_some();
    let label = if playback.track.is_none() {
        " sin reproducción (Enter en resultados reproduce) ".to_string()
    } else if stalled {
        format!(
            " {} stream lento · rellenando buffer…  {} / {} ",
            spinner_phase(frame_anim),
            format_duration(position),
            format_duration(duration)
        )
    } else {
        format!(
            " {} / {} ",
            format_duration(position),
            format_duration(duration)
        )
    };

    let gauge_style = if stalled {
        Style::new().fg(Color::Yellow)
    } else {
        Style::new().fg(Color::Cyan)
    };

    let gauge = Gauge::default()
        .block(block)
        .gauge_style(gauge_style)
        .ratio(ratio)
        .label(label);

    frame.render_widget(gauge, area);
}
