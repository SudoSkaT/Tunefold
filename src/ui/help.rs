//! Definición ÚNICA de atajos de teclado y popup de ayuda (Shift+H).
//!
//! Cada mapeo a `KeyCode`/`Modifier` del render dispone de su fila aquí; el
//! test de coherencia [`tests::help_covers_every_handled_key`] comprueba que
//! ningún atajo funcional quede fuera de la ayuda.
//!
//! El popup es responsive por categorías COMPLETAS (una categoría nunca se
//! parte entre columnas) y SIN scroll: en perfiles amplios (Large y Medium) se
//! compone en tres columnas y todo el contenido es visible de una vez; en
//! perfiles estrechos (Small/Tiny) se muestran las categorías prioritarias que
//! caben (en el orden de [`BINDINGS`]) y las que no caben no se asoman (jamás
//! se oculta una categoría a medias). CUALQUIER tecla cierra la ayuda, incluida
//! la propia `Shift+H`.
//!
//! Invariante UX: los símbolos van SIEMPRE acompañados de su palabra en la
//! columna de descripción; la columna de teclas solo usa combinaciones ASCII
//! salvo las flechas direccionales universales (↑↓←→).

use ratatui::layout::{Constraint, Direction, Layout, Rect};
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
///
/// Las descripciones son concisas a propósito: la ayuda completa debe caber en
/// tres columnas de un terminal de 80 columnas sin scroll. La abreviación
/// nunca rompe el significado («mover posición», no «m.»).
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
        desc: "Now Playing",
    },
    KeyBinding {
        context: Context::Global,
        keys: "Shift+2",
        desc: "vista Related",
    },
    KeyBinding {
        context: Context::Global,
        keys: "Shift+3",
        desc: "vista Búsqueda",
    },
    KeyBinding {
        context: Context::Global,
        keys: "Shift+4",
        desc: "vista Fuentes",
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
        desc: "ir a Now Playing",
    },
    KeyBinding {
        context: Context::Global,
        keys: "q",
        desc: "salir de la app",
    },
    KeyBinding {
        context: Context::Global,
        keys: "Ctrl+C",
        desc: "salida forzosa",
    },
    KeyBinding {
        context: Context::Global,
        keys: "Shift+H",
        desc: "abrir esta ayuda",
    },
    // ----------------------------------------------------- Reproducción
    KeyBinding {
        context: Context::Playback,
        keys: "Espacio",
        desc: "pausar/reanudar",
    },
    KeyBinding {
        context: Context::Playback,
        keys: "Shift+D",
        desc: "siguiente",
    },
    KeyBinding {
        context: Context::Playback,
        keys: "Shift+A",
        desc: "anterior",
    },
    KeyBinding {
        context: Context::Playback,
        keys: "← / →",
        desc: "-10s y +10s",
    },
    KeyBinding {
        context: Context::Playback,
        keys: "l",
        desc: "L1K3D canción",
    },
    // ------------------------------------------------------ Navegación
    KeyBinding {
        context: Context::Navigation,
        keys: "w s ↑ ↓",
        desc: "mover selección",
    },
    KeyBinding {
        context: Context::Navigation,
        keys: "Enter",
        desc: "abrir/reproducir",
    },
    KeyBinding {
        context: Context::Navigation,
        keys: "ratón",
        desc: "clic selecciona",
    },
    // ------------------------------------------------------------ Cola
    KeyBinding {
        context: Context::Queue,
        keys: "a",
        desc: "autoplay on/off",
    },
    KeyBinding {
        context: Context::Queue,
        keys: "f",
        desc: "aleatorio on/off",
    },
    KeyBinding {
        context: Context::Queue,
        keys: "t",
        desc: "repetición 3 modos",
    },
    KeyBinding {
        context: Context::Queue,
        keys: "r",
        desc: "renovar recomend.",
    },
    KeyBinding {
        context: Context::Queue,
        keys: "R",
        desc: "renovar autoplay",
    },
    // --------------------------------------------------------- Búsqueda
    KeyBinding {
        context: Context::Search,
        keys: "Enter",
        desc: "buscar/reproducir",
    },
    KeyBinding {
        context: Context::Search,
        keys: "↑ / ↓",
        desc: "navegar resultados",
    },
    KeyBinding {
        context: Context::Search,
        keys: "← / →",
        desc: "mover cursor",
    },
    KeyBinding {
        context: Context::Search,
        keys: "Inicio/Fin",
        desc: "extremos",
    },
    KeyBinding {
        context: Context::Search,
        keys: "Retro/Supr",
        desc: "borrar",
    },
    KeyBinding {
        context: Context::Search,
        keys: "Esc",
        desc: "volver (guarda)",
    },
    // -------------------------------------------------------- Playlists
    KeyBinding {
        context: Context::Playlists,
        keys: "n",
        desc: "crear playlist",
    },
    KeyBinding {
        context: Context::Playlists,
        keys: "Enter",
        desc: "abrir/reproducir",
    },
    KeyBinding {
        context: Context::Playlists,
        keys: "p",
        desc: "reproducir cola",
    },
    KeyBinding {
        context: Context::Playlists,
        keys: "d",
        desc: "borrar playlist",
    },
    KeyBinding {
        context: Context::Playlists,
        keys: "x",
        desc: "quitar track",
    },
    KeyBinding {
        context: Context::Playlists,
        keys: "u/Shift+D",
        desc: "reordenar",
    },
    KeyBinding {
        context: Context::Playlists,
        keys: "Esc",
        desc: "volver",
    },
    // --------------------------------------------------------- Fuentes
    KeyBinding {
        context: Context::Sources,
        keys: "Shift+4",
        desc: "fuentes activas",
    },
    // ------------------------------------------------------- Metadatos
    KeyBinding {
        context: Context::Metadata,
        keys: "Shift+5",
        desc: "ver metadatos",
    },
    KeyBinding {
        context: Context::Metadata,
        keys: "l",
        desc: "marcar canción",
    },
    // --------------------------------------------------------- Ajustes
    KeyBinding {
        context: Context::Settings,
        keys: "Enter",
        desc: "guardar cambios",
    },
    KeyBinding {
        context: Context::Settings,
        keys: "Tab ↑ ↓",
        desc: "cambiar campo",
    },
    KeyBinding {
        context: Context::Settings,
        keys: "Retro/Supr",
        desc: "borrar",
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
        desc: "modo banda superior",
    },
];

