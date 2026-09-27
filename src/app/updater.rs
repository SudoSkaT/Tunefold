//! Actualizador de Tunefold: `tunefold update`.
//!
//! Consulta las releases públicas del repositorio oficial, descarga el binario
//! de la plataforma actual (asset `tunefold-<TARGET>`), lo sustituye en sitio y
//! purga las cachés (`~/.cache/tunefold`). Los datos de usuario y la
//! configuración nunca se tocan — y la caché es la ÚNICA parte reciclable.

use anyhow::{Context, Result};

use crate::infrastructure::dirs;

const RELEASES_API: &str = "https://api.github.com/repos/SudoSkaT/Tunefold/releases/latest";
const REPOS_API: &str = "https://api.github.com/repos/SudoSkaT/Tunefold";

/// Ejecuta `tunefold update`. `dry_run` solo informa de la versión disponible.
pub async fn run(dry_run: bool) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    let client = reqwest::Client::new();

    let release = match fetch_latest(&client).await? {
        Some(release) => release,
        None => {
            // El repo existe pero no ha publicado ninguna Release todavía. No es
            // un error: es un mensaje de estado. Un `git tag` suelto no basta
            // para `update`, porque las binaries se distribuyen como assets de
            // una GitHub Release.
            println!("El repositorio todavía no tiene releases publicadas.");
            println!(
                "Crea una GitHub Release con el asset «{}» para habilitar `update`.",
                expected_asset_name()
            );
            return Ok(());
        }
    };
    let tag = release.tag_name.trim_start_matches('v').to_string();

    println!("Versión actual: {current}");
    println!("Última release: {tag}");
    println!(
        "Ejecutable: {}",
        std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "<desconocido>".to_string())
    );

    // Migración one-time del esquema 1.7.x → 0.17.x: numéricamente 0 < 1, así
    // que sin esta regla ninguna instalación 1.x vería jamás una 0.x como
    // nueva (quedaría clavada diciendo "ya estás en la última versión").
    let migrating = is_epoch_migration(current, &tag);
    if migrating {
        println!("Migración de esquema de versión: 1.7.x → 0.17.x (one-time).");
    }
    if !migrating && !newer(&tag, current) {
        println!("Ya estás en la última versión.");
        return Ok(());
    }

    let asset = release
        .assets
        .iter()
        .find(|a| a.name == expected_asset_name())
        .with_context(|| {
            format!(
                "No hay asset «{}» en esta release (solo se distribuye para la plataforma en la que compilaste)",
                expected_asset_name()
            )
        })?;

    if dry_run {
        println!("Disponible: {} ({})", asset.name, asset.size);
        return Ok(());
    }

    println!("Descargando {} ...", asset.name);
    let bytes = client
        .get(&asset.download_url)
        .send()
        .await?
        .bytes()
        .await?;
    if bytes.is_empty() {
        anyhow::bail!("Descarga vacía: el asset de GitHub puede estar pendiente de subir.");
    }

    replace_current_executable(&bytes, &tag)?;
    purge_caches()?;

    println!("Actualizado a {tag}. Reinicia Tunefold.");
    println!("Verifica con: tunefold --version");
    Ok(())
}

/// GET a la API oficial de GitHub con los headers mínimos exigidos (User-Agent
/// da nombre a la app y el `Accept` fija el esquema de la respuesta JSON).
fn gh_get(client: &reqwest::Client, url: &str) -> reqwest::RequestBuilder {
    client
        .get(url)
        .header(
            "User-Agent",
            format!("Tunefold/{env}", env = env!("CARGO_PKG_VERSION")),
        )
        .header("Accept", "application/vnd.github+json")
}

/// Resolución de la última release desde la API de GitHub.
///
/// `Ok(None)` significa "el repositorio existe pero todavía no tiene releases"
/// (caso normal antes de la primera publicación); `Err` cubre los fallos de red
/// y los status que sí requieren atención.
async fn fetch_latest(client: &reqwest::Client) -> Result<Option<Release>> {
    let resp = gh_get(client, RELEASES_API).send().await?;
    let status = resp.status();
    if status.is_success() {
        return resp
            .json()
            .await
            .map(Some)
            .context("respuesta inválida de la API de releases");
    }
    match status {
        reqwest::StatusCode::NOT_FOUND => {
            // `/releases/latest` da 404 en dos casos que hay que distinguir:
            // repo inexistente/privado, o repo existente sin releases todavía.
            // El segundo GET lo desambigua.
            let repo = gh_get(client, REPOS_API).send().await?;
            if repo.status().is_success() {
                Ok(None)
            } else {
                anyhow::bail!(
                    "No se encontró el repositorio SudoSkaT/Tunefold (¿privado o renombrado?)."
                )
            }
        }
        _ => anyhow::bail!(gh_error_message(status)),
    }
}

