//! Definición ÚNICA de atajos de teclado y popup de ayuda (Shift+H).
//!
//! Cada mapeo a `KeyCode`/`Modifier` del render dispone de su fila aquí; el
//! test de coherencia [`tests::help_covers_every_handled_key`] comprueba que
//! ningún atajo funcional quede fuera de la ayuda.
//!
//! El popup es responsive: en anchos amplios (>= 80) se compone en dos/según
//! tres columnas; si no cabe verticalmente aparece scroll (↑/↓, PgUp/PgDn,
//! w/s/k/j) y la mayoría de teclas lo cierra.
//!
//! Invariante UX: los símbolos van SIEMPRE acompañados de su palabra en la
//! columna de descripción; la columna de teclas solo usa combinaciones ASCII.

use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use super::glyphs::UiGlyphs;
use super::layout::TerminalProfile;

/// Contexto de interacción al que pertenece un atajo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Context {
    Global,
    Playback,
    Navigation,
    Queue,
    Search,
    Playlists,
    Sources,
    Metadata,
    Settings,
    Related,
    Other,
}

impl Context {
    /// Etiqueta del grupo en la ayuda (visible).
    pub fn label(self) -> &'static str {
        match self {
            Self::Global => "Global",
            Self::Playback => "Reproducción",
            Self::Navigation => "Navegación",
            Self::Queue => "Cola",
            Self::Search => "Búsqueda",
            Self::Playlists => "Playlists",
            Self::Sources => "Fuentes",
            Self::Metadata => "Metadatos",
            Self::Settings => "Ajustes",
            Self::Related => "Relacionadas",
            Self::Other => "Otros",
        }
    }
}

/// Una fila de la tabla de ayuda. `context` + `keys` deben bastar por sí solos
/// para describir el atajo: la descripción nunca depende del glifo.
#[derive(Debug, Clone, Copy)]
pub struct KeyBinding {
    pub context: Context,
    /// Combinación visible (p. ej. `Shift+H`).
    pub keys: &'static str,
    /// Qué hace. Lleva la palabra; los símbolos son refuerzo, nunca el único
    /// canal.
    pub desc: &'static str,
}

