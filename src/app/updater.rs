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
///
/// Cadena verificable de la operación (cada paso se imprime):
///
/// ```text
/// VERSIÓN EN EJECUCIÓN → RUTA REAL → RELEASE/TAG → ASSET → SHA256 →
/// --version DEL ASSET → REEMPLAZO → VERIFICACIÓN POST-INSTALACIÓN
/// ```
pub async fn run(dry_run: bool) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    let client = reqwest::Client::new();
    let identity = resolve_identity();
    println!("Tunefold update");
    println!("  en ejecución: tunefold {current}");
    println!("  ruta real: {}", identity.current_exe_display());
    println!("  argv[0]: {}", identity.argv0_display());
    println!("  target: {}", env!("TUNEFOLD_TARGET"));

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
    println!("  última release: {} (tag {})", tag, release.tag_name);
    match &identity.path_verdict {
        PathVerdict::Same => println!("  PATH resuelve a: mismo ejecutable ✓"),
        PathVerdict::Different(other) => println!(
            "  PATH resuelve a: {} (¡OTRA COPIA! `tunefold` en tu shell NO es este binario)",
            other.display()
        ),
        PathVerdict::NotFound => println!("  PATH resuelve a: no encontrado en PATH"),
    }

    // Migración one-time del esquema 1.7.x → 0.17.x: numéricamente 0 < 1, así
    // que sin esta regla ninguna instalación 1.x vería jamás una 0.x como
    // nueva (quedaría clavada diciendo "ya estás en la última versión").
    let migrating = is_epoch_migration(current, &tag);
    if migrating {
        println!("  migración de esquema: 1.7.x → 0.17.x (one-time).");
    }
    if !migrating && !newer(&tag, current) {
        println!("Ya estás en la última versión.");
        return Ok(());
    }

    // Endurecido contra releases con assets duplicados (p. ej. uno manual y
    // otro de CI con el mismo nombre): elegir a ciegas con `find` instalaría
    // un binario arbitrario. Con 0 o con 2+ candidatos se falla ruidoso.
    let asset = select_asset(&release.assets, &expected_asset_name(), &release.tag_name)?;
    println!("  asset: {} ({} bytes)", asset.name, asset.size);
    if let Some(digest) = asset.digest.as_deref() {
        println!("  sha256 publicado: {digest}");
    } else {
        println!("  sha256 publicado: (la release no lo declara; se verificará --version)");
    }

    if dry_run {
        println!("Disponible: {} ({})", asset.name, asset.size);
        println!("(dry-run: no se descarga ni se reemplaza nada)");
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
    let downloaded_digest = sha256_hex(&bytes);
    println!("  sha256 descargado: {downloaded_digest}");
    if let Some(expected) = asset.digest.as_deref() {
        if !digest_matches(&downloaded_digest, expected) {
            anyhow::bail!(
                "El SHA256 descargado no coincide con el publicado ({expected}): \
                 descarga corrupta o asset manipulado. No se reemplaza nada."
            );
        }
        println!("  sha256: coincide ✓");
    }

    let replaced = replace_current_executable(&bytes, &tag)?;
    println!("  reemplazo: OK → {}", replaced.display());
    // Verificación POST-INSTALACIÓN: el binario ya instalado debe reportar el
    // tag. Sin esto, "descargué el asset" no prueba nada (el síntoma reportado
    // era exactamente ese: update OK pero --version viejo).
    match run_version_query(&replaced) {
        Some(v) if versions_equal(&v, &tag) => {
            println!("  verificación post-instalación (--version): {v} ✓");
        }
        other => {
            anyhow::bail!(
                "El ejecutable instalado reporta «{}» en vez de «{tag}»: \
                 el reemplazo ocurrió pero algo no cuadra (¿otra copia en PATH? ¿caché del shell? `hash -r`).",
                other.as_deref().unwrap_or("<ilegible>")
            );
        }
    }
    purge_caches()?;

    println!("Actualizado a {tag}. Reinicia Tunefold.");
    if !matches!(identity.path_verdict, PathVerdict::Same) {
        println!(
            "OJO: se actualizó {} pero tu shell puede estar ejecutando otra copia. \
             Comprueba con `which -a tunefold` y, si tu shell cachea rutas, ejecuta `hash -r`.",
            replaced.display()
        );
    }
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
fn replace_current_executable(bytes: &[u8], expected_tag: &str) -> Result<std::path::PathBuf> {
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
    Ok(exe)
}

/// SHA256 en hex de unos bytes (cadena de integridad descarga → instalado).
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(bytes);
    format!("{digest:x}")
}

