//! Resolución de directorios de datos según la especificación XDG.
//!
//! Separa explícitamente los tipos de datos (Fase de actualización):
//!
//! - Configuración (`~/.config/tunefold`) — persistente, nunca se toca.
//! - Datos de usuario (`~/.local/share/tunefold`) — persistente, nunca se toca.
//! - Caché (`~/.cache/tunefold`) — reciclable libremente.
//! - Logs/estado (`~/.local/state/tunefold`) — reciclable.
//!
//! Históricamente los datos vivían en `data/` relativo al directorio de
//! trabajo ([`legacy_db_path`]); [`migrate_legacy_data`] los **copia** (nunca
//! borra) a su destino canónico la primera vez.

use std::ffi::OsString;
use std::path::PathBuf;

const APP_NAME: &str = "tunefold";
/// Nombre de la carpeta de exportaciones dentro del directorio de música.
const DOWNLOADS_DIR_NAME: &str = "Tunefold";

/// `$XDG_CONFIG_HOME` (o `~/.config`) + `tunefold`.
pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join(APP_NAME)
}

/// `$XDG_DATA_HOME` (o `~/.local/share`) + `tunefold`.
pub fn data_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share").join(APP_NAME)
}

/// `$XDG_CACHE_HOME` (o `~/.cache`) + `tunefold`.
pub fn cache_dir() -> PathBuf {
    xdg("XDG_CACHE_HOME", ".cache").join(APP_NAME)
}

/// `$XDG_STATE_HOME` (o `~/.local/state`) + `tunefold`.
pub fn state_dir() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state").join(APP_NAME)
}

/// Ruta canónica de la base de datos (`~/.local/share/tunefold/music.db`).
pub fn db_path() -> PathBuf {
    data_dir().join("music.db")
}

/// Ruta legacy de la base de datos (`data/music.db` en el CWD). Solo lectura.
pub fn legacy_db_path() -> PathBuf {
    PathBuf::from("data").join("music.db")
}

/// Copia (sin borrar) una base legacy `data/music.db` al destino canónico la
/// primera vez. Devuelve `true` si se migró, `false` si no hizo falta.
pub fn migrate_legacy_data() -> bool {
    if !legacy_db_path().exists() {
        return false;
    }
    let dest = db_path();
    if dest.exists() {
        // Ambas existen: la canónica tiene prioridad, no se toca nada.
        return false;
    }
    if let Some(parent) = dest.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return false;
        }
    }
    std::fs::copy(legacy_db_path(), &dest).is_ok()
}

/// Ruta del archivo `.env` canónico de configuración.
pub fn env_path() -> PathBuf {
    config_dir().join(".env")
}

/// Directorio canónico de descargas de audio (Fase 5, Nivel 1).
///
/// Prioridad (primera que exista como valor no vacío gana):
/// 1. `$TUNEFOLD_DOWNLOADS_DIR` (override explícito del usuario).
/// 2. `$XDG_MUSIC_DIR/Tunefold` (XDG user-dirs, si el sistema lo define).
/// 3. `~/Music/Tunefold` (convención de música del usuario).
///
/// Nunca es relativo al CWD por defecto (regla de hierro nº 4): quien quiera
/// `audio/tunefold` bajo la raíz del repo debe exportar
/// `TUNEFOLD_DOWNLOADS_DIR=audio/tunefold` de forma explícita y asume que ese
/// contenido no viaja con el repo (está en `.gitignore`).
pub fn downloads_dir() -> PathBuf {
    downloads_dir_from_lookup(|key| std::env::var_os(key))
}

fn downloads_dir_from_lookup(lookup: impl Fn(&str) -> Option<OsString>) -> PathBuf {
    if let Some(dir) = lookup("TUNEFOLD_DOWNLOADS_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    if let Some(music) = lookup("XDG_MUSIC_DIR") {
        if !music.is_empty() {
            return PathBuf::from(music).join(DOWNLOADS_DIR_NAME);
        }
    }
    match lookup("HOME") {
        Some(home) if !home.is_empty() => {
            PathBuf::from(home).join("Music").join(DOWNLOADS_DIR_NAME)
        }
        // Sin HOME: último recurso bajo datos canónicos (sigue sin ser CWD).
        _ => data_dir().join("downloads"),
    }
}

/// Subcarpeta de descarga de una playlist: `downloads_dir/<playlist>`.
///
/// El nombre se sanitiza para filesystem (sin separadores, sin `..`, sin
/// control, longitud acotada). `L1K3D` se conserva tal cual por ser nombre
/// reservado del producto.
pub fn playlist_download_dir(playlist_name: &str) -> PathBuf {
    downloads_dir().join(sanitize_playlist_dir(playlist_name))
}

