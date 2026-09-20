//! Renderer TUI de la escena visual: osciloscopio ESTÉREO de forma de onda +
//! barras.
//!
//! Responsabilidad EXCLUSIVA de renderizar (spec §25/§20): sin análisis, sin
//! HTTP, sin providers, sin relojes. Todo lo que pinta está en el estado que
//! recibe.
//!
//! El osciloscopio es un SCATTER de puntos discretos (referencia conceptual
//! scope-tui `GraphType::Scatter`, nunca `GraphType::Line`) sobre UN plano
//! compartido: L y R usan el mismo eje central (`amplitud == 0` en el medio
//! del área) y las mismas coordenadas de tiempo, cada uno con sus propios
//! puntos y colores. Las curvas se cruzan y superponen como en un osciloscopio
//! estéreo real; donde coinciden en una celda se pinta el tinte de mezcla.
//! Cada columna aporta hasta DOS candidatos por canal
//! (valle y pico de su envolvente, que preservan extremos y transitorios);
//! cada candidato se cuantiza a `(columna, fila)` y solo se emite si aporta
//! novedad a su pista (espaciado en columnas si repite fila, emisión
//! inmediata ante saltos). Así las regiones redundantes (silencio,
//! constantes, mesetas) quedan como puntos espaciados en vez de una línea
//! punteada continua, mientras que la forma real (senos, cuadradas,
//! transitorios) conserva sus picos, valles y cruces por cero.
//!
//! Garantías del trazo: sin `draw_line` entre muestras, sin `fill_rect` del
//! intervalo min..max, sin glifos de bloque (`█▁▂▃▄▅▆▇`) y sin celdas de fondo
//! tintadas como bloques — solo puntos `•` (ASCII `*`), uno por celda como
//! máximo, sobre un fondo plano de la paleta. El camino caliente NO asigna
//! memoria por draw (estado de thinning en el stack); los glifos salen del
//! sistema central [`UiGlyphs`]. Tras las letras el mismo scatter se pinta
//! atenuado ([`render_trace_subdued`], colores fundidos hacia el techo de
//! contraste). Las barras EQ viven en [`render_bars_only`] (vista Now
//! Playing), NUNCA dentro del bloque del osciloscopio. Toda la colorimetría
//! sale de [`VisualPalette`] — nunca se deriva aquí.

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

fn to_color(c: [u8; 3]) -> Color {
    Color::Rgb(c[0], c[1], c[2])
}

/// Mezcla lineal de dos colores RGB (pura).
fn mix_c(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    [l(a[0], b[0]), l(a[1], b[1]), l(a[2], b[2])]
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

/// Proyección estática de amplitud para el osciloscopio: soft-clip `atan` que
/// expande los niveles medios sin mover los extremos.
///
/// Motivación: en una TUI el carril tiene 3–11 filas y el mapeo lineal
/// desperdicia casi todas en señales típicas (0.2–0.3 ⇒ 1–2 filas centrales:
/// la onda se ve como una línea plana). La curva `atan(3·v)/atan(3)` reparte
/// esos niveles por el carril (0.3 ⇒ 0.59) manteniendo fijos 0 y ±1, el orden,
/// el signo y la monotonía — la dinámica relativa L/R se conserva intacta.
///
/// Frente a una potencia `|v|^γ`: la pendiente en cero está acotada
/// (`3/atan(3) ≈ 2.4`, sin ganancia infinita al ruido de fondo) y satura con
/// suavidad hacia ±1.
///
/// Es ESTÁTICA (sin memoria entre frames): no introduce `pumping` ni cambia la
/// semántica del auto-gain existente, que sigue actuando vía `scale`. Pura y
/// barata (un `atan` por candidato).
fn project_amplitude(value: f32) -> f32 {
    const KNEE: f32 = 3.0;
    const NORM: f32 = 1.2490458; // atan(3.0)
    (value.clamp(-1.0, 1.0) * KNEE).atan() / NORM
}

/// Filas candidatas de UN canal en el plano compartido: `min`/`max` de la
/// columna cuantizados con [`row_of`] tras [`project_amplitude`], deduplicados.
///
/// Como máximo 2 filas, sin allocs. Ambos canales usan la MISMA geometría
/// (mismo centro y escala): sus puntos coexisten en las mismas coordenadas de
/// tiempo y se cruzan sobre el eje central como en un osciloscopio estéreo.
fn channel_rows(
    channel: &WaveformEnvelope,
    w: usize,
    col: usize,
    h: usize,
    y0: u16,
    center: f32,
    scale: f32,
) -> ([u16; 2], usize) {
    let mut rows = [0u16; 2];
    if h == 0 {
        return (rows, 0);
    }
    let (mn, mx) = channel_column_span(channel, w, col);
    let mut n = 0usize;
    for value in [mn, mx] {
        // La proyección expande los niveles medios al plano (ver
        // `project_amplitude`): sin ella todo lo moderado cae al centro.
        let row = y0 + row_of(project_amplitude(value), center, scale, h);
        if !rows[..n].contains(&row) {
            rows[n] = row;
            n += 1;
        }
    }
    (rows, n)
}

/// Distancia mínima entre puntos emitidos de la MISMA pista (min o max).
///
/// Sin esto, cada columna pinta su punto y el conjunto degenera en una línea
/// punteada continua (`••••`). La regla es adaptativa: un punto redundante
/// (misma fila que el anterior de su pista) se espacia cada 2 columnas,
/// mientras que CUALQUIER cambio de fila (pendiente, transitorio, cruce) se
/// emite de inmediato aunque la columna anterior pintara. Así las curvas
/// activas y las altas frecuencias dibujan trazos densos y continuos, y lo
/// plano (mesetas, silencios) queda disperso. Criterio de
/// densidad/resolución espacial, no mutilación.
const SCATTER_MIN_DIST: usize = 2;

/// `true` si el candidato `(col, row)` de una pista aporta información nueva
/// (y actualiza el registro de la pista). Estado en el stack, sin allocs.
fn scatter_emit(last: &mut Option<(usize, u16)>, col: usize, row: u16) -> bool {
    let emit = match *last {
        None => true,
        Some((lc, lr)) => col.saturating_sub(lc) >= SCATTER_MIN_DIST || row != lr,
    };
    if emit {
        *last = Some((col, row));
    }
    emit
}

/// Fracción de fundido de los enlaces de pendiente hacia el fondo del panel.
const LINK_DIM: f32 = 0.6;

/// Pinta el enlace tenue de una pendiente: si el punto `(col, row)` continúa
/// la pista desde la columna INMEDIATA anterior con un salto vertical mayor
/// de 1 celda, rellena las filas intermedias (excluidos ambos extremos) con
/// el glifo de punto en color atenuado. Puentes de una sola columna: flujo
/// orgánico sin líneas continuas. Sin allocs.
fn paint_links(
    frame: &mut Frame,
    x: u16,
    prev: Option<(usize, u16)>,
    col: usize,
    row: u16,
    symbol: &str,
    color: [u8; 3],
) {
    let Some((lc, lr)) = prev else { return };
    if col != lc + 1 || row.abs_diff(lr) <= 1 {
        return;
    }
    for r in lr.min(row) + 1..row.max(lr) {
        paint_point(frame, x, r, symbol, color);
    }
}

/// Pinta el punto de trazo (símbolo y color; no toca el fondo plano que dejó
/// [`render_backdrop`]).
fn paint_point(frame: &mut Frame, x: u16, y: u16, symbol: &str, color: [u8; 3]) {
    // SAFETY(ninguna): API pública de ratatui; celdas del área interior.
    if let Some(cell) = frame.buffer_mut().cell_mut(Position { x, y }) {
        cell.set_symbol(symbol);
        cell.set_style(Style::new().fg(to_color(color)));
    }
}

/// Pinta la capa ambiental sobre TODO `area`: fondo PLANO y reseteo de celdas.
///
/// Resetea cada celda del área (símbolo a `" "`) y la tiñe con un fondo
/// uniforme: `background` de la paleta en modo vivo, techo de contraste
/// ([`VisualPalette::karaoke_bg_ceiling`]) en modo `subdued` (banda de
/// letras/mensajes). A propósito SIN resplandor por celda: en el terminal
/// cada celda tintada se lee como un bloque sólido de color, y el halo del
/// osciloscopio producía 9 de cada 10 celdas tintadas — bloques por encima,
/// por debajo y detrás de los puntos y las letras. La señal la llevan
/// únicamente los puntos del trazo (colores de canal derivados de la
/// portada); el fondo nunca compite con ellos.
///
/// La capa ambiental sigue siendo dueña de sus celdas: al resetear los
/// símbolos elimina los fantasmas del frame anterior (barras `█`, puntos
/// viejos, letras) que el buffer reutilizado de la TUI real conservaría — el
/// trazo posterior solo pinta celdas dispersas y jamás limpiaría el resto. El
/// karaoke pinta su texto DESPUÉS, preservando este fondo.
///
/// Al ser el fondo aplacado exactamente el color contra el que se resolvieron
/// los colores del karaoke, los umbrales χ ≥ 4.5/3.0 se cumplen por
/// construcción.
pub fn render_backdrop(frame: &mut Frame, area: Rect, state: &VisualState, subdued: bool) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let palette = &state.scene.palette;

    let flat = if subdued {
        to_color(palette.karaoke_bg_ceiling())
    } else {
        to_color(palette.background)
    };
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            if let Some(cell) = frame.buffer_mut().cell_mut(Position { x, y }) {
                cell.set_symbol(" ");
                cell.set_bg(flat);
            }
        }
    }
}

