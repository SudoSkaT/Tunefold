//! Fuente de origen de una canción.
//!
//! Tunefold usa exclusivamente YouTube / YouTube Music como fuente de datos
//! (metadata, portadas, letras, recomendados y reproducstrm). Este enum sirve
//! como marcador estable del único origen soportado y de etiqueta en la BD.

use std::fmt;

/// Origen de una canción. Únicamente YouTube / YouTube Music.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Source {
    YouTube,
}

impl Source {
    /// Identificador corto estable usado como prefijo de IDs externos.
    pub fn as_str(self) -> &'static str {
        match self {
            Source::YouTube => "youtube",
        }
    }

    /// `true` si este origen puede reproducir audio. Todos lo son aquí.
    pub fn is_playable(self) -> bool {
        match self {
            Source::YouTube => true,
        }
    }

    /// Human-readable label para mostrar en la UI.
    pub fn label(self) -> &'static str {
        match self {
            Source::YouTube => "YouTube",
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

impl std::str::FromStr for Source {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "youtube" | "yt" | "ytm" => Ok(Source::YouTube),
            _ => Err(format!("fuente desconocida: {s}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Source;
    use std::str::FromStr;

    #[test]
    fn parses_back() {
        assert_eq!(Source::from_str("youtube").unwrap(), Source::YouTube);
        assert_eq!("yt".parse::<Source>().unwrap(), Source::YouTube);
        assert!(Source::from_str("nope").is_err());
    }

    #[test]
    fn stable_key() {
        assert_eq!(Source::YouTube.as_str(), "youtube");
    }

    #[test]
    fn playability() {
        assert!(Source::YouTube.is_playable());
    }
}