/// Sanitiza un nombre de playlist para usarlo como carpeta.
///
/// - Recorta espacios externos; vacío → `sin-playlist`.
/// - `/`, `\` y NUL → `-`; `.` líder se neutraliza (evita `..`/ocultos).
/// - Solo conserva alfanumérico, `_-+. ()[]{}`, resto → `_`.
/// - Acota a 64 chars (límite conservador portable).
pub fn sanitize_playlist_dir(name: &str) -> String {
    const MAX: usize = 64;
    let trimmed = name.trim();
    let base = if trimmed.is_empty() {
        "sin-playlist".to_string()
    } else {
        let mut out = String::with_capacity(trimmed.len());
        for c in trimmed.chars() {
            let mapped = match c {
                '/' | '\\' | '\0' => '-',
                c if c.is_alphanumeric() => c,
                '-' | '_' | '+' | '.' | ' ' | '(' | ')' | '[' | ']' | '{' | '}' => c,
                _ => '_',
            };
            out.push(mapped);
            if out.chars().count() >= MAX {
                break;
            }
        }
        // Evita `..`, `.` y carpetas ocultas accidentales, además de guiones
        // líderes/restantes de una sanitización (`../x` → `x`, no `-x`).
        let clean = out.trim().trim_matches(['.', '-', ' ', '_']).trim();
        if clean.is_empty() {
            "sin-playlist".to_string()
        } else {
            clean.to_string()
        }
    };
    // `L1K3D` es nombre reservado: se conserva exacto si el usuario lo pidió.
    if name.trim() == crate::domain::playlist::Playlist::LIKED_NAME {
        return crate::domain::playlist::Playlist::LIKED_NAME.to_string();
    }
    base
}

fn xdg(var: &str, fallback_component: &str) -> PathBuf {
    if let Some(dir) = std::env::var_os(var) {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    match std::env::var_os("HOME") {
        Some(home) if !home.is_empty() => PathBuf::from(home).join(fallback_component),
        // Sin HOME: relativo al CWD (comportamiento histórico).
        _ => PathBuf::from(fallback_component),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xdg_uses_explicit_var_when_set() {
        const VAR: &str = "XDG_TUNEFOLD_TEST_DIR";
        let previous = std::env::var_os(VAR);
        std::env::set_var(VAR, "/x");
        assert_eq!(xdg(VAR, ".local/share"), PathBuf::from("/x"));
        match previous {
            Some(v) => std::env::set_var(VAR, v),
            None => std::env::remove_var(VAR),
        }
    }

    #[test]
    fn xdg_falls_back_to_home() {
        assert_eq!(xdg("XDG_SOMETHING_ABSENT", ".local/share"), {
            let home = std::env::var_os("HOME").unwrap();
            PathBuf::from(home).join(".local/share")
        });
    }

    #[test]
    fn app_dirs_are_namespaced_under_tunefold() {
        assert_eq!(
            data_dir().file_name(),
            Some(std::ffi::OsStr::new("tunefold"))
        );
        assert_eq!(
            db_path().file_name(),
            Some(std::ffi::OsStr::new("music.db"))
        );
    }

    #[test]
    fn legacy_is_read_only_path() {
        assert_eq!(legacy_db_path(), PathBuf::from("data").join("music.db"));
    }

    fn lookup_of<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |key| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| OsString::from(*v))
        }
    }

    #[test]
    fn downloads_prefers_explicit_override() {
        let dir = downloads_dir_from_lookup(lookup_of(&[
            ("TUNEFOLD_DOWNLOADS_DIR", "/tmp/mis-audios"),
            ("XDG_MUSIC_DIR", "/tmp/music"),
            ("HOME", "/home/u"),
        ]));
        assert_eq!(dir, PathBuf::from("/tmp/mis-audios"));
    }

    #[test]
    fn downloads_uses_xdg_music_when_no_override() {
        let dir = downloads_dir_from_lookup(lookup_of(&[
            ("XDG_MUSIC_DIR", "/tmp/music"),
            ("HOME", "/home/u"),
        ]));
        assert_eq!(dir, PathBuf::from("/tmp/music").join("Tunefold"));
    }

    #[test]
    fn downloads_falls_back_to_home_music() {
        let dir = downloads_dir_from_lookup(lookup_of(&[("HOME", "/home/u")]));
        assert_eq!(dir, PathBuf::from("/home/u").join("Music").join("Tunefold"));
    }

    #[test]
    fn downloads_never_defaults_to_cwd_relative() {
        // Sin HOME ni overrides: bajo data_dir, jamás `audio/` relativo.
        let dir = downloads_dir_from_lookup(|_| None);
        assert!(dir.is_absolute() || dir.starts_with(data_dir()));
        assert_ne!(dir, PathBuf::from("audio").join("tunefold"));
    }

    #[test]
    fn sanitize_playlist_dir_blocks_traversal_and_separators() {
        assert_eq!(sanitize_playlist_dir("../mi/lista\\x"), "mi-lista-x");
        assert_eq!(sanitize_playlist_dir(""), "sin-playlist");
        assert_eq!(sanitize_playlist_dir("..."), "sin-playlist");
        assert_eq!(
            sanitize_playlist_dir("L1K3D"),
            crate::domain::playlist::Playlist::LIKED_NAME
        );
        assert!(sanitize_playlist_dir(&"a".repeat(200)).chars().count() <= 64);
    }
}