/// Dibuja el osciloscopio estéreo: L y R superpuestos en un plano compartido
/// con eje central, sobre un fondo plano de la paleta.
///
/// El área interior se dedica ÍNTEGRA al trazo (la franja EQ vive en
/// [`render_bars_only`], nunca aquí: así la forma de onda no se fusiona con
/// bloques de espectro).
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
            format!(" Visual L/R {} ", pulse_dot),
            Style::new().fg(title_color),
        ))
        .title_bottom(Span::styled(
            format!(" fase {:.2} · pos {:.0}s ", state.phase, position_secs),
            Style::new().fg(Color::DarkGray),
        ));
    frame.render_widget(block, area);

    // El área interior es TODA para el trazo superpuesto L/R: la EQ no
    // vive aquí (ver `render_bars_only`), así la onda nunca se lee fusionada
    // con bloques de espectro.
    let trace_area = area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    if trace_area.width == 0 || trace_area.height == 0 {
        return;
    }

    render_backdrop(frame, trace_area, state, false);
    render_trace(frame, trace_area, state);
}

/// Pinta el osciloscopio: SCATTER de puntos en un plano compartido (L+R).
///
/// Cada columna aporta hasta DOS candidatos por canal (VALLE `min` y PICO
/// `max` de su envolvente, spec §8), cuantizados con el MISMO centro
/// (`amplitud == 0` en el medio del área) y la MISMA escala: L y R coexisten
/// en las mismas coordenadas de tiempo y se cruzan sobre el eje central.
/// Cada candidato solo se pinta si aporta novedad a su pista: misma fila que
/// el anterior de la pista ⇒ se espacia en columnas; CUALQUIER cambio de
/// fila ⇒ se emite de inmediato, así las pendientes dibujan trazos densos.
/// Si el salto vertical entre columnas adyacentes supera 1 celda, las filas
/// intermedias se puntean con el color de enlace tenue (una sola columna).
/// Las columnas redundantes quedan VACÍAS en vez de forzar una línea
/// punteada continua. Cada canal conserva sus puntos y su color; donde ambos
/// caen en la misma celda se pinta el tinte de mezcla (la coincidencia se ve,
/// ningún canal se pierde).
///
/// Garantía Scatter: SOLO se pintan puntos discretos (muestras y enlaces
/// tenues de pendiente adyacente). Nunca se une un punto con otro lejano
/// (sin `draw_line`), nunca se rellena el intervalo vertical y nunca se
/// emite un glifo de bloque en esta capa.
///
/// Sin allocations por draw: por columna, como máximo 2 candidatos por canal
/// y cuatro registros `(columna, fila)` en el stack. No toca el fondo (plano,
/// lo dejó `render_backdrop`); los glifos salen del sistema
/// [`UiGlyphs`] de la sesión.
fn render_trace(frame: &mut Frame, area: Rect, state: &VisualState) {
    render_trace_points(frame, area, state, *crate::ui::glyphs::GLYPHS);
}

/// Núcleo de [`render_trace`] con tema de glifos explícito (para tests).
fn render_trace_points(frame: &mut Frame, area: Rect, state: &VisualState, glyphs: UiGlyphs) {
    trace_points_impl(frame, area, state, glyphs, false);
}

/// Trazo del osciloscopio ATENUADO para la banda de letras: el mismo scatter
/// superpuesto, pero con los colores de canal fundidos hacia el fondo del
/// techo de contraste (siempre derivados de la paleta de la portada) y sin el
/// blanqueado de brillo. El osciloscopio sigue vivo tras el texto sin
/// competir con él; el fondo no se toca (lo dejó `render_backdrop`).
pub fn render_trace_subdued(frame: &mut Frame, area: Rect, state: &VisualState) {
    trace_points_impl(frame, area, state, *crate::ui::glyphs::GLYPHS, true);
}

/// Fracción de fundido de los puntos atenuados hacia el fondo del techo.
const SUBDUED_TRACE_DIM: f32 = 0.55;

/// Fracción de la semialtura del plano que ocupa la amplitud ±1.
///
/// Por debajo de 1.0 a propósito: con el plano compartido por L y R, los
/// picos a toda escala deben dejar margen para que ambas ondas se aprecien
/// por separado en vez de aplanarse contra los bordes superpuestas.
const PLANE_FILL: f32 = 0.65;

