//! Miniatura renderizada con medio bloques (`▀`): cada celda de terminal
//! representa 2 píxeles verticales (arriba = foreground, abajo = background).
//!
//! Solo sabe pintar el estado que recibe: no hace HTTP, caché ni decodificación.

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::thumbnail::{DecodedThumb, ThumbnailState};

use super::spinner_phase;

/// Como [`render`], pero rodeando el contenido con un marco cuadrado de
/// líneas `|` (ASCII: `+ - | +`) cuyo color sigue un degradado de los tres
/// colores dominantes de la portada. Si no hay imagen (cargando, fallo, sin
/// miniatura) o el área no da sitio, usa un gris fijo.
///
/// El marco es de **2 celdas** cuando el área lo permite: un borde exterior con
/// el degradado de la portada (con contraste reforzado para que no se pierda
/// contra el fondo oscuro) y un acento interior tenue a modo de "passe-partout"
/// que separa la imagen del borde. En áreas pequeñas se cae a un borde único.
pub fn render_framed(frame: &mut Frame, area: Rect, state: &ThumbnailState, frame_anim: u64) {
    let area = centered_frame_rect(area, state);
    if area.width <= 2 || area.height <= 2 {
        render(frame, area, state, frame_anim);
        return;
    }
    let palette = match state {
        ThumbnailState::Loaded(img) => img.palette,
        _ => None,
    };
    let thick = area.width >= 6 && area.height >= 6;

    paint_frame(frame, area, palette, FrameRole::Outer);
    let inner = inset_rect(area, 1);
    if thick {
        paint_frame(frame, inner, palette, FrameRole::Inner);
        render(frame, inset_rect(inner, 1), state, frame_anim);
    } else {
        render(frame, inner, state, frame_anim);
    }
}

/// El marco se ajusta a la proporción de la portada *después* de descontar
/// sus dos anillos. Así los márgenes laterales y verticales alrededor del
/// marco son resultado de la misma geometría, no de un gutter arbitrario.
fn centered_frame_rect(area: Rect, state: &ThumbnailState) -> Rect {
    let ThumbnailState::Loaded(img) = state else {
        return area;
    };
    if area.width < 6 || area.height < 6 {
        return area;
    }
    let max_w = area.width.saturating_sub(4) as usize;
    let max_h_px = area.height.saturating_sub(4) as usize * 2;
    if max_w == 0 || max_h_px == 0 {
        return area;
    }
    let scale =
        (max_w as f64 / img.width.max(1) as f64).min(max_h_px as f64 / img.height.max(1) as f64);
    let content_w = ((img.width as f64 * scale).round() as usize).clamp(1, max_w);
    let content_h = ((img.height as f64 * scale).round() as usize).clamp(1, max_h_px);
    let width = (content_w + 4).min(area.width as usize) as u16;
    // La imagen se pinta con medio-bloques; reservar una celda adicional si
    // hace falta contiene el píxel impar sin sesgar el centro geométrico.
    let height = (content_h.div_ceil(2) + 4).min(area.height as usize) as u16;
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

/// Un anillo de marco consume una celda por lado. Mantener esta operación en
/// un único sitio hace explícita la cadena geométrica:
/// `outer area → outer border → inner border/mat → image content rect`.
fn inset_rect(area: Rect, inset: u16) -> Rect {
    Rect {
        x: area.x.saturating_add(inset),
        y: area.y.saturating_add(inset),
        width: area.width.saturating_sub(inset.saturating_mul(2)),
        height: area.height.saturating_sub(inset.saturating_mul(2)),
    }
}

/// Papel de cada anillo del marco en el degradado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameRole {
    /// Borde exterior: contraste reforzado contra el fondo oscuro.
    Outer,
    /// Acento interior: degradado atenuado hacia un gris oscuro (passe-partout).
    Inner,
}

