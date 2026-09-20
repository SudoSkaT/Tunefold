//! Perfil de terminal y geometría adaptativa de las vistas.
//!
//! La resolución decide CÓMO se organiza la información, nunca QUÉ información
//! se muestra: todos los perfiles conservan las mismas secciones y solo
//! cambian el reparto (compactación, apilado y reducción de espacios
//! decorativos antes que scroll interno).

use ratatui::layout::{Constraint, Rect};

/// Perfil de terminal derivado del área de la pantalla.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalProfile {
    /// Terminal amplia: espaciado confortable, artwork grande.
    Large,
    /// Terminal media: se reducen huecos, se mantiene todo.
    Medium,
    /// Terminal pequeña: layouts verticales y elementos compactados.
    Small,
    /// Terminal excepcionalmente pequeña: máxima compactación con scroll.
    Tiny,
}

impl TerminalProfile {
    /// Perfil desde el área total de la pantalla.
    pub fn from_rect(area: Rect) -> Self {
        let (w, h) = (area.width, area.height);
        if w >= 100 && h >= 30 {
            Self::Large
        } else if w >= 80 && h >= 24 {
            Self::Medium
        } else if w >= 60 && h >= 16 {
            Self::Small
        } else {
            Self::Tiny
        }
    }

    /// Filas del casco de la app: `(encabezado, pie de estado)`. En el perfil
    /// Tiny el encabezado comprime su barra de atajos a una sola línea.
    pub fn shell_heights(self) -> (u16, u16) {
        match self {
            Self::Large | Self::Medium | Self::Small => (3, 1),
            Self::Tiny => (2, 1),
        }
    }

    /// Altura del panel de recomendaciones cuando el perfil no comprime (el
    /// resto del espacio se le da a la tarjeta y los controles).
    fn recs_default(self) -> u16 {
        match self {
            Self::Large | Self::Medium => 7,
            Self::Small => 5,
            Self::Tiny => 4,
        }
    }

    /// Rango `(mín, máx)` de la altura de la tarjeta del Now Playing.
    fn card_range(self) -> (u16, u16) {
        match self {
            Self::Large => (12, 34),
            Self::Medium => (10, 26),
            Self::Small => (8, 18),
            Self::Tiny => (6, 12),
        }
    }

    /// Mínima altura que se garantiza a las secciones no-tarjeta.
    fn other_floor(self) -> (u16, u16, u16) {
        // (progreso, visual, control) — consistentes con dashboard_layout.
        match self {
            Self::Large | Self::Medium => (3, 4, 3),
            Self::Small => (2, 3, 2),
            Self::Tiny => (1, 2, 2),
        }
    }
}

/// Reparto vertical del cuerpo de la vista Now Playing.
///
/// Las cinco secciones (tarjeta, banda visual, barra de progreso,
/// recomendaciones y controles) están SIEMPRE presentes: el reparto cambia
/// con el perfil y, si el terminal no alcanza, las secciones se compactan en
/// orden de prioridad (progreso → visual → recomendaciones → controles →
/// tarjeta) sin que ninguna llegue a desaparecer por debajo de 1 fila.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DashboardLayout {
    /// Tarjeta Now Playing (título, artista, álbum, duración, fuente, arte).
    pub card: u16,
    /// Banda del visualizador.
    pub visual: u16,
    /// Barra de progreso.
    pub progress: u16,
    /// Panel de recomendaciones (lista con scroll interno).
    pub recs: u16,
    /// Estado de reproducción / modos de cola / L1K3D.
    pub controls: u16,
}

impl DashboardLayout {
    /// Constraints verticales listos para `Layout::split`.
    pub fn constraints(self) -> [Constraint; 5] {
        [
            Constraint::Length(self.card),
            Constraint::Length(self.visual),
            Constraint::Length(self.progress),
            Constraint::Length(self.recs),
            Constraint::Length(self.controls),
        ]
    }

    pub fn total(self) -> u16 {
        self.card + self.visual + self.progress + self.recs + self.controls
    }
}