fn trace_points_impl(
    frame: &mut Frame,
    area: Rect,
    state: &VisualState,
    glyphs: UiGlyphs,
    subdued: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let scene = &state.scene;
    let palette = &scene.palette;
    let w = area.width as usize;
    let h = area.height as usize;
    let brightness = if scene.active && !subdued {
        scene.brightness
    } else {
        0.0
    };

    // Plano único: centro discreto del área y una sola escala para AMBOS
    // canales (el 0 compartido queda en la fila central). El llenado es
    // parcial a propósito (ver `PLANE_FILL`): los picos dejan margen arriba y
    // abajo para que L y R se distingan en vez de saturar los bordes.
    let gain = scene.waveform.gain;
    let center = (h as f32 - 1.0) * 0.5;
    let scale = ((h as f32 - 1.0) * 0.5 * PLANE_FILL).max(1.0) * gain;

    let channels = palette.channel_colors();
    let mut left_c = channels.left;
    let mut right_c = channels.right;
    if subdued {
        let ceiling = palette.karaoke_bg_ceiling();
        left_c = mix_c(left_c, ceiling, SUBDUED_TRACE_DIM);
        right_c = mix_c(right_c, ceiling, SUBDUED_TRACE_DIM);
    }
    if brightness > 0.0 {
        left_c = mix_c(left_c, [255, 255, 255], brightness * 0.12);
        right_c = mix_c(right_c, [255, 255, 255], brightness * 0.12);
    }
    let both_c = mix_c(left_c, right_c, 0.5);
    let g_left = glyphs.trace_left();
    let g_right = glyphs.trace_right();
    let g_both = glyphs.trace_both();
    // Enlaces tenues de pendiente: mismo glifo con el color fundido hacia el
    // fondo del panel. Solo rellenan el hueco vertical entre columnas
    // ADYACENTES (nunca tramos largos): la pendiente se lee orgánica sin
    // convertirse en línea continua.
    let panel_bg = if subdued {
        palette.karaoke_bg_ceiling()
    } else {
        palette.background
    };
    let link_l = mix_c(left_c, panel_bg, LINK_DIM);
    let link_r = mix_c(right_c, panel_bg, LINK_DIM);

    let waveform = &scene.waveform;
    // Una pista por extremo (min/max) y canal: los raíles persistentes
    // (cuadradas, mesetas) se espacian por pista y dejan huecos, mientras que
    // un transitorio salta de fila y se emite aunque comparta columna.
    let mut last_lmn: Option<(usize, u16)> = None;
    let mut last_lmx: Option<(usize, u16)> = None;
    let mut last_rmn: Option<(usize, u16)> = None;
    let mut last_rmx: Option<(usize, u16)> = None;
    for col in 0..w {
        let x = area.x + col as u16;
        // Candidatos de cada canal tras el thinning (filas absolutas). Los
        // enlaces se pintan al momento (siempre ANTES que los puntos reales
        // de la columna, que los pisan si coinciden).
        let (lrows, ln) = channel_rows(&waveform.left, w, col, h, area.y, center, scale);
        let mut lsel = [0u16; 2];
        let mut lsel_n = 0usize;
        for (i, &row) in lrows[..ln].iter().enumerate() {
            let last = if i == 0 { &mut last_lmn } else { &mut last_lmx };
            let prev = *last;
            if scatter_emit(last, col, row) {
                paint_links(frame, x, prev, col, row, g_left, link_l);
                lsel[lsel_n] = row;
                lsel_n += 1;
            }
        }
        let (rrows, rn) = channel_rows(&waveform.right, w, col, h, area.y, center, scale);
        let mut rsel = [0u16; 2];
        let mut rsel_n = 0usize;
        for (i, &row) in rrows[..rn].iter().enumerate() {
            let last = if i == 0 { &mut last_rmn } else { &mut last_rmx };
            let prev = *last;
            if scatter_emit(last, col, row) {
                paint_links(frame, x, prev, col, row, g_right, link_r);
                rsel[rsel_n] = row;
                rsel_n += 1;
            }
        }
        // L primero; donde R coincide se pinta la mezcla (coincidencia
        // visible, como trazos superpuestos del osciloscopio de referencia).
        for &row in &lsel[..lsel_n] {
            let (symbol, color) = if rsel[..rsel_n].contains(&row) {
                (g_both, both_c)
            } else {
                (g_left, left_c)
            };
            paint_point(frame, x, row, symbol, color);
        }
        for &row in &rsel[..rsel_n] {
            if lsel[..lsel_n].contains(&row) {
                continue;
            }
            paint_point(frame, x, row, g_right, right_c);
        }
    }
}

/// Banda del visual para Now Playing: SOLO barras del espectro, ampliadas a
/// toda el área interior (el osciloscopio de puntos vive solo en Related).
///
/// Composición autocontenida: marco + barras que usan TODAS las filas
/// disponibles (cada fila aporta 8 subniveles desde la base, clásico de un
/// EQ). Cada celda del interior se repinta entera (símbolo + fg + bg), así la
/// banda nunca arrastra fantasmas del frame anterior.
pub fn render_bars_only(frame: &mut Frame, area: Rect, state: &VisualState) {
    let title_color = if state.active {
        bar_color(state.intensity.max(0.15), &state.scene.palette)
    } else {
        Color::DarkGray
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" Ecualizador ", Style::new().fg(title_color)));
    frame.render_widget(block, area);

    let inner = area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    render_bars(frame, inner, state);
}