/// Tabla de atajos. Es el punto de verdad ÚNICO del teclado.
pub const BINDINGS: &[KeyBinding] = &[
    // ---------------------------------------------------------- Global
    KeyBinding {
        context: Context::Global,
        keys: "Shift+1",
        desc: "vista Now Playing (la canción que suena)",
    },
    KeyBinding {
        context: Context::Global,
        keys: "Shift+2",
        desc: "vista Related (relacionadas, letras, visual)",
    },
    KeyBinding {
        context: Context::Global,
        keys: "Shift+3",
        desc: "vista Búsqueda",
    },
    KeyBinding {
        context: Context::Global,
        keys: "Shift+4",
        desc: "vista Fuentes (YouTube, LRCLIB…)",
    },
    KeyBinding {
        context: Context::Global,
        keys: "Shift+5",
        desc: "vista Metadatos",
    },
    KeyBinding {
        context: Context::Global,
        keys: "Shift+6",
        desc: "vista Historial",
    },
    KeyBinding {
        context: Context::Global,
        keys: "Shift+7",
        desc: "vista Ajustes",
    },
    KeyBinding {
        context: Context::Global,
        keys: "Shift+8",
        desc: "vista Playlists",
    },
    KeyBinding {
        context: Context::Global,
        keys: "Esc",
        desc: "volver a Now Playing desde casi cualquier vista",
    },
    KeyBinding {
        context: Context::Global,
        keys: "q",
        desc: "salir de Tunefold",
    },
    KeyBinding {
        context: Context::Global,
        keys: "Ctrl+C",
        desc: "salida forzosa inmediata",
    },
    KeyBinding {
        context: Context::Global,
        keys: "Shift+H",
        desc: "abrir esta ayuda · cualquier tecla la cierra",
    },
    // ----------------------------------------------------- Reproducción
    KeyBinding {
        context: Context::Playback,
        keys: "Espacio",
        desc: "pausar / reanudar la canción",
    },
    KeyBinding {
        context: Context::Playback,
        keys: "Shift+D",
        desc: "siguiente canción de la cola",
    },
    KeyBinding {
        context: Context::Playback,
        keys: "Shift+A",
        desc: "canción anterior",
    },
    KeyBinding {
        context: Context::Playback,
        keys: "← / →",
        desc: "saltar -10s / +10s",
    },
    KeyBinding {
        context: Context::Playback,
        keys: "l",
        desc: "L1K3D: marcar / dejar la canción que suena",
    },
    // ------------------------------------------------------ Navegación
    KeyBinding {
        context: Context::Navigation,
        keys: "w / s · ↑ / ↓",
        desc: "mover la selección de la lista",
    },
    KeyBinding {
        context: Context::Navigation,
        keys: "Enter",
        desc: "abrir / reproducir el elemento seleccionado",
    },
    KeyBinding {
        context: Context::Navigation,
        keys: "ratón",
        desc: "clic selecciona la fila bajo el cursor",
    },
    // ------------------------------------------------------------ Cola
    KeyBinding {
        context: Context::Queue,
        keys: "a",
        desc: "autoplay on/off (rellena recomendaciones al terminar)",
    },
    KeyBinding {
        context: Context::Queue,
        keys: "f",
        desc: "shuffle on/off",
    },
    KeyBinding {
        context: Context::Queue,
        keys: "t",
        desc: "repetición: off / todas / la que suena",
    },
    KeyBinding {
        context: Context::Queue,
        keys: "r",
        desc: "recargar las recomendaciones de la canción",
    },
    KeyBinding {
        context: Context::Queue,
        keys: "R",
        desc: "renovar SOLO el autoplay con recomendaciones frescas",
    },
    // --------------------------------------------------------- Búsqueda
    KeyBinding {
        context: Context::Search,
        keys: "Enter",
        desc: "buscar · sobre un resultado, reproducirlo",
    },
    KeyBinding {
        context: Context::Search,
        keys: "↑ / ↓",
        desc: "navegar los resultados",
    },
    KeyBinding {
        context: Context::Search,
        keys: "← / →",
        desc: "mover el cursor del campo",
    },
    KeyBinding {
        context: Context::Search,
        keys: "Inicio / Fin",
        desc: "cursores al principio / al final",
    },
    KeyBinding {
        context: Context::Search,
        keys: "Retroceso / Supr",
        desc: "borrar carácter",
    },
    KeyBinding {
        context: Context::Search,
        keys: "Esc",
        desc: "volver a Now Playing (guardando la consulta)",
    },
    // -------------------------------------------------------- Playlists
    KeyBinding {
        context: Context::Playlists,
        keys: "n",
        desc: "crear playlist (escribe el nombre, Enter guarda)",
    },
    KeyBinding {
        context: Context::Playlists,
        keys: "Enter",
        desc: "abrir la playlist · en detalle, reproducir el track",
    },
    KeyBinding {
        context: Context::Playlists,
        keys: "p",
        desc: "reproducir la playlist como cola",
    },
    KeyBinding {
        context: Context::Playlists,
        keys: "d",
        desc: "borrar (solo playlists de usuario)",
    },
    KeyBinding {
        context: Context::Playlists,
        keys: "x",
        desc: "quitar el track de la playlist",
    },
    KeyBinding {
        context: Context::Playlists,
        keys: "u / Shift+D",
        desc: "mover el track arriba / abajo",
    },
    KeyBinding {
        context: Context::Playlists,
        keys: "Esc",
        desc: "volver al listado / Now Playing",
    },
    // --------------------------------------------------------- Fuentes
    KeyBinding {
        context: Context::Sources,
        keys: "Shift+4",
        desc: "ver las fuentes activas (YouTube, LRCLIB…)",
    },
    // ------------------------------------------------------- Metadatos
    KeyBinding {
        context: Context::Metadata,
        keys: "Shift+5",
        desc: "ver los metadatos de la canción",
    },
    KeyBinding {
        context: Context::Metadata,
        keys: "l",
        desc: "L1K3D de la canción",
    },
    // --------------------------------------------------------- Ajustes
    KeyBinding {
        context: Context::Settings,
        keys: "Enter",
        desc: "guardar los cambios en el archivo de configuración",
    },
    KeyBinding {
        context: Context::Settings,
        keys: "↑ / ↓ o Tab",
        desc: "cambiar de campo",
    },
    KeyBinding {
        context: Context::Settings,
        keys: "Retroceso",
        desc: "borrar carácter",
    },
    KeyBinding {
        context: Context::Settings,
        keys: "Esc",
        desc: "volver sin guardar",
    },
    // ----------------------------------------------------------- Otros
    KeyBinding {
        context: Context::Related,
        keys: "v",
        desc: "banda superior de la vista 2: auto / letras / visualizador",
    },
];

/// Filas planas de la ayuda (una por binding) en el orden de `BINDINGS`.
fn help_lines() -> Vec<(Context, &'static str, &'static str)> {
    BINDINGS
        .iter()
        .map(|b| (b.context, b.keys, b.desc))
        .collect()
}

/// Dimensiones del popup: casi toda la pantalla, nunca más ancho que 92.
fn popup_dimensions(area: Rect) -> (u16, u16) {
    let w = area.width.saturating_sub(2).min(92);
    let h = area.height.saturating_sub(2);
    (w.max(20), h.max(8))
}

