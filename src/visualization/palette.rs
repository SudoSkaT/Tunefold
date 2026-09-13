//! Paleta visual derivada de la portada (spec §4).
//!
//! El track en curso ya produce tres colores dominantes (`DecodedThumb.palette`,
//! `[[u8;3];3]`). Aquí se formalizan como [`VisualPalette`], un CONTRATO propio
//! de la capa de visualización: el motor visual la recibe y la funde con la de
//! la canción anterior (transición de cambio de track), y el renderer solo la
//! consume. La extracción nunca ocurre en el renderer.
//!
//! Pura y determinista: sin aleatoriedad, sin estado, sin dependencia del
//! renderer (solo `[u8;3]`).
//!
//! ## Contraste adaptativo del karaoke (spec §2, legibilidad)
//!
//! Las letras se pintan sobre una capa ambiental viva (osciloscopio) cuyo
//! fondo base es `background` aplacado (`subdued`). En vez de forzar el
//! contraste con "oscurecer y blanquear" ad-hoc, aquí se resuelve por
//! luminancia/contraste estilo WCAG:
//!
//! - `relative_luminance` / `contrast_ratio`: matemática de contraste pura.
//! - `ensure_contrast`: dado un color objetivo y un fondo, acerca el color a
//!   blanco o negro (según la luminancia del fondo) solo lo necesario para
//!   alcanzar el ratio. Conserva el tinte de la portada y evita tonalidades
//!   similares al fondo.
//! - [`VisualPalette::karaoke_colors`]: resuelve los tres estados (leído /
//!   en lectura / no leído) contra el fondo más claro que el ambient aplacado
//!   puede producir, garantizando χ ≥ 4.5 en la línea activa y ≥ 3.0 en las
//!   históricas independientemente del color dominante de la portada.

/// Tres colores dominantes + fondo derivado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisualPalette {
    /// Color más dominante (barras altas, línea de karaoke en lectura).
    pub primary: [u8; 3],
    /// Segundo dominante (barras medias, karaoke ya leído).
    pub secondary: [u8; 3],
    /// Tercer dominante (barras bajas, karaoke no leído).
    pub accent: [u8; 3],
    /// Fondo de escena: versión oscurecida del dominante.
    pub background: [u8; 3],
}

impl VisualPalette {
    /// Paleta fija para cuando no hay portada (determinista, sin `rand`).
    pub const fn fallback() -> Self {
        Self {
            primary: [226, 120, 224],
            secondary: [96, 168, 252],
            accent: [250, 176, 96],
            background: [20, 14, 26],
        }
    }

    /// De los tres colores dominantes de la portada (`None` ⇒ [`Self::fallback`]).
    pub fn from_cover(cover: Option<[[u8; 3]; 3]>) -> Self {
        let Some(p) = cover else {
            return Self::fallback();
        };
        Self {
            primary: p[0],
            secondary: p[1],
            accent: p[2],
            background: Self::shade(p[0], 0.22),
        }
    }

    /// Mezcla lineal hacia `other` (t=0 mantiene `self`, t=1 llega a `other`).
    ///
    /// Pura y determinista: el motor la usa una vez por frame para fundir la
    /// paleta del track anterior con la nueva.
    pub fn mix(&self, other: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Self {
            primary: [
                lerp(self.primary[0], other.primary[0]),
                lerp(self.primary[1], other.primary[1]),
                lerp(self.primary[2], other.primary[2]),
            ],
            secondary: [
                lerp(self.secondary[0], other.secondary[0]),
                lerp(self.secondary[1], other.secondary[1]),
                lerp(self.secondary[2], other.secondary[2]),
            ],
            accent: [
                lerp(self.accent[0], other.accent[0]),
                lerp(self.accent[1], other.accent[1]),
                lerp(self.accent[2], other.accent[2]),
            ],
            background: [
                lerp(self.background[0], other.background[0]),
                lerp(self.background[1], other.background[1]),
                lerp(self.background[2], other.background[2]),
            ],
        }
    }

    /// Oscurece un color por `k` (0..1).
    fn shade(c: [u8; 3], k: f32) -> [u8; 3] {
        [
            (c[0] as f32 * k).round().clamp(0.0, 255.0) as u8,
            (c[1] as f32 * k).round().clamp(0.0, 255.0) as u8,
            (c[2] as f32 * k).round().clamp(0.0, 255.0) as u8,
        ]
    }