/// Dibuja barras del espectro en `rect` (cualquier altura ≥ 1 fila).
///
/// Cada columna es una barra con niveles discretos que crece desde la base:
/// cada fila aporta 8 subniveles (0..=8·filas en total). Con 1 fila usa la
/// rampa corta; con 2 o más, la rampa alta (▁..█). Sin señal la columna queda
/// en el plano de fondo con una guía tenue.
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
        // Niveles por columna: 8 por fila (8 con una fila, 16 con dos, …).
        let steps = (v * (8 * rows) as f32).round() as usize;
        for off in 0..rows {
            let row = bottom - off;
            // Fila inferior (base) se llena primero; las superiores suben
            // cuando la barra las rebasa. Idéntico al comportamiento previo
            // para 1–2 filas; para N filas escala igual.
            let level = steps.saturating_sub(usize::from(off) * 8).min(8);
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

    /// Glifos de punto del trazado estéreo (Unicode `•` / ASCII `*`).
    fn is_point(s: &str) -> bool {
        matches!(s, "•" | "*")
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
    fn louder_waveform_paints_more_cells() {
        // Misma escena salvo la amplitud de la envolvente: una señal fuerte
        // (min y max separados ⇒ dos pistas por carril) pinta más celdas que
        // una suave (min≈max ⇒ una sola pista por carril).
        let case = |amp: f32| {
            let mut st = active_state(0.9, VisualPalette::fallback());
            st.scene.waveform = view(amp, 1);
            st.scene.brightness = 0.0;
            let buf = drawing(&|f| render(f, f.area(), &st, 0.0));
            buf.content()
                .iter()
                .filter(|c| !c.symbol().trim().is_empty())
                .count()
        };
        assert!(case(0.9) > case(0.15), "más nivel ⇒ más celdas pintadas");
    }

    #[test]
    fn palette_colors_the_bars() {
        // Con paleta, una barra activa usa el color de la portada (tameado,
        // no el RGB crudo) en vez del esquema cian fijo: intensidad alta →
        // dominante, leve → acento.
        let cover = Some([[220u8, 30, 30], [40, 200, 60], [30, 60, 220]]);
        let high = active_state(0.9, VisualPalette::from_cover(cover));
        let buf = drawing(&|f| render(f, f.area(), &high, 0.0));
        let has_palette_color = buf
            .content()
            .iter()
            .any(|c| c.fg == Color::Rgb(215, 35, 35));
        assert!(
            has_palette_color,
            "el trazo/barras usan colores de la portada"
        );
        let low = active_state(0.3, VisualPalette::from_cover(cover));
        let buf = drawing(&|f| render(f, f.area(), &low, 0.0));
        let has_accent = buf
            .content()
            .iter()
            .any(|c| c.fg == Color::Rgb(35, 63, 215));
        assert!(has_accent, "una columna leve usa el acento de la portada");
    }

    #[test]
    fn backdrop_is_flat_panel_regardless_of_energy() {
        // Sin celdas tintadas como bloques: el fondo es el plano de la paleta
        // en modo vivo y el techo de contraste en modo aplacado, con CUALQUIER
        // energía (la señal la llevan solo los puntos).
        let palette =
            VisualPalette::from_cover(Some([[200u8, 30, 80], [40, 160, 240], [240, 180, 80]]));
        let mut low_s = active_state(0.05, palette);
        low_s.scene.energy = 0.0;
        let mut high = active_state(1.0, palette); // energía alta
        high.scene.brightness = 1.0;
        for st in [&low_s, &high] {
            let buf = drawing(&|f| render_backdrop(f, f.area(), st, false));
            assert!(
                buf.content().iter().all(|c| c.bg
                    == Color::Rgb(
                        palette.background[0],
                        palette.background[1],
                        palette.background[2]
                    )),
                "fondo plano de la paleta sin importar la energía"
            );
        }
        let dim = drawing(&|f| render_backdrop(f, f.area(), &high, true));
        let ceiling = palette.karaoke_bg_ceiling();
        assert!(
            dim.content()
                .iter()
                .all(|c| c.bg == Color::Rgb(ceiling[0], ceiling[1], ceiling[2])),
            "fondo aplacado plano del techo"
        );
    }

    /// Distancia vertical máxima de los puntos del trazo al eje central.
    ///
    /// El área de `drawing()` (40×8) con `render()` deja un interior de 6 filas
    /// (y=1..7) con eje en y=3.5, compartido por L y R.
    fn sweep_of(buf: &ratatui::buffer::Buffer) -> f32 {
        let mut offsets = Vec::new();
        for y in 1..7u16 {
            for x in 1..39u16 {
                if is_point(buf.cell((x, y)).unwrap().symbol()) {
                    offsets.push((y as f32 - 3.5).abs());
                }
            }
        }
        offsets.iter().cloned().fold(f32::MIN, f32::max)
    }

    #[test]
    fn louder_waveform_trace_sweeps_farther_from_center() {
        // Misma escena, solo cambia la amplitud de la envolvente: los puntos
        // del trazo de una señal de pico 0.9 se alejan del eje central más
        // que los de pico 0.2 (la ganancia es 1.0).
        let mut loud = active_state(0.9, VisualPalette::fallback());
        loud.scene.waveform = view(0.9, 1);
        let buf_loud = drawing(&|f| render(f, f.area(), &loud, 0.0));
        let mut quiet = active_state(0.9, VisualPalette::fallback());
        quiet.scene.waveform = view(0.2, 1);
        let buf_quiet = drawing(&|f| render(f, f.area(), &quiet, 0.0));
        assert!(
            sweep_of(&buf_loud) > sweep_of(&buf_quiet),
            "señal más fuerte ⇒ puntos más lejos del eje central"
        );
    }

    #[test]
    fn scatter_leaves_gaps_for_redundant_signal() {
        // Anti-línea-punteada: una señal constante no fuerza un punto por
        // columna; el thinning espacia los puntos redundantes y deja columnas
        // VACÍAS, sin perder la amplitud (todos los puntos en la misma fila).
        let st = plain_state(WaveformView {
            left: WaveformEnvelope::from_window(&[0.5; 2048]),
            right: WaveformEnvelope::from_window(&[0.5; 2048]),
            gain: 1.0,
        });
        let buf =
            drawing(&|f| render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode)));
        let cols_with_points = (0..40u16)
            .filter(|&x| (0..8u16).any(|y| is_point(buf.cell((x, y)).unwrap().symbol())))
            .count();
        assert!(cols_with_points > 0, "la señal constante sigue visible");
        assert!(
            cols_with_points < 40,
            "pero no ocupa todas las columnas ({cols_with_points}/40): hay huecos"
        );
    }

    #[test]
    fn stereo_channels_render_as_distinct_point_sets() {
        // L fuerte (0.9), R suave (0.2): ambas curvas coexisten en el plano
        // compartido con puntos `•`, cada una con su propio color de canal;
        // donde coinciden se ve la mezcla. L barre más lejos del eje que R.
        let mut st = active_state(0.9, VisualPalette::fallback());
        st.scene.waveform = stereo_view(0.9, 0.2, 3);
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

        let sweep = |target: Color| {
            let mut best = f32::MIN;
            for x in 1..39u16 {
                for y in 1..7u16 {
                    let cell = buf.cell((x, y)).unwrap();
                    if is_point(cell.symbol()) && cell.fg == target {
                        best = best.max((y as f32 - 3.5).abs());
                    }
                }
            }
            best
        };
        let l_sweep = sweep(left_color);
        let r_sweep = sweep(right_color);
        assert!(l_sweep > 0.0, "L llega al trazo con color de canal L");
        assert!(r_sweep >= 0.0, "R llega al trazo con color de canal R");
        assert!(
            l_sweep > r_sweep,
            "L (0.9) barre más lejos del eje que R (0.2): {l_sweep} vs {r_sweep}"
        );
    }

    #[test]
    fn mono_content_overlaps_with_mixed_color() {
        // Contenido mono (L y R idénticos): ambas curvas caen en las mismas
        // celdas y se pintan con el tinte de mezcla — la coincidencia se ve
        // como trazos superpuestos, sin perder ningún canal.
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

        let mixed = buf
            .content()
            .iter()
            .filter(|c| is_point(c.symbol()) && c.fg == both_color)
            .count();
        assert!(
            mixed > 0,
            "canales idénticos se superponen con color de mezcla"
        );
    }
    #[test]
    fn square_wave_keeps_headroom_with_visible_extremes() {
        // Onda cuadrada ±0.9 sobre el plano compartido: con el llenado parcial
        // los extremos quedan VIBLES pero sin aplanarse contra los bordes
        // (y=2 arriba, y=5 abajo en el interior y=1..7): hay margen para
        // distinguir ambas ondas.
        let sq = square_stereo(0.9).left;
        let mut st = active_state(0.9, VisualPalette::fallback());
        st.scene.waveform = WaveformView {
            left: sq,
            right: sq,
            gain: 1.0,
        };
        let buf = drawing(&|f| render(f, f.area(), &st, 0.0));
        assert!(
            (1..39u16).any(|x| is_point(buf.cell((x, 2)).unwrap().symbol())),
            "el pico queda visible arriba con margen"
        );
        assert!(
            (1..39u16).any(|x| is_point(buf.cell((x, 5)).unwrap().symbol())),
            "el valle queda visible abajo con margen"
        );
        for y in [1u16, 6] {
            assert!(
                (1..39u16).all(|x| !is_point(buf.cell((x, y)).unwrap().symbol())),
                "los bordes quedan libres (y={y}): sin saturación"
            );
        }
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
            unicode.content().iter().any(|c| matches!(c.symbol(), "•")),
            "el tema Unicode usa puntos `•`"
        );
    }

    #[test]
    fn silence_keeps_sparse_center_baseline() {
        // Escena inactiva: ambos canales reposan en el eje central compartido
        // (y=3..4) como puntos ESPACIADOS (thinning), sin NaN ni saltos y sin
        // pintar una línea sólida: el silencio se lee como silencio.
        let buf = drawing(&|f| render(f, f.area(), &VisualState::inactive(), 0.0));
        let mut cols = 0usize;
        for x in 1..39u16 {
            if (1..7u16).any(|y| is_point(buf.cell((x, y)).unwrap().symbol())) {
                cols += 1;
            }
            // Nada en los bordes del plano.
            for y in [1u16, 6] {
                assert!(
                    !is_point(buf.cell((x, y)).unwrap().symbol()),
                    "en silencio los bordes quedan vacíos (x={x}, y={y})"
                );
            }
        }
        assert!(cols > 0, "la línea base central visible");
        assert!(cols < 38, "pero espaciada, no sólida ({cols}/38)");
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
        // La franja EQ vive en `render_bars_only` (Now Playing), NUNCA dentro
        // del bloque del osciloscopio: con 6 filas interiores ocupa las DOS
        // inferiores (la base siempre tiene barra y las fuertes suben).
        let st = active_state(1.0, VisualPalette::fallback());
        let buf = drawing(&|f| render_bars_only(f, f.area(), &st));
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
    fn bars_only_expanded_fills_height_without_trace_points() {
        // Modo Now Playing: solo barras, ampliadas a todo el interior, sin
        // ningún punto `•` del osciloscopio.
        let st = active_state(1.0, VisualPalette::fallback());
        let buf = drawing(&|f| render_bars_only(f, f.area(), &st));
        assert!(
            buf.content().iter().all(|c| c.symbol() != "•"),
            "ni un solo punto del osciloscopio en el modo barras"
        );
        // Con señal fuerte las barras llegan hasta la fila superior interior.
        let is_bar = |x: u16, y: u16| {
            let c = buf.cell((x, y)).unwrap();
            !c.symbol().trim().is_empty() && RAMP_TALL.contains(&c.symbol())
        };
        assert!(
            (1..39u16).all(|x| is_bar(x, 6)),
            "la base del EQ tiene barra en cada columna"
        );
        assert!(
            (1..39u16).any(|x| is_bar(x, 1)),
            "las barras ampliadas alcanzan la fila superior"
        );
        assert!(
            buf.content()
                .iter()
                .map(|c| c.symbol())
                .collect::<String>()
                .contains("Ecualizador"),
            "la banda se titula como ecualizador"
        );
    }

    #[test]
    fn bars_levels_scale_with_available_rows() {
        // Misma señal fuerte: a más filas interiores, más filas con barra
        // (la escalera crece desde la base sin saltos ni pánicos).
        let st = active_state(1.0, VisualPalette::fallback());
        let lit_rows_at = |h: u16| {
            let buf = drawing_size(40, h, &|f| render_bars_only(f, f.area(), &st));
            let inner_h = h.saturating_sub(2);
            (1..=inner_h)
                .filter(|&y| {
                    (1..39u16).any(|x| {
                        let c = buf.cell((x, y)).unwrap();
                        !c.symbol().trim().is_empty() && RAMP_TALL.contains(&c.symbol())
                    })
                })
                .count()
        };
        let small = lit_rows_at(5);
        let big = lit_rows_at(12);
        assert!(small >= 1, "con poco alto hay barras visibles");
        assert!(
            big > small,
            "más alto ⇒ más filas con barra ({small} → {big})"
        );
    }

    #[test]
    fn subdued_backdrop_is_flat_ceiling_for_lyrics() {
        // El modo aplacado (banda de letras/mensajes) es un plano EXACTO del
        // techo de contraste: sin moteado por columna que se lea como bloques
        // superpuestos al texto, y con el color contra el que se resolvió el
        // karaoke (contraste por construcción).
        let palette = VisualPalette::fallback();
        let st = active_state(0.9, palette);
        let dim = drawing(&|f| render_backdrop(f, f.area(), &st, true));
        let ceiling = palette.karaoke_bg_ceiling();
        let expected = Color::Rgb(ceiling[0], ceiling[1], ceiling[2]);
        assert!(
            dim.content().iter().all(|c| c.symbol() == " "),
            "sin glifos heredados ni puntos del trazo"
        );
        assert!(
            dim.content().iter().all(|c| c.bg == expected),
            "fondo plano del techo en TODA la banda (sin bandas de glow)"
        );
        // El modo vivo también es plano (fondo de la paleta): la señal la
        // llevan solo los puntos, nunca celdas tintadas que se lean como
        // bloques por encima o por debajo del trazo.
        let full = drawing(&|f| render_backdrop(f, f.area(), &st, false));
        let base = palette.background;
        assert!(
            full.content()
                .iter()
                .all(|c| c.bg == Color::Rgb(base[0], base[1], base[2])),
            "el modo vivo también es fondo plano de la paleta"
        );
    }

    #[test]
    fn backdrop_resets_symbols_and_sets_background() {
        let st = active_state(0.9, VisualPalette::fallback());
        let buf = drawing(&|f| render_backdrop(f, f.area(), &st, false));
        // La capa ambiental resetea los glifos a blanco (mata los fantasmas del
        // frame anterior: bloques de barras, puntos viejos) y deja un fondo
        // plano, así el texto superior (karaoke) o el trazo posterior parten
        // de una capa limpia.
        assert!(
            buf.content().iter().all(|c| c.symbol() == " "),
            "backdrop deja la capa en blanco (sin símbolos heredados)"
        );
    }

    #[test]
    fn visual_band_contains_zero_block_cells() {
        // Barrido completo del modo visual con señal fuerte: ningún símbolo de
        // bloque y ningún fondo tintado — todo lo que no sea cromado del marco
        // es punto del trazo o fondo plano de la paleta.
        const BLOCKS: [&str; 8] = ["█", "▇", "▆", "▅", "▄", "▃", "▂", "▁"];
        let st = active_state(0.9, VisualPalette::fallback());
        let buf = drawing(&|f| render(f, f.area(), &st, 0.0));
        let base = VisualPalette::fallback().background;
        let base_bg = Color::Rgb(base[0], base[1], base[2]);
        for (i, c) in buf.content().iter().enumerate() {
            let x = (i as u16) % 40;
            let y = (i as u16) / 40;
            let interior = x > 0 && x < 39 && y > 0 && y < 7;
            assert!(
                !BLOCKS.contains(&c.symbol()),
                "bloque en ({x},{y}): {:?}",
                c.symbol()
            );
            if interior {
                assert!(
                    is_point(c.symbol()) || c.symbol().trim().is_empty(),
                    "interior: solo puntos o vacío ({x},{y}={:?})",
                    c.symbol()
                );
                assert_eq!(c.bg, base_bg, "fondo plano tras los puntos ({x},{y})");
            }
        }
        assert!(
            buf.content().iter().any(|c| is_point(c.symbol())),
            "el trazo sigue visible (puntos presentes)"
        );
    }

    #[test]
    fn subdued_trace_uses_dimmed_palette_colors() {
        // El trazo tras las letras usa los colores de canal fundidos hacia el
        // techo (derivados de la portada), nunca a plena intensidad: visible
        // sin competir con el texto. El fondo no se toca. Constantes opuestas
        // para que cada punto conserve su color propio (sin mezcla).
        let st = plain_state(WaveformView {
            left: WaveformEnvelope::from_window(&[0.9; 2048]),
            right: WaveformEnvelope::from_window(&[-0.9; 2048]),
            gain: 1.0,
        });
        let backend = TestBackend::new(40, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_trace_subdued(f, f.area(), &st))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let palette = VisualPalette::fallback();
        let ceiling = palette.karaoke_bg_ceiling();
        let channels = palette.channel_colors();
        let dim_l = to_color(mix_c(channels.left, ceiling, SUBDUED_TRACE_DIM));
        let dim_r = to_color(mix_c(channels.right, ceiling, SUBDUED_TRACE_DIM));
        let full_l = to_color(channels.left);
        let full_r = to_color(channels.right);
        let mut dim_count = 0usize;
        for c in buf.content().iter() {
            if is_point(c.symbol()) {
                assert!(
                    c.fg == dim_l || c.fg == dim_r,
                    "punto atenuado con color fundido: {:?}",
                    c.fg
                );
                assert!(
                    c.fg != full_l && c.fg != full_r,
                    "nunca a plena intensidad tras las letras"
                );
                dim_count += 1;
            }
        }
        assert!(dim_count > 0, "puntos atenuados visibles tras las letras");
    }

    #[test]
    fn backdrop_clears_stale_glyphs_from_previous_frames() {
        // Simula el buffer reutilizado de la TUI real: celdas con bloques de
        // un frame anterior (barras/Gauge). Tras el backdrop no debe quedar
        // ningún bloque que luego "pisaría" los puntos del trazo.
        let st = active_state(0.9, VisualPalette::fallback());
        let backend = TestBackend::new(40, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                for y in 0..8u16 {
                    for x in 0..40u16 {
                        if let Some(cell) = f.buffer_mut().cell_mut(Position { x, y }) {
                            cell.set_symbol("█");
                        }
                    }
                }
                render_backdrop(f, f.area(), &st, false);
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        assert!(
            buf.content().iter().all(|c| c.symbol() == " "),
            "ni un solo bloque fantasma sobrevive al backdrop"
        );
    }

    #[test]
    fn bucket_column_mapping_covers_every_bucket_at_any_width() {
        // El mapeo buckets→columnas cubre sin huecos en ambas direcciones, en
        // terminales estrechas y anchas: cada columna toca ≥1 bucket y cada
        // bucket llega a ≥1 columna (el thinning posterior decide qué puntos
        // se emiten, nunca el mapeo).
        for w in [1usize, 17, 38, 100, 128, 200, WAVEFORM_BUCKETS + 40] {
            let mut bucket_hit = [false; WAVEFORM_BUCKETS];
            for col in 0..w {
                let (lo, hi) = column_bucket_range(w, col);
                assert!(hi > lo, "columna {col} no vacía con ancho {w}");
                assert!(hi <= WAVEFORM_BUCKETS, "rango acotado con ancho {w}");
                bucket_hit[lo..hi].fill(true);
            }
            assert!(
                bucket_hit.iter().all(|h| *h),
                "todos los buckets llegan a alguna columna con ancho {w}"
            );
        }
    }

    /// Estado mínimo sin brillo para asserts de color puros (sin el
    /// blanqueado del 12% que aplica `brightness`).
    fn plain_state(waveform: WaveformView) -> VisualState {
        let mut st = active_state(0.9, VisualPalette::fallback());
        st.scene.waveform = waveform;
        st.scene.energy = 0.0;
        st.scene.brightness = 0.0;
        st
    }

    #[test]
    fn projection_fixes_endpoints_sign_and_order() {
        // La proyección es transparente en lo esencial: 0→0, ±1→±1, impar,
        // monótona y acotada a [-1, 1] (nunca expande picos ni crea valores
        // fuera de rango).
        assert_eq!(project_amplitude(0.0), 0.0);
        assert!((project_amplitude(1.0) - 1.0).abs() < 1e-6);
        assert!((project_amplitude(-1.0) + 1.0).abs() < 1e-6);
        let mut prev = f32::NEG_INFINITY;
        let mut v = -1.0f32;
        while v <= 1.0 {
            let p = project_amplitude(v);
            assert!((-1.0..=1.0).contains(&p), "acotada: {p}");
            assert!(p > prev, "monótona en {v}");
            assert_eq!(p.signum(), v.signum(), "conserva el signo en {v}");
            prev = p;
            v += 0.05;
        }
        // Fuera de rango se clampéa antes de proyectar.
        assert_eq!(project_amplitude(99.0), project_amplitude(1.0));
        assert_eq!(project_amplitude(-99.0), project_amplitude(-1.0));
    }

    #[test]
    fn projection_expands_mid_levels_without_moving_peaks() {
        // Niveles típicos (0.2–0.5) ganan desviación vertical real; los picos
        // (±1) quedan fijos: la forma se abre sin recortar extremos.
        for v in [0.2f32, 0.3, 0.5] {
            assert!(
                project_amplitude(v) > v,
                "nivel medio {v} se expande: {}",
                project_amplitude(v)
            );
            assert!(project_amplitude(-v) < -v, "simétrico en negativo para {v}");
        }
        assert!(
            (project_amplitude(0.3) - 0.59).abs() < 0.02,
            "0.3 ⇒ ~0.59: {}",
            project_amplitude(0.3)
        );
    }

    #[test]
    fn moderate_sine_spans_plane_rows() {
        // Regresión del aplastamiento: un seno de 0.3 (nivel musical típico)
        // ocupa el plano en vez de colapsar al centro (área alta para dar
        // resolución vertical: 14 filas).
        let sine: Vec<f32> = (0..2048)
            .map(|i| 0.3 * ((i as f32 / 2048.0) * std::f32::consts::TAU * 6.0).sin())
            .collect();
        let env = WaveformEnvelope::from_window(&sine);
        let st = plain_state(WaveformView {
            left: env,
            right: WaveformEnvelope::silent(),
            gain: 1.0,
        });
        let buf = drawing_size(40, 14, &|f| {
            render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode))
        });
        let mut rows = std::collections::HashSet::new();
        for x in 0..40u16 {
            for y in 0..14u16 {
                if is_point(buf.cell((x, y)).unwrap().symbol()) {
                    rows.insert(y);
                }
            }
        }
        assert!(rows.len() >= 5, "el seno 0.3 barre el plano: {rows:?}");
    }

    #[test]
    fn stereo_channels_keep_independent_dynamics() {
        // L con seno 0.7 y R con cuadrada 0.4 sobre el eje compartido: cada
        // canal dibuja SU forma con desviación vertical real (sin espejo);
        // donde coinciden se ve la mezcla.
        let sine: Vec<f32> = (0..1024)
            .map(|i| 0.7 * ((i as f32 / 1024.0) * std::f32::consts::TAU * 5.0).sin())
            .collect();
        let square: Vec<f32> = (0..1024)
            .map(|i| if i % 2 == 0 { 0.4 } else { -0.4 })
            .collect();
        let st = plain_state(WaveformView {
            left: WaveformEnvelope::from_window(&sine),
            right: WaveformEnvelope::from_window(&square),
            gain: 1.0,
        });
        let buf =
            drawing(&|f| render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode)));
        let palette = VisualPalette::fallback();
        let channels = palette.channel_colors();
        let left_color = to_color(channels.left);
        let right_color = to_color(channels.right);
        let mut l_rows = std::collections::HashSet::new();
        let mut r_rows = std::collections::HashSet::new();
        for x in 0..40u16 {
            for y in 0..8u16 {
                let c = buf.cell((x, y)).unwrap();
                if is_point(c.symbol()) {
                    if c.fg == left_color {
                        l_rows.insert(y);
                    }
                    if c.fg == right_color {
                        r_rows.insert(y);
                    }
                }
            }
        }
        assert!(l_rows.len() >= 4, "L oscila con amplitud real: {l_rows:?}");
        assert!(!r_rows.is_empty(), "R muestra sus extremos: {r_rows:?}");
        assert_ne!(
            l_rows, r_rows,
            "canales independientes, no espejo: L={l_rows:?} R={r_rows:?}"
        );
    }

    #[test]
    fn amplitude_zero_maps_to_center_positive_up_negative_down() {
        // Mapping PCM → coordenada vertical (`row_of`): el cero cae al centro,
        // lo positivo arriba y lo negativo abajo. Es la normalización que
        // mantiene la amplitud relativa de cada canal.
        let h = 8usize;
        let center = h as f32 * 0.5;
        let scale = (center * 0.92).max(1.0);
        let middle = row_of(0.0, center, scale, h);
        assert!(
            middle == (h / 2) as u16 || middle == (h / 2) as u16 - 1,
            "amplitud 0 → centro del canal: {middle}"
        );
        let up = row_of(0.8, center, scale, h);
        let down = row_of(-0.8, center, scale, h);
        assert!(up < middle, "+0.8 va a la parte superior: {up} < {middle}");
        assert!(
            down > middle,
            "-0.8 va a la parte inferior: {down} > {middle}"
        );
    }

    #[test]
    fn normalization_minus_one_zero_plus_one_spans_vertical_range() {
        // -1.0 / 0.0 / 1.0 cubren todo el rango vertical sin salirse.
        let h = 10usize;
        let center = h as f32 * 0.5;
        let scale = (center * 0.92).max(1.0);
        let top = row_of(1.0, center, scale, h);
        let mid = row_of(0.0, center, scale, h);
        let bottom = row_of(-1.0, center, scale, h);
        assert_eq!(top, 0, "+1.0 llega a la fila superior");
        assert_eq!(bottom, (h - 1) as u16, "-1.0 llega a la fila inferior");
        assert!(
            top < mid && mid < bottom,
            "orden vertical monótono: {top} < {mid} < {bottom}"
        );
        // Fuera de rango se clampéa, nunca pánica ni sale del área.
        assert_eq!(row_of(99.0, center, scale, h), 0);
        assert_eq!(row_of(-99.0, center, scale, h), (h - 1) as u16);
    }

    #[test]
    fn trace_never_fills_vertical_interval_between_min_and_max() {
        // Garantía Scatter: con onda cuadrada ±0.9 superpuesta, el plano pinta
        // SOLO los dos extremos discretos (arriba y abajo); las filas
        // intermedias quedan vacías (sin `draw_line`, sin `fill_rect`). Además
        // el thinning deja columnas enteras vacías: no hay línea continua.
        let sq = square_stereo(0.9).left;
        let st = plain_state(WaveformView {
            left: sq,
            right: sq,
            gain: 1.0,
        });
        let buf =
            drawing(&|f| render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode)));
        // Extremos y=0..1 / y=6..7 con medio y=3..5 vacío donde hay extremos.
        let mut checked = 0usize;
        for x in 0..40u16 {
            let top = (0..2u16).any(|y| is_point(buf.cell((x, y)).unwrap().symbol()));
            let bottom = (6..8u16).any(|y| is_point(buf.cell((x, y)).unwrap().symbol()));
            if top && bottom {
                for y in 3..5u16 {
                    assert!(
                        !is_point(buf.cell((x, y)).unwrap().symbol()),
                        "columna {x}: el intervalo no se rellena (fila {y} vacía)"
                    );
                }
                checked += 1;
            }
        }
        assert!(checked > 0, "hay columnas con pico+valle para verificar");
        let empty_cols = (0..40u16)
            .filter(|&x| (0..8u16).all(|y| !is_point(buf.cell((x, y)).unwrap().symbol())))
            .count();
        assert!(
            empty_cols > 0,
            "el thinning deja columnas vacías ({empty_cols})"
        );
    }

    #[test]
    fn trace_uses_only_point_glyphs_never_blocks_or_lines() {
        // El trazo es Scatter puro: en el área del osciloscopio solo aparecen
        // `•` (Unicode) o `*` (ASCII). Ni bloques de la rampa EQ
        // (`█▁▂▃▄▅▆▇`), ni el círculo pesado `●`, ni líneas — la onda nunca se
        // construye con rectángulos ni segmentos.
        const BLOCKS: [&str; 9] = ["█", "▇", "▆", "▅", "▄", "▃", "▂", "▁", "●"];
        for theme in [GlyphTheme::Unicode, GlyphTheme::Ascii] {
            let glyphs = UiGlyphs::new(theme);
            let st = plain_state(view(0.7, 2));
            let buf = drawing(&|f| render_trace_points(f, f.area(), &st, glyphs));
            for c in buf.content().iter() {
                let s = c.symbol();
                if s.trim().is_empty() {
                    continue;
                }
                assert!(
                    is_point(s),
                    "símbolo inesperado «{s}» en el trazo (tema {theme:?}): solo puntos"
                );
                assert!(
                    !BLOCKS.contains(&s),
                    "bloque «{s}» en el trazo: la onda no usa bloques"
                );
            }
        }
    }

    #[test]
    fn column_downsampling_preserves_peaks_at_terminal_width() {
        // Reducción a resolución de terminal: un transitorio en la ventana
        // debe seguir visible tras `from_window` + fold por columna (no se
        // promedia hasta desaparecer).
        let mut window = [0.0f32; 2048];
        window[500] = 0.95;
        window[1500] = -0.95;
        let env = WaveformEnvelope::from_window(&window);
        // El fold de `channel_column_span` sobre un ancho típico (38 cols)
        // conserva el extremo: alguna columna lo contiene.
        let w = 38usize;
        let mut seen_peak = false;
        let mut seen_valley = false;
        for col in 0..w {
            let (mn, mx) = channel_column_span(&env, w, col);
            if (mx - 0.95).abs() < 1e-6 {
                seen_peak = true;
            }
            if (mn + 0.95).abs() < 1e-6 {
                seen_valley = true;
            }
        }
        assert!(seen_peak, "el pico sobrevive al fold por columna");
        assert!(seen_valley, "el valle sobrevive al fold por columna");
    }

    #[test]
    fn stereo_render_never_swaps_left_and_right_colors() {
        // L fuerte con seno / R silente sobre el plano compartido: los
        // extremos (lejos del eje) solo llevan color L; la línea base central
        // lleva el color R de su silencio. Ningún punto extremo es R.
        let samples_l: Vec<f32> = (0..1024)
            .map(|i| 0.9 * ((i as f32 / 1024.0) * std::f32::consts::TAU * 5.0).sin())
            .collect();
        let left = WaveformEnvelope::from_window(&samples_l);
        let st = plain_state(WaveformView {
            left,
            right: WaveformEnvelope::silent(),
            gain: 1.0,
        });
        let buf =
            drawing(&|f| render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode)));
        let channels = st.scene.palette.channel_colors();
        let left_color = to_color(channels.left);
        let right_color = to_color(channels.right);
        let mut extreme_left = 0usize;
        let mut extreme_right = 0usize;
        let mut center_right = 0usize;
        for x in 0..40u16 {
            for y in [0u16, 1, 6, 7] {
                let c = buf.cell((x, y)).unwrap();
                if is_point(c.symbol()) {
                    if c.fg == left_color {
                        extreme_left += 1;
                    }
                    if c.fg == right_color {
                        extreme_right += 1;
                    }
                }
            }
            for y in 3..5u16 {
                let c = buf.cell((x, y)).unwrap();
                if is_point(c.symbol()) && c.fg == right_color {
                    center_right += 1;
                }
            }
        }
        assert!(extreme_left > 0, "L alcanza los extremos con su color");
        assert_eq!(
            extreme_right, 0,
            "R silente nunca pinta los extremos (sin swap)"
        );
        assert!(center_right > 0, "R marca su base en el eje central");
    }

    #[test]
    fn constant_signal_keeps_amplitude_without_solid_bar() {
        // Señales constantes +0.5 (L) y -0.5 (R) sobre el eje compartido: cada
        // una cae en UNA sola fila a su lado del eje (se conserva la amplitud
        // con signo) pero espaciada (sin barra sólida).
        // Prueba sobre la representación de display, no sobre el glifo.
        let st = plain_state(WaveformView {
            left: WaveformEnvelope::from_window(&[0.5; 2048]),
            right: WaveformEnvelope::from_window(&[-0.5; 2048]),
            gain: 1.0,
        });
        let buf =
            drawing(&|f| render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode)));
        let mut rows: Vec<u16> = Vec::new();
        for x in 0..40u16 {
            for y in 0..8u16 {
                if is_point(buf.cell((x, y)).unwrap().symbol()) {
                    rows.push(y);
                }
            }
        }
        assert!(!rows.is_empty(), "constantes visibles");
        let distinct: std::collections::HashSet<u16> = rows.iter().copied().collect();
        assert_eq!(
            distinct.len(),
            2,
            "dos filas (una por amplitud con signo): {distinct:?}"
        );
        assert!(
            distinct.iter().any(|&y| y < 4) && distinct.iter().any(|&y| y >= 4),
            "+0.5 arriba del eje y -0.5 abajo: {distinct:?}"
        );
        let cols = (0..40u16)
            .filter(|&x| (0..8u16).any(|y| is_point(buf.cell((x, y)).unwrap().symbol())))
            .count();
        assert!(cols < 40, "espaciadas, no sólidas ({cols}/40)");
    }

    #[test]
    fn alternating_samples_are_not_averaged_to_center() {
        // Muestras alternas ±0.9 (variación máxima): el downsampling NO debe
        // promediarlas al centro; los extremos sobreviven en cada carril.
        let alt: Vec<f32> = (0..2048)
            .map(|i| if i % 2 == 0 { 0.9 } else { -0.9 })
            .collect();
        let env = WaveformEnvelope::from_window(&alt);
        let st = plain_state(WaveformView {
            left: env,
            right: env,
            gain: 1.0,
        });
        let buf =
            drawing(&|f| render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode)));
        // Extremos del plano tocados, sin colapsar al centro: hay puntos fuera
        // de la banda central (y=3..5).
        assert!(
            (0..40u16).any(|x| (0..3u16).any(|y| is_point(buf.cell((x, y)).unwrap().symbol()))),
            "el pico alterno llega arriba"
        );
        assert!(
            (0..40u16).any(|x| (5..8u16).any(|y| is_point(buf.cell((x, y)).unwrap().symbol()))),
            "el valle alterno llega abajo del plano"
        );
        let center_only = (0..40u16).all(|x| {
            (0..8u16)
                .all(|y| !is_point(buf.cell((x, y)).unwrap().symbol()) || (3..5u16).contains(&y))
        });
        assert!(!center_only, "no colapsa al centro: hay extremos");
    }

    #[test]
    fn slow_sine_spans_plane_rows_with_gaps() {
        // Baja frecuencia (1 ciclo en la ventana): forma reconocible que barre
        // varias filas del plano Y deja columnas vacías (scatter, no línea).
        let slow: Vec<f32> = (0..2048)
            .map(|i| 0.9 * ((i as f32 / 2048.0) * std::f32::consts::TAU).sin())
            .collect();
        let env = WaveformEnvelope::from_window(&slow);
        let st = plain_state(WaveformView {
            left: env,
            right: WaveformEnvelope::silent(),
            gain: 1.0,
        });
        let buf =
            drawing(&|f| render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode)));
        let mut distinct = std::collections::HashSet::new();
        for x in 0..40u16 {
            for y in 0..8u16 {
                if is_point(buf.cell((x, y)).unwrap().symbol()) {
                    distinct.insert(y);
                }
            }
        }
        assert!(
            distinct.len() >= 4,
            "el seno lento barre filas del plano: {distinct:?}"
        );
        let mut empty = 0usize;
        for x in 0..40u16 {
            let mut any = false;
            for y in 0..8u16 {
                if is_point(buf.cell((x, y)).unwrap().symbol()) {
                    any = true;
                    break;
                }
            }
            if !any {
                empty += 1;
            }
        }
        assert!(
            empty > 0,
            "con huecos entre puntos ({empty} columnas vacías)"
        );
    }

    #[test]
    fn active_signal_is_denser_than_flat_signal() {
        // La densidad sigue al contenido: un seno fuerte emite muchos más
        // puntos que una constante (que queda espaciada), sin que ninguno de
        // los dos llene todas las columnas como una línea sólida.
        let sine: Vec<f32> = (0..2048)
            .map(|i| 0.9 * ((i as f32 / 2048.0) * std::f32::consts::TAU * 6.0).sin())
            .collect();
        let env = WaveformEnvelope::from_window(&sine);
        let count = |left: WaveformEnvelope, right: WaveformEnvelope| {
            let st = plain_state(WaveformView {
                left,
                right,
                gain: 1.0,
            });
            let buf = drawing(&|f| {
                render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode))
            });
            buf.content()
                .iter()
                .filter(|c| is_point(c.symbol()))
                .count()
        };
        let active = count(env, WaveformEnvelope::silent());
        let flat = count(
            WaveformEnvelope::from_window(&[0.5; 2048]),
            WaveformEnvelope::silent(),
        );
        assert!(
            active > flat * 3 / 2,
            "señal activa ({active}) más densa que plana ({flat})"
        );
    }

    #[test]
    fn steep_step_paints_dim_link_cells_between() {
        // Escalón (+0.9 → -0.9 a mitad de ventana): la columna del salto une
        // ambos extremos con puntos de enlace TENUES (mismo glifo, color
        // fundido), sin tocar el fondo y sin extenderse a más columnas.
        let mut step = [0.9f32; 2048];
        for v in step.iter_mut().skip(1000) {
            *v = -0.9;
        }
        let env = WaveformEnvelope::from_window(&step);
        let st = plain_state(WaveformView {
            left: env,
            right: WaveformEnvelope::silent(),
            gain: 1.0,
        });
        let backend = TestBackend::new(40, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                // Composición real: fondo plano + trazo (el enlace nunca debe
                // teñir el fondo, solo su propio glifo).
                render_backdrop(f, f.area(), &st, false);
                render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode));
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let palette = VisualPalette::fallback();
        let channels = palette.channel_colors();
        let link = to_color(mix_c(channels.left, palette.background, LINK_DIM));
        // Filas extremas globales del trazo L (arriba y abajo del plano).
        let mut top = u16::MAX;
        let mut bottom = 0u16;
        for y in 0..8u16 {
            for x in 0..40u16 {
                if is_point(buf.cell((x, y)).unwrap().symbol()) {
                    top = top.min(y);
                    bottom = bottom.max(y);
                }
            }
        }
        assert!(top < bottom, "el escalón toca ambos extremos");
        // Celdas de enlace: color fundido, estrictamente entre extremos.
        let links: Vec<(u16, u16)> = (0..40u16)
            .flat_map(|x| (0..8u16).map(move |y| (x, y)))
            .filter(|&(x, y)| {
                let c = buf.cell((x, y)).unwrap();
                is_point(c.symbol()) && c.fg == link
            })
            .collect();
        assert!(!links.is_empty(), "el salto pinta enlaces tenues");
        assert!(
            links.iter().all(|&(_, y)| y > top && y < bottom),
            "enlaces solo entre extremos: {links:?} en ({top},{bottom})"
        );
        // El fondo sigue intacto bajo trazo y enlaces (puntos, no bloques).
        let base = palette.background;
        for (x, y) in links {
            assert_eq!(
                buf.cell((x, y)).unwrap().bg,
                Color::Rgb(base[0], base[1], base[2]),
                "el enlace no tiñe el fondo ({x},{y})"
            );
        }
    }

    #[test]
    fn mono_envelopes_overlap_with_mixed_color() {
        // Comportamiento mono del proyecto (L=R duplicado): ambas curvas caen
        // en las mismas celdas del plano y se ven con el tinte de mezcla.
        // Ningún punto mono conserva un color puro de canal.
        let samples: Vec<f32> = (0..1024)
            .map(|i| 0.7 * ((i as f32 / 1024.0) * std::f32::consts::TAU * 3.0).sin())
            .collect();
        let env = WaveformEnvelope::from_window(&samples);
        let st = plain_state(WaveformView {
            left: env,
            right: env,
            gain: 1.0,
        });
        let buf =
            drawing(&|f| render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode)));
        let palette = VisualPalette::fallback();
        let channels = palette.channel_colors();
        let pure_l = to_color(channels.left);
        let pure_r = to_color(channels.right);
        let both = to_color(mix_c(channels.left, channels.right, 0.5));
        let mut mixed = 0usize;
        let mut pure = 0usize;
        for c in buf.content().iter() {
            if is_point(c.symbol()) {
                if c.fg == both {
                    mixed += 1;
                }
                if c.fg == pure_l || c.fg == pure_r {
                    pure += 1;
                }
            }
        }
        assert!(mixed > 0, "coincidencia visible con mezcla");
        assert_eq!(pure, 0, "mono idéntico: ningún punto con color puro");
    }

    #[test]
    fn point_density_adapts_to_terminal_width() {
        // Resize: la densidad se adapta al ancho; más ancho ⇒ más puntos pero
        // de forma acotada, y con huecos en ambos tamaños.
        let st = plain_state(view(0.8, 1));
        let count = |w: u16, h: u16| {
            let buf = drawing_size(w, h, &|f| {
                render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode))
            });
            let pts = buf
                .content()
                .iter()
                .filter(|c| is_point(c.symbol()))
                .count();
            let empty_cols = (0..w)
                .filter(|&x| (0..h).all(|y| !is_point(buf.cell((x, y)).unwrap().symbol())))
                .count();
            (pts, empty_cols)
        };
        let (p40, e40) = count(40, 8);
        let (p100, e100) = count(100, 12);
        assert!(p40 > 0 && p100 > 0, "puntos en ambos anchos");
        assert!(e40 > 0 && e100 > 0, "huecos en ambos anchos");
        assert!(p100 >= p40, "más ancho no pierde puntos ({p40} → {p100})");
        assert!(
            p100 <= 3 * p40,
            "crecimiento acotado ante 2.5× ancho ({p40} → {p100})"
        );
    }
}
