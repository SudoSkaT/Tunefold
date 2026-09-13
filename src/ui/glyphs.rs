//! Símbolos funcionales de la TUI, centralizados en un único punto.
//!
//! Todo carácter con carga semántica (corazón, estados de reproducción,
//! marcadores de cola, badges de metadata) vive aquí y solo aquí, con dos
//! temas: [`GlyphTheme::Unicode`] (predeterminado) y [`GlyphTheme::Ascii`]
//! (respaldo portable para terminales/fuentes sin esos glifos).
//!
//! Reglas del sistema:
//!
//! - Ningún estado funcional depende SOLO del glifo: cada uso va acompañado de
//!   una palabra (p. ej. `▶ reproduciendo`, `♥ L1K3D`, `● reciente · 3×`).
//! - Se evitan emojis (ancho variable, fallback inconsistente entre
//!   terminales) como dependencia funcional. El corazón usa `♥`/`♡`, glifos
//!   de ancho 1 presentes en las fuentes monoespaciadas habituales, con
//!   `*`/`o` como tema ASCII.
//! - Los glifos Unicode elegidos son de la mitad inferior de los planos
//!   básicos (U+25xx, U+26xx, U+27xx, U+2Axx), consistentes en DejaVu Sans
//!   Mono y fuentes terminales dominantes. Los emoji de planos suplementarios
//!   (`🎚`, `⏸`, `⏹`, `⏳`) se eliminan: eran la causa de los caracteres
//!   corruptos/fallback en terminales sin esas fuentes.

/// Tema de glifos activo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphTheme {
    /// Símbolos Unicode de ancho estable y presencia generalizada.
    Unicode,
    /// Respaldo 7-bit ASCII, legible en cualquier terminal.
    Ascii,
}

impl GlyphTheme {
    /// Tema por defecto para la sesión.
    ///
    /// - La variable `TUNEFOLD_GLYPHS=ascii` fuerza el tema ASCII.
    /// - `TERM=dumb` o la consola de Linux (`linux*`) usan ASCII porque sus
    ///   fuentes VGA carecen de muchos glifos Unicode.
    fn detect() -> Self {
        if std::env::var_os("TUNEFOLD_GLYPHS").is_some_and(|v| v.eq_ignore_ascii_case("ascii")) {
            return Self::Ascii;
        }
        match std::env::var("TERM").as_deref() {
            Ok(term) if term.is_empty() || term == "dumb" || term.starts_with("linux") => {
                Self::Ascii
            }
            _ => Self::Unicode,
        }
    }
}

/// Juego de glifos de la sesión. Se construye una vez (avaro trivial) y se
/// consulta vía [`GLYPHS`]; cada método devuelve `&'static str` o un `char`
/// de ancho 1 para que el render no asigne por fila.
#[derive(Debug, Clone, Copy)]
pub struct UiGlyphs {
    theme: GlyphTheme,
}

/// Tema activo calculado una sola vez por proceso.
pub static GLYPHS: std::sync::LazyLock<UiGlyphs> =
    std::sync::LazyLock::new(|| UiGlyphs::new(GlyphTheme::detect()));

/// Secuencia de barras braille del spinner de actividad (~15 Hz, 8 fases).
const SPINNER_BRAILLE: [char; 8] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧'];
/// Respaldo ASCII clásico del spinner.
const SPINNER_ASCII: [char; 4] = ['-', '\\', '|', '/'];

/// Ecualizador de actividad de reproducción (4 fases, ancho 3).
const EQ_UNICODE: [&str; 4] = ["▁▄▁", "▂▅▃", "▃▆▅", "▂▅▃"];
/// Ecualizador ASCII: un punto que se desplaza.
const EQ_ASCII: [&str; 4] = ["*..", ".*.", "..*", ".*."];

/// Fases del spinner de actividad: 8 de braille (Unicode) o 4 de rayas
/// (respaldo ASCII).
impl UiGlyphs {
    pub const fn new(theme: GlyphTheme) -> Self {
        Self { theme }
    }

    pub fn theme(self) -> GlyphTheme {
        self.theme
    }