/// Dibuja el marco `+ - | +` en el buffer; cada celda de borde recibe el color
/// del degradado en esa posición del perímetro (o `DarkGray` sin paleta).
fn paint_frame(frame: &mut Frame, area: Rect, palette: Option<[[u8; 3]; 3]>, role: FrameRole) {
    let buf = frame.buffer_mut();
    let (w, h) = (area.width, area.height);
    if w == 0 || h == 0 {
        return;
    }
    let steps = 2.0 * (w.saturating_sub(1) + h.saturating_sub(1)) as f64;
    if steps == 0.0 {
        set_border(buf, area.x, area.y, "+", frame_color(None, role, 0.0));
        return;
    }
    let grad = |t: f64| -> Color { frame_color(palette, role, t / steps) };

    if w == 1 {
        for y in 0..h {
            set_border(buf, area.x, area.y + y, "|", grad(y as f64));
        }
        return;
    }
    if h == 1 {
        for x in 0..w {
            set_border(buf, area.x + x, area.y, "-", grad(x as f64));
        }
        return;
    }

    let (tw, th) = (w - 1, h - 1);
    set_border(buf, area.x, area.y, "+", grad(0.0));
    set_border(buf, area.x + tw, area.y, "+", grad(tw as f64));
    set_border(buf, area.x, area.y + th, "+", grad((tw + th) as f64));
    set_border(
        buf,
        area.x + tw,
        area.y + th,
        "+",
        grad((2 * tw + th) as f64),
    );
    for x in 1..tw {
        set_border(buf, area.x + x, area.y, "-", grad(x as f64));
        set_border(
            buf,
            area.x + x,
            area.y + th,
            "-",
            grad((2 * tw + th - x) as f64),
        );
    }
    for y in 1..th {
        set_border(buf, area.x, area.y + y, "|", grad((tw + y) as f64));
        set_border(
            buf,
            area.x + tw,
            area.y + y,
            "|",
            grad((tw + 2 * th - y) as f64),
        );
    }
}

/// Color de una celda del marco en la posición `t` del perímetro.
///
/// Sin paleta se usa un gris fijo; con paleta, el degradado de sus tres colores
/// y después se adapta al papel del anillo:
/// - exterior → se aclara si es demasiado oscuro (contraste contra el fondo);
/// - interior → se atenúa hacia un gris oscuro (separación sutil de la imagen).
fn frame_color(palette: Option<[[u8; 3]; 3]>, role: FrameRole, t: f64) -> Color {
    let base = match palette {
        Some(p) => gradient_base(&p, t),
        None => [110u8, 110, 110],
    };
    match role {
        FrameRole::Outer => Color::Rgb(
            contrast_boost(base)[0],
            contrast_boost(base)[1],
            contrast_boost(base)[2],
        ),
        FrameRole::Inner => {
            let d = dim_for_mat(base);
            Color::Rgb(d[0], d[1], d[2])
        }
    }
}

