//! Renderer TUI de la escena visual: osciloscopio de forma de onda + barras.
//!
//! Responsabilidad EXCLUSIVA de renderizar (spec §25/§20): sin análisis, sin
//! HTTP, sin providers, sin relojes. Todo lo que pinta está en el estado que
//! recibe. La escena es la envolvente min/max del PCM (`WaveformView`)
//! decimada al ancho del área: por cada columna se vuelcan los buckets que le
//! tocan y se pinta una LÍNEA delgada que sigue la señal (`(min+max)/2`), con
//! medios bloques (▀/▄) para resolución sub-celda y un resplandor suave de
//! fondo a su alrededor — un trazado fino y limpio, en vez de una banda
//! vertical rellena entre min y max. Toda la colorimetría sale de
//! [`VisualPalette`] — nunca se deriva aquí.

use ratatui::layout::{Margin, Position, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

use crate::analysis::WAVEFORM_BUCKETS;
use crate::visualization::engine::VisualState;
use crate::visualization::palette::VisualPalette;
use crate::visualization::VISUAL_BARS;

/// Escalera de una fila de barras (0 = vacío, 7 = lleno).
const RAMP: [&str; 8] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇"];

/// Radio (en filas) del resplandor que acompaña a la línea del osciloscopio:
/// la celda de la línea se tiñe al máximo y el halo decae linealmente hasta
/// cero a esta distancia. Fino, pero vivo: no rellena la banda vertical.
const GLOW_RADIUS: f32 = 1.5;

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

/// Envolvente min/max de los buckets que tocan la columna.
fn column_span(
    view: &crate::visualization::engine::WaveformView,
    w: usize,
    col: usize,
) -> (f32, f32) {
    let (lo, hi) = column_bucket_range(w, col);
    let mut mn = f32::INFINITY;
    let mut mx = f32::NEG_INFINITY;
    for b in lo..hi {
        mn = mn.min(view.min[b]);
        mx = mx.max(view.max[b]);
    }
    (mn, mx)
}

/// Color del trazo según la amplitud de la columna (misma rampa que las barras).
fn trace_rgb(amplitude: f32, palette: &VisualPalette) -> [u8; 3] {
    match (amplitude * 4.0) as usize {
        0 => mix_c(palette.primary, [110, 110, 110], 0.5),
        1 => palette.accent,
        2 => palette.secondary,
        _ => palette.primary,
    }
}

/// Pinta la capa ambiental (osciloscopio) sobre TODO `area`.
///
/// Solo toca el fondo de cada celda (spec: la capa ambiental debe poder vivir
/// detrás de las letras). En lugar de rellenar la banda vertical entre min y
/// max, tiñe solo un RESPLANDOR suave alrededor de la línea de la señal
/// (decae a [`GLOW_RADIUS`] filas): el trazado queda fino y el área, limpia.
/// `subdued` aterriza la escena (reduce resplandor y energía) para que el
/// texto superior siga siendo legible.
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

    for col in 0..w {
        let (mn, mx) = column_span(&scene.waveform, w, col);
        // La línea sigue la señal por su valor medio de columna; el halo usa su
        // amplitud (lo que da color al resplandor, igual que al trazo).
        let mid = (mn + mx) * 0.5;
        let amplitude = mn.abs().max(mx.abs()) * scene.waveform.gain;
        let y_f = center - mid.clamp(-1.0, 1.0) * scale;
        let trail = trace_rgb(amplitude, palette);
        for row in 0..h {
            let x = area.x + col as u16;
            let y = area.y + row as u16;
            let dist = (row as f32 + 0.5 - y_f).abs();
            let falloff = (1.0 - dist / GLOW_RADIUS).clamp(0.0, 1.0);
            let bg_color = if falloff > 0.0 {
                let mut color = mix_c(bg, trail, lit_k * falloff);
                if brightness > 0.0 {
                    color = mix_c(color, [255, 255, 255], brightness * 0.12);
                }
                to_color(color)
            } else {
                to_color(bg)
            };
            // SAFETY(ninguna): API pública de ratatui; celdas dentro de `area`.
            if let Some(cell) = frame.buffer_mut().cell_mut(Position { x, y }) {
                cell.set_bg(bg_color);
            }
        }
    }
}

/// Dibuja el visualizador completo: línea del osciloscopio + franja de barras,
/// con marco de estado. (El resplandor ambiental de fondo se pinta aquí
/// también, antes del trazo.)
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

    // Franja de barras: SOLO la fila inferior del área interior (espectro
    // discreto bajo el trazo del osciloscopio; el resto es todo forma de onda).
    let bars_h = 1usize.min(inner.height as usize);
    let trace_area = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(bars_h as u16),
    );
    let bars_area = Rect::new(
        inner.x,
        inner.y + trace_area.height,
        inner.width,
        bars_h as u16,
    );

    render_backdrop(frame, trace_area, state, false);
    render_trace(frame, trace_area, state);
    render_bars(frame, bars_area, state);
}

