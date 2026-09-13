//! Renderer TUI de la escena visual: osciloscopio ESTÉREO de forma de onda +
//! barras.
//!
//! Responsabilidad EXCLUSIVA de renderizar (spec §25/§20): sin análisis, sin
//! HTTP, sin providers, sin relojes. Todo lo que pinta está en el estado que
//! recibe. La escena es la envolvente min/max del PCM POR CANAL (`WaveformView`)
//! decimada al ancho del área: por cada columna se vuelcan los buckets que le
//! tocan y se pintan PUNTOS en las filas del PICO (max) y del VALLE (min) de
//! cada canal (spec §8). Cada canal tiene su propio color (`ChannelColors`,
//! derivado de la portada) y su propio resplandor de fondo; todo el trazo se
//! dibuja con puntos `●` (ASCII `*`), y cuando L y R comparten celda se pinta el
//! tinte combinado de mezcla — ninguna de las dos curvas se pierde.
//!
//! El trazado es un plot de puntos (no una banda ni una línea interpolada):
//! pico y valle de la TRUE forma de onda quedan visibles columna a columna. El
//! camino caliente NO asigna memoria por draw (solo `[u16; 2]` en el stack por
//! columna); los glifos salen del sistema central [`UiGlyphs`] (Unicode `●` /
//! ASCII `*`). Un resplandor suave acompaña a cada curva de fondo. Las barras
//! EQ son el espectro discreto bajo el trazo. Toda la colorimetría sale de
//! [`VisualPalette`] — nunca se deriva aquí.

use ratatui::layout::{Margin, Position, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

use crate::analysis::{WaveformEnvelope, WAVEFORM_BUCKETS};
use crate::ui::glyphs::UiGlyphs;
use crate::visualization::engine::VisualState;
use crate::visualization::palette::VisualPalette;
use crate::visualization::VISUAL_BARS;

/// Escalera de una fila de barras de espectro (0 = vacío, 8 = lleno).
/// Fila de 1 celda: niveles 0..=7 sobre la versión corta ([`RAMP`]).
const RAMP: [&str; 8] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇"];

/// Escalera completa de una fila (incluye el bloque lleno): para barras de
/// 2 filas, cada fila cubre 0..=8 y la columna total 0..=16 niveles.
const RAMP_TALL: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];

/// Radio (en filas) del resplandor que acompaña a cada curva del osciloscopio:
/// la celda de la curva se tiñe al máximo y el halo decae linealmente hasta
/// cero a esta distancia. Ceñido (menos de una fila): el trazado se lee fino y
/// limpio, sin "engordar" la señal con un brillo difuso ancho.
const GLOW_RADIUS: f32 = 0.75;

fn to_color(c: [u8; 3]) -> Color {
    Color::Rgb(c[0], c[1], c[2])
}

/// Mezcla lineal de dos colores RGB (pura).
fn mix_c(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    [l(a[0], b[0]), l(a[1], b[1]), l(a[2], b[2])]
}

/// Oscurece un color por `k`.
fn shade(c: [u8; 3], k: f32) -> [u8; 3] {
    mix_c(c, [0, 0, 0], 1.0 - k.clamp(0.0, 1.0))
}

/// Color de la barra según la intensidad y la paleta fundida de la portada:
/// bajo → acento, medio → secundario, alto → dominante.
fn bar_color(intensity: f32, palette: &VisualPalette) -> Color {
    match (intensity * 4.0) as usize {
        0 => Color::DarkGray,
        1 => to_color(palette.accent),
        2 => to_color(palette.secondary),
        _ => to_color(palette.primary),
    }
}

/// Índice de los buckets que pinta una columna (nunca vacío, cubre el ancho).
///
/// - Anchos ≤ 128 buckets: varios buckets por columna (fold min/max).
/// - Anchos > 128: cada bucket ilumina al menos UNA columna (`hi = max(lo+1)`)
///   sin dejar huecos en el trazo.
fn column_bucket_range(w: usize, col: usize) -> (usize, usize) {
    let lo = col * WAVEFORM_BUCKETS / w;
    let hi = ((col + 1) * WAVEFORM_BUCKETS / w).max(lo + 1);
    (lo, hi)
}

/// Envolvente min/max de los buckets que tocan la columna, de UN canal.
fn channel_column_span(channel: &WaveformEnvelope, w: usize, col: usize) -> (f32, f32) {
    let (lo, hi) = column_bucket_range(w, col);
    let mut mn = f32::INFINITY;
    let mut mx = f32::NEG_INFINITY;
    for b in lo..hi {
        mn = mn.min(channel.min[b]);
        mx = mx.max(channel.max[b]);
    }
    (mn, mx)
}

