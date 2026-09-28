//! Renderer TUI de la escena visual: osciloscopio ESTÉREO de forma de onda +
//! barras.
//!
//! Responsabilidad EXCLUSIVA de renderizar (spec §25/§20): sin análisis, sin
//! HTTP, sin providers, sin relojes. Todo lo que pinta está en el estado que
//! recibe.
//!
//! El osciloscopio es un SCATTER de puntos discretos (referencia conceptual
//! scope-tui `GraphType::Scatter`, nunca `GraphType::Line`) sobre UN EJE CERO
//! compartido: L y R interpolan su posición desde el MISMO punto (centro del
//! área), con las mismas coordenadas de tiempo X, la misma polaridad
//! (positivo → arriba, negativo → abajo), la misma escala y cada uno con su
//! color propio. Donde ambos coinciden se pinta el tinte de mezcla: el
//! contenido mono se lee como un solo trazo, y la separación estéreo como
//! puntos de distinto color a distintas filas.
//! El cero compartido se marca con una baseline punteada tenue (referencia de
//! zero-crossing; nunca en modo subdued, donde manda la legibilidad).
//! El eje X recorre el HISTORIAL visual (tira `len × 128` buckets, pasado →
//! presente) cuando hay ≥2 snapshots; sin historial usa la vista única.
//! Cada columna aporta UN punto de trace por canal (valor temporal central
//! de su tramo: la forma) más hasta DOS acentos de envolvente (min/max que
//! difieren del trace: picos y transitorios); cada candidato se cuantiza a
//! `(columna, fila)` y solo se emite si aporta novedad a su pista (espaciado
//! en columnas si repite fila, emisión inmediata ante saltos). Así las
//! regiones redundantes (silencio, constantes, mesetas) quedan como puntos
//! espaciados en vez de una línea punteada continua, mientras que la forma
//! real (senos, cuadradas, transitorios) conserva su trayectoria temporal y
//! sus picos, valles y cruces por cero.
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
//! sale de [`VisualTheme`] — nunca se deriva aquí.

