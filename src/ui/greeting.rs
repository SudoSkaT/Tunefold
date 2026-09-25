//! Modal de bienvenida diaria: saluda, pregunta el nombre la primera vez y
//! muestra el hilo de la última sesión (motivo + artistas + continuación).
//!
//! Presentación pura (patrón `help.rs`): el estado vive en `App`, la
//! persistencia en el backend (`LoadGreeting`/`SaveDisplayName`/
//! `DismissGreeting`), y el brief se construye con
//! `recommendation::greeting::build_brief` (sin ML, datos ya cargados).

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

/// Estado del modal de bienvenida (`None` = no mostrar).
#[derive(Debug, Default)]
pub struct GreetingUi {
    /// Franja ya resuelta (`buenos días|buenas tardes|buenas noches`).
    pub franja: String,
    /// Nombre confirmado (si ya lo dio otra vez, no se pregunta).
    pub display_name: Option<String>,
    /// `true` cuando aún no hay nombre: el modal es un input.
    pub asking_name: bool,
    /// Borrador del nombre en edición (UTF-8 seguro por chars).
    pub name_draft: Vec<char>,
    /// `porque escuchaste X ×N` (si hay datos).
    pub reason: Option<String>,
    /// Top artistas (máx 3, si hay datos).
    pub top_artists: Vec<String>,
    /// Etiqueta `Artista - Título` para continuar (si hay historial).
    pub continue_label: Option<String>,
}

impl GreetingUi {
    pub fn insert_char(&mut self, c: char) {
        if self.name_draft.len() < 32 {
            self.name_draft.push(c);
        }
    }

    pub fn backspace(&mut self) {
        self.name_draft.pop();
    }

    pub fn draft_text(&self) -> String {
        self.name_draft.iter().collect()
    }
}

/// Área centrada para el modal (responsive: nunca más ancha/alta que el área).
fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width.saturating_sub(2)).max(20);
    let h = h.min(area.height.saturating_sub(2)).max(8);
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    Rect::new(x, y, w, h)
}

/// Renderiza el modal sobre la vista actual (fondo limpio con `Clear`).
pub fn render(frame: &mut Frame, area: Rect, g: &GreetingUi) {
    let popup = centered(area, 62, 13);
    frame.render_widget(Clear, popup);
    let title = match &g.display_name {
        Some(n) => format!(" {franja}, {n} ", franja = g.franja),
        None => format!(" ¡{franja}! ", franja = g.franja),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(Color::Cyan))
        .title(Span::styled(
            title,
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.height == 0 {
        return;
    }

    let mut lines: Vec<Line> = Vec::new();
    if g.asking_name {
        lines.push(Line::from("¿Cómo quieres que te llame?"));
        lines.push(Line::from(vec![
            Span::styled("> ", Style::new().fg(Color::Cyan)),
            Span::raw(g.draft_text()),
            Span::styled("▌", Style::new().fg(Color::DarkGray)),
        ]));
        lines.push(Line::from(Span::styled(
            "Enter guarda el nombre · Esc seguir sin nombre",
            Style::new().fg(Color::DarkGray),
        )));
    } else {
        lines.push(Line::from("Seguimos donde lo dejaste:"));
    }
    if let Some(r) = &g.reason {
        lines.push(Line::from(vec![
            Span::styled("♥ ", Style::new().fg(Color::LightRed)),
            Span::raw(r.clone()),
        ]));
    }
    if !g.top_artists.is_empty() {
        lines.push(Line::from(format!(
            "Artistas: {}",
            g.top_artists.join(" · ")
        )));
    }
    if let Some(c) = &g.continue_label {
        lines.push(Line::from(vec![
            Span::raw("Continúa con: "),
            Span::styled(c.clone(), Style::new().fg(Color::Yellow)),
        ]));
    }
    if g.reason.is_none() && g.continue_label.is_none() {
        lines.push(Line::from(
            "Aún no hay escucha registrada: busca algo y dale a Enter.",
        ));
    }
    if !g.asking_name {
        lines.push(Line::from(Span::styled(
            "Enter/Esc cerrar · Shift+3 buscar",
            Style::new().fg(Color::DarkGray),
        )));
    }

    let max = inner.height as usize;
    frame.render_widget(
        Paragraph::new(lines.into_iter().take(max).collect::<Vec<_>>()),
        inner,
    );
}