/// Igualdad de digests insensible a mayúsculas y espacios.
fn digest_matches(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// Nombre del binario tal como aparece en PATH.
fn exe_file_name() -> &'static str {
    if cfg!(windows) {
        "tunefold.exe"
    } else {
        "tunefold"
    }
}

/// Cómo resuelve tu PATH el comando `tunefold` (primera coincidencia).
fn find_in_path() -> Option<std::path::PathBuf> {
    let name = exe_file_name();
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

/// Veredicto de identidad entre el ejecutable en ejecución y el que tu shell
/// encontraría con `tunefold`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathVerdict {
    /// Misma ruta (canónica): actualizar este binario actualiza tu `tunefold`.
    Same,
    /// Otra copia: actualizar este binario NO cambia lo que ejecuta tu shell.
    Different(std::path::PathBuf),
    /// `tunefold` no aparece en PATH (invocado por ruta directa).
    NotFound,
}

/// Identidad del binario en ejecución para el diagnóstico del updater.
#[derive(Debug, Clone)]
pub struct ExeIdentity {
    /// `current_exe()` (canonizado: resuelve symlinks).
    pub current_exe: Option<std::path::PathBuf>,
    /// `argv[0]` tal cual lo invocaste (puede ser relativo o un symlink).
    pub argv0: Option<String>,
    /// Comparación entre ambos.
    pub path_verdict: PathVerdict,
}

impl ExeIdentity {
    pub fn current_exe_display(&self) -> String {
        self.current_exe
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<desconocido>".to_string())
    }

    pub fn argv0_display(&self) -> String {
        self.argv0
            .clone()
            .unwrap_or_else(|| "<desconocido>".to_string())
    }
}

/// Resuelve la identidad del ejecutable en ejecución.
fn resolve_identity() -> ExeIdentity {
    let current_exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok());
    let argv0 = std::env::args_os()
        .next()
        .map(|s| s.to_string_lossy().into_owned());
    let path_verdict = check_identity(current_exe.as_deref(), find_in_path());
    ExeIdentity {
        current_exe,
        argv0,
        path_verdict,
    }
}

/// Compara el ejecutable en ejecución (canónico) con lo que PATH resuelve.
///
/// Puro y testeable: distingue "actualicé el ejecutable que está ejecutándose"
/// de "el usuario posteriormente está ejecutando otro tunefold diferente".
/// Canoniza si el fichero existe (resuelve symlinks); si no, devuelve la
/// ruta tal cual para que la comparación siga siendo total.
fn canon(p: &std::path::Path) -> std::path::PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