/// Calcula el reparto del cuerpo de Now Playing (que debe ocupar `body`
/// exactamente, sin hueco residual) para el perfil dado.
pub fn dashboard_layout(body: Rect, profile: TerminalProfile) -> DashboardLayout {
    let avail = body.height;
    let (card_min, card_max) = profile.card_range();
    let (progress_floor, visual_floor, controls_floor) = profile.other_floor();
    let mut card = card_min;
    let mut visual = visual_floor;
    let mut progress = progress_floor;
    let mut recs = profile.recs_default();
    let mut controls = controls_floor;

    // Si no cabe el reparto del perfil, se compacta en orden de prioridad
    // (lo menos esencial primero), sin eliminar ninguna sección.
    loop {
        let total = card + visual + progress + recs + controls;
        if total <= avail {
            break;
        }
        if progress > 1 {
            progress -= 1;
        } else if visual > 1 {
            visual -= 1;
        } else if recs > 3 {
            recs -= 1;
        } else if controls > 1 {
            controls -= 1;
        } else if card > 3 {
            card -= 1;
        } else {
            break;
        }
    }

    // El espacio sobrante crece en orden de prioridad — primero la tarjeta
    // (hasta su tope), luego recomendaciones (≤10) y controles (≤8), y solo
    // al final visual/progreso — hasta llenar el cuerpo exactamente sin
    // huecos residuales.
    let (recs_cap, controls_cap, visual_cap, progress_cap) = match profile {
        TerminalProfile::Large => (10, 8, 8, 6),
        TerminalProfile::Medium => (9, 6, 6, 5),
        TerminalProfile::Small => (8, 4, 4, 4),
        TerminalProfile::Tiny => (6, 3, 3, 3),
    };
    loop {
        let total = card + visual + progress + recs + controls;
        if total >= avail {
            break;
        }
        if card < card_max {
            card += 1;
        } else if recs < recs_cap {
            recs += 1;
        } else if controls < controls_cap {
            controls += 1;
        } else if visual < visual_cap {
            visual += 1;
        } else if progress < progress_cap {
            progress += 1;
        } else {
            break;
        }
    }

    DashboardLayout {
        card,
        visual,
        progress,
        recs,
        controls,
    }
}

/// Ancho máximo del contenido centrado de Related (banda visual +
/// recomendaciones).
///
/// En terminales muy anchos el waveform no debe estirarse indefinidamente: más
/// allá de ~100 columnas cada columna cubre <1 bucket y el trazo gana puntos
/// redundantes en vez de detalle. El espacio sobrante queda como breathing
/// room lateral y la composición se percibe centrada.
pub const RELATED_MAX_WIDTH: u16 = 100;

/// Composición vertical de la vista Related: banda visual + gap + lista.
///
/// Jerarquía objetivo (~40% visual+letras / ~30% recomendaciones del viewport,
/// resto breathing + casco): la banda tiene tope para no convertirse en un
/// waveform gigante y la lista conserva un mínimo funcional con scroll
/// interno. En terminales diminutos los mínimos mandan sobre la proporción.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelatedComposition {
    /// Altura de la banda superior (visual/letras).
    pub band: u16,
    /// Fila de aire entre banda y lista (0 en terminales bajos).
    pub gap: u16,
    /// Altura de la lista de recomendaciones.
    pub list: u16,
}

/// Calcula la composición de Related para un cuerpo de `body_h` filas.
///
/// - La banda persigue ~60% del cuerpo (≃40% del viewport con casco de 4) con
///   topes por perfil para no devorar la lista.
/// - El gap (1 fila) solo aparece con `body_h >= 20` y sitio sobrante.
/// - La lista se queda con el resto (mínimo 1 fila: scroll interno).
pub fn related_composition(body_h: u16, profile: TerminalProfile) -> RelatedComposition {
    let (min, max) = match profile {
        TerminalProfile::Large => (6, 16),
        TerminalProfile::Medium => (5, 13),
        TerminalProfile::Small => (4, 10),
        TerminalProfile::Tiny => (3, 8),
    };
    if body_h == 0 {
        return RelatedComposition {
            band: 0,
            gap: 0,
            list: 0,
        };
    }
    let target = ((body_h as u32 * 60) / 100) as u16;
    let gap: u16 = if body_h >= 20 { 1 } else { 0 };
    // Reserva mínima de la lista (2 filas: borde + 1 item) más el gap.
    let reserve = 2 + gap;
    let max_by_room = body_h.saturating_sub(reserve);
    let band = target.clamp(min, max).min(max_by_room).max(1).min(body_h);
    let list = body_h.saturating_sub(band + gap);
    RelatedComposition { band, gap, list }
}

