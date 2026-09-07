//! Vistas de la TUI y su mapeo a atajos de teclado (Shift+1..Shift+8).

use crossterm::event::{KeyCode, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    NowPlaying,
    Related,
    Search,
    Sources,
    Metadata,
    History,
    Settings,
    Playlists,
}

impl View {
    pub const ALL: [View; 8] = [
        View::NowPlaying,
        View::Related,
        View::Search,
        View::Sources,
        View::Metadata,
        View::History,
        View::Settings,
        View::Playlists,
    ];

    /// Atajo de vista. Recibe el `KeyCode` (sin modificadores) y devuelve la
    /// vista asociada al dígito. Se usa desde `Shift+1..Shift+8` (navegación
    /// global) tanto en la UI como en los tests.
    pub fn from_shortcut(key: KeyCode) -> Option<View> {
        match key {
            KeyCode::Char('1') => Some(View::NowPlaying),
            KeyCode::Char('2') => Some(View::Related),
            KeyCode::Char('3') => Some(View::Search),
            KeyCode::Char('4') => Some(View::Sources),
            KeyCode::Char('5') => Some(View::Metadata),
            KeyCode::Char('6') => Some(View::History),
            KeyCode::Char('7') => Some(View::Settings),
            KeyCode::Char('8') => Some(View::Playlists),
            _ => None,
        }
    }

    /// Navegación `Shift+dígito`. El terminal no siempre reporta el
    /// modificador `SHIFT` junto al dígito: por eso se aceptan las dos formas
    /// en las que puede llegar la tecla:
    ///
    /// - `Char('1'..'8')` + `SHIFT` (protocolo kitty / CSI-u), independiente
    ///   del layout del teclado.
    /// - El símbolo que produce el desplazamiento según el layout. Con el
    ///   protocolo kitty y "alternate keys", o en terminales sin ese
    ///   protocolo, llega el carácter desplazado (p. ej. `@` en US, `"` en
    ///   latam) y crossterm puede limpiar el modificador SHIFT.
    ///
    /// Se cubren los símbolos de `Shift+1..Shift+8` de los layouts US
    /// (`!@#$%^&*`) y latam (`!"#$%&/()`). `&` es ambiguo
    /// (US: Shift+7 · latam/es: Shift+6) y se resuelve hacia latam/es.
    ///
    /// Un dígito suelto (sin `SHIFT`) devuelve `None`.
    pub fn from_shift_key(code: KeyCode, modifiers: KeyModifiers) -> Option<View> {
        let KeyCode::Char(c) = code else {
            return None;
        };
        let digit = if c.is_ascii_digit() && modifiers.contains(KeyModifiers::SHIFT) {
            Some(c)
        } else {
            symbol_to_digit(c).map(|d| char::from_digit(d, 10).unwrap())
        }?;
        Self::from_shortcut(KeyCode::Char(digit))
    }

    pub fn label(self) -> &'static str {
        match self {
            View::NowPlaying => "Now Playing",
            View::Related => "Related",
            View::Search => "Search",
            View::Sources => "Sources",
            View::Metadata => "Metadata",
            View::History => "History",
            View::Settings => "Settings",
            View::Playlists => "Playlists",
        }
    }

    /// Dígito del atajo (1..8).
    pub fn shortcut_digit(self) -> &'static str {
        match self {
            View::NowPlaying => "1",
            View::Related => "2",
            View::Search => "3",
            View::Sources => "4",
            View::Metadata => "5",
            View::History => "6",
            View::Settings => "7",
            View::Playlists => "8",
        }
    }
}

/// Símbolo `Shift+dígito` → dígito (1..8), cubriendo los layouts US, latam y
/// español. `&` es ambiguo entre Shift+6 (latam/es) y Shift+7 (US); se elige
/// latam/es por ser el entorno habitual de esta TUI.
fn symbol_to_digit(c: char) -> Option<u32> {
    match c {
        '!' => Some(1),
        '@' | '"' => Some(2),
        '#' | '·' => Some(3),
        '$' => Some(4),
        '%' => Some(5),
        '^' | '&' => Some(6),
        '/' => Some(7),
        '*' | '(' => Some(8),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyEvent;

    use super::*;

    /// `Shift+dígito` reportado como el símbolo del layout (sin kitty, o con
    /// "alternate keys"): debe navegar a `view`.
    fn expect_symbol_maps_to_view(symbol: char, view: View) {
        let key = KeyEvent::new(KeyCode::Char(symbol), KeyModifiers::NONE);
        assert_eq!(View::from_shift_key(key.code, key.modifiers), Some(view));
    }

    #[test]
    fn us_layout_shift_symbols_navigate() {
        expect_symbol_maps_to_view('!', View::NowPlaying);
        expect_symbol_maps_to_view('@', View::Related);
        expect_symbol_maps_to_view('#', View::Search);
        expect_symbol_maps_to_view('$', View::Sources);
        expect_symbol_maps_to_view('%', View::Metadata);
        expect_symbol_maps_to_view('^', View::History);
        expect_symbol_maps_to_view('*', View::Playlists);
    }

    #[test]
    fn latam_layout_shift_symbols_navigate() {
        // latam: Shift+2 = " (Related), Shift+6 = & (History), Shift+7 = / (Settings), Shift+8 = ).
        expect_symbol_maps_to_view('"', View::Related);
        expect_symbol_maps_to_view('&', View::History);
        expect_symbol_maps_to_view('/', View::Settings);
        expect_symbol_maps_to_view('(', View::Playlists);
    }

    #[test]
    fn spanish_layout_shift_symbols_navigate() {
        // es: Shift+3 = · (Search).
        expect_symbol_maps_to_view('·', View::Search);
    }

    #[test]
    fn unrelated_symbols_do_not_navigate() {
        assert_eq!(
            View::from_shift_key(KeyCode::Char('?'), KeyModifiers::NONE),
            None
        );
        assert_eq!(
            View::from_shift_key(KeyCode::Char('0'), KeyModifiers::NONE),
            None
        );
        assert_eq!(
            View::from_shift_key(KeyCode::Char('a'), KeyModifiers::NONE),
            None
        );
    }

    #[test]
    fn digits_require_shift() {
        // Un dígito suelto nunca navega; con SHIFT (protocolo kitty) sí.
        assert_eq!(
            View::from_shift_key(KeyCode::Char('2'), KeyModifiers::NONE),
            None
        );
        assert_eq!(
            View::from_shift_key(KeyCode::Char('2'), KeyModifiers::SHIFT),
            Some(View::Related)
        );
    }
}
