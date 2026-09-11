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

use std::path::PathBuf;

const APP_NAME: &str = "tunefold";

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
}