/// Interpola a lo largo de los tres colores de la paleta en círculo:
/// `t ∈ [0,1)` recorre c0 → c1 → c2 → c0. Devuelve el RGB interpolado.
fn gradient_base(palette: &[[u8; 3]; 3], t: f64) -> [u8; 3] {
    let scaled = (t * 3.0).clamp(0.0, 2.9999);
    let i = scaled.floor() as usize;
    let f = scaled - i as f64;
    let a = palette[i];
    let b = palette[(i + 1) % 3];
    let lerp = |x: u8, y: u8| {
        (x as f64 + (y as f64 - x as f64) * f)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    [lerp(a[0], b[0]), lerp(a[1], b[1]), lerp(a[2], b[2])]
}

fn luminance(c: [u8; 3]) -> f64 {
    (0.299 * c[0] as f64 + 0.587 * c[1] as f64 + 0.114 * c[2] as f64) / 255.0
}

/// Aclara colores demasiado oscuros para que el borde exterior nunca se pierda
/// contra el fondo oscuro del terminal (p. ej. portadas negras). Los colores
/// con un canal ya brillante se dejan intactos: son visibles por sí mismos.
fn contrast_boost(c: [u8; 3]) -> [u8; 3] {
    let l = luminance(c);
    let peak = *c.iter().max().unwrap_or(&0) as f64 / 255.0;
    if l >= 0.35 || peak >= 0.6 {
        return c;
    }
    let t = (0.35 - l) / 0.35; // 0..1, cuánto falta para el umbral
    let mix = 0.45 + 0.45 * t; // 0.45..0.90
    [
        c[0] + ((255 - c[0]) as f64 * mix) as u8,
        c[1] + ((255 - c[1]) as f64 * mix) as u8,
        c[2] + ((255 - c[2]) as f64 * mix) as u8,
    ]
}

/// Atenúa el degradado hacia un gris oscuro: el acento interior queda como un
/// "passe-partout" que separa la imagen del borde exterior en cualquier portada.
fn dim_for_mat(c: [u8; 3]) -> [u8; 3] {
    const ANCHOR: [u8; 3] = [26, 26, 32];
    const MIX: f64 = 0.55;
    [
        (c[0] as f64 * (1.0 - MIX) + ANCHOR[0] as f64 * MIX) as u8,
        (c[1] as f64 * (1.0 - MIX) + ANCHOR[1] as f64 * MIX) as u8,
        (c[2] as f64 * (1.0 - MIX) + ANCHOR[2] as f64 * MIX) as u8,
    ]
}

fn set_border(buf: &mut ratatui::buffer::Buffer, x: u16, y: u16, ch: &str, color: Color) {
    if let Some(cell) = buf.cell_mut((x, y)) {
        cell.set_symbol(ch);
        cell.set_fg(color);
    }
}

pub fn render(frame: &mut Frame, area: Rect, state: &ThumbnailState, frame_anim: u64) {
    match state {
        ThumbnailState::Loaded(img) => render_image(frame, area, img),
        ThumbnailState::Loading => render_placeholder(
            frame,
            area,
            format!("{} cargando miniatura…", spinner_phase(frame_anim)),
            Color::Yellow,
        ),
        ThumbnailState::Failed(_) => render_placeholder(
            frame,
            area,
            "⚠ miniatura no disponible".to_string(),
            Color::Red,
        ),
        ThumbnailState::None => {
            render_placeholder(frame, area, "sin miniatura".to_string(), Color::DarkGray)
        }
    }
}

/// Dibuja la imagen escalada al área, centrada y con proporción conservada.
///
/// Cada celda del terminal son 2 píxeles verticales. Los cuatro casos se
/// resuelven por separado para que nunca quede un medio bloque con su mitad
/// visible fuera de la imagen (lo que pintaba franjas claras arriba/abajo):
///
/// - ambos píxeles existen → `▀` (foreground = superior, background = inferior);
/// - solo el superior → `▀` con fondo `Reset` (el panel queda visible abajo);
/// - solo el inferior → `▄` con fondo `Reset` (el panel queda visible arriba);
/// - ninguno → espacio, sin pintar nada (se conserva el fondo real del panel).
///
/// El `Color::Reset` solo se usa como **background** de un medio bloque (donde
/// significa "fondo del panel"), nunca como foreground visible.
fn render_image(frame: &mut Frame, area: Rect, img: &DecodedThumb) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let cells_w = area.width as usize;
    let cells_h = area.height as usize;
    let (draw_w, draw_h, x0, y0) =
        fit_rect(cells_w, cells_h, img.width as usize, img.height as usize);

    let mut lines = Vec::with_capacity(cells_h);
    for row in 0..cells_h {
        let upper = row * 2;
        let lower = row * 2 + 1;
        let mut line = Line::default();
        for col in 0..cells_w {
            if col < x0 || col >= x0 + draw_w {
                // Fuera de la imagen en horizontal: espacio, se conserva el fondo.
                line.push_span(Span::raw(" "));
                continue;
            }
            let sx = ((col - x0) * img.width as usize) / draw_w;
            let top = sample_pixel(img, sx, upper, y0, draw_h);
            let bottom = sample_pixel(img, sx, lower, y0, draw_h);
            let cell = match (top, bottom) {
                (Some(t), Some(b)) => Span::styled("▀", Style::new().fg(t).bg(b)),
                (Some(t), None) => Span::styled("▀", Style::new().fg(t).bg(Color::Reset)),
                (None, Some(b)) => Span::styled("▄", Style::new().fg(b).bg(Color::Reset)),
                (None, None) => Span::raw(" "),
            };
            line.push_span(cell);
        }
        lines.push(line);
    }
    frame.render_widget(Paragraph::new(lines), area);
}

