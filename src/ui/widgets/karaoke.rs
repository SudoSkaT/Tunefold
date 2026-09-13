//! Letras en modo karaoke: *scroll continuo en cascada* sobre un **buffer
//! circular**.
//!
//! En vez de reconstruir todas las líneas de la canción en cada frame (O(n)),
//! este widget mantiene una ventana acotada de índices — un anillo (`VecDeque`)
//! sobre las líneas ordenadas del LRC — y la actualiza **por deslizamiento**:
//! cada avance de la línea activa descarta la primera del anillo y encola la
//! siguiente, en O(1) amortizado y con memoria proporcional a las filas
//! visibles (`altura + 1`), no al tamaño de la letra.
//!
//! La línea activa se mantiene alrededor de los dos tercios del panel: conserva
//! contexto de lo ya cantado y anticipa las líneas siguientes. Al cambiar de
//! línea, el bloque se desplaza **exactamente una fila**; nunca se reconstruye
//! ni se re-centra por tick.

use std::collections::VecDeque;

use ratatui::layout::{Alignment, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::domain::lyrics::SyncLyrics;

/// Ventana deslizante del karaoke.
///
/// Guarda únicamente **índices** sobre [`SyncLyrics::lines`]: el texto vive en
/// la letra ya parseada y este tipo solo reserva memoria por el ancho de la
/// ventana. Con `height` filas visibles el anillo guarda a lo sumo `height + 1`
/// líneas, con la extra encolada por detrás esperando a entrar cuando se
/// deslice.
#[derive(Debug, Default)]
pub struct KaraokeScroller {
    /// Ring de índices actualmente "en ventana"; primero = `start`.
    window: VecDeque<usize>,
    /// Filas internas del panel (sin bordes) del último [`Self::advance`].
    height: usize,
}

impl KaraokeScroller {
    /// Índice de la línea que arranca la ventana.
    pub fn start(&self) -> usize {
        self.window.front().copied().unwrap_or(0)
    }

    /// Líneas encoladas (a lo sumo `altura + 1`).
    pub fn len(&self) -> usize {
        self.window.len()
    }

    /// ¿Ventana vacía? (antes de que haya letra, o al acabarla).
    pub fn is_empty(&self) -> bool {
        self.window.is_empty()
    }

    /// Vacía la ventana (al cambiar de canción o al acabar la letra).
    pub fn reset(&mut self) {
        self.window.clear();
    }

    /// Desliza la ventana a la posición actual de la letra.
    ///
    /// - `active`: índice de la línea en curso (la que toca cantar). Se ancla
    ///   a dos tercios del panel, dejando contexto por delante.
    /// - `finished`: la letra ya no tiene nada que cantar → se limpia.
    /// - `active == None` sin haber acabado (todavía no empezó la canción) →
    ///   se muestra desde el principio, toda como "no leída".
    pub fn advance(
        &mut self,
        lyrics: &SyncLyrics,
        active: Option<usize>,
        finished: bool,
        height: usize,
    ) {
        self.height = height;
        // Si el terminal encogió, el anillo se recorta antes de deslizar.
        self.window.truncate(self.capacity(lyrics));
        if finished {
            self.window.clear();
            return;
        }
        let Some(active) = active else {
            self.fill_from(lyrics, 0);
            return;
        };
        let anchor = karaoke_anchor(height);
        let start = active.saturating_sub(anchor);
        self.slide_to(lyrics, start);
    }

    /// Índice (interno) de la fila donde se ancla la línea activa.
    fn capacity(&self, lyrics: &SyncLyrics) -> usize {
        self.height.saturating_add(1).min(lyrics.lines.len())
    }

    /// Desliza el ring para dejar `start` en el frente, rellenando por detrás.
    ///
    /// El caso normal (avance) descarta las primeras líneas con un único
    /// `drain` y encola las siguientes; el retroceso (seek atrás) entra por
    /// delante. En ambos casos el contenido se mantiene contiguo y acotado.
    fn slide_to(&mut self, lyrics: &SyncLyrics, start: usize) {
        let cap = self.capacity(lyrics);
        match self.window.front().copied() {
            None => self.fill_from(lyrics, start),
            Some(base) if base == start => self.top_up(lyrics, cap, start),
            Some(base) if base < start => {
                // Avance (sentido normal): actualización por deslizamiento.
                let drop = (start - base).min(self.window.len());
                self.window.drain(..drop);
                self.top_up(lyrics, cap, start);
            }
            Some(_) => {
                // Retroceso (seek atrás): entra por delante, sobra por detrás.
                self.window.truncate(cap);
                let end = self.window.front().copied().unwrap_or(start + 1);
                for i in (start..end).rev() {
                    self.window.push_front(i);
                }
                while self.window.len() > cap {
                    self.window.pop_back();
                }
            }
        }
    }

    /// Encola por detrás índices contiguos hasta completar `cap`, sin rebasar
    /// el final de la letra. Arranca desde `start` si el anillo quedó vacío
    /// (p. ej. tras descartar de golpe más líneas de las que había).
    fn top_up(&mut self, lyrics: &SyncLyrics, cap: usize, start: usize) {
        let n = lyrics.lines.len();
        let mut next = self.window.back().copied().map_or(start, |i| i + 1);
        while self.window.len() < cap && next < n {
            self.window.push_back(next);
            next += 1;
        }
    }

    /// Reconstruye la ventana completa desde `start` (cambio de letra o antes
    /// de la primera línea). El "rebuild" solo ocurre en esos casos, no en
    /// cada tick de reproducción.
    fn fill_from(&mut self, lyrics: &SyncLyrics, start: usize) {
        let n = lyrics.lines.len();
        let cap = self.capacity(lyrics);
        self.window.clear();
        for i in start..(start + cap).min(n) {
            self.window.push_back(i);
        }
    }
}

/// Fila objetivo de la línea activa: dos tercios es estable y deja siempre
/// varias líneas próximas visibles en paneles suficientemente altos.
fn karaoke_anchor(height: usize) -> usize {
    height.saturating_sub(1).min(height.saturating_mul(2) / 3)
}

/// Renderiza la ventana del karaoke dentro del panel `area`.
///
/// La línea activa queda en una fila estable alrededor de dos tercios del
/// panel; solo se pintan `altura` líneas y cada avance desplaza una fila.
#[allow(clippy::too_many_arguments)] // filas, línea y colores son datos del render
pub fn render_karaoke(
    frame: &mut Frame,
    area: Rect,
    scroller: &mut KaraokeScroller,
    lyrics: &SyncLyrics,
    active: Option<usize>,
    finished: bool,
    colors: (Color, Color, Color),
) {
    let inner_h = area.height.saturating_sub(2) as usize;
    scroller.advance(lyrics, active, finished, inner_h);

    let (read_c, cur_c, unread_c) = colors;
    let lines: Vec<Line> = scroller
        .window
        .iter()
        .take(inner_h)
        .filter_map(|&i| lyrics.lines.get(i).map(|l| (i, l)))
        .map(|(i, l)| {
            let style = if Some(i) == active {
                Style::new().fg(cur_c).add_modifier(Modifier::BOLD)
            } else if active.is_some_and(|a| i < a) {
                Style::new().fg(read_c)
            } else {
                Style::new().fg(unread_c)
            };
            Line::styled(l.text.clone(), style)
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Letras · karaoke (LRCLIB) ");
    let text = Paragraph::new(lines)
        .block(block)
        .alignment(Alignment::Center);
    frame.render_widget(text, area);
}

/// Marco decorativo con título (evita repetir la definición de borde).
pub fn paint_frame(frame: &mut Frame, area: Rect, title: &str, title_color: Color) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(title_color))
        .title(Span::styled(
            title,
            Style::new().fg(title_color).add_modifier(Modifier::BOLD),
        ));
    frame.render_widget(block, area);
}

/// Overlay de karaoke que NO destruye el fondo: pinta solo el marco y el texto
/// de las líneas (fondo transparente), para que el fondo de osciloscopio siga
/// vivo detrás de las letras (spec §7/§15).
///
/// La línea activa queda en una fila estable alrededor de dos tercios del
/// panel; usa el mismo deslizamiento en cascada de [`KaraokeScroller`].
#[allow(clippy::too_many_arguments)] // filas, línea y colores son datos del render
pub fn render_over_scene(
    frame: &mut Frame,
    area: Rect,
    scroller: &mut KaraokeScroller,
    lyrics: &SyncLyrics,
    active: Option<usize>,
    finished: bool,
    colors: (Color, Color, Color),
) {
    let inner_h = area.height.saturating_sub(2) as usize;
    scroller.advance(lyrics, active, finished, inner_h);

    let (read_c, cur_c, unread_c) = colors;
    paint_frame(frame, area, " Letras · karaoke (LRCLIB) ", cur_c);

    let inner = area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    let inner_w = inner.width as usize;
    if inner_w == 0 || inner.height == 0 {
        return;
    }

    for (pos, &i) in scroller
        .window
        .iter()
        .take(inner.height as usize)
        .enumerate()
    {
        let Some(line) = lyrics.lines.get(i) else {
            break;
        };
        let style = if Some(i) == active {
            Style::new().fg(cur_c).add_modifier(Modifier::BOLD)
        } else if active.is_some_and(|a| i < a) {
            Style::new().fg(read_c)
        } else {
            Style::new().fg(unread_c)
        };
        // Escritura directa al buffer: el fondo de la celda NO se toca, así el
        // fondo de osciloscopio permanece detrás del texto.
        let text = &line.text;
        let width = text.chars().count();
        let pad = inner_w.saturating_sub(width) / 2;
        let y = inner.y + pos as u16;
        frame
            .buffer_mut()
            .set_stringn(inner.x + pad as u16, y, text, inner_w, style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn lyrics(n: usize) -> SyncLyrics {
        let mut s = String::new();
        for i in 0..n {
            s.push_str(&format!("[{:02}:{:02}] L{i}\n", i / 60, i % 60));
        }
        SyncLyrics::parse(&s)
    }

    fn indices(scroller: &KaraokeScroller) -> Vec<usize> {
        scroller.window.iter().copied().collect()
    }

    fn assert_contiguous(scroller: &KaraokeScroller) {
        let idx = indices(scroller);
        assert!(
            idx.windows(2).all(|p| p[1] == p[0] + 1),
            "ventana contigua: {idx:?}"
        );
    }

    #[test]
    fn starts_from_line_zero_before_first_line() {
        let mut s = KaraokeScroller::default();
        s.advance(&lyrics(50), None, false, 10);
        assert_eq!(s.start(), 0);
        assert!(indices(&s).contains(&0));
        assert!(s.len() <= 11, "ventana acotada a altura+1");
        assert_contiguous(&s);
    }

    #[test]
    fn anchors_active_at_bottom_once_filled() {
        let mut s = KaraokeScroller::default();
        s.advance(&lyrics(200), Some(30), false, 10);
        // start = active - anchor(6) = 24: quedan líneas futuras visibles.
        assert_eq!(s.start(), 24);
        assert!(indices(&s).contains(&30));
        assert_contiguous(&s);
    }

    #[test]
    fn progressive_advance_slides_one_row() {
        let mut s = KaraokeScroller::default();
        // Mientras caben antes del ancla, la activa desciende sin desplazar nada.
        s.advance(&lyrics(200), Some(1), false, 10);
        assert_eq!(s.start(), 0);
        s.advance(&lyrics(200), Some(6), false, 10);
        assert_eq!(s.start(), 0, "anchor a dos tercios -> 0..10");

        // A partir de ahí cada avance de línea desliza el bloque UNA fila.
        s.advance(&lyrics(200), Some(7), false, 10);
        assert_eq!(s.start(), 1, "primera cascada: desplaza 1");
        assert_eq!(s.len(), 11, "una línea extra encolada");

        // Varias líneas a la vez (seek/frame lento): desliza todas en una pasada.
        s.advance(&lyrics(200), Some(34), false, 10);
        assert_eq!(s.start(), 28);
        let idx = indices(&s);
        assert_eq!(idx.first(), Some(&28));
        assert_eq!(idx.last(), Some(&38));
        assert_contiguous(&s);
    }

    #[test]
    fn rewind_seek_returns_to_start_contiguously() {
        let mut s = KaraokeScroller::default();
        s.advance(&lyrics(200), Some(40), false, 10);
        assert_eq!(s.start(), 34);

        s.advance(&lyrics(200), Some(5), false, 10);
        assert_eq!(s.start(), 0, "seek atrás vuelve a anclar en el inicio");
        let idx = indices(&s);
        assert_eq!(idx.first(), Some(&0));
        assert_eq!(idx.last(), Some(&10));
        assert_contiguous(&s);
    }

    /// Traza posición → línea activa → ventana a lo largo de una reproducción
    /// con seek hacia delante y hacia atrás: la ventana se mantiene contigua,
    /// acotada y dentro de los límites de la letra (sin corrupción del anillo).
    #[test]
    fn position_to_index_to_window_trace_stays_contiguous() {
        // Líneas cada 10s hasta 130s.
        let mut s = String::new();
        for t in (0..=130).step_by(10) {
            s.push_str(&format!("[{:02}:{:02}] L{}\n", t / 60, t % 60, t / 10));
        }
        let sync = SyncLyrics::parse(&s);
        let mut sc = KaraokeScroller::default();
        // (posición, start esperado con altura 10 y ancla=6).
        for (pos, expected_start) in [(0u64, 0usize), (60, 0), (10, 0), (120, 6), (30, 0)] {
            let active = sync.active_index(Duration::from_secs(pos)).unwrap();
            sc.advance(&sync, Some(active), false, 10);
            assert_eq!(sc.start(), expected_start, "start en {pos}s");
            let idx = indices(&sc);
            assert!(
                idx.iter().all(|&i| i < sync.lines.len()),
                "índices dentro de los límites en {pos}s"
            );
            assert!(sc.len() <= 11, "ventana acotada a altura+1 en {pos}s");
            assert_contiguous(&sc);
        }
    }

    #[test]
    fn finished_clears_and_none_fills() {
        let mut s = KaraokeScroller::default();
        s.advance(&lyrics(50), Some(10), false, 10);
        assert_eq!(s.len(), 11);

        s.advance(&lyrics(50), Some(49), true, 10);
        assert!(s.window.is_empty(), "al acabar la letra se limpia");

        s.advance(&lyrics(50), None, false, 10);
        assert_eq!(s.start(), 0, "antes de empezar se muestra desde el inicio");
    }

    #[test]
    fn window_never_exceeds_lyrics_bounds() {
        // Canción corta (8 líneas) con panel alto: el anillo se recorta.
        let mut s = KaraokeScroller::default();
        s.advance(&lyrics(8), Some(7), false, 10);
        let idx = indices(&s);
        assert!(idx.iter().all(|&i| i < 8));
        assert_eq!(idx.first(), Some(&1));
        assert_contiguous(&s);

        // Hacia el final de una canción larga tampoco se pasa del límite.
        s.advance(&lyrics(20), Some(19), false, 10);
        let idx = indices(&s);
        assert_eq!(idx.last(), Some(&19));
        assert!(idx.iter().all(|&i| i < 20));
        assert_contiguous(&s);
    }

    #[test]
    fn zero_height_does_not_panic() {
        let mut s = KaraokeScroller::default();
        s.advance(&lyrics(5), Some(2), false, 0);
        s.advance(&lyrics(5), None, false, 0);
        s.advance(&lyrics(5), Some(2), true, 0);
        assert!(s.window.is_empty());
    }

    #[test]
    fn overlay_preserves_background_below_text() {
        // El karaoke superpuesto NO debe borrar el fondo (a diferencia del
        // Paragraph antiguo): el fondo de osciloscopio queda vivo detrás del texto.
        use ratatui::backend::TestBackend;
        use ratatui::layout::Margin;
        use ratatui::Terminal;

        let sync = SyncLyrics::parse("[00:05] una linea de prueba\n[00:10] dos\n");
        let backend = TestBackend::new(50, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = f.area().inner(Margin {
                    horizontal: 1,
                    vertical: 1,
                });
                // Un fondo "ambiental" artificial en toda la zona interior.
                f.buffer_mut()
                    .set_style(area, Style::new().bg(Color::Magenta));
                let mut scroller = KaraokeScroller::default();
                render_over_scene(
                    f,
                    area,
                    &mut scroller,
                    &sync,
                    Some(0),
                    false,
                    (Color::DarkGray, Color::White, Color::Yellow),
                );
            })
            .unwrap();
        let buf = terminal.backend().buffer();

        // El texto se escribió sobre el fondo… pero las celdas de suelo (marco
        // o zona sin glifos) conservan su Magenta.
        let text_root = buf
            .content()
            .iter()
            .find(|c| c.symbol() == "u")
            .map(|c| c.bg);
        assert!(
            matches!(text_root, Some(Color::Magenta)),
            "la celda del glifo conserva el fondo ambiental"
        );
        assert!(
            buf.content()
                .iter()
                .any(|c| c.symbol() == " " && c.bg == Color::Magenta),
            "las celdas vacías del interior siguen con el fondo (no borrado)"
        );
    }
}