/// Fila (0..h) que le corresponde a una amplitud `value` (con signo): el centro
/// de la celda más cercana a `center - value*scale` (redondeo determinista).
fn row_of(value: f32, center: f32, scale: f32, h: usize) -> u16 {
    let y = center - value.clamp(-1.0, 1.0) * scale;
    y.round().clamp(0.0, (h - 1) as f32) as u16
}

/// Filas donde debe pintarse UN canal en la columna: el VALLE (`min`) y el
/// PICO (`max`) de su envolvente, deduplicados si caen en la misma celda.
///
/// Devuelve como máximo 2 filas (sin allocs: `[u16; 2]` en el stack). Una
/// columna sin datos cae al centro (línea base continua).
fn channel_rows(
    channel: &WaveformEnvelope,
    w: usize,
    col: usize,
    center: f32,
    scale: f32,
    h: usize,
) -> ([u16; 2], usize) {
    let mut rows = [0u16; 2];
    let (mn, mx) = channel_column_span(channel, w, col);
    let mut n = 0usize;
    for value in [mn, mx] {
        let row = row_of(value, center, scale, h);
        if !rows[..n].contains(&row) {
            rows[n] = row;
            n += 1;
        }
    }
    if n == 0 {
        rows[0] = (h / 2) as u16;
        n = 1;
    }
    (rows, n)
}

/// Pinta el punto de trazo (símbolo y color; no toca el fondo, que lo dejó el
/// resplandor de [`render_backdrop`]).
fn paint_point(frame: &mut Frame, x: u16, y: u16, symbol: &str, color: [u8; 3]) {
    // SAFETY(ninguna): API pública de ratatui; celdas del área interior.
    if let Some(cell) = frame.buffer_mut().cell_mut(Position { x, y }) {
        cell.set_symbol(symbol);
        cell.set_style(Style::new().fg(to_color(color)));
    }
}

/// Pinta la capa ambiental (osciloscopio ESTÉREO) sobre TODO `area`.
///
/// Solo toca el fondo de cada celda (spec: la capa ambiental debe poder vivir
/// detrás de las letras). Tiñe un RESPLANDOR suave alrededor de las DOS curvas
/// (L y R, una por canal; decae a [`GLOW_RADIUS`] filas): el trazado queda fino
/// y el área, limpia. `subdued` aterriza la escena (reduce resplandor y
/// energía) para que el texto superior siga siendo legible.
pub fn render_backdrop(frame: &mut Frame, area: Rect, state: &VisualState, subdued: bool) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let scene = &state.scene;
    let palette = &scene.palette;
    let w = area.width as usize;
    let h = area.height as usize;

    let mut bg = palette.background;
    if subdued {
        bg = shade(bg, 0.45);
    }
    let active = scene.active && !subdued;
    let energy = if active { scene.energy } else { 0.0 };
    let brightness = if active { scene.brightness } else { 0.0 };
    let lit_k = if subdued {
        (0.25 + 0.65 * energy).clamp(0.0, 1.0) * 0.50
    } else {
        (0.25 + 0.65 * energy).clamp(0.0, 1.0)
    };
    let center = h as f32 * 0.5;
    let scale = (center * 0.92).max(1.0) * scene.waveform.gain;

    let channels = palette.channel_colors();
    let waveform = &scene.waveform;

    for col in 0..w {
        let (l_mn, l_mx) = channel_column_span(&waveform.left, w, col);
        let (r_mn, r_mx) = channel_column_span(&waveform.right, w, col);
        // Cada curva se tiñe con el color de su canal alrededor de su valor
        // medio de columna (la envolvente, no el plot de puntos).
        let l_y = center - (l_mn + l_mx) * 0.5 * scale;
        let r_y = center - (r_mn + r_mx) * 0.5 * scale;
        for row in 0..h {
            let x = area.x + col as u16;
            let y = area.y + row as u16;
            let dist_l = (row as f32 + 0.5 - l_y).abs();
            let dist_r = (row as f32 + 0.5 - r_y).abs();
            let fall_l = (1.0 - dist_l / GLOW_RADIUS).clamp(0.0, 1.0);
            let fall_r = (1.0 - dist_r / GLOW_RADIUS).clamp(0.0, 1.0);
            let mut color = bg;
            if fall_l > 0.0 {
                color = mix_c(color, channels.left, lit_k * fall_l);
            }
            if fall_r > 0.0 {
                color = mix_c(color, channels.right, lit_k * fall_r);
            }
            if brightness > 0.0 {
                color = mix_c(color, [255, 255, 255], brightness * 0.12);
            }
            // SAFETY(ninguna): API pública de ratatui; celdas dentro de `area`.
            if let Some(cell) = frame.buffer_mut().cell_mut(Position { x, y }) {
                cell.set_bg(to_color(color));
            }
        }
    }
}