fn check_identity(
    current_exe: Option<&std::path::Path>,
    path_hit: Option<std::path::PathBuf>,
) -> PathVerdict {
    let hit = path_hit.map(|p| canon(&p));
    match (current_exe.map(canon), hit) {
        (Some(cur), Some(hit)) if cur == hit => PathVerdict::Same,
        (_, Some(hit)) => PathVerdict::Different(hit),
        (_, None) => PathVerdict::NotFound,
    }
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

/// Elige el asset de la plataforma actual dentro de una release.
///
/// Exactamente 1 candidato con el nombre esperado: se devuelve. Con 0 se
/// informa (quizá la release solo cubre otras plataformas); con 2+ (p. ej.
/// resto manual + asset de CI) se falla ruidoso en vez de instalar al azar.
fn select_asset<'a>(assets: &'a [Asset], expected: &str, tag: &str) -> Result<&'a Asset> {
    let mut hits = assets.iter().filter(|a| a.name == expected);
    let first = hits.next();
    match (first, hits.next()) {
        (Some(one), None) => Ok(one),
        (None, _) => anyhow::bail!(
            "No hay asset «{expected}» en esta release (solo se distribuye para la plataforma en la que compilaste)"
        ),
        (Some(_), Some(_)) => anyhow::bail!(
            "Hay varios assets llamados «{expected}» en la release {tag}: imposible elegir sin ambigüedad. Avisa al mantenedor."
        ),
    }
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
    /// Digest publicado por GitHub (`sha256:...`). Ausente en releases
    /// antiguas/manuals: entonces solo se verifica `--version`.
    #[serde(default)]
    digest: Option<String>,
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
    fn sha256_is_stable_hex_and_case_insensitive() {
        // SHA256("") canónico: evita depender de fixtures externos.
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(sha256_hex(b"abc").len(), 64);
        assert!(digest_matches("ABCDEF", "abcdef"));
        assert!(digest_matches("  abc ", "ABC"));
        assert!(!digest_matches("abc", "abd"));
    }

    #[test]
    fn path_identity_distinguishes_same_from_other_copy() {
        use std::path::PathBuf;
        // Misma ruta canonizada ⇒ actualizar aquí actualiza tu shell.
        assert_eq!(
            check_identity(
                Some(std::path::Path::new("/home/u/.cargo/bin/tunefold")),
                Some(PathBuf::from("/home/u/.cargo/bin/tunefold")),
            ),
            PathVerdict::Same
        );
        // Otra copia (p. ej. /usr/local/bin vs ~/.cargo/bin) ⇒ el update no
        // cambia lo que ejecuta tu shell: el falso positivo del síntoma.
        assert_eq!(
            check_identity(
                Some(std::path::Path::new("/tmp/tunefold")),
                Some(PathBuf::from("/usr/local/bin/tunefold")),
            ),
            PathVerdict::Different(PathBuf::from("/usr/local/bin/tunefold"))
        );
        // Invocado por ruta directa fuera de PATH.
        assert_eq!(check_identity(None, None), PathVerdict::NotFound);
    }

    #[test]
    fn find_in_path_resolves_first_match() {
        // Directorio temporal con un `tunefold` falso al frente del PATH.
        let dir = std::env::temp_dir().join(format!("tunefold-pathtest-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let fake = dir.join(exe_file_name());
        std::fs::write(&fake, b"x").unwrap();
        let old = std::env::var_os("PATH");
        let mut paths = vec![dir.clone()];
        if let Some(p) = &old {
            paths.extend(std::env::split_paths(p));
        }
        let joined = std::env::join_paths(paths).unwrap();
        std::env::set_var("PATH", &joined);
        let hit = find_in_path();
        if let Some(o) = old {
            std::env::set_var("PATH", o);
        } else {
            std::env::remove_var("PATH");
        }
        let _ = std::fs::remove_file(&fake);
        let _ = std::fs::remove_dir(&dir);
        assert_eq!(hit, Some(fake));
    }

    #[test]
    fn asset_selection_needs_exactly_one_candidate() {
        let mk = |name: &str| Asset {
            name: name.to_string(),
            size: 1,
            download_url: String::new(),
            digest: None,
        };
        let expected = "tunefold-x86_64-unknown-linux-gnu";
        // 1 candidato: se elige.
        let one = vec![mk("otro"), mk(expected)];
        assert_eq!(select_asset(&one, expected, "0.17.44").unwrap().size, 1);
        // 0 candidatos: error que menciona el nombre esperado.
        let none: Vec<Asset> = vec![mk("otro")];
        assert!(select_asset(&none, expected, "0.17.44")
            .unwrap_err()
            .to_string()
            .contains(expected));
        // 2+ candidatos (manual + CI): error de ambigüedad, nunca al azar.
        let two = vec![mk(expected), mk(expected)];
        assert!(select_asset(&two, expected, "0.17.44")
            .unwrap_err()
            .to_string()
            .contains("varios"));
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