/// Mensaje para un status de la API que no es 2xx ni el 404 de "repos sin
/// releases": el del rate-limit se distingue de cualquier otro fallo.
fn gh_error_message(status: reqwest::StatusCode) -> String {
    match status {
        reqwest::StatusCode::TOO_MANY_REQUESTS | reqwest::StatusCode::FORBIDDEN => {
            "Rate-limit de la API de GitHub: reintenta en unos minutos.".to_string()
        }
        s => format!("GitHub respondió {s}."),
    }
}

/// Reemplaza el binario en ejecución por `bytes` (escritura temporal + rename
/// atómico; en Windows se avisa si no se puede sobreescribir el ejecutable).
///
/// Verificación post-descarga (guarda contra el síntoma "dice que actualiza
/// pero `--version` reporta otra versión"): antes del rename se ejecuta
/// `<tmp> --version` y se exige que coincida con el tag de la release. Si el
/// asset trae otra versión horneada (build del commit equivocado o árbol
/// sucio), se aborta SIN reemplazar nada.
fn replace_current_executable(bytes: &[u8], expected_tag: &str) -> Result<()> {
    let mut exe = std::env::current_exe().context("no se pudo localizar el ejecutable")?;
    let tmp = exe.with_extension("update.tmp");
    std::fs::write(&tmp, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
    }
    match run_version_query(&tmp) {
        Some(reported) if versions_equal(&reported, expected_tag) => {}
        other => {
            let _ = std::fs::remove_file(&tmp);
            anyhow::bail!(
                "El asset descargado reporta versión «{}» pero la release es «{expected_tag}»: \
                 no se reemplaza nada. Avisa al mantenedor (build publicado del commit equivocado).",
                other.as_deref().unwrap_or("<ilegible>")
            );
        }
    }
    if let Err(e) = std::fs::rename(&tmp, &exe) {
        // Windows: el ejecutable en uso no se sobreescribe; dejamos el nuevo y
        // avisamos.
        let _ = std::fs::remove_file(&tmp);
        exe = exe.canonicalize().unwrap_or(exe);
        anyhow::bail!(
            "No se pudo reemplazar {exe:?} en caliente ({e}).\
            \nGuarda la caché de {tmp:?} y reemplázalo manualmente."
        );
    }
    Ok(())
}

/// Purga `~/.cache/tunefold` (miniaturas + repertorios de la feature YouTube).
/// Es un rescate explícito: los datos (`~/.local/share`) y la configuración
/// (`~/.config`) no se tocan.
fn purge_caches() -> Result<()> {
    let cache = dirs::cache_dir();
    if cache.exists() {
        std::fs::remove_dir_all(&cache)?;
        println!("Caché purgada: {}", cache.display());
    }
    Ok(())
}

/// Nombre del asset para la plataforma actual (el que produce el workflow de
/// release): `tunefold-<TARGET>` (o `.exe` en Windows). El target triple lo
/// expone `build.rs`.
fn expected_asset_name() -> String {
    let target = env!("TUNEFOLD_TARGET");
    if cfg!(windows) {
        format!("tunefold-{target}.exe")
    } else {
        format!("tunefold-{target}")
    }
}

/// Ejecuta `<bin> --version` y devuelve la versión reportada (`Some("0.17.42")`
/// para la salida `tunefold 0.17.42`). `None` si no se puede ejecutar o la
/// salida no tiene el formato esperado (p. ej. asset de otra arquitectura).
fn run_version_query(bin: &std::path::Path) -> Option<String> {
    let out = std::process::Command::new(bin)
        .arg("--version")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_reported_version(&String::from_utf8_lossy(&out.stdout))
}