    /// Fondo base (aplacado) de la escena tras las letras: `background` a 45%.
    ///
    /// El renderer ambiental oscurece la escena igualmente (modo `subdued`),
    /// así que este es el color de la gran mayoría de celdas bajo el texto.
    pub fn karaoke_subdued_bg(&self) -> [u8; 3] {
        Self::shade(self.background, KARAOKE_SUBDUED)
    }

    /// Cota superior conservadora del fondo tras el texto.
    ///
    /// El ambient aplacado no es un plano: sus trazos iluminan celdas locales.
    /// La celda más clara que puede producir está acotada por una mezcla de la
    /// base aplacada con el color de la traza (el renderer garantiza esta cota;
    /// ver `render.rs`). Las letras se resuelven contra este techo para seguir
    /// legibles aunque un pico de señal pase por detrás de la línea activa.
    pub fn karaoke_bg_ceiling(&self) -> [u8; 3] {
        let base = self.karaoke_subdued_bg();
        let t = KARAOKE_TRACE_CEILING;
        [
            (base[0] as f32 + (self.accent[0] as f32 - base[0] as f32) * t).round() as u8,
            (base[1] as f32 + (self.accent[1] as f32 - base[1] as f32) * t).round() as u8,
            (base[2] as f32 + (self.accent[2] as f32 - base[2] as f32) * t).round() as u8,
        ]
    }

    /// Colores de los tres estados del karaoke con contraste garantizado.
    ///
    /// - `current`: línea en lectura → `primary`, χ ≥ [`KARAOKE_ACTIVE_CONTRAST`]
    ///   (normalmente 4.5:1; además el renderer la marca en negrita).
    /// - `read` / `unread`: líneas históricas/futuras → `secondary`/`accent`,
    ///   χ ≥ [`KARAOKE_DIM_CONTRAST`] (3.0:1): legibles sin eclipsar a la activa.
    ///
    /// `ensure_contrast` mueve cada estado hacia blanco/negro solo lo necesario;
    /// si un color ya cumple, se conserva intacto (armonía con la portada).
    pub fn karaoke_colors(&self) -> KaraokeColors {
        let bg = self.karaoke_bg_ceiling();
        KaraokeColors {
            read: ensure_contrast(self.secondary, bg, KARAOKE_DIM_CONTRAST),
            current: ensure_contrast(self.primary, bg, KARAOKE_ACTIVE_CONTRAST),
            unread: ensure_contrast(self.accent, bg, KARAOKE_DIM_CONTRAST),
        }
    }

    /// Mezcla lineal sRGB (función auxiliar).
    fn blend(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
        let t = t.clamp(0.0, 1.0);
        [
            (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t).round() as u8,
            (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t).round() as u8,
            (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t).round() as u8,
        ]
    }
}

/// Aplazamiento del fondo tras el texto (mismo factor que el modo `subdued`).
pub const KARAOKE_SUBDUED: f32 = 0.45;

/// Fracción máxima de color de traza sobre la base aplacada que el renderer
/// ambiental aplica a una celda (techo de brillo para [`VisualPalette`]).
pub const KARAOKE_TRACE_CEILING: f32 = 0.27;

/// Contraste mínimo (WCAG) de la línea de karaoke en lectura (la resolución
/// además la marca en negrita).
pub const KARAOKE_ACTIVE_CONTRAST: f64 = 4.5;

/// Contraste mínimo (WCAG) de las líneas históricas/futuras del karaoke.
pub const KARAOKE_DIM_CONTRAST: f64 = 3.0;

/// Colores resueltos de los tres estados del karaoke (ya leído / en lectura /
/// no leído) garantizando contraste contra el fondo de escena.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KaraokeColors {
    /// Líneas ya leídas (segundo dominante de la portada, χ ≥ 3:1).
    pub read: [u8; 3],
    /// Línea en lectura (dominante, χ ≥ 4.5:1; el renderer la marca en negrita).
    pub current: [u8; 3],
    /// Líneas no leídas (tercer dominante, χ ≥ 3:1).
    pub unread: [u8; 3],
}