/// Altura de la banda superior de Related (visual/letras) según el perfil.
/// La banda se conserva siempre (mínimo 3 filas); el resto del cuerpo va a la
/// lista de tracks.
///
/// Wrapper de compatibilidad sobre [`related_composition`].
pub fn related_band_height(body_h: u16, profile: TerminalProfile) -> u16 {
    related_composition(body_h, profile).band
}
/// Centra `area` horizontalmente con ancho máximo `max`.
/// Devuelve el mismo área si ya cabe; si no, un rectángulo centrado de ancho
/// `max` con la misma `y`/`height`. Sin allocs, O(1).
pub fn centered_width(area: Rect, max: u16) -> Rect {
    if area.width <= max {
        return area;
    }
    let dx = (area.width - max) / 2;
    Rect {
        x: area.x + dx,
        y: area.y,
        width: max,
        height: area.height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prof(w: u16, h: u16) -> TerminalProfile {
        TerminalProfile::from_rect(Rect::new(0, 0, w, h))
    }

    #[test]
    fn thresholds_map_to_profiles() {
        assert_eq!(prof(120, 40), TerminalProfile::Large);
        assert_eq!(prof(100, 30), TerminalProfile::Large);
        assert_eq!(prof(80, 24), TerminalProfile::Medium);
        assert_eq!(prof(70, 20), TerminalProfile::Small);
        assert_eq!(prof(60, 15), TerminalProfile::Tiny);
    }

    #[test]
    fn shell_heights_only_shrink_in_tiny() {
        assert_eq!(TerminalProfile::Large.shell_heights(), (3, 1));
        assert_eq!(TerminalProfile::Medium.shell_heights(), (3, 1));
        assert_eq!(TerminalProfile::Small.shell_heights(), (3, 1));
        assert_eq!(TerminalProfile::Tiny.shell_heights(), (2, 1));
    }

    /// El reparto incluye TODAS las secciones (ninguna desaparece) en todos
    /// los tamaños requeridos.
    #[test]
    fn dashboard_keeps_all_sections_at_every_size() {
        for (w, h) in [(120, 40), (100, 30), (80, 24), (70, 20), (60, 15), (60, 12)] {
            let area = Rect::new(0, 0, w, h);
            let profile = TerminalProfile::from_rect(area);
            let (header, status) = profile.shell_heights();
            let body_h = h.saturating_sub(header + status);
            if body_h < 6 {
                continue; // terminal diminuto irreal: sin lugar físico para nada
            }
            let body = Rect::new(0, header, w, body_h);
            let l = dashboard_layout(body, profile);
            assert_eq!(l.total(), body_h, "el reparto llena exactamente el cuerpo");
            assert!(l.card >= 3, "tarjeta presente ({l:?})");
            assert!(l.visual >= 1, "visual presente ({l:?})");
            assert!(l.progress >= 1, "progreso presente ({l:?})");
            assert!(l.recs >= 3, "recomendaciones presentes ({l:?})");
            assert!(l.controls >= 1, "controles presentes ({l:?})");
        }
    }

    #[test]
    fn dashboard_grows_card_then_recs_then_controls() {
        // Terminal muy alta: la tarjeta crece hasta su tope (34), luego las
        // recomendaciones (10) y los controles (8), siempre llenando el cuerpo.
        let body = Rect::new(0, 4, 120, 60);
        let l = dashboard_layout(body, TerminalProfile::Large);
        assert_eq!(l.card, 34, "tarjeta al tope en terminal amplia");
        assert_eq!(l.recs, 10);
        assert_eq!(l.controls, 8);
        assert_eq!(l.total(), 60);
    }

    #[test]
    fn dashboard_compacts_but_never_drops_sections() {
        // 70x20 → cuerpo 16: el reparto Small (8+3+2+5+2=20) se compacta a 16
        // sin perder ninguna sección.
        let body = Rect::new(0, 3, 70, 16);
        let l = dashboard_layout(body, TerminalProfile::Small);
        assert_eq!(l.total(), 16);
        assert!(l.visual >= 1 && l.progress >= 1 && l.controls >= 1);
        assert!(l.recs >= 3);
        assert!(l.card >= 3);
    }

    #[test]
    fn related_band_never_grows_beyond_small_terminals() {
        for (h, profile) in [
            (20, TerminalProfile::Small),
            (15, TerminalProfile::Tiny),
            (12, TerminalProfile::Tiny),
        ] {
            let band = related_band_height(h, profile);
            assert!((3..=18).contains(&band), "banda {profile:?}@{h}: {band}");
            assert!(band + 2 <= h, "siempre queda sitio para la lista");
        }
    }

    #[test]
    fn related_composition_hits_target_proportions() {
        // Composición objetivo ~55% banda / resto lista del cuerpo (≈40/30 del
        // viewport con casco): la banda persigue la proporción con topes y la
        // lista conserva el resto con scroll interno.
        let cases: &[(u16, TerminalProfile)] = &[
            (16, TerminalProfile::Small),  // 60x20
            (20, TerminalProfile::Medium), // 80x24
            (26, TerminalProfile::Large),  // 100x30 / 120x30
            (36, TerminalProfile::Large),  // 140x40
            (46, TerminalProfile::Large),  // 160x50
        ];
        for (body_h, profile) in cases {
            let c = related_composition(*body_h, *profile);
            assert_eq!(
                c.band + c.gap + c.list,
                *body_h,
                "la composición llena el cuerpo sin huecos ni solapes"
            );
            // Banda ≈40-60% del cuerpo (proporción, no rigidez matemática);
            // en cuerpos enormes manda el tope para no crear un waveform
            // gigante y el resto es lista con scroll + breathing.
            if *body_h > 30 {
                assert_eq!(c.band, 16, "banda topada en cuerpo {body_h}: {c:?}");
            } else {
                let frac = c.band as f32 / *body_h as f32;
                assert!(
                    (0.35..=0.65).contains(&frac),
                    "banda ~55% del cuerpo {body_h}: {c:?}"
                );
            }
            // Lista ≈30%+ del cuerpo (mínimo funcional con scroll).
            assert!(
                c.list as f32 / *body_h as f32 >= 0.25 || c.list >= 5,
                "recomendaciones visibles en cuerpo {body_h}: {c:?}"
            );
        }
    }

    #[test]
    fn related_composition_caps_band_and_keeps_list() {
        // En terminales grandes la banda NO crece indefinidamente (tope 14) y
        // la lista conserva filas suficientes para varias recomendaciones.
        let c = related_composition(46, TerminalProfile::Large);
        assert!(c.band <= 16, "banda topada: {c:?}");
        assert!(c.list >= 10, "lista amplia: {c:?}");
        assert_eq!(c.gap, 1, "breathing entre secciones: {c:?}");
        // En terminales bajos no hay gap y los mínimos mandan.
        let tiny = related_composition(10, TerminalProfile::Tiny);
        assert_eq!(tiny.gap, 0);
        assert!(tiny.band >= 3 && tiny.list >= 1, "mínimos: {tiny:?}");
        assert_eq!(tiny.band + tiny.list, 10);
    }

    #[test]
    fn centered_width_caps_and_centers_content() {
        // Ancho normal: intacto. Ancho excesivo: centrado a RELATED_MAX_WIDTH.
        let area = Rect::new(0, 0, 60, 20);
        assert_eq!(centered_width(area, RELATED_MAX_WIDTH), area);
        let wide = Rect::new(0, 0, 160, 50);
        let c = centered_width(wide, RELATED_MAX_WIDTH);
        assert_eq!(c.width, RELATED_MAX_WIDTH);
        assert_eq!(c.x, (160 - RELATED_MAX_WIDTH) / 2, "centrado: {c:?}");
        assert_eq!(c.y, 0);
        assert_eq!(c.height, 50);
        // Sin solape vertical: banda + gap + lista particionan el cuerpo.
        let comp = related_composition(46, TerminalProfile::Large);
        assert_eq!(comp.band + comp.gap + comp.list, 46);
    }
}