/// Dibuja el visualizador completo: el trazo del osciloscopio (puntos por
/// canal) seguido de la franja de barras y el marco de estado. El resplandor
/// ambiental de fondo
/// se pinta aquí también, antes del trazo.
///
/// Con `state.active == false` pinta un marco apagado sobre la escena dormida
/// (línea base centrada): la vista nunca "desaparece" ni salta de layout.
pub fn render(frame: &mut Frame, area: Rect, state: &VisualState, position_secs: f32) {
    let pulse_dot = if state.pulse > 0.55 {
        "●"
    } else {
        if state.pulse > 0.2 {
            "◉"
        } else {
            "○"
        }
    };
    let title_color = if state.active {
        bar_color(state.intensity.max(0.15), &state.scene.palette)
    } else {
        Color::DarkGray
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            format!(" Visual {} ", pulse_dot),
            Style::new().fg(title_color),
        ))
        .title_bottom(Span::styled(
            format!(" fase {:.2} · pos {:.0}s ", state.phase, position_secs),
            Style::new().fg(Color::DarkGray),
        ));
    frame.render_widget(block, area);

    let inner = area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    // Franja de barras: las DOS filas inferiores del área interior (espectro
    // discreto bajo el trazo del osciloscopio). Dos filas dan escala y
    // presencia al EQ; en áreas diminutas se cae a una sola fila.
    let bars_h = if inner.height >= 5 { 2u16 } else { 1u16 };
    let trace_area = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(bars_h),
    );
    let bars_area = Rect::new(inner.x, inner.y + trace_area.height, inner.width, bars_h);

    render_backdrop(frame, trace_area, state, false);
    render_trace(frame, trace_area, state);
    render_bars(frame, bars_area, state);
}

/// Pinta la curva del osciloscopio: un PLOT DE PUNTOS por columna y canal.
///
/// Cada columna vuelca los buckets que le tocan y pinta hasta DOS puntos por
/// canal — el VALLE (`min`) y el PICO (`max`) de la columnna — en la fila más
/// cercana a `center - value*scale` (spec §8). Ambas curvas (L/R) se dibujan
/// con puntos `●` (ASCII `*`), distinguiéndose por el color de cada canal y el
/// resplandor ambiental de fondo; cuando caen en la MISMA celda se pinta el
/// punto con el tinte combinado de mezcla: ni pico, ni valle, ni canal se
/// pierden.
///
/// Sin allocations por draw: por cada columna solo se calculan 2 filas por
/// canal en el stack. No toca el fondo (el resplandor lo dejó `render_backdrop`);
/// los glifos salen del sistema [`UiGlyphs`] de la sesión.
fn render_trace(frame: &mut Frame, area: Rect, state: &VisualState) {
    render_trace_points(frame, area, state, *crate::ui::glyphs::GLYPHS);
}

/// Núcleo de [`render_trace`] con tema de glifos explícito (para tests).
fn render_trace_points(frame: &mut Frame, area: Rect, state: &VisualState, glyphs: UiGlyphs) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let scene = &state.scene;
    let palette = &scene.palette;
    let w = area.width as usize;
    let h = area.height as usize;
    let brightness = if scene.active { scene.brightness } else { 0.0 };

    let center = h as f32 * 0.5;
    let scale = (center * 0.92).max(1.0) * scene.waveform.gain;

    let channels = palette.channel_colors();
    let mut left_c = channels.left;
    let mut right_c = channels.right;
    if brightness > 0.0 {
        left_c = mix_c(left_c, [255, 255, 255], brightness * 0.12);
        right_c = mix_c(right_c, [255, 255, 255], brightness * 0.12);
    }
    let both_c = mix_c(left_c, right_c, 0.5);
    let g_left = glyphs.trace_left();
    let g_right = glyphs.trace_right();
    let g_both = glyphs.trace_both();

    let waveform = &scene.waveform;
    for col in 0..w {
        let (lrows, ln) = channel_rows(&waveform.left, w, col, center, scale, h);
        let (rrows, rn) = channel_rows(&waveform.right, w, col, center, scale, h);
        let x = area.x + col as u16;

        // Pasada L: el canal izquierdo; donde R comparte fila, mezcla.
        for &row in &lrows[..ln] {
            let both = rrows[..rn].contains(&row);
            let (symbol, color) = if both {
                (g_both, both_c)
            } else {
                (g_left, left_c)
            };
            paint_point(frame, x, area.y + row, symbol, color);
        }
        // Pasada R: lo que L no pintó (los destinos compartidos ya salieron
        // como mezcla en la pasada L).
        for &row in &rrows[..rn] {
            if lrows[..ln].contains(&row) {
                continue;
            }
            paint_point(frame, x, area.y + row, g_right, right_c);
        }
    }
}