/// Extrae la versión de una línea `tunefold X`.
fn parse_reported_version(line: &str) -> Option<String> {
    line.trim()
        .strip_prefix("tunefold")
        .map(|rest| rest.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Igualdad de versiones normalizada (recorta `v` inicial y espacios, para
/// comparar el tag de la release con lo que reporta el binario).
fn versions_equal(a: &str, b: &str) -> bool {
    normalize(a) == normalize(b)
}

/// Normaliza una versión para comparar: recorta `v` inicial y espacios.
fn normalize(v: &str) -> String {
    v.trim().trim_start_matches('v').trim().to_string()
}

/// Componente mayor numérico (`"1.7.32"` → 1, `"0.17.42"` → 0).
fn major(v: &str) -> Option<u64> {
    normalize(v)
        .split('.')
        .next()?
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()
}

/// ¿Es una migración de esquema 1.7.x → 0.17.x? One-time: las instalaciones
/// 1.x deben aceptar la línea 0.x aunque numéricamente 0 < 1.
fn is_epoch_migration(current: &str, latest: &str) -> bool {
    matches!((major(current), major(latest)), (Some(1), Some(0)))
}

/// Compara versiones semver simples `a > b` (solo numéricas).
///
/// No cubre el salto de esquema 1.7.x → 0.17.x (ver [`is_epoch_migration`]):
/// el llamador combina ambas condiciones.
fn newer(a: &str, b: &str) -> bool {
    let cmp = |v: &str| -> Vec<u64> {
        v.split('.')
            .filter_map(|p| {
                p.chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse()
                    .ok()
            })
            .collect()
    };
    cmp(a) > cmp(b)
}

/// Respuesta mínima de la API de GitHub para la carrera de releases.
#[derive(Debug, serde::Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Debug, serde::Deserialize)]
struct Asset {
    name: String,
    size: u64,
    #[serde(rename = "browser_download_url")]
    download_url: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_cmp() {
        assert!(newer("1.6.0", "1.5.3"));
        assert!(newer("1.5.10", "1.5.9"));
        assert!(!newer("1.5.3", "1.5.3"));
        assert!(!newer("1.5.2", "1.9.9"));
    }

    #[test]
    fn epoch_migration_covers_1x_to_0x_only() {
        // La instalación 1.7.32 debe aceptar la línea 0.17.x (one-time).
        assert!(is_epoch_migration("1.7.32", "0.17.42"));
        assert!(is_epoch_migration("1.7.34", "0.17.41"));
        // Todo lo demás lo decide la comparación numérica normal.
        assert!(!is_epoch_migration("0.17.41", "0.17.42"));
        assert!(!is_epoch_migration("0.17.42", "0.17.42"));
        assert!(!is_epoch_migration("1.7.32", "1.7.34"));
        assert!(!is_epoch_migration("0.17.42", "1.7.32"));
    }

    #[test]
    fn reported_version_parses_and_compares_with_tag() {
        assert_eq!(
            parse_reported_version("tunefold 0.17.42\n"),
            Some("0.17.42".to_string())
        );
        assert_eq!(
            parse_reported_version("tunefold 1.7.32"),
            Some("1.7.32".to_string())
        );
        assert_eq!(parse_reported_version("otra cosa"), None);
        assert_eq!(parse_reported_version("tunefold "), None);
        // La verificación post-descarga tolera el prefijo `v` del tag.
        assert!(versions_equal("0.17.42", "0.17.42"));
        assert!(versions_equal("0.17.42", "v0.17.42"));
        assert!(!versions_equal("0.17.41", "0.17.42"));
    }

    #[test]
    fn asset_name_matches_target() {
        let name = expected_asset_name();
        assert!(name.starts_with("tunefold-"), "{name}");
        if cfg!(windows) {
            assert!(name.ends_with(".exe"));
        }
    }

    #[test]
    fn gh_errors_rate_limit_and_generic() {
        let rate_limited = |msg: &str| msg.to_lowercase().contains("rate-limit");
        assert!(
            rate_limited(&gh_error_message(reqwest::StatusCode::TOO_MANY_REQUESTS)),
            "el 429 menciona el rate-limit"
        );
        assert!(
            rate_limited(&gh_error_message(reqwest::StatusCode::FORBIDDEN)),
            "el 403 también se trata como rate-limit"
        );
        let generic = gh_error_message(reqwest::StatusCode::SERVICE_UNAVAILABLE);
        assert!(
            generic.contains("503"),
            "el resto reporta el status literal"
        );
        assert!(
            !generic.contains("¿tag no existente"),
            "no reaparece el mensaje ambiguo"
        );
    }
}
