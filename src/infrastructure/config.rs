//! Configuración de Tunefold.
//!
//! Con YouTube como única fuente no hay credenciales que configurar
//! (`rustypipe` no requiere API key); el único ajuste editable es la política
//! de reproducción. Se carga desde variables de entorno (o `.env`) y puede
//! persistirse desde la vista de ajustes de la TUI.

use std::path::Path;

use crate::app::audio::PlaybackPolicy;
use crate::domain::source::Source;

/// Flags de funcionalidad (spec §32).
///
/// Controlan QUÉ capacidades se registran en la composición SIN recompilar:
/// un flag en `false` elimina el camino completo (p. ej. YouTube apagado ⇒
/// registro de streams y catálogo vacíos; la app arranca sana y degrada con
/// mensajes claros). Se leen del entorno/`.env` (`1|true|yes|on` = activo).
///
/// Los flags de analysis/visualización se consumirán en las Fases 6-7.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureFlags {
    pub youtube_provider: bool,
    pub official_provider: bool,
    pub alternate_provider: bool,
    /// Respetar los proxies del entorno (`HTTP_PROXY`/`HTTPS_PROXY`/...).
    pub proxy: bool,
    pub audio_analysis: bool,
    pub advanced_visualization: bool,
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            youtube_provider: true,
            // Aún no existen proveedores de estos tipos: apagados por defecto
            // hasta que haya implementaciones que registrar.
            official_provider: false,
            alternate_provider: false,
            proxy: true,
            audio_analysis: true,
            advanced_visualization: true,
        }
    }
}

impl FeatureFlags {
    /// Núcleo de parsing PURO (testeable sin tocar el entorno): resuelve cada
    /// flag desde una función de lookup clave→valor opcional.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let f = |key: &str, default: bool| {
            lookup(key)
                .map(|v| parse_bool(&v).unwrap_or(default))
                .unwrap_or(default)
        };
        Self {
            youtube_provider: f(
                "YOUTUBE_PROVIDER_ENABLED",
                FeatureFlags::default().youtube_provider,
            ),
            official_provider: f("OFFICIAL_PROVIDER_ENABLED", false),
            alternate_provider: f("ALTERNATE_PROVIDER_ENABLED", false),
            proxy: f("PROXY_ENABLED", true),
            audio_analysis: f("AUDIO_ANALYSIS_ENABLED", true),
            advanced_visualization: f("ADVANCED_VISUALIZATION_ENABLED", true),
        }
    }

    /// Lee los flags desde el entorno (`dotenvy` ya aplicó el `.env`).
    pub fn from_env() -> Self {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Resumen legible para la vista de ajustes (informativo).
    pub fn summary(&self) -> String {
        let on_off = |b: bool| if b { "ON" } else { "OFF" };
        format!(
            "YouTube:{} oficial:{} alterno:{} proxy:{}",
            on_off(self.youtube_provider),
            on_off(self.official_provider),
            on_off(self.alternate_provider),
            on_off(self.proxy),
        )
    }
}

/// Interpreta el valor textual de un flag: verdadero para 1/true/yes/on/sí.
fn parse_bool(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" | "sí" | "si" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Configuración global: política de reproducción + feature flags.
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub playback_policy: PlaybackPolicy,
    pub flags: FeatureFlags,
}

impl Config {
    /// Lee la configuración desde el entorno y el `.env` canónico
    /// (`~/.config/tunefold/.env`), con fallback al `.env` legacy del
    /// directorio de trabajo. `dotenvy` no pisa variables ya definidas: el
    /// `.env` canónico (cargado primero) tiene prioridad sobre el legacy.
    pub fn load() -> Self {
        let _ = dotenvy::from_path(super::dirs::env_path());
        let _ = dotenvy::dotenv();
        Self::from_env()
    }

    pub fn from_env() -> Self {
        let playback_policy = std::env::var("PLAYBACK_POLICY")
            .unwrap_or_default()
            .to_ascii_lowercase();
        let playback_policy = if playback_policy.is_empty() || playback_policy == "auto" {
            PlaybackPolicy::Auto
        } else {
            PlaybackPolicy::Global(playback_policy)
        };

        Config {
            playback_policy,
            flags: FeatureFlags::from_env(),
        }
    }

    /// Fuentes activas según los flags de proveedor.
    pub fn available_sources(&self) -> Vec<Source> {
        let mut sources = Vec::new();
        if self.flags.youtube_provider {
            sources.push(Source::YouTube);
        }
        sources
    }

    /// Aplica la política de proxy a un builder de cliente HTTP propio:
    /// con `proxy=false` se IGNORAN las variables del entorno (`.no_proxy()`).
    pub fn apply_proxy_policy(&self, builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
        if self.flags.proxy {
            builder
        } else {
            builder.no_proxy()
        }
    }

    /// Valores de los campos editables en la vista de ajustes. `proxy` es solo
    /// informativo: el valor de `HTTP_PROXY`/`HTTPS_PROXY`/`ALL_PROXY` del
    /// entorno (que reqwest usa automáticamente en todas las peticiones).
    pub fn form(&self) -> ConfigForm {
        ConfigForm {
            playback_policy: self.playback_policy_display(),
            proxy: std::env::var("HTTP_PROXY")
                .or_else(|_| std::env::var("HTTPS_PROXY"))
                .or_else(|_| std::env::var("ALL_PROXY"))
                .unwrap_or_default(),
            providers: self.flags.summary(),
        }
    }