/// Dibuja la franja de barras del espectro (`rect` de 1 o 2 filas).
///
/// Cada columna es una barra con niveles discretos: con 2 filas, la fila
/// inferior se llena primero (▁..█) y la superior sube cuando la barra la
/// rebasa (0..=16 niveles) — crecimiento desde la base, clásico de un EQ. Sin
/// señal la columna queda en el plano de fondo con una guía tenue.
fn render_bars(frame: &mut Frame, rect: Rect, state: &VisualState) {
    if rect.width == 0 || rect.height == 0 {
        return;
    }
    let color = if state.active {
        bar_color(state.intensity, &state.scene.palette)
    } else {
        Color::DarkGray
    };
    let bg = state.scene.palette.background;
    let rows = rect.height;
    let bottom = rect.y + rect.height - 1;

    for col in rect.x..rect.x + rect.width {
        let idx =
            ((col - rect.x) as usize * VISUAL_BARS / rect.width as usize).min(VISUAL_BARS - 1);
        let v = (state.bars[idx] + state.pulse * 0.06).clamp(0.0, 1.0);
        // Niveles por columna: 16 con dos filas (8 por fila), 8 con una.
        let steps = (v * (8 * rows) as f32).round() as usize;
        for off in 0..rows {
            let row = bottom - off;
            let level = if off == 0 {
                // Fila inferior (base): se llena primero.
                steps.min(8)
            } else {
                steps.saturating_sub(8)
            };
            let glyph = if rows == 1 {
                RAMP[level.min(RAMP.len() - 1)]
            } else {
                RAMP_TALL[level]
            };
            if let Some(cell) = frame.buffer_mut().cell_mut(Position { x: col, y: row }) {
                cell.set_symbol(glyph);
                cell.set_style(
                    Style::new()
                        .fg(if level > 0 { color } else { Color::DarkGray })
                        .bg(to_color(bg)),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::WaveformEnvelope;
    use crate::ui::glyphs::{GlyphTheme, UiGlyphs};
    use crate::visualization::engine::{SceneState, WaveformView};
    use crate::visualization::VISUAL_BARS;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn active_state(level: f32, palette: VisualPalette) -> VisualState {
        let mut bars = [0.0f32; VISUAL_BARS];
        for (i, b) in bars.iter_mut().enumerate() {
            *b = (level * (1.0 - i as f32 / VISUAL_BARS as f32)).clamp(0.0, 1.0);
        }
        VisualState {
            bars,
            level,
            intensity: level,
            pulse: level * 0.8,
            phase: 0.25,
            active: true,
            scene: SceneState {
                waveform: view(0.6, 0),
                energy: level,
                brightness: level,
                palette,
                active: true,
            },
        }
    }

    /// Envolvente de un seno (min/max por bucket): señal oscilante con la
    /// amplitud dada; L y R idénticas (contenido mono → puntos solapados).
    fn view(amp: f32, seed: usize) -> WaveformView {
        let cycles = 6.0 + (seed % 3) as f32;
        let samples: Vec<f32> = (0..1024)
            .map(|i| {
                let ang =
                    std::f32::consts::TAU * (i as f32 / 1024.0) * cycles + (seed as f32) * 0.7;
                amp * ang.sin()
            })
            .collect();
        let env = WaveformEnvelope::from_window(&samples);
        WaveformView {
            left: env,
            right: env,
            gain: 1.0,
        }
    }

    /// Vista con curvas ASIMÉTRICAS: cada canal un seno de amplitud propia.
    fn stereo_view(left_amp: f32, right_amp: f32, seed: usize) -> WaveformView {
        let samples_l: Vec<f32> = (0..1024)
            .map(|i| {
                let ang = std::f32::consts::TAU * (i as f32 / 1024.0) * 5.0 + (seed as f32) * 0.7;
                left_amp * ang.sin()
            })
            .collect();
        let samples_r: Vec<f32> = (0..1024)
            .map(|i| {
                let ang = std::f32::consts::TAU * (i as f32 / 1024.0) * 5.0
                    + (seed as f32) * 0.7
                    + std::f32::consts::PI;
                right_amp * ang.sin()
            })
            .collect();
        let left = WaveformEnvelope::from_window(&samples_l);
        let right = WaveformEnvelope::from_window(&samples_r);
        WaveformView {
            left,
            right,
            gain: 1.0,
        }
    }

    /// Vista onda cuadrada (min=-amp, max=+amp por bucket) en L; R silente.
    fn square_stereo(amp: f32) -> WaveformView {
        let square: Vec<f32> = (0..1024)
            .map(|i| if i % 2 == 0 { amp } else { -amp })
            .collect();
        let left = WaveformEnvelope::from_window(&square);
        WaveformView {
            left,
            right: WaveformEnvelope::silent(),
            gain: 1.0,
        }
    }

    fn drawing(ch: &dyn Fn(&mut Frame)) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(40, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(ch).unwrap();
        terminal.backend().buffer().clone()
    }

    fn drawing_size(w: u16, h: u16, ch: &dyn Fn(&mut Frame)) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(ch).unwrap();
        terminal.backend().buffer().clone()
    }

    fn tint_of(c: ratatui::style::Color, base: [u8; 3]) -> u32 {
        match c {
            Color::Rgb(r, g, b) => {
                (r as i32 - base[0] as i32).unsigned_abs()
                    + (g as i32 - base[1] as i32).unsigned_abs()
                    + (b as i32 - base[2] as i32).unsigned_abs()
            }
            _ => 0,
        }
    }

    fn max_tint(buf: &ratatui::buffer::Buffer, base: [u8; 3]) -> u32 {
        buf.content()
            .iter()
            .map(|c| tint_of(c.bg, base))
            .max()
            .unwrap_or(0)
    }

    /// Glifos de punto del trazado estéreo (Unicode `●` / ASCII `*`).
    fn is_point(s: &str) -> bool {
        matches!(s, "●" | "*")
    }

    #[test]
    fn renders_active_and_inactive_without_panic() {
        let backend = TestBackend::new(60, 6);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render(
                    f,
                    f.area(),
                    &active_state(0.9, VisualPalette::fallback()),
                    42.0,
                )
            })
            .unwrap();
        terminal
            .draw(|f| render(f, f.area(), &VisualState::inactive(), 0.0))
            .unwrap();
        terminal
            .draw(|f| render_backdrop(f, f.area(), &VisualState::inactive(), true))
            .unwrap();
    }

    #[test]
    fn tiny_and_profile_areas_do_not_panic() {
        let profile_sizes: &[(u16, u16)] = &[(120, 40), (100, 30), (80, 24), (70, 20), (60, 15)];
        for (w, h) in profile_sizes {
            let buf = drawing_size(*w, *h, &|f| {
                render_backdrop(
                    f,
                    f.area(),
                    &active_state(1.0, VisualPalette::fallback()),
                    false,
                )
            });
            assert_eq!(buf.area.width, *w, "siempre pinta el área completa");
        }
        let backend = TestBackend::new(10, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render(
                    f,
                    f.area(),
                    &active_state(1.0, VisualPalette::fallback()),
                    1.0,
                )
            })
            .unwrap();
        terminal
            .draw(|f| {
                render_backdrop(
                    f,
                    Rect::new(0, 0, 5, 2),
                    &active_state(1.0, VisualPalette::fallback()),
                    false,
                )
            })
            .unwrap();
        terminal
            .draw(|f| render_backdrop(f, Rect::new(0, 0, 0, 0), &VisualState::inactive(), false))
            .unwrap();
    }

    #[test]
    fn louder_state_paints_more_filled_cells() {
        let case = |level: f32| {
            let buf = drawing(&|f| {
                render(
                    f,
                    f.area(),
                    &active_state(level, VisualPalette::fallback()),
                    0.0,
                )
            });
            buf.content()
                .iter()
                .filter(|c| !c.symbol().trim().is_empty())
                .count()
        };
        assert!(case(0.9) > case(0.15), "más nivel ⇒ más celdas pintadas");
    }

    #[test]
    fn palette_colors_the_bars() {
        // Con paleta, una barra activa usa un color RGB de la portada en vez
        // del esquema cian fijo: intensidad alta → dominante, leve → acento.
        let cover = Some([[220u8, 30, 30], [40, 200, 60], [30, 60, 220]]);
        let high = active_state(0.9, VisualPalette::from_cover(cover));
        let buf = drawing(&|f| render(f, f.area(), &high, 0.0));
        let has_palette_color = buf
            .content()
            .iter()
            .any(|c| c.fg == Color::Rgb(220, 30, 30));
        assert!(
            has_palette_color,
            "el trazo/barras usan colores de la portada"
        );
        let low = active_state(0.3, VisualPalette::from_cover(cover));
        let buf = drawing(&|f| render(f, f.area(), &low, 0.0));
        let has_accent = buf
            .content()
            .iter()
            .any(|c| c.fg == Color::Rgb(30, 60, 220));
        assert!(has_accent, "una columna leve usa el acento de la portada");
    }

    /// Máximo tinte de trazo sobre el plano de fondo del buffer.
    fn first_lit_cell(buf: &ratatui::buffer::Buffer, state: &VisualState) -> Option<u32> {
        let base = state.scene.palette.background;
        let t = max_tint(buf, base);
        (t > 0).then_some(t)
    }

    #[test]
    fn more_energy_brighter_trace() {
        let palette =
            VisualPalette::from_cover(Some([[200u8, 30, 80], [40, 160, 240], [240, 180, 80]]));
        let mut low_s = active_state(0.05, palette);
        low_s.scene.energy = 0.0;
        let mut high = active_state(1.0, palette); // energía alta
        high.scene.brightness = 0.0;

        let buf_low = drawing(&|f| render_backdrop(f, f.area(), &low_s, false));
        let buf_high = drawing(&|f| render_backdrop(f, f.area(), &high, false));
        let lit_low = first_lit_cell(&buf_low, &low_s).unwrap_or(0);
        let lit_high = first_lit_cell(&buf_high, &high).unwrap_or(0);
        assert!(
            lit_high > 0,
            "con energía el trazo tiñe el fondo por encima del plano"
        );
        assert!(lit_high > lit_low, "más energía ⇒ más tinte de trazo");
    }

    /// Distancia vertical máxima de los puntos del trazo al centro del área.
    fn sweep_of(buf: &ratatui::buffer::Buffer) -> f32 {
        let mut offsets = Vec::new();
        for y in 1..5u16 {
            for x in 1..39u16 {
                if is_point(buf.cell((x, y)).unwrap().symbol()) {
                    offsets.push((y as f32 - 3.0).abs());
                }
            }
        }
        offsets.iter().cloned().fold(f32::MIN, f32::max)
    }

    #[test]
    fn louder_waveform_trace_sweeps_farther_from_center() {
        // Misma escena, solo cambia la amplitud de la envolvente: los puntos
        // del trazo de una señal de pico 0.9 barre más lejos del centro que la
        // de pico 0.2 (el plot sigue pico Y valle; la ganancia es 1.0).
        let mut loud = active_state(0.9, VisualPalette::fallback());
        loud.scene.waveform = view(0.9, 1);
        let buf_loud = drawing(&|f| render(f, f.area(), &loud, 0.0));
        let mut quiet = active_state(0.9, VisualPalette::fallback());
        quiet.scene.waveform = view(0.2, 1);
        let buf_quiet = drawing(&|f| render(f, f.area(), &quiet, 0.0));
        assert!(
            sweep_of(&buf_loud) > sweep_of(&buf_quiet),
            "señal más fuerte ⇒ puntos más lejos del centro"
        );
    }

    #[test]
    fn point_plot_spans_every_column_without_gaps() {
        // Cada columna del área interior pinta al menos un punto (los canales
        // siempre caen en una celda; una columna sin datos cae al centro).
        let st = active_state(0.9, VisualPalette::fallback());
        let buf = drawing(&|f| render(f, f.area(), &st, 0.0));
        for x in 1..39u16 {
            assert!(
                (1..5u16).any(|y| is_point(buf.cell((x, y)).unwrap().symbol())),
                "la columna {x} del trazo tiene al menos un punto"
            );
        }
    }

    #[test]
    fn stereo_channels_render_as_distinct_point_sets() {
        // L fuerte, R débil (amplitudes distintas): ambas curvas son
        // visibles a la vez con puntos `●`, cada una con su propio color
        // de canal y su propio resplandor de fondo.
        let mut st = active_state(0.9, VisualPalette::fallback());
        st.scene.waveform = stereo_view(0.9, 0.5, 3);
        let buf = drawing(&|f| render(f, f.area(), &st, 0.0));
        let channels = st.scene.palette.channel_colors();
        let brightness = if st.scene.active {
            st.scene.brightness
        } else {
            0.0
        };
        let mut left_rgb = channels.left;
        let mut right_rgb = channels.right;
        if brightness > 0.0 {
            left_rgb = mix_c(left_rgb, [255, 255, 255], brightness * 0.12);
            right_rgb = mix_c(right_rgb, [255, 255, 255], brightness * 0.12);
        }
        let left_color = to_color(left_rgb);
        let right_color = to_color(right_rgb);

        let mut l_count = 0usize;
        let mut r_count = 0usize;
        for x in 1..39u16 {
            for y in 1..5u16 {
                let cell = buf.cell((x, y)).unwrap();
                if is_point(cell.symbol()) {
                    if cell.fg == left_color {
                        l_count += 1;
                    }
                    if cell.fg == right_color {
                        r_count += 1;
                    }
                }
            }
        }
        assert!(l_count > 0, "L llega al trazo con color de canal L");
        assert!(r_count > 0, "R llega al trazo con color de canal R");
        // Con amplitudes distintas, cada canal tiene más puntos exclusivos
        // que de mezcla: la proporción de puntos propios debe ser razonable.
        assert!(
            l_count + r_count >= 10,
            "conjuntos de puntos lo suficientemente grandes: L={l_count} R={r_count}"
        );
    }

    #[test]
    fn channel_collision_paints_both_with_mixed_color() {
        // L y R idénticos (contenido mono): en la mayoría de columnas ambos
        // caen a la misma celda → punto `●` con color de mezcla `both_c`,
        // sin perder ninguna curva.
        let st = active_state(0.9, VisualPalette::fallback());
        let buf = drawing(&|f| render(f, f.area(), &st, 0.0));
        let channels = st.scene.palette.channel_colors();
        let brightness = if st.scene.active {
            st.scene.brightness
        } else {
            0.0
        };
        let mut left_rgb = channels.left;
        let mut right_rgb = channels.right;
        if brightness > 0.0 {
            left_rgb = mix_c(left_rgb, [255, 255, 255], brightness * 0.12);
            right_rgb = mix_c(right_rgb, [255, 255, 255], brightness * 0.12);
        }
        let both_color = to_color(mix_c(left_rgb, right_rgb, 0.5));

        let has_both = buf
            .content()
            .iter()
            .any(|c| is_point(c.symbol()) && c.fg == both_color);
        assert!(
            has_both,
            "canales solapados pinta punto `●` con color de mezcla"
        );
    }

    #[test]
    fn min_and_max_of_a_column_are_drawn_separately() {
        // Onda cuadrada ±0.9 en L (cada bucket conserva valle Y pico), R mudo:
        // el plot llega tanto a la fila alta (pico) como a la baja (valle) en
        // la misma columna.
        let mut st = active_state(0.9, VisualPalette::fallback());
        st.scene.waveform = square_stereo(0.9);
        let buf = drawing(&|f| render(f, f.area(), &st, 0.0));
        // Filas bajas del área interior (bottom = fila 4) y filas altas (= 1).
        let top_filled = (1..39u16).any(|x| is_point(buf.cell((x, 1)).unwrap().symbol()));
        let bottom_filled = (1..39u16).any(|x| is_point(buf.cell((x, 4)).unwrap().symbol()));
        assert!(top_filled, "el pico de la onda cuadrada pinta arriba");
        assert!(bottom_filled, "el valle de la onda cuadrada pinta abajo");
        // Y, como L domina y R calla, hay columnas con DSOs puntos: los dos
        // extremos de la columa en la misma columna.
        let columns_with_two = (1..39u16)
            .filter(|&x| {
                (1..5u16)
                    .filter(|&y| is_point(buf.cell((x, y)).unwrap().symbol()))
                    .count()
                    >= 2
            })
            .count();
        assert!(columns_with_two > 0, "min y max conviven en la columna");
    }

    #[test]
    fn ascii_theme_uses_only_ascii_points() {
        let st = active_state(0.9, VisualPalette::fallback());
        let buf =
            drawing(&|f| render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Ascii)));
        let ascii_points = buf
            .content()
            .iter()
            .filter(|c| !c.symbol().trim().is_empty())
            .all(|c| matches!(c.symbol(), "*"));
        assert!(ascii_points, "el tema ASCII solo produce puntos 7-bit `*`");
        let unicode =
            drawing(&|f| render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode)));
        assert!(
            unicode.content().iter().any(|c| matches!(c.symbol(), "●")),
            "el tema Unicode usa puntos `●`"
        );
    }

    #[test]
    fn silence_keeps_the_trace_on_the_center_baseline() {
        // Con la escena inactiva el trazo es una línea base centrada: todos los
        // puntos caen en la fila central del área interior (sin NaN ni saltos).
        let buf = drawing(&|f| render(f, f.area(), &VisualState::inactive(), 0.0));
        let rows: Vec<u16> = (1..39u16)
            .filter_map(|x| (1..5u16).find(|&y| is_point(buf.cell((x, y)).unwrap().symbol())))
            .collect();
        assert_eq!(rows.len(), 38, "la línea base pinta todas las columnas");
        assert!(
            rows.iter().all(|&y| y == 2 || y == 3),
            "todos los puntos en la banda central: {rows:?}"
        );
    }

    #[test]
    fn live_envelope_gain_scales_the_trace() {
        // Dos snapshots del MISMO audio (misma envolvente, distinto gain): al
        // aplicar un gain 2× los puntos se alejan del centro.
        let mut st = active_state(0.9, VisualPalette::fallback());
        st.scene.waveform = view(0.7, 5);
        let g1 = drawing(&|f| render(f, f.area(), &st, 0.0));
        st.scene.waveform.gain = 2.0;
        let g2 = drawing(&|f| render(f, f.area(), &st, 0.0));
        assert!(
            sweep_of(&g2) > sweep_of(&g1) - 1e-3,
            "más ganancia ⇒ puntos más lejos del centro"
        );
    }

    #[test]
    fn bars_grow_into_two_rows_when_room() {
        // En un terminal con área interior de 6 filas, la franja del EQ ocupa
        // las DOS filas inferiores: la base siempre tiene barra y las fuertes
        // suben a la fila de arriba (crecimiento desde la base).
        let st = active_state(1.0, VisualPalette::fallback());
        let buf = drawing(&|f| render(f, f.area(), &st, 0.0));
        // Barras del EQ: columnas x 1..=38 sobre las filas y 5..=6.
        let is_bar = |x: u16, y: u16| {
            let c = buf.cell((x, y)).unwrap();
            !c.symbol().trim().is_empty() && RAMP_TALL.contains(&c.symbol())
        };
        assert!(
            (1..39u16).all(|x| is_bar(x, 6)),
            "la fila base (inferior) del EQ tiene barra en cada columna"
        );
        assert!(
            (1..39u16).any(|x| is_bar(x, 5)),
            "las barras fuertes crecen a la fila superior"
        );
        assert!(
            (1..39u16).any(|x| !is_bar(x, 5)),
            "no toda columna llega arriba: la escalera es gradual"
        );
    }

    #[test]
    fn subdued_dims_the_trace_for_legibility() {
        let palette = VisualPalette::fallback();
        let st = active_state(0.9, palette);
        let full = drawing(&|f| render_backdrop(f, f.area(), &st, false));
        let dim = drawing(&|f| render_backdrop(f, f.area(), &st, true));
        let sum_rgb = |c: Color| -> u32 {
            match c {
                Color::Rgb(r, g, b) => u32::from(r) + u32::from(g) + u32::from(b),
                _ => 0,
            }
        };
        let dimmer = full
            .content()
            .iter()
            .zip(dim.content().iter())
            .all(|(c1, c2)| sum_rgb(c2.bg) <= sum_rgb(c1.bg));
        assert!(dimmer, "sin excepción, el fondo aplacado es más oscuro");
        assert!(
            dim.content()
                .iter()
                .zip(full.content().iter())
                .any(|(c2, c1)| sum_rgb(c2.bg) < sum_rgb(c1.bg)),
            "y al menos una celda se atenúa de verdad"
        );
    }

    #[test]
    fn backdrop_only_sets_background_not_symbols() {
        let st = active_state(0.9, VisualPalette::fallback());
        let buf = drawing(&|f| render_backdrop(f, f.area(), &st, false));
        // La capa ambiental no pinta glifos (solo fondo), así el texto superior
        // (karaoke) puede superponerse limpiamente.
        assert!(
            buf.content().iter().all(|c| c.symbol() == " "),
            "backdrop es fondo puro (celda en blanco, sin símbolos)"
        );
    }

    #[test]
    fn wide_terminals_never_gap_columns() {
        // Un ancho mayor que WAVEFORM_BUCKETS debe pintar el trazo sin huecos:
        // cada columna cae en ≥1 bucket y ninguna queda en blanco.
        let backend = TestBackend::new(WAVEFORM_BUCKETS as u16 + 40, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_backdrop(
                    f,
                    f.area(),
                    &active_state(1.0, VisualPalette::fallback()),
                    false,
                )
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let base = VisualPalette::fallback().background;
        let col_hits: Vec<bool> = (0..buf.area.width)
            .map(|x| (0..buf.area.height).any(|y| tint_of(buf.cell((x, y)).unwrap().bg, base) > 0))
            .collect();
        assert!(
            col_hits.first().copied().unwrap(),
            "columna inicial pintada"
        );
        assert!(col_hits.last().copied().unwrap(), "columna final pintada");
        assert!(
            col_hits.iter().all(|b| *b),
            "el trazo no gapea en pantallas anchas"
        );
    }
}
