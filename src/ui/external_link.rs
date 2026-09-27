//! Popup dedicado de incorporación de enlaces externos.
//!
//! Flujo esperado:
//!
//! ```text
//! Usuario
//!   ↓
//! Click izquierdo en la lupa de la vista de consulta
//!   ↓
//! Aparece POPUP dedicado
//!   ↓
//! Click DERECHO dentro del campo del popup
//!   ↓
//! Clipboard → Input (solo en este campo, en ningún otro sitio)
//!   ↓
//! Detección del tipo de URL (reutiliza `providers::youtube::link`)
//!   ↓
//! Resolución mediante la arquitectura existente (`ResolveLink`)
//!   ↓
//! Incorporación automática a la playlist persistente de enlaces externos
//!   ↓
//! Feedback explícito en cada etapa
//! ```
//!
//! Reglas duras:
//! - El clic derecho NO cambia en ningún otro sitio: listas, reproductor,
//!   historial, playlists, navegación, resultados, visualizador. Solo el campo
//!   de este popup interpreta el clic derecho como pegar.
//! - La detección de URLs reutiliza la frontera existente (`looks_like_url`);
//!   este módulo no duplica conocimiento YouTube.
//! - El módulo es presentación + geometría + edición local. No hace red, no
//!   resuelve streams, no toca la BD: eso lo hace el backend vía comandos.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use super::glyphs::UiGlyphs;

/// Nombre estable y visible de la playlist destino de enlaces externos.
///
/// "Fija" significa que el sistema la conoce como destino predeterminado;
/// el usuario puede renombrarla, editarla y borrar canciones con normalidad.
pub const EXTERNAL_LINKS_PLAYLIST_NAME: &str = "Enlaces externos";

/// Ancho deseado del popup en terminales amplias.
pub const POPUP_W: u16 = 64;
/// Alto deseado del popup en terminales amplias.
pub const POPUP_H: u16 = 12;
/// Ancho del botón de lupa en la vista de consulta (celdas, ASCII-safe).
pub const LUPA_W: u16 = 12;
/// Longitud máxima del campo (evita pegar buffers gigantes en el input).
pub const MAX_INPUT: usize = 512;

/// Estado interno del flujo de incorporación. Semántica equivalente a:
/// `LINK_DETECTED / RESOLVING / RESOLVED / ADDING / ADDED / DUPLICATE /
/// REJECTED / FAILED`. No es obligatorio mostrar estos nombres al usuario;
/// el feedback visible va en `feedback` + línea de estado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LinkFlow {
    /// Campo editable, esperando pegar o escribir.
    #[default]
    Editing,
    /// URL detectada en el campo, pendiente de resolver.
    Detected,
    /// Resolución en vuelo (`ResolveLink` enviado con generación).
    Resolving,
    /// Recurso resuelto, pendiente de asegurar playlist / añadir.
    Resolved,
    /// Añadiendo a la playlist (`SaveAndAddToPlaylist` / `CreatePlaylist`).
    Adding,
    /// Incorporado correctamente.
    Added,
    /// El recurso ya estaba en la playlist (sin duplicar).
    Duplicate,
    /// Enlace rechazado (no es una URL soportada).
    Rejected,
    /// Fallo de resolución o de red/proveedor.
    Failed,
}

impl LinkFlow {
    /// ¿Acepta edición de teclado en este estado?
    pub fn editable(self) -> bool {
        matches!(
            self,
            Self::Editing | Self::Detected | Self::Rejected | Self::Failed
        )
    }

    /// ¿Hay una operación asíncrona en vuelo? (no aceptar nuevos envíos)
    pub fn in_flight(self) -> bool {
        matches!(self, Self::Resolving | Self::Adding)
    }

    /// Mensaje de feedback visible dentro del popup para cada estado.
    pub fn feedback(self) -> &'static str {
        match self {
            Self::Editing => "Pega el enlace con clic derecho dentro del campo.",
            Self::Detected => "Detectando enlace…",
            Self::Resolving => "Resolviendo…",
            Self::Resolved => "Recurso encontrado… Agregando a playlist…",
            Self::Adding => "Agregando a playlist…",
            Self::Added => "Agregado correctamente.",
            Self::Duplicate => "Ya existe en la playlist.",
            Self::Rejected => "Enlace inválido.",
            Self::Failed => "No se pudo resolver.",
        }
    }
}