/// Píxel de la fila `py` (en coordenadas de la rejilla doblada de la terminal)
/// si cae dentro de la imagen dibujada; `None` si está fuera o es transparente.
fn sample_pixel(
    img: &DecodedThumb,
    x: usize,
    py: usize,
    y0: usize,
    draw_h: usize,
) -> Option<Color> {
    if py < y0 || py >= y0 + draw_h {
        return None;
    }
    let sy = ((py - y0) * img.height as usize) / draw_h;
    pixel_color(img, x, sy)
}

/// Color del píxel RGBA en `(x, y)`; `None` si el píxel está fuera o es
/// prácticamente transparente (alfa < 16): en ese caso no debe pintarse nada,
/// de modo que el foreground de un medio bloque nunca sea `Reset` en una zona
/// visible.
fn pixel_color(img: &DecodedThumb, x: usize, y: usize) -> Option<Color> {
    let y = y.min(img.height.saturating_sub(1) as usize);
    let idx = (y * img.width as usize + x.min(img.width.saturating_sub(1) as usize)) * 4;
    let [r, g, b, a] = [
        img.rgba[idx],
        img.rgba[idx + 1],
        img.rgba[idx + 2],
        img.rgba[idx + 3],
    ];
    if a < 16 {
        None
    } else {
        Some(Color::Rgb(r, g, b))
    }
}

/// Encaja la imagen (proporción conservada, centrada, sin deformar) dentro de
/// un área de `cells_w`×`cells_h` celdas, usando la rejilla vertical doblada
/// (cada celda = 2 píxeles). Devuelve `(ancho, alto, x0, y0)` en esa rejilla.
///
/// La escala llena geométricamente el rectángulo real de contenido: en
/// terminales grandes también amplía fuentes pequeñas, evitando que una
/// portada diminuta parezca descentrada dentro de un marco enorme.
fn fit_rect(
    cells_w: usize,
    cells_h: usize,
    img_w: usize,
    img_h: usize,
) -> (usize, usize, usize, usize) {
    if cells_w == 0 || cells_h == 0 {
        return (0, 0, 0, 0);
    }
    let img_w = img_w.max(1);
    let img_h = img_h.max(1);
    let scale = (cells_w as f64 / img_w as f64).min(cells_h as f64 * 2.0 / img_h as f64);
    if scale <= 0.0 {
        return (0, 0, 0, 0);
    }
    let draw_w = ((img_w as f64 * scale).round() as usize).clamp(1, cells_w);
    let draw_h = ((img_h as f64 * scale).round() as usize).clamp(1, cells_h * 2);
    let x0 = (cells_w - draw_w) / 2;
    let y0 = (cells_h * 2 - draw_h) / 2;
    (draw_w, draw_h, x0, y0)
}