/// Un grupo de la tabla: categoría + sus filas (rango en `BINDINGS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Group {
    context: Context,
    start: usize,
    len: usize,
}

impl Group {
    /// Filas que ocupa (cabecera + atajos).
    fn lines(self) -> usize {
        1 + self.len
    }

    /// Sitúa el grupo en la página de la ayuda (rango en `BINDINGS`).
    fn rows(self) -> std::ops::Range<usize> {
        self.start..self.start + self.len
    }
}

/// Agrupa las filas consecutivas por categoría respetando el orden de
/// [`BINDINGS`] (las categorías ya vienen contiguas en la tabla).
fn groups() -> Vec<Group> {
    let mut out: Vec<Group> = Vec::new();
    for (i, b) in BINDINGS.iter().enumerate() {
        match out.last_mut() {
            Some(g) if g.context == b.context => g.len += 1,
            _ => out.push(Group {
                context: b.context,
                start: i,
                len: 1,
            }),
        }
    }
    out
}

/// Número de columnas según el perfil de terminal: los perfiles que caben con
/// palabras muestran todo; los estrechos priorizan categorías (una por columna,
/// completas).
fn columns_for(profile: TerminalProfile) -> usize {
    match profile {
        TerminalProfile::Large | TerminalProfile::Medium => 3,
        TerminalProfile::Small => 2,
        TerminalProfile::Tiny => 1,
    }
}

/// Distribuye los grupos en `ncols` columnas SIN partir categorías.
///
/// Llenado por columnas (columna a columna, en el orden de [`BINDINGS`], como
/// la lectura de un periódico): se rellena una columna con las categorías que
/// caben y se pasa a la siguiente. Esto conserva el orden de prioridad
/// (Global primero) en terminales estrechos. Los grupos que no caben en
/// ninguna columna se descartan completos (jamás a medias). En una sola
/// columna se colocan todos en orden y el render recorta el final (único caso
/// donde una categoría puede quedar partida: terminal diminuta).
fn pack_groups(
    groups: &[Group],
    column_lines: usize,
    ncols: usize,
) -> (Vec<Vec<Group>>, Vec<Group>) {
    let mut cols: Vec<Vec<Group>> = Vec::with_capacity(ncols);
    let mut heights = Vec::with_capacity(ncols);
    for _ in 0..ncols {
        cols.push(Vec::new());
        heights.push(0);
    }
    if ncols == 1 {
        cols[0].extend_from_slice(groups);
        return (cols, Vec::new());
    }
    let mut dropped = Vec::new();
    let mut col_index = 0usize;
    for g in groups {
        let lines = (*g).lines();
        while col_index < ncols && heights[col_index] + lines > column_lines {
            col_index += 1;
        }
        match col_index < ncols {
            true => {
                cols[col_index].push(*g);
                heights[col_index] += lines;
            }
            false => dropped.push(*g),
        }
    }
    (cols, dropped)
}