/// Estado del popup (`None` en `App` = oculto).
#[derive(Debug, Clone, Default)]
pub struct ExternalLinkPopup {
    input: Vec<char>,
    cursor: usize,
    flow: LinkFlow,
    feedback: String,
    /// Generación del `ResolveLink` en vuelo (0 = ninguno). Las respuestas
    /// con generación distinta se ignoran: una resolución tardía nunca
    /// corrompe una operación nueva.
    pub generation: u64,
    /// Texto exacto que se envió a resolver (para emparejar la respuesta).
    pub submitted: String,
    /// Track resuelto pendiente de añadir (se conserva mientras se asegura
    /// la playlist destino).
    pub pending: Option<crate::domain::track::Track>,
}

impl ExternalLinkPopup {
    pub fn new() -> Self {
        Self {
            input: Vec::new(),
            cursor: 0,
            flow: LinkFlow::Editing,
            feedback: LinkFlow::Editing.feedback().to_string(),
            generation: 0,
            submitted: String::new(),
            pending: None,
        }
    }

    pub fn flow(&self) -> LinkFlow {
        self.flow
    }

    /// Fija el estado y refresca el feedback visible.
    pub fn set_flow(&mut self, flow: LinkFlow) {
        self.flow = flow;
        self.feedback = flow.feedback().to_string();
    }

    /// Fija el estado con un detalle explícito (p. ej. motivo del rechazo).
    pub fn set_flow_with(&mut self, flow: LinkFlow, detail: &str) {
        self.flow = flow;
        self.feedback = if detail.is_empty() {
            flow.feedback().to_string()
        } else {
            detail.to_string()
        };
    }

    pub fn feedback(&self) -> &str {
        &self.feedback
    }

    pub fn insert_char(&mut self, c: char) {
        if !self.flow.editable() || self.input.len() >= MAX_INPUT {
            return;
        }
        // Saltos de línea pegados del terminal se aplanan a espacio.
        let c = if c == '\n' || c == '\r' || c == '\t' {
            ' '
        } else {
            c
        };
        self.input.insert(self.cursor, c);
        self.cursor += 1;
        if self.flow != LinkFlow::Editing {
            self.set_flow(LinkFlow::Editing);
        }
    }

    pub fn backspace(&mut self) {
        if !self.flow.editable() || self.cursor == 0 {
            return;
        }
        self.input.remove(self.cursor - 1);
        self.cursor -= 1;
        if self.flow != LinkFlow::Editing {
            self.set_flow(LinkFlow::Editing);
        }
    }

    pub fn delete(&mut self) {
        if !self.flow.editable() || self.cursor >= self.input.len() {
            return;
        }
        self.input.remove(self.cursor);
        if self.flow != LinkFlow::Editing {
            self.set_flow(LinkFlow::Editing);
        }
    }

    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.input.len());
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.input.len();
    }

    pub fn clear(&mut self) {
        self.input.clear();
        self.cursor = 0;
        self.set_flow(LinkFlow::Editing);
    }

    /// Reemplaza el contenido con el texto pegado (el pegado manda).
    pub fn replace_with_paste(&mut self, text: &str) {
        self.input = text.chars().take(MAX_INPUT).collect();
        self.cursor = self.input.len();
        self.set_flow(LinkFlow::Detected);
    }

    /// Texto visible del campo.
    pub fn text(&self) -> String {
        self.input.iter().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.input.is_empty()
    }
}

/// ¿Parece un enlace externo soportado? Delegación pura a la frontera
/// existente (`providers::youtube::link`), sin duplicar conocimiento.
pub fn looks_like_external_link(raw: &str) -> bool {
    #[cfg(feature = "youtube")]
    {
        crate::providers::youtube::link::looks_like_url(raw)
    }
    #[cfg(not(feature = "youtube"))]
    {
        let _ = raw;
        false
    }
}