/// Caja pequeña con el mensaje de estado (cargando / fallo / sin miniatura).
///
/// Sin borde propio: la tarjeta lo aporta para que todos los estados tengan el
/// mismo margen (lo que ya pinte aquí son solo textos sobre el área interior).
fn render_placeholder(frame: &mut Frame, area: Rect, msg: String, fg: Color) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let text = Paragraph::new(msg).style(Style::new().fg(fg));
    frame.render_widget(text, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn thumb(
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        palette: Option<[[u8; 3]; 3]>,
    ) -> ThumbnailState {
        ThumbnailState::Loaded(Arc::new(DecodedThumb {
            width,
            height,
            rgba,
            palette,
            matrix: None,
        }))
    }

    fn opaque(w: u32, h: u32) -> Vec<u8> {
        // Rellena con un color opaco no-negro (p. ej. verde) para que ningún
        // píxel sea transparente ni "fondo".
        let mut v = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..(w * h) {
            v.extend_from_slice(&[40, 200, 90, 255]);
        }
        v
    }

    /// Renderiza `state` en un área de `w`×`h` celdas y devuelve el buffer.
    fn render_buf(
        w: u16,
        h: u16,
        state: &ThumbnailState,
        framed: bool,
    ) -> (ratatui::buffer::Buffer, (usize, usize, usize, usize)) {
        let backend = ratatui::backend::TestBackend::new(w, h);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                if framed {
                    render_framed(f, f.area(), state, 0);
                } else {
                    render(f, f.area(), state, 0);
                }
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let fit = if let ThumbnailState::Loaded(img) = state {
            fit_rect(
                w as usize,
                h as usize,
                img.width as usize,
                img.height as usize,
            )
        } else {
            (0, 0, 0, 0)
        };
        (buf, fit)
    }

    fn half_block_cells(buf: &ratatui::buffer::Buffer) -> Vec<(u16, u16, &str, Color)> {
        buf.content()
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                let s = c.symbol();
                if s == "▀" || s == "▄" {
                    let (x, y) = buf.pos_of(i);
                    Some((x, y, s, c.fg))
                } else {
                    None
                }
            })
            .collect()
    }

    #[test]
    fn loading_and_failed_do_not_panic() {
        let backend = ratatui::backend::TestBackend::new(10, 5);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        for state in [
            ThumbnailState::Loading,
            ThumbnailState::Failed("red".to_string()),
            ThumbnailState::None,
        ] {
            terminal.draw(|f| render(f, f.area(), &state, 0)).unwrap();
        }
    }

    #[test]
    fn loaded_draws_half_blocks() {
        let state = thumb(2, 4, (0..(2 * 4 * 4)).map(|i| i as u8).collect(), None);
        let (buf, _) = render_buf(10, 5, &state, false);
        assert!(
            buf.content()
                .iter()
                .any(|c| c.symbol() == "▀" || c.symbol() == "▄"),
            "debe pintar medio bloques"
        );
    }

    /// No debe aparecer un medio bloque en ninguna celda que quede fuera del
    /// rectángulo donde se dibuja la imagen (letterboxing de aspecto).
    #[test]
    fn no_half_blocks_outside_drawn_image() {
        // Imagen 4x4 en un área 6x6: se amplía hasta el límite geométrico,
        // conservando proporción y sin pintar fuera de él.
        let state = thumb(4, 4, opaque(4, 4), None);
        let (buf, (dw, dh, x0, y0)) = render_buf(6, 6, &state, false);
        assert_eq!((dw, dh), (6, 6));

        for (x, y, s, _) in half_block_cells(&buf) {
            let px_y = y as usize * 2;
            let inside_x = (x as usize) >= x0 && (x as usize) < x0 + dw;
            let inside_y = (px_y >= y0 && px_y < y0 + dh) || (px_y + 1 >= y0 && px_y + 1 < y0 + dh);
            assert!(
                inside_x && inside_y,
                "medio bloque '{s}' en ({x},{y}) fuera de la imagen"
            );
        }
        // La escala llena el eje limitante y conserva el letterboxing centrado
        // que exige la proporción física de la terminal.
        let rows_with_blocks: std::collections::HashSet<u16> = half_block_cells(&buf)
            .into_iter()
            .map(|(_, y, _, _)| y)
            .collect();
        // Los extremos caen en medios bloques distintos, por eso seis píxeles
        // ocupan cuatro celdas sin salirse del rectángulo geométrico.
        assert_eq!(rows_with_blocks.len(), 4, "debe ocupar las filas centradas");
    }

    /// Ningún medio bloque puede tener un foreground `Reset` en una zona vacía
    /// (esa mitad visible se pintaría con el color por defecto del terminal).
    #[test]
    fn half_block_foreground_never_reset_in_empty_zone() {
        // Imagen más pequeña que el área: genera medios bloques con solo una
        // de sus mitades dentro de la imagen (p. ej. filas de borde).
        let state = thumb(2, 2, opaque(2, 2), None);
        let (buf, _) = render_buf(6, 6, &state, false);
        let blocks = half_block_cells(&buf);
        assert!(!blocks.is_empty(), "debe haber medio bloques");
        for (x, y, s, fg) in &blocks {
            assert_ne!(
                *fg,
                Color::Reset,
                "foreground Reset visible en medio bloque ({x},{y}) '{s}'"
            );
        }
    }

    /// Píxeles transparentes (alfa ~0) nunca pintan un medio bloque: se deja la
    /// celda como espacio y se conserva el fondo del panel.
    #[test]
    fn transparent_pixels_render_as_spaces() {
        let mut rgba = Vec::with_capacity(2 * 2 * 4);
        for _ in 0..4 {
            rgba.extend_from_slice(&[255, 0, 0, 0]);
        }
        let state = thumb(2, 2, rgba, None);
        let (buf, _) = render_buf(6, 6, &state, false);
        assert!(
            buf.content()
                .iter()
                .all(|c| c.symbol() != "▀" && c.symbol() != "▄"),
            "píxeles transparentes no deben pintarse"
        );
    }

    /// El borde exterior debe seguir visible con portadas oscuras (contraste
    /// reforzado) y coloreado con portadas claras.
    #[test]
    fn frame_stays_visible_for_dark_and_light_covers() {
        for palette in [
            Some([[0, 0, 0], [4, 4, 4], [10, 10, 12]]),
            Some([[255, 255, 255], [255, 0, 0], [0, 0, 255]]),
        ] {
            let state = thumb(4, 4, opaque(4, 4), palette);
            let (buf, _) = render_buf(12, 8, &state, true);
            // El marco exterior existe (esquinas `+` en el perímetro).
            assert_eq!(buf[(0, 0)].symbol(), "+", "esquina superior izquierda");
            assert_eq!(buf[(11, 7)].symbol(), "+", "esquina inferior derecha");
            // Alguna celda del borde exterior tiene color real (no Reset).
            let mut colored = false;
            for x in 0..12 {
                for y in [0u16, 7] {
                    if let Color::Rgb(r, g, b) = buf[(x, y)].fg {
                        colored = true;
                        if palette == Some([[0, 0, 0], [4, 4, 4], [10, 10, 12]]) {
                            // Portada negra: el contraste debe haberla aclarado.
                            assert!(
                                r as u32 + g as u32 + b as u32 >= 240,
                                "borde demasiado oscuro contra el fondo: {r},{g},{b}"
                            );
                        }
                    }
                }
            }
            assert!(colored, "el borde debe estar coloreado");
        }
    }

    /// `contrast_boost` nunca devuelve un color que desaparezca contra el fondo.
    #[test]
    fn contrast_boost_keeps_dark_border_visible() {
        let boosted = contrast_boost([0, 0, 0]);
        assert!(luminance(boosted) >= 0.35, "negro puro debe aclararse");
        assert_eq!(contrast_boost([255, 0, 0]), [255, 0, 0], "claros sin tocar");
    }

    #[test]
    fn frame_insets_define_the_real_image_content_rect() {
        let outer = Rect::new(10, 4, 20, 14);
        let after_outer = inset_rect(outer, 1);
        let content = inset_rect(after_outer, 1);
        assert_eq!(after_outer, Rect::new(11, 5, 18, 12));
        assert_eq!(content, Rect::new(12, 6, 16, 10));
    }

    /// `fit_rect` conserva la proporción (dentro del redondeo de la rejilla),
    /// no deforma y nunca desborda el área.
    #[test]
    fn fit_rect_preserves_aspect_ratio() {
        let cases: &[(usize, usize, usize, usize)] = &[
            (10, 5, 100, 100),
            (10, 5, 1, 1),
            (10, 5, 4, 3),
            (10, 5, 16, 9),
            (10, 20, 3, 4),
            (40, 12, 320, 180),
            (6, 6, 2, 4),
        ];
        for &(cw, ch, iw, ih) in cases {
            let (dw, dh, x0, y0) = fit_rect(cw, ch, iw, ih);
            // Encaja dentro del área (y de la rejilla vertical doblada).
            assert!(dw >= 1 && dw <= cw, "ancho fuera de límites ({dw} > {cw})");
            assert!(
                dh >= 1 && dh <= ch * 2,
                "alto fuera de límites ({dh} > {})",
                ch * 2
            );
            assert!(x0 + dw <= cw && y0 + dh <= ch * 2, "desborda el área");
            // Proporción conservada (tolerancia de una celda de redondeo).
            let img_ratio = iw as f64 / ih as f64;
            let draw_ratio = dw as f64 / dh as f64;
            let tol = (1.0 / dw as f64) + (1.0 / dh as f64);
            assert!(
                (draw_ratio - img_ratio).abs() <= tol.max(0.08),
                "proporción rota en {cw}x{ch} con imagen {iw}x{ih}: {draw_ratio:.3} vs {img_ratio:.3}"
            );
        }
    }

    /// Imágenes verticales/horizontales extremas no producen artefactos: se
    /// recortan al área sin desbordes ni medio bloques fuera de la imagen.
    #[test]
    fn extreme_images_produce_no_artifacts() {
        for (w, h) in [(1u32, 200u32), (200u32, 1u32), (300, 200), (1, 1)] {
            let state = thumb(w, h, opaque(w, h), None);
            let (buf, (dw, dh, x0, y0)) = render_buf(10, 5, &state, false);
            assert!(!buf.content().is_empty());
            for (x, y, s, _) in half_block_cells(&buf) {
                let inside_x = (x as usize) >= x0 && (x as usize) < x0 + dw;
                let py = y as usize * 2;
                let inside_y = (py >= y0 && py < y0 + dh) || (py + 1 >= y0 && py + 1 < y0 + dh);
                assert!(
                    inside_x && inside_y,
                    "artefacto '{s}' en ({x},{y}) para imagen {w}x{h}"
                );
            }
        }
    }

    #[test]
    fn framed_renders_ascii_box_with_palette_gradient() {
        let state = thumb(
            2,
            2,
            vec![255; 16],
            Some([[255, 0, 0], [0, 255, 0], [0, 0, 255]]),
        );
        let backend = ratatui::backend::TestBackend::new(8, 6);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_framed(f, f.area(), &state, 0))
            .unwrap();
        let buf = terminal.backend().buffer();
        // Esquinas del marco ASCII (exterior: 2 celdas → (0,0) y el interior).
        assert_eq!(buf[(0, 0)].symbol(), "+");
        assert_eq!(buf[(7, 0)].symbol(), "+");
        assert_eq!(buf[(7, 5)].symbol(), "+");
        assert_eq!(buf[(1, 1)].symbol(), "+", "marco interior (passe-partout)");
        // Lados verticales con `|` y horizontales con `-`.
        assert_eq!(buf[(0, 3)].symbol(), "|");
        assert_eq!(buf[(7, 2)].symbol(), "|");
        assert_eq!(buf[(4, 0)].symbol(), "-");
        assert_eq!(buf[(4, 5)].symbol(), "-");
        // El degradado colorea el borde con los colores de la paleta.
        let Color::Rgb(r, g, b) = buf[(1, 0)].fg else {
            panic!("borde superior debe estar coloreado");
        };
        assert!(r as u32 + g as u32 + b as u32 > 0);
    }
}