    fn pick(self, unicode: &'static str, ascii: &'static str) -> &'static str {
        match self.theme {
            GlyphTheme::Unicode => unicode,
            GlyphTheme::Ascii => ascii,
        }
    }

    // ------------------------------------------------------------ corazón

    /// Corazón de L1K3D (liked). Unicode `♥` / ASCII `*`.
    pub fn heart_liked(self) -> &'static str {
        self.pick("♥", "*")
    }

    /// Corazón vacío de L1K3D (no liked). Unicode `♡` / ASCII `o`.
    pub fn heart_empty(self) -> &'static str {
        self.pick("♡", "o")
    }

    // ------------------------------------------------------ reproducción

    pub fn play(self) -> &'static str {
        self.pick("▶", ">")
    }

    pub fn pause(self) -> &'static str {
        self.pick("‖", "||")
    }

    pub fn stop(self) -> &'static str {
        self.pick("■", "[]")
    }

    /// Fase del spinner de actividad (buffering/stream lento).
    pub fn spinner(self, frame: u64) -> char {
        match self.theme {
            GlyphTheme::Unicode => SPINNER_BRAILLE[frame as usize % SPINNER_BRAILLE.len()],
            GlyphTheme::Ascii => SPINNER_ASCII[frame as usize % SPINNER_ASCII.len()],
        }
    }

    /// Glifo decorativo de "buscando" (va acompañado de la palabra).
    pub fn seeking(self) -> &'static str {
        self.pick("»", ">>")
    }

    /// Ecualizador animado de actividad de reproducción (3 columnas).
    ///
    /// Decorativo por diseño: el estado real siempre lleva su palabra
    /// ("reproduciendo").
    pub fn activity(self, frame: u64) -> &'static str {
        let f = frame as usize % 4;
        match self.theme {
            GlyphTheme::Unicode => EQ_UNICODE[f],
            GlyphTheme::Ascii => EQ_ASCII[f],
        }
    }

    // --------------------------------------------------------- marcadores

    /// Marcador de fila: track en curso.
    pub fn current(self) -> &'static str {
        self.pick("▶ ", "> ")
    }

    /// Marcador de fila: recién añadida a la cola (verde).
    pub fn newly_added(self) -> &'static str {
        self.pick("✚ ", "+ ")
    }

    /// Marcador de fila: aportada por el autoplay (tenue).
    pub fn auto(self) -> &'static str {
        self.pick("↻ ", "~ ")
    }

    /// Marcador de fila: elección explícita del usuario (tenue).
    pub fn explicit(self) -> &'static str {
        self.pick("· ", "- ")
    }

    /// Marcador de selección resaltada de las listas.
    pub fn select(self) -> &'static str {
        "> "
    }

    // ----------------------------------------------------------- badges

    /// Badge de "reciente" (metadata de escucha). El texto `reciente` va
    /// siempre a continuación; el glifo es decorativo.
    pub fn recent(self) -> &'static str {
        self.pick("●", "*")
    }

    /// Badge de conteo de reproducción en una fila.
    pub fn listens(self) -> &'static str {
        self.pick("↻", "~")
    }

    /// Badge de "recomendación relacionada" de la búsqueda.
    pub fn recommended(self) -> &'static str {
        self.pick("↳ ", "-> ")
    }

    /// Playlist del sistema (L1K3D).
    pub fn playlist_system(self) -> &'static str {
        self.pick("♥ ", "* ")
    }

    /// Playlist de usuario.
    pub fn playlist_user(self) -> &'static str {
        self.pick("♪ ", "~ ")
    }

    /// Fuente activa en el panel de fuentes.
    pub fn active_source(self) -> &'static str {
        self.pick("● ", "* ")
    }

    /// Prefijo del campo de texto (input de crear playlist, etc.).
    pub fn prompt(self) -> &'static str {
        self.pick("▸ ", "> ")
    }

    /// Cursor de edición.
    pub fn edit_cursor(self) -> &'static str {
        self.pick("▌", "|")
    }

    /// Aviso del pie de diagnóstico (área derecha de la barra de estado).
    pub fn notice(self) -> &'static str {
        "!"
    }

    /// Prefijo de la línea de estado.
    pub fn status(self) -> &'static str {
        self.pick("›", ">")
    }

    // ------------------------------------------ osciloscopio estéreo (§8)

    /// Punto de trazo del canal IZQUIERDO (ch0).
    pub fn trace_left(self) -> &'static str {
        self.pick("●", "*")
    }

    /// Punto de trazo del canal DERECHO (ch1).
    pub fn trace_right(self) -> &'static str {
        self.pick("●", "*")
    }

    /// Punto de trazo cuando L y R comparten celda (ambos caen ahí).
    pub fn trace_both(self) -> &'static str {
        self.pick("●", "*")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const U: UiGlyphs = UiGlyphs::new(GlyphTheme::Unicode);
    const A: UiGlyphs = UiGlyphs::new(GlyphTheme::Ascii);

    #[test]
    fn every_state_has_an_ascii_fallback() {
        assert_ne!(U.heart_liked(), A.heart_liked());
        assert_ne!(U.heart_empty(), A.heart_empty());
        assert_ne!(U.play(), A.play());
        assert_ne!(U.pause(), A.pause());
        assert_ne!(U.stop(), A.stop());
        assert_ne!(U.seeking(), A.seeking());
        assert_ne!(U.trace_left(), A.trace_left());
        assert_ne!(U.trace_right(), A.trace_right());
        assert_ne!(U.trace_both(), A.trace_both());
        let ascii_variants = [
            A.current(),
            A.newly_added(),
            A.auto(),
            A.explicit(),
            A.recent(),
            A.listens(),
            A.recommended(),
            A.playlist_system(),
            A.playlist_user(),
            A.active_source(),
            A.prompt(),
            A.status(),
            A.trace_left(),
            A.trace_right(),
            A.trace_both(),
        ];
        for name in ascii_variants {
            assert_eq!(
                name.chars().filter(|c| !c.is_ascii()).count(),
                0,
                "el tema ASCII de «{name}» debe ser 7-bit ASCII"
            );
        }
    }

    #[test]
    fn trace_points_are_width_one_in_both_themes() {
        for g in [U.trace_left(), U.trace_right(), U.trace_both()] {
            assert_eq!(g.chars().count(), 1, "punto «{g}» es un único carácter");
        }
        for g in [A.trace_left(), A.trace_right(), A.trace_both()] {
            assert_eq!(g.chars().count(), 1);
        }
    }

    #[test]
    fn functional_glyphs_are_single_column() {
        // Los glifos de cola/marcadores no rompen la alineación de la fila:
        // un carácter (Unicode de ancho 1) + un espacio.
        for g in [U.current(), U.newly_added(), U.auto(), U.explicit()] {
            assert_eq!(g.chars().count(), 2, "marcador «{g}» = glifo + espacio");
            assert_eq!(g.bytes().last(), Some(b' '), "siempre con espacio");
        }
    }

    #[test]
    fn heart_is_width_one_in_both_themes() {
        assert_eq!(U.heart_liked().chars().count(), 1);
        assert_eq!(U.heart_empty().chars().count(), 1);
        assert_eq!(A.heart_liked().chars().count(), 1);
        assert_eq!(A.heart_empty().chars().count(), 1);
    }

    #[test]
    fn spinner_cycles_without_panicking() {
        for f in 0..32 {
            let c = U.spinner(f);
            assert_eq!(c.len_utf8(), c.len_utf8().min(4));
            assert!(SPINNER_BRAILLE.contains(&c) || SPINNER_ASCII.contains(&c));
        }
    }

    #[test]
    fn activity_always_three_columns() {
        for f in 0..16 {
            assert_eq!(U.activity(f).chars().count(), 3);
            assert_eq!(A.activity(f).chars().count(), 3);
        }
    }

    #[test]
    fn theme_detect_prefers_unicode_by_default() {
        assert_eq!(U.theme(), GlyphTheme::Unicode);
        assert_eq!(A.theme(), GlyphTheme::Ascii);
    }

    #[test]
    fn pick_returns_both_branches() {
        assert_eq!(U.pick("x", "y"), "x");
        assert_eq!(A.pick("x", "y"), "y");
    }
}