/// Primer token del texto (recorta espacios y envoltorios habituales del
/// portapapeles: `<url>`, `"url"`, `(url)`). Los enlaces no llevan espacios.
pub fn first_link_token(raw: &str) -> Option<String> {
    let token = raw.split_whitespace().next()?.trim();
    let token = token.trim_matches(|c| matches!(c, '<' | '>' | '"' | '\'' | '(' | ')' | '[' | ']'));
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

/// Lee el portapapeles del sistema de forma robusta, en cualquier contexto.
///
/// - Usa `arboard` (X11/Wayland/macOS/Windows) sin pánicos.
/// - Filtra caracteres de control (conserva `\n` para tokenizar después).
/// - Devuelve el primer token (un enlace no lleva espacios ni saltos).
/// - En sesiones sin clipboard (SSH/TTY) devuelve el motivo para mostrar el
///   fallback nativo del terminal (`Ctrl+Shift+V` / pegar del terminal).
pub fn read_clipboard_text() -> Result<String, String> {
    let raw = arboard::Clipboard::new()
        .and_then(|mut cb| cb.get_text())
        .map_err(|e| e.to_string())?;
    let clean: String = raw
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .collect();
    first_link_token(&clean).ok_or_else(|| "vacío".to_string())
}

/// Área centrada y clampada para el modal (responsive: nunca excede `area`).
///
/// Sigue la misma gramática que `greeting::centered`: como mucho `area - 2`,
/// con mínimos que garantizan campo + feedback + hints incluso en 30x8.
pub fn popup_rect(area: Rect) -> Rect {
    let avail_w = area.width.saturating_sub(2);
    let avail_h = area.height.saturating_sub(2);
    let w = POPUP_W.min(avail_w.max(20)).max(avail_w.min(22));
    let h = POPUP_H.min(avail_h.max(7)).max(avail_h.min(8));
    let w = w.min(area.width).max(1);
    let h = h.min(area.height).max(1);
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    Rect::new(x, y, w, h)
}

/// Rect del campo de entrada dentro del popup (1 fila, con 1 celda de margen).
///
/// Debe coincidir EXACTAMENTE con lo que pinta [`render`]: el hit-test del
/// clic derecho usa este mismo rectángulo.
pub fn field_rect(popup: Rect) -> Rect {
    let inner = Block::default().borders(Borders::ALL).inner(popup);
    if inner.width < 6 || inner.height < 4 {
        return Rect::new(inner.x, inner.y, 0, 0);
    }
    Rect::new(inner.x + 1, inner.y + 1, inner.width.saturating_sub(2), 1)
}

/// Botón de lupa en la fila del borde superior del bloque de consulta.
///
/// Ocupa la esquina superior derecha del área del input (ancho [`LUPA_W`]).
/// El hit-test del clic IZQUIERDO usa este mismo rectángulo.
pub fn lupa_rect(input_area: Rect) -> Rect {
    if input_area.width < LUPA_W + 4 || input_area.height == 0 {
        return Rect::new(input_area.x, input_area.y, 0, 0);
    }
    Rect::new(
        input_area.x + input_area.width.saturating_sub(LUPA_W + 1),
        input_area.y,
        LUPA_W,
        1,
    )
}

/// ¿Está `(x, y)` dentro de `r`? (hit-test puro para tests y mouse).
pub fn contains(r: Rect, x: u16, y: u16) -> bool {
    r.width > 0 && r.height > 0 && x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

/// Renderiza el botón de lupa sobre la fila del borde del bloque de consulta.
///
/// Overlay mínimo (no toca el layout de `search::render`): pinta el segmento
/// clickable en la esquina superior derecha del área del input.
pub fn render_lupa(frame: &mut Frame, input_area: Rect, hovered: bool, glyphs: UiGlyphs) {
    let r = lupa_rect(input_area);
    if r.width == 0 {
        return;
    }
    let label = format!(" [{} Enlace] ", glyphs.link_lupa());
    let style = if hovered {
        Style::new()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    };
    frame.render_widget(Paragraph::new(Line::from(Span::styled(label, style))), r);
}

/// Renderiza el popup sobre la vista actual (fondo limpio con `Clear`).
pub fn render(frame: &mut Frame, area: Rect, popup: &ExternalLinkPopup, glyphs: UiGlyphs) {
    let rect = popup_rect(area);
    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(Color::Cyan))
        .title(Span::styled(
            " Agregar enlace externo ",
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    if inner.width < 6 || inner.height < 4 {
        return;
    }

    let field = field_rect(rect);
    let text = popup.text();
    // Ventana visible del campo: cola que cabe (scroll horizontal implícito).
    let cap = field.width.saturating_sub(4) as usize;
    let visible: String = if cap == 0 {
        String::new()
    } else {
        let chars: Vec<char> = text.chars().collect();
        let start = chars.len().saturating_sub(cap);
        chars[start..].iter().collect()
    };
    let cursor = glyphs.edit_cursor();
    let field_line = Line::from(vec![
        Span::styled("> ", Style::new().fg(Color::Cyan)),
        Span::raw(visible),
        Span::styled(cursor, Style::new().fg(Color::DarkGray)),
    ]);
    if field.width > 0 {
        frame.render_widget(Paragraph::new(field_line), field);
    }

    let (fb_color, fb_bold) = match popup.flow {
        LinkFlow::Added => (Color::Green, true),
        LinkFlow::Duplicate => (Color::Yellow, true),
        LinkFlow::Rejected | LinkFlow::Failed => (Color::LightRed, true),
        LinkFlow::Resolving | LinkFlow::Adding | LinkFlow::Resolved | LinkFlow::Detected => {
            (Color::Yellow, false)
        }
        LinkFlow::Editing => (Color::DarkGray, false),
    };
    let mut style = Style::new().fg(fb_color);
    if fb_bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    let lines = vec![
        Line::from(Span::styled(
            "Pega con clic derecho dentro del campo.",
            Style::new().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(popup.feedback.clone(), style)),
        Line::from(Span::styled(
            "Enter resolver · Esc cerrar · Ctrl+V pegar",
            Style::new().fg(Color::DarkGray),
        )),
    ];
    // Filas bajo el campo dentro del área interior.
    let list_area = Rect::new(
        inner.x,
        field.y.saturating_add(1),
        inner.width,
        inner
            .height
            .saturating_sub(field.y.saturating_sub(inner.y) + 1),
    );
    if list_area.height > 0 {
        let max = list_area.height as usize;
        frame.render_widget(
            Paragraph::new(lines.into_iter().take(max).collect::<Vec<_>>()),
            list_area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::glyphs::{GlyphTheme, UiGlyphs};

    fn glyphs() -> UiGlyphs {
        UiGlyphs::new(GlyphTheme::Unicode)
    }

    #[test]
    fn popup_never_exceeds_area_on_required_sizes() {
        // 120x40, 100x30, 80x24, 70x20, 60x15, 50x12, 40x10, 30x8.
        for (w, h) in [
            (120, 40),
            (100, 30),
            (80, 24),
            (70, 20),
            (60, 15),
            (50, 12),
            (40, 10),
            (30, 8),
        ] {
            let area = Rect::new(0, 0, w, h);
            let p = popup_rect(area);
            assert!(p.width <= area.width, "popup más ancho que {w}x{h}");
            assert!(p.height <= area.height, "popup más alto que {w}x{h}");
            assert!(
                p.x + p.width <= area.x + area.width,
                "desborda a la derecha en {w}x{h}"
            );
            assert!(
                p.y + p.height <= area.y + area.height,
                "desborda abajo en {w}x{h}"
            );
            // Campo utilizable incluso en 30x8.
            let f = field_rect(p);
            assert!(f.width >= 10, "campo visible en {w}x{h}: {f:?}");
            assert_eq!(f.height, 1);
            assert!(contains(p, f.x, f.y), "campo dentro del popup en {w}x{h}");
        }
    }

    #[test]
    fn popup_is_centered_on_large_terminals() {
        let area = Rect::new(0, 0, 120, 40);
        let p = popup_rect(area);
        assert_eq!(p.width, POPUP_W);
        assert_eq!(p.x, (120 - POPUP_W) / 2);
        assert!(p.y > 0 && p.y + p.height < 40);
    }

    #[test]
    fn lupa_button_sits_top_right_and_hit_tests() {
        let input = Rect::new(0, 5, 80, 3);
        let b = lupa_rect(input);
        assert_eq!(b.height, 1);
        assert_eq!(b.y, 5);
        assert!(b.x + b.width <= input.x + input.width);
        assert!(contains(b, b.x, b.y));
        assert!(!contains(b, 0, 5), "fuera a la izquierda no pega");
        assert!(!contains(b, b.x, 0), "otra fila no pega");
        // Área diminuta: botón nulo, sin pánicos ni pegados fantasma.
        let tiny = lupa_rect(Rect::new(0, 0, 10, 3));
        assert_eq!(tiny.width, 0);
        assert!(!contains(tiny, 0, 0));
    }

    #[test]
    fn token_takes_first_link_and_strips_wrapping() {
        assert_eq!(
            first_link_token("  https://youtu.be/abc123XYZ-_  "),
            Some("https://youtu.be/abc123XYZ-_".to_string())
        );
        assert_eq!(
            first_link_token("<https://youtu.be/abc123XYZ-_>"),
            Some("https://youtu.be/abc123XYZ-_".to_string())
        );
        assert_eq!(
            first_link_token("https://youtu.be/abc123XYZ-_\nhttps://other"),
            Some("https://youtu.be/abc123XYZ-_".to_string())
        );
        assert_eq!(first_link_token("   "), None);
        assert_eq!(first_link_token(""), None);
    }

    #[test]
    fn flow_feedback_covers_every_state() {
        for flow in [
            LinkFlow::Editing,
            LinkFlow::Detected,
            LinkFlow::Resolving,
            LinkFlow::Resolved,
            LinkFlow::Adding,
            LinkFlow::Added,
            LinkFlow::Duplicate,
            LinkFlow::Rejected,
            LinkFlow::Failed,
        ] {
            assert!(!flow.feedback().is_empty(), "{flow:?} sin feedback");
        }
        assert!(LinkFlow::Editing.editable());
        assert!(LinkFlow::Failed.editable());
        assert!(!LinkFlow::Resolving.editable());
        assert!(LinkFlow::Resolving.in_flight());
        assert!(!LinkFlow::Editing.in_flight());
    }

    #[test]
    fn editing_is_utf8_safe_and_capped() {
        let mut p = ExternalLinkPopup::new();
        for c in ['ñ', 'á', '🎵', 'x'] {
            p.insert_char(c);
        }
        assert_eq!(p.text(), "ñá🎵x");
        p.move_home();
        p.backspace();
        assert_eq!(p.text(), "ñá🎵x", "backspace en 0 no rompe");
        p.move_end();
        p.backspace();
        assert_eq!(p.text(), "ñá🎵");
        // Pegado largo se recorta a MAX_INPUT sin pánico.
        p.replace_with_paste(&"y".repeat(MAX_INPUT + 100));
        assert_eq!(p.text().chars().count(), MAX_INPUT);
    }

    #[test]
    fn render_never_panics_on_required_sizes() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        for (w, h) in [(120, 40), (80, 24), (60, 15), (40, 10), (30, 8)] {
            let backend = TestBackend::new(w, h);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal
                .draw(|f| {
                    render(f, f.area(), &ExternalLinkPopup::new(), glyphs());
                    render_lupa(f, Rect::new(0, 0, w.min(80), 3), false, glyphs());
                })
                .unwrap_or_else(|_| panic!("popup a {w}x{h}"));
        }
    }

    #[test]
    fn popup_buffer_shows_title_field_and_hints() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render(f, f.area(), &ExternalLinkPopup::new(), glyphs()))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("Agregar enlace externo"), "título visible");
        assert!(
            text.contains("clic derecho"),
            "hint de pegado visible: {text}"
        );
        assert!(text.contains("Esc cerrar"), "hint de cierre visible");
    }

    #[test]
    fn detector_delegates_without_false_positives() {
        // Sin feature youtube nunca detecta (sin red, sin pánicos).
        #[cfg(not(feature = "youtube"))]
        {
            assert!(!looks_like_external_link("https://youtu.be/abc123XYZ-_"));
        }
        // Con feature delega a la frontera existente.
        #[cfg(feature = "youtube")]
        {
            assert!(looks_like_external_link("https://youtu.be/abc123XYZ-_"));
            assert!(!looks_like_external_link("queen bohemian rhapsody"));
        }
        // El nombre destino es estable y visible.
        assert_eq!(EXTERNAL_LINKS_PLAYLIST_NAME, "Enlaces externos");
    }
}