/// Pinta la curva del osciloscopio: UNA célula por columna, siguiendo la señal
/// (`(min+max)/2` de la columna) con medios bloques ▀/▄ según la mitad de la
/// celda donde caiga el trazo — resolución sub-celda, línea fina y limpia.
///
/// No toca el fondo: el resplandor de [`render_backdrop`] queda vivo detrás y
/// el área fuera de la línea conserva su ambiental, sin "rellenar" la banda.
fn render_trace(frame: &mut Frame, area: Rect, state: &VisualState) {
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
    let last_row = (h - 1) as f32;

    for col in 0..w {
        let (mn, mx) = column_span(&scene.waveform, w, col);
        let mid = (mn + mx) * 0.5;
        let amplitude = mn.abs().max(mx.abs()) * scene.waveform.gain;
        let y_f = (center - mid.clamp(-1.0, 1.0) * scale).clamp(0.0, last_row);
        // Mitad de la celda: arriba → ▀ (bloque superior), abajo → ▄ (inferior).
        // Al desplazarse el trazo por dentro de la fila, el glifo pasa de ▀ a ▄
        // en el punto medio, dando el aspecto de línea continua de medio bloque.
        let row = y_f as u16;
        let frac = y_f - y_f.floor();
        let glyph = if frac < 0.5 { "▀" } else { "▄" };
        let mut color = trace_rgb(amplitude, palette);
        if brightness > 0.0 {
            color = mix_c(color, [255, 255, 255], brightness * 0.12);
        }
        let x = area.x + col as u16;
        let y = area.y + row;
        // SAFETY(ninguna): API pública de ratatui; celdas del área.
        if let Some(cell) = frame.buffer_mut().cell_mut(Position { x, y }) {
            cell.set_symbol(glyph);
            // Solo el trazo: el fondo (glow) lo dejó render_backdrop.
            cell.set_style(Style::new().fg(to_color(color)));
        }
    }
}

/// Dibuja la franja de barras (una fila: cada barra es un paso de la rampa).
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

    let r = rect.y;
    for col in rect.x..rect.x + rect.width {
        let idx =
            ((col - rect.x) as usize * VISUAL_BARS / rect.width as usize).min(VISUAL_BARS - 1);
        let v = (state.bars[idx] + state.pulse * 0.06).clamp(0.0, 1.0);
        let step = (v * (RAMP.len() - 1) as f32).round() as usize;
        if let Some(cell) = frame.buffer_mut().cell_mut(Position { x: col, y: r }) {
            cell.set_symbol(RAMP[step]);
            cell.set_style(
                Style::new()
                    .fg(if step > 0 { color } else { Color::DarkGray })
                    .bg(to_color(bg)),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::WaveformEnvelope;
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
                waveform: oscillating_view(0.6, 0),
                energy: level,
                brightness: level,
                palette,
                active: true,
            },
        }
    }

    /// Envolvente con min<max por bucket (señal oscilante: la línea del trazo
    /// sigue el valor medio de cada columna, con amplitud ∝ la señal de fondo).
    fn oscillating_view(amp: f32, seed: usize) -> WaveformView {
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
            min: env.min,
            max: env.max,
            gain: 1.0,
        }
    }

    fn drawing(ch: &dyn Fn(&mut Frame)) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(40, 8);
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
    fn tiny_areas_do_not_panic() {
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

    #[test]
    fn louder_waveform_trace_sweeps_farther_from_center() {
        // Mismo estado, solo cambia la amplitud de la envolvente: la línea del
        // trazo de una señal de pico 0.9 barre más lejos del centro vertical
        // que la de pico 0.2 (la curva sigue la señal; no es una banda de
        // espesor fijo).
        let sweep = |st: VisualState| {
            let buf = drawing(&|f| render(f, f.area(), &st, 0.0));
            // Traza: interior 40x8 → x 1..=38, y 1..=5 (sin la fila de barras).
            let mut offsets = Vec::new();
            for y in 1..6u16 {
                for x in 1..39u16 {
                    if matches!(buf.cell((x, y)).unwrap().symbol(), "▀" | "▄") {
                        offsets.push((y as f32 - 3.0).abs());
                    }
                }
            }
            offsets.iter().sum::<f32>() / offsets.len() as f32
        };
        let mut loud = active_state(0.9, VisualPalette::fallback());
        loud.scene.waveform = oscillating_view(0.9, 1);
        let mut quiet = active_state(0.9, VisualPalette::fallback());
        quiet.scene.waveform = oscillating_view(0.2, 1);
        assert!(
            sweep(loud) > sweep(quiet),
            "señal más fuerte ⇒ la línea barre más lejos del centro"
        );
    }

    #[test]
    fn thin_line_draws_one_glyph_per_trace_column() {
        // El osciloscopio es fino: exactamente UNA celda por columna de traza,
        // con medio bloque (▀/▄), en vez de una banda vertical rellena.
        let st = active_state(0.9, VisualPalette::fallback());
        let buf = drawing(&|f| render(f, f.area(), &st, 0.0));
        let mut counts = Vec::new();
        for x in 1..39u16 {
            let n = (1..6u16)
                .filter(|&y| matches!(buf.cell((x, y)).unwrap().symbol(), "▀" | "▄"))
                .count();
            counts.push(n);
        }
        assert!(
            counts.iter().all(|&n| n == 1),
            "una línea, una celda por columna: {counts:?}"
        );
        assert!(
            buf.content()
                .iter()
                .any(|c| matches!(c.symbol(), "▀" | "▄")),
            "el trazo pinta glifos de medio bloque"
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