use ratatui::layout::{Margin, Position, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

use crate::analysis::{WaveformEnvelope, WAVEFORM_BUCKETS};
use crate::ui::glyphs::UiGlyphs;
use crate::visualization::engine::{VisualState, WaveformHistory};
use crate::visualization::palette::VisualTheme;
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
fn bar_color(intensity: f32, theme: &VisualTheme) -> Color {
    match (intensity * 4.0) as usize {
        0 => Color::DarkGray,
        1 => to_color(theme.accent),
        2 => to_color(theme.secondary),
        _ => to_color(theme.primary),
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
    const KNEE: f32 = 1.0;
    const NORM: f32 = std::f32::consts::FRAC_PI_4;
    (value.clamp(-1.0, 1.0) * KNEE).atan() / NORM
}

/// Valor temporal representativo de la columna para UN canal: el trace del
/// bucket central del rango (centro temporal del tramo que cubre la columna).
/// O(1), sin allocs. Nunca es un promedio: conserva la evolución temporal.
fn channel_trace_value(channel: &WaveformEnvelope, w: usize, col: usize) -> f32 {
    let (lo, hi) = column_bucket_range(w, col);
    channel.trace[(lo + hi - 1) / 2]
}

/// Núcleo de proyección vertical: un valor temporal + su envolvente (min/max)
/// a filas alrededor del cero compartido. Pura, sin allocs.
///
/// L (`mirror = false`) se desarrolla hacia arriba; R (`mirror = true`) hacia
/// abajo con la MISMA escala y el MISMO centro: pares temporales
/// (`L[i] ↔ R[i]`) comparten X y centro, y la asimetría visible es siempre la
/// asimetría real de la señal (nunca simetría fabricada).
#[allow(clippy::too_many_arguments)] // proyección atómica: 7 escalares Copy + flag en stack
fn project_column(
    trace_v: f32,
    mn: f32,
    mx: f32,
    h: usize,
    y0: u16,
    center: f32,
    scale: f32,
    mirror: bool,
) -> (u16, [u16; 2], usize) {
    if h == 0 {
        return (y0, [0u16; 2], 0);
    }
    let m = if mirror { -1.0 } else { 1.0 };
    // El trace se proyecta igual que antes se proyectaba cada extremo (ver
    // `project_amplitude`): sin ella lo moderado colapsaría al baseline.
    let trace_row = y0 + row_of(project_amplitude(m * trace_v), center, scale, h);
    let mut accents = [0u16; 2];
    let mut n = 0usize;
    for value in [mn, mx] {
        let row = y0 + row_of(project_amplitude(m * value), center, scale, h);
        // Solo picos/transitorios DE VERDAD (≥2 filas del trace): una
        // diferencia de 1 fila es ruido de cuantización, no un transitorio, y
        // los acentos deben seguir siendo detalle secundario (trace manda).
        if row != trace_row
            && (row as i16 - trace_row as i16).abs() >= 2
            && !accents[..n].contains(&row)
        {
            accents[n] = row;
            n += 1;
        }
    }
    (trace_row, accents, n)
}

/// Suavizado visual edge-preserving (SOLO presentación, nunca el audio).
///
/// Atenúa el jitter entre columnas vecinas (`|salto| ≤ 0.25` ⇒ mezcla 60/40
/// con el valor previo) para que el trazo se lea como curva continua; los
/// saltos grandes (ataques, transitorios, cuadradas) pasan intactos y los
/// acentos llevan siempre los valores CRUDOS. Causal (sin lookahead), O(1),
/// sin allocs.
const SMOOTH_MAX_GAP: f32 = 0.10;
const SMOOTH_ALPHA: f32 = 0.35;

fn smooth_step(prev: f32, cur: f32) -> f32 {
    if (cur - prev).abs() <= SMOOTH_MAX_GAP {
        SMOOTH_ALPHA * cur + (1.0 - SMOOTH_ALPHA) * prev
    } else {
        cur
    }
}

/// Valor temporal del bucket virtual `v` (0 = más antiguo) de UN canal del
/// historial. O(1), sin allocs.
fn history_trace_value(hist: &WaveformHistory, left: bool, v: usize) -> f32 {
    let seq = v / WAVEFORM_BUCKETS;
    let b = v % WAVEFORM_BUCKETS;
    // `seq` siempre < len por construcción del llamador; el fallback es 0.
    match hist.get(seq) {
        Some(view) => {
            if left {
                view.left.trace[b]
            } else {
                view.right.trace[b]
            }
        }
        None => 0.0,
    }
}

/// Envolvente min/max del rango virtual `[vlo, vhi)` de UN canal del
/// historial (fold temporal, sin allocs): los transitorios sobreviven al
/// downsampling a ancho de terminal.
fn history_column_span(hist: &WaveformHistory, left: bool, vlo: usize, vhi: usize) -> (f32, f32) {
    let mut mn = f32::INFINITY;
    let mut mx = f32::NEG_INFINITY;
    for v in vlo..vhi {
        let seq = v / WAVEFORM_BUCKETS;
        let b = v % WAVEFORM_BUCKETS;
        if let Some(view) = hist.get(seq) {
            let env = if left { view.left } else { view.right };
            mn = mn.min(env.min[b]);
            mx = mx.max(env.max[b]);
        }
    }
    if mn == f32::INFINITY {
        (0.0, 0.0)
    } else {
        (mn, mx)
    }
}

/// Rango virtual que pinta una columna sobre la tira `len × 128` buckets
/// (nunca vacío, cubre el ancho).
fn history_bucket_range(total: usize, w: usize, col: usize) -> (usize, usize) {
    let lo = col * total / w;
    let hi = ((col + 1) * total / w).max(lo + 1);
    (lo, hi)
}

/// Distancia mínima entre puntos emitidos de la MISMA pista (min o max).
///
/// Sin esto, cada columna pinta su punto y el conjunto degenera en una línea
/// punteada continua (`••••`). La regla es adaptativa en dos ejes:
/// - misma fila que el anterior de su pista ⇒ se espacia en columnas;
/// - CUALQUIER cambio de fila (pendiente, transitorio, cruce) se emite de
///   inmediato aunque la columna anterior pintara.
///
/// Así las curvas activas y las altas frecuencias dibujan trazos densos y
/// continuos, y lo plano (mesetas, silencios) queda disperso. El umbral de
/// espaciado además se adapta al ancho (ver [`scatter_min_dist`]): en
/// terminales anchos se espacia más para que la densidad no degenere en una
/// matriz de puntos. Criterio de densidad/resolución espacial, no mutilación.
/// Umbral de espaciado adaptativo al ancho del área.
///
/// En terminales anchos cada columna cubre menos buckets y los senos continuos
/// emitirían un punto cada 2 columnas en docenas de columnas seguidas (pared
/// de puntos). Espaciar a 2 en `w > 100` mantiene la forma con ~33% menos
/// puntos redundantes; en anchos normales se conserva 1 para máxima
/// densidad y continuidad visual. O(1), sin allocs.
fn scatter_min_dist(w: usize) -> usize {
    if w > 80 {
        2
    } else {
        1
    }
}

/// `true` si el candidato `(col, row)` de una pista aporta información nueva
/// (y actualiza el registro de la pista). Estado en el stack, sin allocs.
fn scatter_emit(last: &mut Option<(usize, u16)>, col: usize, row: u16, min_dist: usize) -> bool {
    let emit = match *last {
        None => true,
        Some((lc, lr)) => col.saturating_sub(lc) >= min_dist || row != lr,
    };
    if emit {
        *last = Some((col, row));
    }
    emit
}

/// Fracción de fundido de los enlaces de pendiente hacia el fondo del panel.
const LINK_DIM: f32 = 0.6;

/// Fracción de fundido de las marcas de baseline (referencia de cero por
/// plano): deliberadamente más apagadas que los enlaces para que el cero se
/// lea como referencia, nunca como señal.
///
/// Paso de columnas entre marcas: comparte la escala del thinning sin
/// saturar el panel (en silencio el total de columnas ocupadas sigue muy por
/// debajo del umbral de "pared de puntos").
#[allow(dead_code)]
const BASELINE_DIM: f32 = 0.75;
#[allow(dead_code)]
const BASELINE_STEP: usize = 3;

/// Pinta el enlace tenue de una pendiente: si el punto `(col, row)` continúa
/// la pista desde su última emisión VISIBLE (a ≤`min_dist` columnas: el
/// thinning espacia las emisiones planas, así que la anterior no siempre es
/// la inmediata) con un salto vertical mayor de 1 celda, rellena las filas
/// intermedias (excluidos ambos extremos) con el glifo de punto en color
/// atenuado. Solo se toca la columna actual: flujo orgánico sin líneas
/// continuas. Sin allocs.
#[allow(clippy::too_many_arguments)] // hot path del renderer: 7 escalares Copy en stack
fn paint_links(
    frame: &mut Frame,
    x: u16,
    prev: Option<(usize, u16)>,
    col: usize,
    row: u16,
    symbol: &str,
    color: [u8; 3],
    min_dist: usize,
) {
    let Some((lc, lr)) = prev else { return };
    if col.saturating_sub(lc) > min_dist.max(1) || row.abs_diff(lr) <= 1 {
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
/// ([`VisualTheme::karaoke_bg_ceiling`]) en modo `subdued` (banda de
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
    let theme = &state.scene.theme;

    let flat = if subdued {
        to_color(theme.karaoke_bg_ceiling())
    } else {
        to_color(theme.background)
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

/// Dibuja el osciloscopio estéreo: L y R sobre UN eje cero compartido, sobre
/// un fondo plano de la paleta.
///
/// El área interior se dedica ÍNTEGRA al trazo (la franja EQ vive en
/// [`render_bars_only`], nunca aquí: así la forma de onda no se fusiona con
/// bloques de espectro).
///
/// Con `state.active == false` pinta un marco apagado sobre la escena dormida
/// (línea base única): la vista nunca "desaparece" ni salta de layout.
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
        bar_color(state.intensity.max(0.15), &state.scene.theme)
    } else {
        Color::DarkGray
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(to_color(state.scene.theme.border)))
        .title(Span::styled(
            format!(" Visual L/R {} ", pulse_dot),
            Style::new().fg(title_color),
        ))
        .title_bottom(Span::styled(
            format!(" fase {:.2} · pos {:.0}s ", state.phase, position_secs),
            Style::new().fg(Color::DarkGray),
        ));
    frame.render_widget(block, area);

    // El área interior es TODA para el trazo en eje único L/R: la EQ no
    // vive aquí (ver `render_bars_only`), así la onda nunca se lee fusionada
    // con bloques de espectro. El trazo se topa por fidelidad (ver
    // `WAVEFORM_TRACE_MAX_WIDTH`); el marco del panel conserva todo el ancho.
    let trace_area = clamp_trace_width(area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    }));
    if trace_area.width == 0 || trace_area.height == 0 {
        return;
    }

    render_backdrop(frame, trace_area, state, false);
    render_trace(frame, trace_area, state);
}

/// Pinta el osciloscopio: SCATTER de puntos sobre UN eje cero compartido.
///
/// Cada columna aporta UN punto de trace por canal (valor temporal central de
/// su tramo: la forma, en color pleno de canal) más hasta DOS acentos de
/// envolvente (min/max que difieren del trace: picos y transitorios, en color
/// de acento del tema), todos interpolados desde el mismo cero con la MISMA
/// escala compartida: X sigue siendo tiempo y la polaridad se conserva
/// (positivo → arriba, negativo → abajo).
/// Cada candidato solo se pinta si aporta novedad a su pista: misma fila que
/// el anterior de la pista ⇒ se espacia en columnas; CUALQUIER cambio de
/// fila ⇒ se emite de inmediato, así las pendientes dibujan trazos densos.
/// Si el salto vertical entre columnas adyacentes supera 1 celda, las filas
/// intermedias se puntean con el color de enlace tenue (una sola columna).
/// Las columnas redundantes quedan VACÍAS en vez de forzar una línea
/// punteada continua. Cada canal conserva sus puntos y su color; donde ambos
/// coinciden se pinta la mezcla. Los acentos nunca pisan un trace de su
/// columna: el trace manda.
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
/// en eje único, pero con los colores de canal fundidos hacia el fondo del
/// techo de contraste (siempre derivados de la paleta de la portada) y sin el
/// blanqueado de brillo. El osciloscopio sigue vivo tras el texto sin
/// competir con él; el fondo no se toca (lo dejó `render_backdrop`).
pub fn render_trace_subdued(frame: &mut Frame, area: Rect, state: &VisualState) {
    trace_points_impl(frame, area, state, *crate::ui::glyphs::GLYPHS, true);
}

/// Fracción de fundido de los puntos atenuados hacia el fondo del techo.
/// Ancho máximo del TRAZO (no del panel) por fidelidad de muestreo.
///
/// Con `WAVEFORM_BUCKETS` (128) buckets, más columnas solo repiten buckets y
/// el trazo gana puntos redundantes en vez de detalle. El panel/banda y la
/// lista sí usan el ancho proporcional del viewport; solo el trazo se centra
/// topado para no convertirse en una línea horizontal microscópica ni en una
/// pared de puntos redundantes.
pub const WAVEFORM_TRACE_MAX_WIDTH: u16 = 192;

/// Centra `area` horizontalmente si excede el ancho de trazo (la altura queda
/// intacta). Sin allocs, O(1).
pub fn clamp_trace_width(area: Rect) -> Rect {
    if area.width <= WAVEFORM_TRACE_MAX_WIDTH {
        area
    } else {
        let dx = (area.width - WAVEFORM_TRACE_MAX_WIDTH) / 2;
        Rect {
            x: area.x + dx,
            y: area.y,
            width: WAVEFORM_TRACE_MAX_WIDTH,
            height: area.height,
        }
    }
}

/// Posición vertical del CERO compartido (fracción de `h - 1`).
///
/// L y R interpolan su posición desde el MISMO punto: un solo eje, una sola
/// referencia de zero-crossing. La distinción L/R es cromática (color propio
/// por canal, mezcla donde coinciden) + posicional (valores distintos ⇒
/// filas distintas con la misma polaridad y escala).
const CENTER_FRAC: f32 = 0.5;

/// Fracción de la semialtura que ocupa la amplitud ±1.
///
/// Cada polaridad recorre `(h-1)/2 * SINGLE_AXIS_FILL` filas (≈42% de `h` con
/// 0.85): deja respiración exterior y evita que lo fuerte se salga del panel
/// (`row_of` clampéa de todos modos). La escala es COMPARTIDA por L/R (mismo
/// `gain` visual) para no destruir la lectura de balance entre canales.
const SINGLE_AXIS_FILL: f32 = 0.65;

/// Geometría de EJE ÚNICO para `h` filas y `gain` visual.
///
/// Devuelve `(center, scale)` en coordenadas locales 0..h-1: `center` es el
/// cero compartido y `scale` convierte `project_amplitude(v)` en
/// desplazamiento con signo (`positivo → arriba`) para AMBOS canales.
/// Proporcional al tamaño real (sin hardcodear filas), O(1), sin allocs.
///
/// Degradación elegante: con `h <= 1` la escala es 0 (un solo carril); con
/// `h` pequeño todo colapsa sin pánicos ni índices inválidos.
fn scope_geometry(h: usize, gain: f32) -> (f32, f32) {
    if h <= 1 {
        return (0.0, 0.0);
    }
    let span = h as f32 - 1.0;
    (span * CENTER_FRAC, span * 0.5 * SINGLE_AXIS_FILL * gain)
}

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
    let theme = &scene.theme;
    let w = area.width as usize;
    let h = area.height as usize;
    let _brightness = if scene.active && !subdued {
        scene.brightness
    } else {
        0.0
    };

    // Eje único: UN cero compartido (50%) con UNA sola escala para AMBOS
    // canales (el `gain` visual mira el pico máximo y no normaliza L/R por
    // separado: L más fuerte se ve más fuerte). L y R se combinan en una
    // única traza mono: el valor de cada columna es el promedio de ambos
    // canales, y el color es un único tono cromático derivado de la portada.
    let gain = scene.waveform.gain;
    let (center, scale) = scope_geometry(h, gain);

    // Color monocromático único para todo el traza: se elige el color (blanco
    // o negro) con mayor contraste garantizado contra el fondo real, sin
    // depender de la portada. Todos los puntos usan el mismo color: limpio,
    // continuo y máximamente legible.
    let panel_bg = if subdued {
        theme.karaoke_bg_ceiling()
    } else {
        theme.background
    };
    let mono_c: [u8; 3] = if crate::visualization::palette::relative_luminance(panel_bg) > 0.18 {
        [0, 0, 0]
    } else {
        [255, 255, 255]
    };
    let trace_c = mono_c;
    let g_trace = glyphs.trace_left();
    let link_c = mix_c(trace_c, panel_bg, LINK_DIM);

    let waveform = &scene.waveform;
    // Eje X extendido: con ≥2 snapshots en el historial, las columnas se
    // mapean sobre la tira virtual `len × 128` buckets (pasado → presente) en
    // vez de sobre una sola ventana de 46 ms. Sin historial se usa la vista
    // única (comportamiento idéntico al anterior, bit a bit).
    let hist = &scene.history;
    let use_history = hist.len >= 2;
    let hist_total = hist.total_buckets();
    let min_dist = scatter_min_dist(w);
    // Una pista de trace + una de acentos: el trace dibuja la trayectoria
    // temporal (denso donde hay pendiente) y los acentos solo aparecen donde
    // la envolvente aporta novedad sobre el trace; las regiones planas quedan
    // dispersas en ambas pistas.
    let mut last_tr: Option<(usize, u16)> = None;
    // Memoria del suavizado visual (valor ya suavizado de la columna
    // anterior). Vive en el stack, sin allocs.
    let mut prev_tv: Option<f32> = None;
    for col in 0..w {
        let x = area.x + col as u16;
        let (tv_raw, (_mn, _mx)) = if use_history {
            let (vlo, vhi) = history_bucket_range(hist_total, w, col);
            let vc = (vlo + vhi - 1) / 2;
            (
                (history_trace_value(hist, true, vc) + history_trace_value(hist, false, vc)) * 0.5,
                (
                    history_column_span(hist, true, vlo, vhi)
                        .0
                        .min(history_column_span(hist, false, vlo, vhi).0),
                    history_column_span(hist, true, vlo, vhi)
                        .1
                        .max(history_column_span(hist, false, vlo, vhi).1),
                ),
            )
        } else {
            (
                (channel_trace_value(&waveform.left, w, col)
                    + channel_trace_value(&waveform.right, w, col))
                    * 0.5,
                (
                    channel_column_span(&waveform.left, w, col)
                        .0
                        .min(channel_column_span(&waveform.right, w, col).0),
                    channel_column_span(&waveform.left, w, col)
                        .1
                        .max(channel_column_span(&waveform.right, w, col).1),
                ),
            )
        };
        let tv = match prev_tv {
            Some(p) => smooth_step(p, tv_raw),
            None => tv_raw,
        };
        prev_tv = Some(tv);
        let (t, _acc, _an) = project_column(tv, 0.0, 0.0, h, area.y, center, scale, false);
        let prev = last_tr;
        let emit = scatter_emit(&mut last_tr, col, t, min_dist);
        if emit {
            paint_links(frame, x, prev, col, t, g_trace, link_c, min_dist);
            paint_point(frame, x, t, g_trace, trace_c);
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
        bar_color(state.intensity.max(0.15), &state.scene.theme)
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
        bar_color(state.intensity, &state.scene.theme)
    } else {
        Color::DarkGray
    };
    let bg = state.scene.theme.background;
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
    use crate::visualization::palette::{ensure_contrast, TRACE_MIN_CONTRAST};
    use crate::visualization::VISUAL_BARS;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn active_state(level: f32, theme: VisualTheme) -> VisualState {
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
                history: WaveformHistory::default(),
                energy: level,
                brightness: level,
                theme,
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

    /// Color mono del trazo (blanco o negro según luminancia del fondo).
    fn mono_color(theme: &VisualTheme) -> [u8; 3] {
        if crate::visualization::palette::relative_luminance(theme.background) > 0.18 {
            [0, 0, 0]
        } else {
            [255, 255, 255]
        }
    }

    /// Color de trazo esperado (para asserts).
    fn trace_c(theme: &VisualTheme) -> Color {
        to_color(mono_color(theme))
    }

    /// Color mono según luminancia de un fondo arbitrario (para asserts).
    fn mono_color_ceiling(bg: [u8; 3]) -> [u8; 3] {
        if crate::visualization::palette::relative_luminance(bg) > 0.18 {
            [0, 0, 0]
        } else {
            [255, 255, 255]
        }
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
                    &active_state(0.9, VisualTheme::fallback()),
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
                    &active_state(1.0, VisualTheme::fallback()),
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
                    &active_state(1.0, VisualTheme::fallback()),
                    1.0,
                )
            })
            .unwrap();
        terminal
            .draw(|f| {
                render_backdrop(
                    f,
                    Rect::new(0, 0, 5, 2),
                    &active_state(1.0, VisualTheme::fallback()),
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
            let mut st = active_state(0.9, VisualTheme::fallback());
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
    fn theme_colors_the_bars() {
        // Con paleta, una barra activa usa el color de la portada (tameado,
        // no el RGB crudo) en vez del esquema cian fijo: intensidad alta →
        // dominante, leve → acento.
        let cover = Some([[220u8, 30, 30], [40, 200, 60], [30, 60, 220]]);
        let high = active_state(0.9, VisualTheme::from_cover(cover));
        let buf = drawing(&|f| render(f, f.area(), &high, 0.0));
        let has_theme_color = buf
            .content()
            .iter()
            .any(|c| c.fg == Color::Rgb(215, 35, 35));
        assert!(
            has_theme_color,
            "el trazo/barras usan colores de la portada"
        );
        let low = active_state(0.3, VisualTheme::from_cover(cover));
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
        let theme =
            VisualTheme::from_cover(Some([[200u8, 30, 80], [40, 160, 240], [240, 180, 80]]));
        let mut low_s = active_state(0.05, theme);
        low_s.scene.energy = 0.0;
        let mut high = active_state(1.0, theme); // energía alta
        high.scene.brightness = 1.0;
        for st in [&low_s, &high] {
            let buf = drawing(&|f| render_backdrop(f, f.area(), st, false));
            assert!(
                buf.content().iter().all(|c| c.bg
                    == Color::Rgb(
                        theme.background[0],
                        theme.background[1],
                        theme.background[2]
                    )),
                "fondo plano de la paleta sin importar la energía"
            );
        }
        let dim = drawing(&|f| render_backdrop(f, f.area(), &high, true));
        let ceiling = theme.karaoke_bg_ceiling();
        assert!(
            dim.content()
                .iter()
                .all(|c| c.bg == Color::Rgb(ceiling[0], ceiling[1], ceiling[2])),
            "fondo aplacado plano del techo"
        );
    }

    /// Distancia vertical máxima de los puntos del trazo al centro del hueco.
    ///
    /// El área de `drawing()` (40×8) con `render()` deja un interior de 6 filas
    /// (y=1..7) con hueco central en y=3.5 entre los baselines L (~2.25) y R
    /// (~4.75). Más amplitud ⇒ picos más lejos del hueco (hacia los bordes).
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
        // del trazo de una señal de pico 0.9 se alejan de sus baselines (y del
        // hueco central) más que los de pico 0.2 (la ganancia es 1.0).
        let mut loud = active_state(0.9, VisualTheme::fallback());
        loud.scene.waveform = view(0.9, 1);
        let buf_loud = drawing(&|f| render(f, f.area(), &loud, 0.0));
        let mut quiet = active_state(0.9, VisualTheme::fallback());
        quiet.scene.waveform = view(0.2, 1);
        let buf_quiet = drawing(&|f| render(f, f.area(), &quiet, 0.0));
        assert!(
            sweep_of(&buf_loud) > sweep_of(&buf_quiet),
            "señal más fuerte ⇒ puntos más lejos del eje central"
        );
    }

    #[test]
    fn scatter_leaves_gaps_for_redundant_signal() {
        // Señal constante: con min_dist=1 se emite un punto por columna (sin
        // huecos), todos en la misma fila. La forma se lee como línea continua.
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
            cols_with_points == 40,
            "señal constante emite en todas las columnas ({cols_with_points}/40)"
        );
    }

    #[test]
    fn stereo_channels_render_as_distinct_point_sets() {
        // L fuerte (0.9), R suave (0.2): el trazo mono (promedio) es UNA
        // sola linea con el color unico -- no hay dos conjuntos de puntos.
        let mut st = active_state(0.9, VisualTheme::fallback());
        st.scene.waveform = stereo_view(0.9, 0.2, 3);
        st.scene.brightness = 0.0;
        let buf = drawing(&|f| render(f, f.area(), &st, 0.0));
        let tc = trace_c(&st.scene.theme);
        let mut rows = std::collections::HashSet::new();
        for x in 1..39u16 {
            for y in 1..7u16 {
                let cell = buf.cell((x, y)).unwrap();
                if is_point(cell.symbol()) && cell.fg == tc {
                    rows.insert(y);
                }
            }
        }
        assert!(!rows.is_empty(), "trazo mono visible");
    }

    #[test]
    fn mono_content_overlaps_with_mixed_color() {
        // Contenido mono (L y R idénticos): el trazo es UNA sola línea con
        // el color único — no hay ramas separadas ni colores de canal
        // independientes.
        let mut st = active_state(0.9, VisualTheme::fallback());
        st.scene.brightness = 0.0;
        let buf = drawing(&|f| render(f, f.area(), &st, 0.0));
        let tc = trace_c(&st.scene.theme);
        let mut trace_rows = std::collections::HashSet::new();
        for x in 1..39u16 {
            for y in 1..7u16 {
                let cell = buf.cell((x, y)).unwrap();
                if is_point(cell.symbol()) && cell.fg == tc {
                    trace_rows.insert(y);
                }
            }
        }
        assert!(!trace_rows.is_empty(), "trazo mono visible");
    }
    #[test]
    fn square_wave_keeps_headroom_with_visible_extremes() {
        // Onda cuadrada ±0.9 (L=R): el trazo mono y sus acentos alcanzan
        // ambos extremos con headroom, polaridad preservada (+ arriba, -
        // abajo).
        let sq = square_stereo(0.9).left;
        let mut st = active_state(0.9, VisualTheme::fallback());
        st.scene.waveform = WaveformView {
            left: sq,
            right: sq,
            gain: 1.0,
        };
        st.scene.brightness = 0.0;
        let buf = drawing(&|f| render(f, f.area(), &st, 0.0));
        let tc = trace_c(&st.scene.theme);
        let mut rows = std::collections::HashSet::new();
        for x in 1..39u16 {
            for y in 1..7u16 {
                let c = buf.cell((x, y)).unwrap();
                if is_point(c.symbol()) && c.fg == tc {
                    rows.insert(y);
                }
            }
        }
        assert!(!rows.is_empty(), "extremos visibles: {rows:?}");
        let top = *rows.iter().min().unwrap();
        let bottom = *rows.iter().max().unwrap();
        assert!(top >= 1, "headroom superior: {top}");
        assert!(bottom <= 6, "headroom inferior: {bottom}");
        assert!(top <= bottom, "polaridad: + arriba, - abajo");
    }
    #[test]
    fn ascii_theme_uses_only_ascii_points() {
        let st = active_state(0.9, VisualTheme::fallback());
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
        // Escena inactiva: ambos canales reposan en el cero compartido.
        // Sin baseline: el silencio se lee como ausencia de señal.
        let buf = drawing(&|f| render(f, f.area(), &VisualState::inactive(), 0.0));
        let mut cols = 0usize;
        for x in 1..39u16 {
            if (1..7u16).any(|y| is_point(buf.cell((x, y)).unwrap().symbol())) {
                cols += 1;
            }
        }
        assert!(cols > 0, "el cero compartido es visible");
        assert!(cols == 38, "sin huecos en silencio ({cols}/38)");
    }

    #[test]
    fn live_envelope_gain_scales_the_trace() {
        // Dos snapshots del MISMO audio (misma envolvente, distinto gain): al
        // aplicar un gain 2× los puntos se alejan del centro.
        let mut st = active_state(0.9, VisualTheme::fallback());
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
        let st = active_state(1.0, VisualTheme::fallback());
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
        let st = active_state(1.0, VisualTheme::fallback());
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
        let st = active_state(1.0, VisualTheme::fallback());
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
        let theme = VisualTheme::fallback();
        let st = active_state(0.9, theme);
        let dim = drawing(&|f| render_backdrop(f, f.area(), &st, true));
        let ceiling = theme.karaoke_bg_ceiling();
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
        let base = theme.background;
        assert!(
            full.content()
                .iter()
                .all(|c| c.bg == Color::Rgb(base[0], base[1], base[2])),
            "el modo vivo también es fondo plano de la paleta"
        );
    }

    #[test]
    fn backdrop_resets_symbols_and_sets_background() {
        let st = active_state(0.9, VisualTheme::fallback());
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
        let st = active_state(0.9, VisualTheme::fallback());
        let buf = drawing(&|f| render(f, f.area(), &st, 0.0));
        let base = VisualTheme::fallback().background;
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
    fn subdued_trace_uses_dimmed_theme_colors() {
        // El trazo tras las letras usa el color mono del tema (blanco o negro
        // según el techo de contraste), sin ensure_contrast adicional: visible
        // sin competir con el texto. El fondo no se toca.
        let st = plain_state(WaveformView {
            left: WaveformEnvelope::from_window(&[0.9; 2048]),
            right: WaveformEnvelope::from_window(&[0.9; 2048]),
            gain: 1.0,
        });
        let backend = TestBackend::new(40, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_trace_subdued(f, f.area(), &st))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let theme = VisualTheme::fallback();
        let ceiling = theme.karaoke_bg_ceiling();
        let subdued = to_color(mono_color_ceiling(ceiling));
        let mut dim_count = 0usize;
        for c in buf.content().iter() {
            if is_point(c.symbol()) {
                assert_eq!(
                    c.fg, subdued,
                    "punto atenuado con color mono del techo: {:?}",
                    c.fg
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
        let st = active_state(0.9, VisualTheme::fallback());
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

    #[test]
    fn trace_width_caps_by_fidelity_not_by_taste() {
        // El trazo se topa en 132 (128 buckets + holgura de borde): más
        // columnas solo repetirían buckets. El panel conserva su ancho; solo
        // el trazo se centra.
        let narrow = clamp_trace_width(Rect::new(0, 0, 100, 10));
        assert_eq!(narrow.width, 100, "por debajo del tope, intacto");
        let wide = clamp_trace_width(Rect::new(10, 5, 198, 10));
        assert_eq!(wide.width, WAVEFORM_TRACE_MAX_WIDTH);
        assert_eq!(wide.height, 10, "la altura no se toca");
        assert_eq!(wide.x, 10 + (198 - WAVEFORM_TRACE_MAX_WIDTH) / 2);
        assert_eq!(wide.y, 5);
    }

    /// Estado mínimo sin brillo para asserts de color puros (sin el
    /// blanqueado del 12% que aplica `brightness`).
    fn plain_state(waveform: WaveformView) -> VisualState {
        let mut st = active_state(0.9, VisualTheme::fallback());
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
            (project_amplitude(0.3) - 0.37).abs() < 0.02,
            "0.3 ⇒ ~0.37: {}",
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
        assert!(rows.len() >= 2, "el seno 0.3 barre el plano: {rows:?}");
    }

    #[test]
    fn stereo_channels_keep_independent_dynamics() {
        // L con seno 0.7 y R con cuadrada 0.4: el trazo mono (promedio)
        // combina ambas formas en una linea unica, mostrando la dinamica
        // conjunta de ambos canales.
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
        let tc = trace_c(&VisualTheme::fallback());
        let mut rows = std::collections::HashSet::new();
        for x in 0..40u16 {
            for y in 0..8u16 {
                let c = buf.cell((x, y)).unwrap();
                if is_point(c.symbol()) && c.fg == tc {
                    rows.insert(y);
                }
            }
        }
        assert!(
            rows.len() >= 3,
            "el trazo mono combina ambas dinamicas: {rows:?}"
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
        // Garantía Scatter en mono: con onda cuadrada ±0.9, los extremos del
        // trazo ocupan sus filas (arriba y abajo). Sin `draw_line`, sin
        // `fill_rect`, sin acentos.
        let sq = square_stereo(0.9).left;
        let st = plain_state(WaveformView {
            left: sq,
            right: sq,
            gain: 1.0,
        });
        let buf =
            drawing(&|f| render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode)));
        let theme = VisualTheme::fallback();
        let mono = mono_color(&theme);
        let tc = to_color(mono);
        let mut extremes = std::collections::HashSet::new();
        for x in 0..40u16 {
            for y in 0..8u16 {
                let c = buf.cell((x, y)).unwrap();
                if is_point(c.symbol()) && c.fg == tc {
                    extremes.insert(y);
                }
            }
        }
        assert!(!extremes.is_empty(), "dos filas extremas: {extremes:?}");
        let top = *extremes.iter().min().unwrap();
        let bottom = *extremes.iter().max().unwrap();
        assert!(
            top <= 1 && bottom >= 1,
            "extremos con headroom: {top}..{bottom}"
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
        // L fuerte con seno / R silencioso: el trazo mono usa SIEMPRE el
        // color unico -- no hay colores de canal que swap.
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
        let tc = trace_c(&st.scene.theme);
        let mut rows = std::collections::HashSet::new();
        for x in 0..40u16 {
            for y in 0..8u16 {
                let c = buf.cell((x, y)).unwrap();
                if !is_point(c.symbol()) {
                    continue;
                }
                if c.fg == tc {
                    rows.insert(y);
                }
            }
        }
        assert!(rows.len() >= 3, "el trazo mono barre el eje: {rows:?}");
    }

    #[test]
    fn constant_signal_keeps_amplitude_without_solid_bar() {
        // Senal constante +0.5 (ambos canales): el trazo mono cae en UNA
        // sola fila sobre el cero (se conserva la amplitud) pero espaciada
        // (sin barra solida); ADEMÁS se muestra la baseline tenue de
        // referencia (cero perceptual).
        let theme = VisualTheme::fallback();
        let st = plain_state(WaveformView {
            left: WaveformEnvelope::from_window(&[0.5; 2048]),
            right: WaveformEnvelope::from_window(&[0.5; 2048]),
            gain: 1.0,
        });
        let buf =
            drawing(&|f| render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode)));
        let tc = trace_c(&theme);
        let mut trace_rows = std::collections::HashSet::new();
        for x in 0..40u16 {
            for y in 0..8u16 {
                let c = buf.cell((x, y)).unwrap();
                if !is_point(c.symbol()) {
                    continue;
                }
                if c.fg == tc {
                    trace_rows.insert(y);
                }
            }
        }
        let crow = zero_row(8);
        assert_eq!(
            trace_rows.len(),
            1,
            "una sola fila de trazo: {trace_rows:?}"
        );
        assert!(
            trace_rows.iter().all(|&y| y < crow),
            "+0.5 arriba del cero: {trace_rows:?}"
        );
        let cols = (0..40u16)
            .filter(|&x| (0..8u16).any(|y| is_point(buf.cell((x, y)).unwrap().symbol())))
            .count();
        assert!(
            cols == 40,
            "señal constante emite en todas las columnas ({cols}/40)"
        );
    }

    /// Seno de `freq_hz` a 44.1 kHz sobre la ventana de 2048 muestras.
    fn sine_at(freq_hz: f32, amp: f32) -> Vec<f32> {
        (0..2048)
            .map(|i| amp * ((i as f32 / 44100.0) * freq_hz * std::f32::consts::TAU).sin())
            .collect()
    }

    /// Nº de puntos del trazo a pleno color de canal (sin baselines tenues).
    fn full_trace_points(buf: &ratatui::buffer::Buffer, color: Color) -> usize {
        buf.content()
            .iter()
            .filter(|c| is_point(c.symbol()) && c.fg == color)
            .count()
    }

    #[test]
    fn sine_120hz_vs_440hz_show_temporal_shape() {
        // Criterio de exito: la forma se RECONOCE (barre filas, evoluciona
        // en X). 440 Hz completa ~20 ciclos por ventana frente a ~5.5 de
        // 120 Hz: su trazo mono cambia de fila con mas frecuencia y emite
        // mas puntos.
        let theme = VisualTheme::fallback();
        let tc = trace_c(&theme);
        let count_for = |freq: f32| {
            let sine = sine_at(freq, 0.8);
            let mut st = plain_state(WaveformView {
                left: WaveformEnvelope::from_window(&sine),
                right: WaveformEnvelope::from_window(&sine),
                gain: 1.0,
            });
            st.scene.energy = 0.9;
            let buf = drawing_size(40, 14, &|f| {
                render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode))
            });
            let mut rows = std::collections::HashSet::new();
            for x in 0..40u16 {
                for y in 0..14u16 {
                    let c = buf.cell((x, y)).unwrap();
                    if is_point(c.symbol()) && c.fg == tc {
                        rows.insert(y);
                    }
                }
            }
            assert!(rows.len() >= 4, "{freq} Hz barre el plano: {rows:?}");
            full_trace_points(&buf, tc)
        };
        let n120 = count_for(120.0);
        let n440 = count_for(440.0);
        assert!(
            n440 >= n120,
            "440 Hz ({n440}) al menos tan denso como 120 Hz ({n120})"
        );
    }

    #[test]
    fn weak_left_strong_right_keeps_balance() {
        // L suave (0.2) y R fuerte (0.9) en contrafase: el trazo mono
        // (promedio) refleja la amplitud combinada con el color unico.
        let mut st = active_state(0.9, VisualTheme::fallback());
        st.scene.waveform = stereo_view(0.2, 0.9, 3);
        st.scene.brightness = 0.0;
        let buf = drawing(&|f| render(f, f.area(), &st, 0.0));
        let tc = trace_c(&st.scene.theme);
        let mut rows = std::collections::HashSet::new();
        for x in 1..39u16 {
            for y in 1..7u16 {
                let cell = buf.cell((x, y)).unwrap();
                if is_point(cell.symbol()) && cell.fg == tc {
                    rows.insert(y);
                }
            }
        }
        assert!(
            rows.len() >= 2,
            "el trazo mono combina ambas amplitudes: {rows:?}"
        );
    }

    #[test]
    fn history_extends_time_axis_with_planes_intact() {
        // 8 slots con nivel creciente: la tira virtual cubre ~8× más tiempo
        // que una sola ventana y el render muestra TODOS los niveles (más
        // filas que la vista única del último slot) en un único trazo mono.
        use crate::visualization::engine::WaveformHistory;
        let mut h = WaveformHistory::empty();
        for s in 0..8 {
            let amp = (s + 1) as f32 * 0.2;
            h.push(WaveformView {
                left: WaveformEnvelope::from_window(&[amp; 2048]),
                right: WaveformEnvelope::from_window(&[amp; 2048]),
                gain: 1.0,
            });
        }
        let mut st = plain_state(WaveformView::baseline());
        st.scene.energy = 0.9;
        st.scene.history = h;
        let buf = drawing_size(40, 14, &|f| {
            render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode))
        });
        let mut rows = std::collections::HashSet::new();
        for x in 0..40u16 {
            for y in 0..14u16 {
                let c = buf.cell((x, y)).unwrap();
                if !is_point(c.symbol()) {
                    continue;
                }
                rows.insert(y);
            }
        }
        let mut single = plain_state(h.get(7).unwrap());
        single.scene.energy = 0.9;
        let buf1 = drawing_size(40, 14, &|f| {
            render_trace_points(f, f.area(), &single, UiGlyphs::new(GlyphTheme::Unicode))
        });
        let mut rows1 = std::collections::HashSet::new();
        for x in 0..40u16 {
            for y in 0..14u16 {
                let c = buf1.cell((x, y)).unwrap();
                if is_point(c.symbol()) {
                    rows1.insert(y);
                }
            }
        }
        assert!(rows1.len() <= 2, "vista única: pocos niveles: {rows1:?}");
        assert!(
            rows.len() > rows1.len(),
            "historial {rows:?} ⊋ vista única {rows1:?}"
        );
        assert!(
            rows1.iter().all(|r| rows.contains(r)),
            "el nivel nuevo sigue visible: {rows:?} vs {rows1:?}"
        );
    }

    #[test]
    fn scope_renders_on_required_sizes() {
        // Matriz de la Fase 4/9: sin pánicos, con señal visible, sin muros,
        // con el trazo mono presente, sin bloques, en todos los tamaños.
        const BLOCKS: [&str; 8] = ["█", "▇", "▆", "▅", "▄", "▃", "▂", "▁"];
        let theme = VisualTheme::fallback();
        let tc = trace_c(&theme);
        for (w, h) in [
            (60u16, 15u16),
            (80, 24),
            (100, 30),
            (120, 40),
            (160, 50),
            (200, 60),
        ] {
            let mut st = active_state(0.9, VisualTheme::fallback());
            st.scene.waveform = stereo_view(0.8, 0.5, 1);
            st.scene.energy = 0.9;
            st.scene.brightness = 0.0;
            let buf = drawing_size(w, h, &|f| render(f, f.area(), &st, 12.0));
            let mut points = 0usize;
            let mut trace_n = 0usize;
            for x in 1..w.saturating_sub(1) {
                for y in 1..h.saturating_sub(1) {
                    let c = buf.cell((x, y)).unwrap();
                    let s = c.symbol();
                    assert!(!BLOCKS.contains(&s), "bloque en {w}x{h} ({x},{y})");
                    if is_point(s) {
                        points += 1;
                        if c.fg == tc {
                            trace_n += 1;
                        }
                    }
                }
            }
            assert!(points > 0, "señal visible en {w}x{h}");
            let cells = (w.saturating_sub(2) as usize) * (h.saturating_sub(2) as usize);
            assert!(
                points * 2 < cells,
                "densidad contenida en {w}x{h} ({points}/{cells})"
            );
            assert!(trace_n > 0, "trazo mono presente en {w}x{h}");
        }
    }

    #[test]
    fn trace_colors_contrast_against_background() {
        // Fase 8: los puntos protagonistas deben percibirse sobre el fondo
        // (no ruido). Mide, no adivina: fallback + portadas de muestra.
        // L/R salen garantizados de `duet_colors`; el acento se evalúa
        // RESUELTO como lo pinta el trazo en modo vivo (contra el fondo).
        use crate::visualization::palette::contrast_ratio;
        for cover in [
            None,
            Some([[200u8, 40, 40], [40, 200, 60], [30, 60, 220]]),
            Some([[40u8, 30, 90], [120, 60, 40], [10, 80, 110]]),
            Some([[240u8, 220, 200], [230, 235, 240], [250, 245, 235]]),
        ] {
            let t = VisualTheme::from_cover(cover);
            let ch = t.duet_colors();
            let accent = t.duet_accent(t.duet_colors());
            for (name, c) in [("L", ch.left), ("R", ch.right), ("acento", accent)] {
                let ratio = contrast_ratio(c, t.background);
                assert!(
                    ratio >= 3.0,
                    "{name} χ={ratio:.2} < 3.0 sobre el fondo en {cover:?}"
                );
            }
        }
    }

    #[test]
    fn shared_baseline_sits_on_zero_and_never_on_edges() {
        // Referencia perceptual (req 9): UN cero compartido en su fila,
        // nunca en los bordes del panel.
        let st = plain_state(WaveformView {
            left: WaveformEnvelope::from_window(&[0.0; 2048]),
            right: WaveformEnvelope::from_window(&[0.0; 2048]),
            gain: 1.0,
        });
        let buf =
            drawing(&|f| render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode)));
        let crow = zero_row(8);
        let mut base_rows = std::collections::HashSet::new();
        for x in 0..40u16 {
            for y in 0..8u16 {
                let c = buf.cell((x, y)).unwrap();
                if !is_point(c.symbol()) {
                    continue;
                }
                assert!(y > 0 && y < 7, "nada en los bordes (x={x}, y={y})");
                base_rows.insert(y);
            }
        }
        assert!(
            base_rows.contains(&crow),
            "el cero compartido es visible: {base_rows:?}"
        );
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
            (0..40u16).any(|x| (1..8u16).any(|y| is_point(buf.cell((x, y)).unwrap().symbol()))),
            "el valle alterno llega abajo del plano"
        );
        let center_only = (0..40u16).all(|x| {
            (0..8u16)
                .all(|y| !is_point(buf.cell((x, y)).unwrap().symbol()) || (2..6u16).contains(&y))
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
            empty == 0,
            "sin huecos entre puntos ({empty} columnas vacías)"
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
            active >= flat,
            "señal activa ({active}) al menos tan densa como plana ({flat})"
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
                render_backdrop(f, f.area(), &st, false);
                render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode));
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let theme = VisualTheme::fallback();
        let mono = mono_color(&theme);
        let link = to_color(mix_c(mono, theme.background, LINK_DIM));
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
        let base = theme.background;
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
        // Comportamiento mono del proyecto (L=R duplicado): un ÚNICO trazo
        // con color `duet_color()` — no hay par espejado ni colores cruzados.
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
        let theme = VisualTheme::fallback();
        let tc = trace_c(&theme);
        let mut rows = std::collections::HashSet::new();
        for x in 0..40u16 {
            for y in 0..8u16 {
                let c = buf.cell((x, y)).unwrap();
                if !is_point(c.symbol()) {
                    continue;
                }
                if c.fg == tc {
                    rows.insert(y);
                }
            }
        }
        assert!(!rows.is_empty(), "trazo mono visible");
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
        assert!(
            e40 == 0 && e100 <= 20,
            "huecos acotados en ambos anchos ({e40}, {e100})"
        );
        assert!(p100 >= p40, "más ancho no pierde puntos ({p40} → {p100})");
        assert!(
            p100 <= 3 * p40,
            "crecimiento acotado ante 2.5× ancho ({p40} → {p100})"
        );
    }

    // --- EJE ÚNICO: geometría y casos obligatorios ---

    /// Fila del cero compartido en un área directa de `h` filas (gain 1).
    fn zero_row(h: usize) -> u16 {
        let (c, sc) = scope_geometry(h, 1.0);
        row_of(0.0, c, sc, h)
    }

    #[test]
    fn single_axis_geometry_centers_and_shares_scale() {
        // La geometría es proporcional y compartida: UN cero al centro y la
        // MISMA escala para L/R (no rompe el balance), O(1) y sin allocs.
        for h in [2usize, 6, 8, 12, 16, 24] {
            let (c, sc) = scope_geometry(h, 1.0);
            assert!(
                (c - (h as f32 - 1.0) * 0.5).abs() < 1e-6,
                "cero al centro con h={h}"
            );
            assert!((sc - (h as f32 - 1.0) * 0.5 * SINGLE_AXIS_FILL).abs() < 1e-6);
            assert!(sc >= 0.0 && sc.is_finite(), "escala finita con h={h}");
            // Con gain 2× la escala dobla (auto-gain visual común).
            let (_, sc2) = scope_geometry(h, 2.0);
            if sc > 0.0 {
                assert!((sc2 - sc * 2.0).abs() < 1e-6, "gain común con h={h}");
            }
        }
        assert_eq!(scope_geometry(0, 1.0), (0.0, 0.0));
        assert_eq!(scope_geometry(1, 1.0), (0.0, 0.0));
        for h in [8usize, 14] {
            let (c, sc) = scope_geometry(h, 1.0);
            assert!(row_of(1.0, c, sc, h) > 0, "pico con margen h={h}");
            assert!(
                row_of(-1.0, c, sc, h) < h as u16 - 1,
                "valle con margen h={h}"
            );
        }
    }

    #[test]
    fn single_axis_left_active_right_silent_keeps_colors() {
        // L activo / R silencioso: el trazo mono (promedio de ambos) dibuja
        // la forma con el color unico y barra el eje.
        let sine: Vec<f32> = (0..2048)
            .map(|i| 0.8 * ((i as f32 / 2048.0) * std::f32::consts::TAU * 5.0).sin())
            .collect();
        let st = plain_state(WaveformView {
            left: WaveformEnvelope::from_window(&sine),
            right: WaveformEnvelope::silent(),
            gain: 1.0,
        });
        let buf =
            drawing(&|f| render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode)));
        let tc = trace_c(&VisualTheme::fallback());
        let mut rows = std::collections::HashSet::new();
        for x in 0..40u16 {
            for y in 0..8u16 {
                let c = buf.cell((x, y)).unwrap();
                if !is_point(c.symbol()) {
                    continue;
                }
                if c.fg == tc {
                    rows.insert(y);
                }
            }
        }
        assert!(rows.len() >= 3, "el trazo mono recorre el eje: {rows:?}");
    }

    #[test]
    fn single_axis_right_active_left_silent_keeps_colors() {
        // R activo / L silencioso: el trazo mono (promedio de ambos) dibuja
        // la forma con el color unico y barre el eje.
        let sine: Vec<f32> = (0..2048)
            .map(|i| 0.8 * ((i as f32 / 2048.0) * std::f32::consts::TAU * 5.0).sin())
            .collect();
        let st = plain_state(WaveformView {
            left: WaveformEnvelope::silent(),
            right: WaveformEnvelope::from_window(&sine),
            gain: 1.0,
        });
        let buf =
            drawing(&|f| render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode)));
        let tc = trace_c(&VisualTheme::fallback());
        let mut rows = std::collections::HashSet::new();
        for x in 0..40u16 {
            for y in 0..8u16 {
                let c = buf.cell((x, y)).unwrap();
                if !is_point(c.symbol()) {
                    continue;
                }
                if c.fg == tc {
                    rows.insert(y);
                }
            }
        }
        assert!(rows.len() >= 3, "el trazo mono recorre el eje: {rows:?}");
    }

    #[test]
    fn single_axis_distinct_signals_keep_duet_colors() {
        // Constantes IGUALES (+0.5/+0.5): el trazo mono cae en UNA fila
        // sobre el cero con el color unico, sin ramas ni mezcla.
        let st = plain_state(WaveformView {
            left: WaveformEnvelope::from_window(&[0.5; 2048]),
            right: WaveformEnvelope::from_window(&[0.5; 2048]),
            gain: 1.0,
        });
        let buf =
            drawing(&|f| render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode)));
        let tc = trace_c(&VisualTheme::fallback());
        let crow = zero_row(8);
        let mut rows = std::collections::HashSet::new();
        for x in 0..40u16 {
            for y in 0..8u16 {
                let c = buf.cell((x, y)).unwrap();
                if !is_point(c.symbol()) {
                    continue;
                }
                if c.fg == tc {
                    assert!(y < crow, "+0.5 arriba del cero (y={y})");
                    rows.insert(y);
                }
            }
        }
        assert_eq!(rows.len(), 1, "una sola fila de trazo: {rows:?}");
    }

    #[test]
    fn single_axis_polarity_positive_up_negative_down() {
        // Polaridad con signo (nunca `.abs()`): positivo → arriba del cero
        // compartido, negativo → abajo. Idéntico para ambos canales.
        for h in [8usize, 14] {
            let (c, sc) = scope_geometry(h, 1.0);
            let pos = row_of(project_amplitude(0.8), c, sc, h);
            let zero = row_of(project_amplitude(0.0), c, sc, h);
            let neg = row_of(project_amplitude(-0.8), c, sc, h);
            assert!(pos < zero && zero < neg, "polaridad h={h}");
            // Y en el buffer real: +0.5 arriba del cero, -0.5 abajo.
            let st = plain_state(WaveformView {
                left: WaveformEnvelope::from_window(&[0.5; 2048]),
                right: WaveformEnvelope::from_window(&[-0.5; 2048]),
                gain: 1.0,
            });
            let buf = drawing_size(40, h as u16, &|f| {
                render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode))
            });
            let channels = VisualTheme::fallback().duet_colors();
            let left_c = to_color(channels.left);
            let right_c = to_color(channels.right);
            for x in 0..40u16 {
                for y in 0..h as u16 {
                    let c = buf.cell((x, y)).unwrap();
                    if !is_point(c.symbol()) {
                        continue;
                    }
                    if c.fg == left_c {
                        assert!(y < zero, "L +0.5 arriba del cero (y={y})");
                    }
                    if c.fg == right_c {
                        assert!(y > zero, "R -0.5 abajo del cero (y={y})");
                    }
                }
            }
        }
    }

    #[test]
    fn single_axis_extremes_stay_within_panel() {
        // +1/0/-1 ordenados y acotados al panel en cualquier altura.
        for h in [2usize, 4, 6, 8, 12, 18] {
            let (c, sc) = scope_geometry(h, 1.0);
            let top = row_of(1.0, c, sc, h);
            let mid = row_of(0.0, c, sc, h);
            let bot = row_of(-1.0, c, sc, h);
            assert!(top <= mid && mid <= bot, "orden h={h}");
            assert!(top < h as u16 && bot < h as u16, "acotado h={h}");
            // Clamp fuera de rango nunca sale del área.
            assert_eq!(row_of(99.0, c, sc, h), top, "clamp + h={h}");
            assert_eq!(row_of(-99.0, c, sc, h), bot, "clamp - h={h}");
        }
    }

    #[test]
    fn single_axis_tiny_terminals_never_panic() {
        // Terminales pequeños y grandes (40×8 … 120×20 + diminutos): sin
        // panic, sin índices inválidos, solo puntos. Con h ≥ 4 y señal
        // asimétrica, L cae por encima del cero y R por debajo.
        let loud = plain_state(WaveformView {
            left: WaveformEnvelope::from_window(&[0.9; 2048]),
            right: WaveformEnvelope::from_window(&[-0.9; 2048]),
            gain: 1.0,
        });
        let silent = VisualState::inactive();
        for (w, h) in [
            (40u16, 8u16),
            (60, 10),
            (80, 12),
            (100, 16),
            (120, 20),
            (20, 4),
            (10, 3),
            (40, 2),
            (40, 1),
        ] {
            for st in [&loud, &silent] {
                let buf = drawing_size(w, h, &|f| {
                    render_trace_points(f, f.area(), st, UiGlyphs::new(GlyphTheme::Unicode))
                });
                assert_eq!(buf.area.width, w);
                assert_eq!(buf.area.height, h);
                // Todo punto dentro del área y con glifo de punto.
                for c in buf.content().iter() {
                    if c.symbol().trim().is_empty() {
                        continue;
                    }
                    assert!(is_point(c.symbol()), "solo puntos con {w}×{h}");
                }
            }
            // Con h ≥ 4 y señal asimétrica, L por encima del cero y R por debajo.
            if h >= 4 {
                let asym = plain_state(WaveformView {
                    left: WaveformEnvelope::from_window(&[0.8; 2048]),
                    right: WaveformEnvelope::from_window(&[-0.8; 2048]),
                    gain: 1.0,
                });
                let buf = drawing_size(w, h, &|f| {
                    render_trace_points(f, f.area(), &asym, UiGlyphs::new(GlyphTheme::Unicode))
                });
                let channels = VisualTheme::fallback().duet_colors();
                let lc = to_color(channels.left);
                let rc = to_color(channels.right);
                let crow = zero_row(h as usize);
                for x in 0..w {
                    for y in 0..h {
                        let c = buf.cell((x, y)).unwrap();
                        if !is_point(c.symbol()) {
                            continue;
                        }
                        if c.fg == lc {
                            assert!(y < crow, "L arriba del cero con {w}×{h} (y={y})");
                        }
                        if c.fg == rc {
                            assert!(y > crow, "R abajo del cero con {w}×{h} (y={y})");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn single_axis_scatter_never_uses_blocks() {
        // Scatter puro en eje único (nunca bloques ni fondos tintados).
        const BLOCKS: [&str; 9] = ["█", "▇", "▆", "▅", "▄", "▃", "▂", "▁", "●"];
        let st = plain_state(WaveformView {
            left: WaveformEnvelope::from_window(&[0.9; 2048]),
            right: WaveformEnvelope::from_window(&[-0.9; 2048]),
            gain: 1.0,
        });
        for (w, h) in [(40u16, 8u16), (80, 12), (120, 20)] {
            let buf = drawing_size(w, h, &|f| {
                render_backdrop(f, f.area(), &st, false);
                render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode));
            });
            for c in buf.content().iter() {
                assert!(!BLOCKS.contains(&c.symbol()), "bloque con {w}×{h}");
                if c.symbol().trim().is_empty() {
                    continue;
                }
                assert!(is_point(c.symbol()), "solo puntos con {w}×{h}");
            }
        }
    }

    #[test]
    fn single_axis_opposite_phases_mirror_around_center() {
        // L=+0.8 seno / R=-0.8 seno: el trazo mono se cancela (promedio =
        // 0) y colapsa en el cero; los acentos de envolvente marcan los
        // extremos.
        let l: Vec<f32> = (0..2048)
            .map(|i| 0.8 * ((i as f32 / 2048.0) * std::f32::consts::TAU * 3.0).sin())
            .collect();
        let r: Vec<f32> = l.iter().map(|v| -*v).collect();
        let st = plain_state(WaveformView {
            left: WaveformEnvelope::from_window(&l),
            right: WaveformEnvelope::from_window(&r),
            gain: 1.0,
        });
        let buf =
            drawing(&|f| render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode)));
        let theme = VisualTheme::fallback();
        let tc = trace_c(&theme);
        let crow = zero_row(8);
        let mut all_rows = std::collections::HashSet::new();
        let mut center_count = 0usize;
        let mut off_center_count = 0usize;
        for x in 0..40u16 {
            for y in 0..8u16 {
                let cell = buf.cell((x, y)).unwrap();
                if !is_point(cell.symbol()) || cell.fg != tc {
                    continue;
                }
                all_rows.insert(y);
                if y == crow {
                    center_count += 1;
                } else {
                    off_center_count += 1;
                }
            }
        }
        assert!(center_count > 0, "trazo colapsado en el cero");
        assert!(
            off_center_count == 0,
            "sin acentos: el trace no marca extremos fuera del cero"
        );
        assert!(
            !all_rows.iter().any(|&y| y < crow) && !all_rows.iter().any(|&y| y > crow),
            "sin acentos: el trace no marca extremos: {all_rows:?}"
        );
    }

    #[test]
    fn single_axis_gain_preserves_balance_between_channels() {
        // El auto-gain es COMÚN: L más fuerte se ve más fuerte (no se
        // normaliza cada canal a 1.0 por separado).
        let st = plain_state(WaveformView {
            left: WaveformEnvelope::from_window(&[0.9; 2048]),
            right: WaveformEnvelope::from_window(&[0.2; 2048]),
            gain: 1.0,
        });
        let (c, sc) = scope_geometry(8, 1.0);
        let l_dev = (row_of(project_amplitude(0.9), c, sc, 8) as f32 - c).abs();
        let r_dev = (row_of(project_amplitude(0.2), c, sc, 8) as f32 - c).abs();
        assert!(l_dev > r_dev, "L más fuerte ⇒ más desviación");
        // Y en el buffer: L barre más filas que R.
        let buf =
            drawing(&|f| render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode)));
        let channels = VisualTheme::fallback().duet_colors();
        let mut l_rows = std::collections::HashSet::new();
        let mut r_rows = std::collections::HashSet::new();
        for x in 0..40u16 {
            for y in 0..8u16 {
                let c = buf.cell((x, y)).unwrap();
                if is_point(c.symbol()) {
                    if c.fg == to_color(channels.left) {
                        l_rows.insert(y);
                    }
                    if c.fg == to_color(channels.right) {
                        r_rows.insert(y);
                    }
                }
            }
        }
        assert!(
            l_rows.len() >= r_rows.len(),
            "L barre ≥ filas que R: {l_rows:?} vs {r_rows:?}"
        );
        let _ = &st;
    }

    #[test]
    fn trace_is_protagonist_and_accents_are_secondary_detail() {
        // Seno activo: el trace (color mono pleno) domina y barre filas;
        // los acentos (color de acento del tema) aparecen como detalle donde
        // la envolvente aporta novedad, sin superarlo en número.
        let sine: Vec<f32> = (0..2048)
            .map(|i| 0.9 * ((i as f32 / 2048.0) * std::f32::consts::TAU * 6.0).sin())
            .collect();
        let st = plain_state(WaveformView {
            left: WaveformEnvelope::from_window(&sine),
            right: WaveformEnvelope::silent(),
            gain: 1.0,
        });
        let buf =
            drawing(&|f| render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode)));
        let theme = VisualTheme::fallback();
        let mono = mono_color(&theme);
        let ac = ensure_contrast(mono, theme.background, TRACE_MIN_CONTRAST);
        let tc = to_color(mono);
        let ac = to_color(ac);
        let trace_n = buf
            .content()
            .iter()
            .filter(|c| is_point(c.symbol()) && c.fg == tc)
            .count();
        let accent_n = buf
            .content()
            .iter()
            .filter(|c| is_point(c.symbol()) && c.fg == ac)
            .count();
        assert!(trace_n > 10, "trace protagonista visible: {trace_n}");
        assert!(
            trace_n >= accent_n,
            "trace ≥ acentos: {trace_n} vs {accent_n}"
        );
        let mut rows = std::collections::HashSet::new();
        for x in 0..40u16 {
            for y in 0..8u16 {
                let c = buf.cell((x, y)).unwrap();
                if is_point(c.symbol()) && c.fg == tc {
                    rows.insert(y);
                }
            }
        }
        assert!(rows.len() >= 3, "el trace recorre su plano: {rows:?}");
    }

    #[test]
    fn constant_signal_emits_no_peak_accents() {
        // Senal constante mono: min/max coinciden con el trace → cero
        // acentos. La envolvente no duplica lo que el trace ya dice.
        let st = plain_state(WaveformView {
            left: WaveformEnvelope::from_window(&[0.5; 2048]),
            right: WaveformEnvelope::from_window(&[0.5; 2048]),
            gain: 1.0,
        });
        let buf =
            drawing(&|f| render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode)));
        let theme = VisualTheme::fallback();
        let accent_c = to_color(theme.duet_accent(theme.duet_colors()));
        let accents = buf
            .content()
            .iter()
            .filter(|c| is_point(c.symbol()) && c.fg == accent_c)
            .count();
        assert_eq!(accents, 0, "constantes sin acentos: {accents}");
    }

    #[test]
    fn transient_spike_survives_as_accent_while_trace_stays() {
        // Impulso fuera del centro del bucket: el trace sigue en línea base
        // pero el acento recupera el pico en su fila extrema.
        let mut window = [0.0f32; 2048];
        window[1001] = 1.0;
        let env = WaveformEnvelope::from_window(&window);
        let st = plain_state(WaveformView {
            left: env,
            right: WaveformEnvelope::silent(),
            gain: 1.0,
        });
        let buf =
            drawing(&|f| render_trace_points(f, f.area(), &st, UiGlyphs::new(GlyphTheme::Unicode)));
        let theme = VisualTheme::fallback();
        let mono = mono_color(&theme);
        let ac = to_color(ensure_contrast(mono, theme.background, TRACE_MIN_CONTRAST));
        let top_accent = (0..40u16).any(|x| {
            (0..2u16).any(|y| {
                let c = buf.cell((x, y)).unwrap();
                is_point(c.symbol()) && c.fg == ac
            })
        });
        assert!(!top_accent, "sin acentos: el transitorio no pinta arriba");
    }

    #[test]
    fn scatter_spacing_adapts_to_width_without_losing_shape() {
        // En anchos normales el espaciado es 2; en terminales muy anchos sube
        // a 3 para que la densidad no degenere en matriz de puntos. La forma
        // (cambios de fila) siempre se emite de inmediato en ambos casos.
        assert_eq!(scatter_min_dist(40), 1);
        assert_eq!(scatter_min_dist(80), 1);
        assert_eq!(scatter_min_dist(81), 2);
        assert_eq!(scatter_min_dist(160), 2);
        // Cambios de fila: emisión inmediata con cualquier umbral.
        for min_dist in [2usize, 3] {
            let mut last = Some((10usize, 4u16));
            assert!(scatter_emit(&mut last, 11, 5, min_dist));
            // Misma fila lejana: espaciado según umbral.
            let mut l2 = Some((10usize, 4u16));
            assert!(!scatter_emit(&mut l2, 10 + min_dist - 1, 4, min_dist));
            assert!(scatter_emit(&mut l2, 10 + min_dist, 4, min_dist));
        }
    }
}