/// Dibuja el popup de ayuda centrado sobre el contenido.
///
/// `scroll` es el desplazamiento por saltos de página (en filas renderizadas);
/// los perfiles estrechos lo activan automáticamente, los anchos caben sin él.
/// Devuelve el scroll efectivo (ya recortado) y `true` si alguien debería
/// saber que el contenido se desborda (para pintar la pista de teclas).
pub fn render_help(
    frame: &mut Frame,
    area: Rect,
    scroll: usize,
    profile: TerminalProfile,
    glyphs: UiGlyphs,
) -> (usize, bool) {
    let (w, h) = popup_dimensions(area);
    let left = (area.width - w) / 2;
    let top = (area.height - h) / 2;
    let popup = Rect::new(area.x + left, area.y + top, w, h);

    frame.render_widget(Clear, popup);

    let inner = popup.inner(Margin::new(1, 1));
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            " Ayuda · teclado ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(
            format!(
                "{} ↑/↓ · w/s · k/j desplazar · cualquier otra tecla cierra",
                glyphs.status()
            ),
            Style::default().fg(Color::DarkGray),
        ));
    let inner = block.inner(inner);

    // Contenido en columnas, como un periódico: `cols` columnas balanceadas en
    // filas (`rows`). En anchos pequeños se colapsa a una sola columna.
    let all = help_lines();
    let (cols, rows) = match profile {
        TerminalProfile::Large => (3, all.len().div_ceil(3) as u16),
        TerminalProfile::Medium => (2, all.len().div_ceil(2) as u16),
        _ => (1, inner.height),
    };
    // Las columnas pueden ser más altas de lo que la ventana muestra (perfil
    // bajo) → ahí entra el scroll; nunca se deja una columna más alta que la
    // propia ventana para que el recorte vertical sea predecible.
    let rows = rows.clamp(3, inner.height.max(1));
    let lines_in_page = rows * cols;

    // Si hay más filas que página, activamos scroll y recortamos a la página.
    let (start, scrollable) = if all.len() > lines_in_page as usize {
        let max = all.len().saturating_sub(lines_in_page as usize);
        (scroll.min(max), true)
    } else {
        (0, false)
    };

    // Construir las líneas visibles en su columna.
    let mut cols_rects = vec![inner];
    if cols > 1 {
        cols_rects = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Fill(1); cols as usize])
            .split(inner)
            .to_vec();
    }

    let mut idx = start;
    for rect in &cols_rects {
        let mut lines: Vec<Line> = Vec::new();
        let mut in_page = 0usize;
        while in_page < rows as usize && idx < all.len() {
            let (ctx, keys, desc) = all[idx];
            lines.push(Line::from(vec![
                Span::styled(
                    ctx.label(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    keys,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(desc, Style::default().fg(Color::Gray)),
            ]));
            in_page += 1;
            idx += 1;
        }
        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, *rect);
    }

    frame.render_widget(block, popup);
    (start, scrollable)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La tabla de ayuda cubre TODO tecla que el render maneja en
    /// `app.rs::on_key`. Si alguien añade un atajo sin documentarlo, falla.
    #[test]
    fn help_covers_every_handled_key() {
        let keys: String = BINDINGS
            .iter()
            .flat_map(|b| b.keys.chars().collect::<Vec<_>>())
            .collect();
        for need in [
            "Shift+1",
            "Shift+2",
            "Shift+3",
            "Shift+4",
            "Shift+5",
            "Shift+6",
            "Shift+8",
            "Esc",
            "q",
            "Ctrl+C",
            "Shift+H",
            "Espacio",
            "Shift+D",
            "Shift+A",
            "←",
            "→",
            "l",
            "w",
            "s",
            "↑",
            "↓",
            "Enter",
            "a",
            "v",
            "f",
            "t",
            "r",
            "R",
            "n",
            "p",
            "d",
            "x",
            "u",
            "Tab",
            "Retroceso",
            "Inicio",
            "Fin",
            "Supr",
        ] {
            assert!(
                keys.contains(need),
                "atajo «{need}» sin documentar en BINDINGS"
            );
        }
    }

    #[test]
    fn every_context_is_labeled() {
        for ctx in [
            Context::Global,
            Context::Playback,
            Context::Navigation,
            Context::Queue,
            Context::Search,
            Context::Playlists,
            Context::Sources,
            Context::Metadata,
            Context::Settings,
            Context::Related,
            Context::Other,
        ] {
            assert!(!ctx.label().is_empty(), "contexto sin etiqueta: {ctx:?}");
        }
    }

    #[test]
    fn descriptions_never_rely_on_glyphs() {
        // Ninguna descripción depende de un símbolo Unicode: siempre la palabra
        // primero. La columna de teclas admite las flechas direccionales
        // universales (↑↓←→), presentes en cualquier terminal ámbito.
        for b in BINDINGS {
            assert!(
                b.keys
                    .chars()
                    .all(|c| c.is_ascii() || matches!(c, '↑' | '↓' | '←' | '→' | '·' | 'ó')),
                "teclas con glifos poco presentes: {b:?}"
            );
            assert!(
                !b.desc.contains('♥') && !b.desc.contains("▶"),
                "descripción con glifo funcional: {b:?}"
            );
        }
    }
}
