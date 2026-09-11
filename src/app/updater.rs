//! Actualizador de Tunefold: `tunefold update`.
//!
//! Consulta las releases públicas del repositorio oficial, descarga el binario
//! de la plataforma actual (asset `tunefold-<TARGET>`), lo sustituye en sitio y
//! purga las cachés (`~/.cache/tunefold`). Los datos de usuario y la
//! configuración nunca se tocan — y la caché es la ÚNICA parte reciclable.

use anyhow::{Context, Result};

use crate::infrastructure::dirs;

const RELEASES_API: &str = "https://api.github.com/repos/SudoSkaT/PlayFusion/releases/latest";

/// Ejecuta `tunefold update`. `dry_run` solo informa de la versión disponible.
pub async fn run(dry_run: bool) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    let client = reqwest::Client::new();

    let release = fetch_latest(&client).await?;
    let tag = release.tag_name.trim_start_matches('v').to_string();

    println!("Versión actual: {current}");
    println!("Última release: {tag}");

    if !newer(&tag, current) {
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

    replace_current_executable(&bytes)?;
    purge_caches()?;

    println!("Actualizado a {tag}. Reinicia Tunefold.");
    Ok(())
}

/// Resolución de la última release desde la API de GitHub.
async fn fetch_latest(client: &reqwest::Client) -> Result<Release> {
    let resp = client
        .get(RELEASES_API)
        .header(
            "User-Agent",
            format!("Tunefold/{env}", env = env!("CARGO_PKG_VERSION")),
        )
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!(
            "GitHub respondió {} (¿tag no existente o rate-limit?).",
            resp.status()
        );
    }
    resp.json()
        .await
        .context("respuesta inválida de la API de releases")
}

/// Reemplaza el binario en ejecución por `bytes` (escritura temporal + rename
/// atómico; en Windows se avisa si no se puede sobreescribir el ejecutable).
fn replace_current_executable(bytes: &[u8]) -> Result<()> {
    let mut exe = std::env::current_exe().context("no se pudo localizar el ejecutable")?;
    let tmp = exe.with_extension("update.tmp");
    std::fs::write(&tmp, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
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

/// Compara versiones semver simples `a > b` (solo numéricas).
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
    fn asset_name_matches_target() {
        let name = expected_asset_name();
        assert!(name.starts_with("tunefold-"), "{name}");
        if cfg!(windows) {
            assert!(name.ends_with(".exe"));
        }
    }
}