    fn playback_policy_display(&self) -> String {
        match &self.playback_policy {
            PlaybackPolicy::Auto => "auto".to_string(),
            PlaybackPolicy::Global(id) => id.clone(),
        }
    }

    /// Aplica un formulario a la configuración en memoria.
    pub fn apply_form(&mut self, form: &ConfigForm) {
        self.playback_policy =
            if form.playback_policy.trim().is_empty() || form.playback_policy.trim() == "auto" {
                PlaybackPolicy::Auto
            } else {
                PlaybackPolicy::Global(form.playback_policy.trim().to_string())
            };
    }

    /// Persiste un formulario al `.env` canónico (`~/.config/tunefold/.env`)
    /// sin aplicar cambios.
    ///
    /// Los feature flags NO se persisten aquí: su superficie de control es el
    /// entorno/`.env` manual (apagable sin recompilar y sin tocar ajustes UI).
    pub fn persist(form: &ConfigForm) -> anyhow::Result<()> {
        let updates: [(&str, Option<&str>); 1] = [kv("PLAYBACK_POLICY", &form.playback_policy)];
        let path = super::dirs::env_path();
        upsert_env_file(path.to_str().unwrap_or(".env"), &updates)?;
        Ok(())
    }
}

/// Modelo editable de la vista de ajustes (valores planos para la TUI).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConfigForm {
    pub playback_policy: String,
    /// Estado del proxy del entorno (solo lectura, no se persiste).
    pub proxy: String,
    /// Resumen de feature flags de proveedores (solo lectura).
    pub providers: String,
}

/// Convierte un campo del formulario en una entrada (clave, valor opcional).
fn kv<'a>(key: &'static str, value: &'a str) -> (&'static str, Option<&'a str>) {
    let v = value.trim();
    if v.is_empty() {
        (key, None)
    } else {
        (key, Some(v))
    }
}

/// Actualiza (o elimina) una clave en `path`. Conserva las demás líneas y comentarios.
fn upsert_env_file(path: &str, updates: &[(&str, Option<&str>)]) -> std::io::Result<()> {
    let mut lines = std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(ToOwned::to_owned)
        .collect::<Vec<String>>();

    for (raw_key, value) in updates {
        // Elimina cualquier línea previa con esa clave.
        lines.retain(|l| !line_matches(l, raw_key));

        if let Some(value) = value {
            lines.push(format!("{raw_key}={value}"));
        }
    }

    if let Some(parent) = Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(path, lines.join("\n") + "\n")
}

fn line_matches(line: &str, raw_key: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return false;
    }
    key_of(line) == key_of(raw_key)
}

fn key_of(raw: &str) -> String {
    raw.trim()
        .split('=')
        .next()
        .unwrap_or(raw)
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn upsert_preserves_other_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(&path, "KEEP=1\n# a comment\n").unwrap();

        upsert_env_file(
            path.to_str().unwrap(),
            &[("PLAYBACK_POLICY", Some("rodio")), ("GONE", None)],
        )
        .unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("KEEP=1"));
        assert!(text.contains("# a comment"));
        assert!(text.contains("PLAYBACK_POLICY=rodio"));
        assert!(!text.contains("GONE"));
    }

    #[test]
    fn apply_form_set_global_policy() {
        let mut cfg = Config::default();
        cfg.apply_form(&ConfigForm {
            playback_policy: "rodio".to_string(),
            proxy: String::new(),
            providers: String::new(),
        });
        assert_eq!(
            cfg.playback_policy,
            PlaybackPolicy::Global("rodio".to_string())
        );
        assert_eq!(cfg.available_sources(), vec![Source::YouTube]);
    }

    // ------------------------------------------------------- feature flags

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k: &str| map.get(k).cloned()
    }

    #[test]
    fn flags_default_to_youtube_on_and_others_documented() {
        let f = FeatureFlags::from_lookup(|_| None);
        assert!(f.youtube_provider);
        assert!(!f.official_provider, "no hay implementación aún");
        assert!(!f.alternate_provider, "no hay implementación aún");
        assert!(f.proxy);
        assert!(f.audio_analysis);
        assert!(f.advanced_visualization);
    }

    #[test]
    fn flags_parse_truthy_and_falsy_values() {
        let f = FeatureFlags::from_lookup(lookup(&[
            ("YOUTUBE_PROVIDER_ENABLED", "0"),
            ("OFFICIAL_PROVIDER_ENABLED", "true"),
            ("PROXY_ENABLED", "off"),
            ("AUDIO_ANALYSIS_ENABLED", "SI"),
        ]));
        assert!(!f.youtube_provider);
        assert!(f.official_provider);
        assert!(!f.proxy);
        assert!(f.audio_analysis);
    }

    #[test]
    fn invalid_flag_value_falls_back_to_default() {
        let f = FeatureFlags::from_lookup(lookup(&[("YOUTUBE_PROVIDER_ENABLED", "quizás")]));
        assert!(
            f.youtube_provider,
            "un valor no interpretable no debe apagar un proveedor"
        );
    }

    #[test]
    fn youtube_off_leaves_no_active_sources() {
        let mut cfg = Config::default();
        cfg.flags.youtube_provider = false;
        assert!(
            cfg.available_sources().is_empty(),
            "apagado YouTube ⇒ ninguna fuente activa"
        );
    }

    #[test]
    fn provider_summary_is_human_readable() {
        let f = FeatureFlags {
            youtube_provider: false,
            ..FeatureFlags::default()
        };
        let s = f.summary();
        assert!(s.contains("YouTube:OFF"));
        assert!(s.contains("proxy:ON"));
    }
}