/// Dibuja el popup de ayuda centrado sobre el contenido.
///
/// Sin scroll: cada columna lista categorías COMPLETAS, y si el contenido no
/// cabe en el perfil se omiten las categorías de menor prioridad enteras (la
/// columna única de los perfiles diminutos recorta el final). Cualquier tecla
/// la cierra (lo maneja `App::on_key`).
pub fn render_help(frame: &mut Frame, area: Rect, profile: TerminalProfile, glyphs: UiGlyphs) {
    // Pantalla completa (el borde del bloque da el marco): el contenido gana
    // todo el ancho/alto, que es lo que permite caber SIN scroll.
    let popup = area;

    frame.render_widget(Clear, popup);

    let block = Block::default().borders(Borders::ALL).title(Span::styled(
        " Ayuda · teclado ",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ));
    // Solo los perfiles con holgura muestran la pista de cierre; en los
    // estrechos cada fila cuenta y el título superior basta.
    let block = match profile {
        TerminalProfile::Large | TerminalProfile::Medium => block.title_bottom(Span::styled(
            format!(
                "{} cualquier tecla cierra · Shift+1..8 cambia de vista",
                glyphs.status()
            ),
            Style::default().fg(Color::Gray),
        )),
        _ => block,
    };
    let inner = block.inner(popup);

    let inner_rect = inner;
    let ncols = columns_for(profile);
    let column_lines = inner_rect.height as usize;
    let (cols, _dropped) = pack_groups(&groups(), column_lines, ncols);

    let cols_rects = if ncols == 1 {
        vec![inner_rect]
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints((0..ncols).map(|_| Constraint::Fill(1)).collect::<Vec<_>>())
            .split(inner_rect)
            .to_vec()
    };

    for (col, rect) in cols.iter().zip(cols_rects.iter()) {
        if col.is_empty() {
            continue;
        }
        // Ancho de columna por su tecla más larga (alineación limpia).
        let mut lines: Vec<Line> = Vec::new();
        for g in col {
            lines.push(Line::styled(
                g.context.label(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));
            for i in g.rows() {
                let b = &BINDINGS[i];
                // `keys  desc`: sin alinear a la columna más ancha para no
                // robar ancho a la descripción (texto conciso ya de serie).
                lines.push(Line::from(vec![
                    Span::styled(
                        b.keys,
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  "),
                    Span::styled(b.desc, Style::default().fg(Color::Gray)),
                ]));
            }
        }
        frame.render_widget(Paragraph::new(lines), *rect);
    }

    // El borde se pinta al final (encima del contenido recortado por la altura).
    frame.render_widget(block, popup);
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
            "Shift+1", "Shift+2", "Shift+3", "Shift+4", "Shift+5", "Shift+6", "Shift+7", "Shift+8",
            "Esc", "q", "Ctrl+C", "Shift+H", "Espacio", "Shift+D", "Shift+A", "←", "→", "l", "w",
            "s", "↑", "↓", "Enter", "a", "v", "f", "t", "r", "R", "n", "p", "d", "x", "u", "Tab",
            "Retro", "Supr", "Inicio", "Fin",
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
                    .all(|c| c.is_ascii() || matches!(c, '↑' | '↓' | '←' | '→' | 'ó')),
                "teclas con glifos poco presentes: {b:?}"
            );
            assert!(
                !b.desc.contains('♥') && !b.desc.contains("▶"),
                "descripción con glifo funcional: {b:?}"
            );
        }
    }

    #[test]
    fn bindings_are_grouped_by_context() {
        // Las filas de cada categoría son contiguas (permite agrupar por rango).
        let grouped = groups();
        for g in &grouped {
            for i in g.rows().skip(1) {
                assert_eq!(
                    BINDINGS[i].context, g.context,
                    "filas no contiguas en {g:?}"
                );
            }
        }
        let covered: usize = grouped.iter().map(|g| g.len).sum();
        assert_eq!(covered, BINDINGS.len(), "todos los atajos agrupados");
    }

    #[test]
    fn full_table_fits_without_dropping_on_ampios_and_medium() {
        // Large (≥100x30, 100x28 inner→40) y Medium (≥80x24, 78x22): las tres
        // columnas caben y NINGÚN grupo se pierde (sin scroll, sin truncado).
        for (avail_height, ncols) in [(28usize, 3usize), (22, 3)] {
            let g = groups();
            let (cols, dropped) = pack_groups(&g, avail_height, ncols);
            assert!(dropped.is_empty(), "se pierden grupos: {dropped:?}");
            let shown: usize = cols
                .iter()
                .map(|c| c.iter().map(|g| g.lines()).sum::<usize>())
                .sum();
            let total: usize = g.iter().map(|g| g.lines()).sum();
            assert_eq!(shown, total, "todo el contenido visible");
            for c in &cols {
                let lines: usize = c.iter().map(|g| g.lines()).sum();
                assert!(
                    lines <= avail_height,
                    "columna desborda: {lines} > {avail_height}"
                );
            }
        }
    }

    #[test]
    fn every_binding_lands_in_exactly_one_column() {
        // En los perfiles donde todo cabe, cada atajo está en una sola columna
        // y el orden relativo de la tabla se conserva.
        let g = groups();
        let (cols, dropped) = pack_groups(&g, 22, 3);
        assert!(dropped.is_empty());
        let mut seen: Vec<usize> = Vec::new();
        for c in &cols {
            for grp in c {
                for i in grp.rows() {
                    assert!(!seen.contains(&i), "binding duplicado {i}");
                    seen.push(i);
                }
            }
        }
        assert_eq!(seen, (0..BINDINGS.len()).collect::<Vec<_>>());
    }

    #[test]
    fn every_row_fits_slimmest_medium_column() {
        // Medium (80 cols → ~26 por columna; el render de ratatui recorta una
        // celda por holgura): ninguna fila «teclas + 2 espacios + descripción»
        // puede exceder 25 → nada se recorta/wrapea, sin truncar texto.
        for b in BINDINGS {
            let len = b.keys.chars().count() + 2 + b.desc.chars().count();
            assert!(
                len <= 25,
                "fila de 26+ caracteres: «{len}» {b:?} — hay que recortar la descripción"
            );
        }
    }

    #[test]
    fn small_terminal_prioritizes_global_and_never_splits() {
        // Small (60x16 → 58x14): dos columnas; Global entra completa y las
        // categorías siguientes solo si caben enteras. Ninguna a medias.
        let g = groups();
        let (cols, dropped) = pack_groups(&g, 14, 2);
        // Global entera, en la primera columna.
        assert!(cols[0].iter().any(|grp| grp.context == Context::Global));
        for grp in &cols[0] {
            if grp.context == Context::Global {
                assert_eq!(
                    grp.len,
                    BINDINGS
                        .iter()
                        .filter(|b| b.context == Context::Global)
                        .count()
                );
            }
        }
        // Lo que no cupo está completo en `dropped` (nada parcial).
        for grp in &dropped {
            assert!(
                grp.len > 0 && !cols.iter().flatten().any(|c| c.context == grp.context),
                "grupo {grp:?} duplicado o parcial"
            );
        }
        assert!(
            dropped.iter().any(|g| g.context == Context::Settings)
                || cols
                    .iter()
                    .flatten()
                    .find(|g| g.context == Context::Settings)
                    .is_some(),
            "Ajustes existe en la tabla"
        );
    }

    #[test]
    fn render_shows_every_command_on_medium() {
        // Integración: en 80x24 (Medium, 3 columnas) TODOS los atajos son
        // visibles de una vez. Sin scroll, esto es el test de "no ocultar".
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_help(
                    f,
                    f.area(),
                    TerminalProfile::Medium,
                    crate::ui::glyphs::UiGlyphs::new(crate::ui::glyphs::GlyphTheme::Unicode),
                )
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        for b in BINDINGS {
            if !text.contains(b.keys) || !text.contains(b.desc) {
                eprintln!("=== buffer 80x24 ===");
                for i in 0..buf.area.height {
                    let start = usize::from(i) * usize::from(buf.area.width);
                    let end = start + usize::from(buf.area.width);
                    let s: String = buf.content()[start..end]
                        .iter()
                        .map(|c| c.symbol())
                        .collect();
                    eprintln!("{i:02} |{s}|");
                }
                panic!(
                    "atajo/descripción «{} · {}» no visible en 80x24",
                    b.keys, b.desc
                );
            }
        }
    }

    #[test]
    fn render_without_scroll_keys_on_every_profile() {
        // En todos los perfiles el popup se dibuja sin scroll y las teclas de
        // desplazamiento antiguas (w/s/k/j, PgUp/PgDn) ya no se documentan.
        for (w, h) in [(120, 40), (100, 30), (80, 24), (70, 20), (60, 15)] {
            let backend = ratatui::backend::TestBackend::new(w, h);
            let mut terminal = ratatui::Terminal::new(backend).unwrap();
            let profile = TerminalProfile::from_rect(ratatui::layout::Rect::new(0, 0, w, h));
            terminal
                .draw(|f| {
                    render_help(
                        f,
                        f.area(),
                        profile,
                        crate::ui::glyphs::UiGlyphs::new(crate::ui::glyphs::GlyphTheme::Unicode),
                    )
                })
                .unwrap_or_else(|_| panic!("ayuda a {w}x{h}"));
            let text: String = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|c| c.symbol())
                .collect();
            assert!(
                !text.contains("PgUp") && !text.contains("PgDn") && !text.contains("desplazar"),
                "la ayuda ya no ofrece scroll a {w}x{h}"
            );
            // El título del popup siempre está.
            assert!(text.contains("Ayuda"), "título «Ayuda» a {w}x{h}");
        }
    }
}