/// Luminancia relativa WCAG 2.x (sRGB linealizada). El canal nulo → `0.0`.
pub fn relative_luminance(c: [u8; 3]) -> f64 {
    fn linearize(c: u8) -> f64 {
        let s = f64::from(c) / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * linearize(c[0]) + 0.7152 * linearize(c[1]) + 0.0722 * linearize(c[2])
}

/// Ratio de contraste WCAG entre dos colores (1.0..21.0).
pub fn contrast_ratio(a: [u8; 3], b: [u8; 3]) -> f64 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Ajusta `fg` sobre `bg` para alcanzar `target` conservando el tinte.
///
/// Si el ratio ya se cumple devuelve `fg` intacto. En caso contrario mueve el
/// color, en pasos de 1/16, hacia el extremo (blanco o negro) que aumente el
/// contraste dado el fondo: hacia blanco si el fondo es oscuro (χ 4.5 sobre
/// escenas aplacadas, el caso habitual del karaoke), hacia negro si es claro.
/// Devuelve el extremo si ni siquiera él llega al objetivo (el mejor posible).
pub fn ensure_contrast(fg: [u8; 3], bg: [u8; 3], target: f64) -> [u8; 3] {
    if contrast_ratio(fg, bg) >= target {
        return fg;
    }
    let end: [u8; 3] = if relative_luminance(bg) <= 0.18 {
        [255, 255, 255]
    } else {
        [0, 0, 0]
    };
    let mut t = 1.0 / 16.0;
    while t < 1.0 {
        let cand = VisualPalette::blend(fg, end, t);
        if contrast_ratio(cand, bg) >= target {
            return cand;
        }
        t += 1.0 / 16.0;
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_cover_equals_fallback_exactly() {
        assert_eq!(VisualPalette::from_cover(None), VisualPalette::fallback());
    }

    #[test]
    fn from_cover_maps_dominants_and_darkens_background() {
        let cover = Some([[200u8, 40, 40], [40, 200, 60], [30, 60, 220]]);
        let p = VisualPalette::from_cover(cover);
        assert_eq!(p.primary, [200, 40, 40]);
        assert_eq!(p.secondary, [40, 200, 60]);
        assert_eq!(p.accent, [30, 60, 220]);
        assert_eq!(p.background, [(200.0f32 * 0.22).round() as u8, 9, 9]);
        assert!(
            u32::from(p.background[0]) + u32::from(p.background[1]) + u32::from(p.background[2])
                < u32::from(p.primary[0]) + u32::from(p.primary[1]) + u32::from(p.primary[2]),
            "el fondo es siempre una versión oscura del dominante"
        );
    }

    #[test]
    fn mix_endpoints_are_exact() {
        let a = VisualPalette::from_cover(Some([[10u8, 10, 10], [20, 20, 20], [30, 30, 30]]));
        let b =
            VisualPalette::from_cover(Some([[200u8, 200, 200], [220, 220, 220], [240, 240, 240]]));
        assert_eq!(a.mix(&b, 0.0), a);
        assert_eq!(a.mix(&b, 1.0), b);
    }

    #[test]
    fn mix_is_deterministic_and_bounded() {
        let a = VisualPalette::fallback();
        let b = VisualPalette::from_cover(Some([[0u8, 20, 250], [10, 30, 0], [5, 5, 5]]));
        let m1 = a.mix(&b, 0.43);
        let m2 = a.mix(&b, 0.43);
        assert_eq!(m1, m2, "la mezcla es una función pura");
        // t fuera de rango se satura a 0/1 (sin NaN ni desbordes).
        assert_eq!(a.mix(&b, -5.0), a, "t < 0 se trata como 0");
        assert_eq!(a.mix(&b, 99.0), b, "t > 1 se trata como 1");
    }

    #[test]
    fn repeated_mix_converges_toward_target() {
        let target = VisualPalette::from_cover(Some([[255u8, 0, 0], [0, 255, 0], [0, 0, 255]]));
        let mut cur = VisualPalette::fallback();
        for _ in 0..40 {
            cur = cur.mix(&target, 0.35);
        }
        let diff = |a: &[u8; 3], b: &[u8; 3]| {
            a.iter()
                .zip(b.iter())
                .map(|(x, y)| (*x as i32 - *y as i32).abs())
                .sum::<i32>()
        };
        // ≤3 por canal admite el residuo de redondeo u8 al converger.
        assert!(
            diff(&cur.primary, &target.primary) <= 3,
            "converge al dominante"
        );
        assert!(diff(&cur.background, &target.background) <= 3);
    }

    // --- Contraste adaptativo del karaoke (spec §2) ---

    #[test]
    fn luminance_of_known_colors() {
        // Negro y blanco: extremos del espacio WCAG.
        assert_eq!(relative_luminance([0, 0, 0]), 0.0);
        assert!((relative_luminance([255, 255, 255]) - 1.0).abs() < 1e-9);
        // Gris medio: la linealización sRGB NO es 0.5 (es mayor).
        let mid = relative_luminance([128, 128, 128]);
        assert!(mid > 0.18 && mid < 0.25, "gris 128 → luminancia {mid}");
    }

    #[test]
    fn contrast_ratio_extremes() {
        assert_eq!(contrast_ratio([0, 0, 0], [255, 255, 255]), 21.0);
        assert!(
            (contrast_ratio([0, 0, 0], [0, 0, 0]) - 1.0).abs() < 1e-9,
            "mismo color → 1:1"
        );
        assert_eq!(contrast_ratio([255, 255, 255], [0, 0, 0]), 21.0);
    }

    #[test]
    fn compliant_color_is_kept_intact() {
        // Blanco sobre fondo oscuro ya cumple: no se toca el tinte.
        let fg = [255, 255, 255];
        assert_eq!(ensure_contrast(fg, [20, 14, 26], 4.5), fg);
        // Negro sobre fondo claro.
        let fg_black = [0, 0, 0];
        assert_eq!(ensure_contrast(fg_black, [230, 230, 230], 4.5), fg_black);
    }

    #[test]
    fn dark_background_resolves_toward_white() {
        // Rojo oscuro (el dominante de una portada tenebrosa) sobre fondo
        // aplacado: el resultado debe alcanzar el objetivo y ser claro, no
        // una copia oscura de sí mismo.
        let bg = VisualPalette::from_cover(Some([[120u8, 20, 20], [10, 140, 40], [20, 30, 60]]))
            .karaoke_bg_ceiling();
        let out = ensure_contrast([120, 20, 20], bg, KARAOKE_ACTIVE_CONTRAST);
        assert!(
            contrast_ratio(out, bg) >= KARAOKE_ACTIVE_CONTRAST,
            "χ {} < {KARAOKE_ACTIVE_CONTRAST} ({out:?} sobre {bg:?})",
            contrast_ratio(out, bg)
        );
        let (r, g, b) = (out[0], out[1], out[2]);
        assert!(
            u32::from(r) + u32::from(g) + u32::from(b) > 3 * 128,
            "se ilumina hacia blanco: {out:?}"
        );
    }

    #[test]
    fn light_background_resolves_toward_black() {
        // Con un fondo realmente claro (p. ej. un theme de UI luminoso) el
        // texto debe oscurecerse en vez de ir a blanco.
        let out = ensure_contrast([240, 240, 240], [200, 200, 200], KARAOKE_ACTIVE_CONTRAST);
        assert!(
            contrast_ratio(out, [200, 200, 200]) >= KARAOKE_ACTIVE_CONTRAST,
            "χ {} < 4.5",
            contrast_ratio(out, [200, 200, 200])
        );
        let l = u32::from(out[0]) + u32::from(out[1]) + u32::from(out[2]);
        assert!(l < 3 * 128, "se oscurece hacia negro: {out:?}");
    }

    #[test]
    fn light_cover_still_yields_dark_scene() {
        // La paleta deriva el fondo oscureciendo el dominante (0.22 * 0.45):
        // incluso una portada casi blanca deja una escena aplacada oscura, y
        // el karaoke se resuelve hacia blanco sin romper el tinte.
        let whiteish = [[240u8, 240, 240], [230, 235, 240], [250, 245, 235]];
        let p = VisualPalette::from_cover(Some(whiteish));
        let bg = p.karaoke_bg_ceiling();
        assert!(
            relative_luminance(bg) < 0.5,
            "el techo del fondo sigue siendo oscuro: {bg:?}"
        );
        let c = p.karaoke_colors();
        assert!(
            contrast_ratio(c.current, bg) >= KARAOKE_ACTIVE_CONTRAST,
            "activa χ {} < 4.5",
            contrast_ratio(c.current, bg)
        );
        assert!(
            c.current[0] >= 200 && c.current[1] >= 200,
            "se resuelve claro: {:?}",
            c.current
        );
    }

    #[test]
    fn impossible_target_returns_best_endpoint() {
        // Gris medio 128 no puede llegar a 21:1; devuelve el extremo elegido
        // (blanco si el fondo es oscuro), nunca un promedio ambiguo.
        let out = ensure_contrast([128, 128, 128], [20, 14, 26], 21.0);
        assert_eq!(out, [255, 255, 255]);
    }

    #[test]
    fn karaoke_colors_meet_thresholds_on_dark_covers() {
        // El caso habitual (portada oscura): la escena aplacada es muy oscura
        // y los tres estados deben superar sus umbrales sin coste de tinte.
        let dark = [[40u8, 30, 90], [120, 60, 40], [10, 80, 110]];
        let p = VisualPalette::from_cover(Some(dark));
        let c = p.karaoke_colors();
        let bg = p.karaoke_bg_ceiling();
        assert!(
            contrast_ratio(c.current, bg) >= KARAOKE_ACTIVE_CONTRAST,
            "activa χ {} < 4.5",
            contrast_ratio(c.current, bg)
        );
        assert!(
            contrast_ratio(c.read, bg) >= KARAOKE_DIM_CONTRAST,
            "leída χ {} < 3",
            contrast_ratio(c.read, bg)
        );
        assert!(
            contrast_ratio(c.unread, bg) >= KARAOKE_DIM_CONTRAST,
            "no leída χ {} < 3",
            contrast_ratio(c.unread, bg)
        );
        // Jerarquía: la línea en lectura nunca queda detrás de la histórica.
        assert!(
            contrast_ratio(c.current, bg) >= contrast_ratio(c.read, bg),
            "la activa es siempre la de mayor contraste"
        );
    }

    #[test]
    fn karaoke_colors_adapt_to_light_covers() {
        // Portada luminosa: el propio fondo aplacado (~0.45*0.22 del dominante
        // + ceiling) puede quedar claro; los colores resueltos al revés.
        let light = [[220u8, 180, 150], [150, 220, 160], [200, 210, 230]];
        let p = VisualPalette::from_cover(Some(light));
        let c = p.karaoke_colors();
        let bg = p.karaoke_bg_ceiling();
        for (name, color) in [
            ("activa", c.current),
            ("leída", c.read),
            ("no leída", c.unread),
        ] {
            assert!(
                contrast_ratio(color, bg) >= KARAOKE_DIM_CONTRAST,
                "{name} χ {} < 3",
                contrast_ratio(color, bg)
            );
        }
    }

    #[test]
    fn karaoke_colors_work_without_cover() {
        // Paleta por defecto (sin portada): mismo contrato de contraste.
        let p = VisualPalette::fallback();
        let c = p.karaoke_colors();
        let bg = p.karaoke_bg_ceiling();
        assert!(contrast_ratio(c.current, bg) >= KARAOKE_ACTIVE_CONTRAST);
        assert!(contrast_ratio(c.read, bg) >= KARAOKE_DIM_CONTRAST);
        assert!(contrast_ratio(c.unread, bg) >= KARAOKE_DIM_CONTRAST);
    }

    #[test]
    fn falls_short_of_cover_reaches_some_contrast() {
        // Caso límite: portada donde dominante y accent son casi tan claros
        // como el techo del fondo. El resultado se degrada hacia negro/blanco
        // según aplique y conserva el mejor ratio posible.
        let p =
            VisualPalette::from_cover(Some([[130u8, 130, 130], [160, 160, 160], [120, 120, 120]]));
        let c = p.karaoke_colors();
        let bg = p.karaoke_bg_ceiling();
        // Con dominante gris, garantizamos al menos el umbral de las históricas.
        assert!(contrast_ratio(c.unread, bg) >= KARAOKE_DIM_CONTRAST);
        assert!(
            contrast_ratio(c.current, bg) >= KARAOKE_ACTIVE_CONTRAST
                || relative_luminance(bg) > 0.2,
            "sobre fondo intermedio se degrada al mejor posible: χ {}",
            contrast_ratio(c.current, bg)
        );
    }
}
