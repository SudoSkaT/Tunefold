//! Modelos de dominio para fuentes de media y resoluciones de stream.
//!
//! [`StreamResolution`] es el resultado de resolver el audio de un track: un
//! recurso potencialmente **temporal** (las URLs remotas caducan) con sus
//! propiedades técnicas. Nada aquí conoce proveedores concretos: solo el
//! origen ([`Source`]) y los datos que el motor necesita para reproducir.
//!
//! La expiración se modela con `chrono::DateTime<Utc>` (serializable y
//! persistible en la caché de resoluciones) y los métodos puros aceptan el
//! instante como parámetro para poder testearlos sin relojes.

use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::domain::source::Source;

/// Calidad aproximada del audio resuelto.
///
/// Los proveedores rara vez declaran una etiqueta de calidad; normalmente se
/// deriva del bitrate con [`Quality::from_bitrate`].
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Quality {
    /// Sin pérdida (FLAC/ALAC u opus >~450 kbps).
    Lossless,
    /// Alta (~192–320 kbps).
    High,
    /// Estándar (~96–192 kbps): el rango habitual de streams de música.
    Standard,
    /// Baja (<96 kbps).
    Low,
}

impl Quality {
    /// Clasifica por tasa de bits (bits/s). `None` si no hay bitrate o es 0
    /// (desconocido): nunca se inventa una calidad.
    pub fn from_bitrate(bitrate: Option<u32>) -> Option<Self> {
        let bps = bitrate?;
        if bps == 0 {
            return None;
        }
        Some(match bps {
            b if b >= 450_000 => Quality::Lossless,
            b if b >= 192_000 => Quality::High,
            b if b >= 96_000 => Quality::Standard,
            _ => Quality::Low,
        })
    }

    /// Etiqueta legible para la UI.
    pub fn label(self) -> &'static str {
        match self {
            Quality::Lossless => "sin pérdida",
            Quality::High => "alta",
            Quality::Standard => "estándar",
            Quality::Low => "baja",
        }
    }
}

/// Stream remoto por HTTP(S): URL lista para descargar/reproducir más las
/// cabeceras de contexto que exige el CDN (User-Agent/Referer/tokens...).
///
/// La URL NO es permanente: puede caducar o dejar de responder en cualquier
/// momento; la vigencia vive en [`StreamResolution::expires_at`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RemoteStream {
    pub url: String,
    pub headers: Vec<(String, String)>,
}

impl RemoteStream {
    /// Cabeceras vacías (para CDNs sin contexto obligatorio).
    pub fn without_headers(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            headers: Vec::new(),
        }
    }
}

/// Fuente de media consumible por el motor de reproducción.
///
/// Es la vista mínima que el reproductor necesita (de dónde obtener bytes y
/// con qué contexto), sin metadatos de resolución (calidad, expiración,
/// proveedor) que al motor no le incumben. Las variantes futuras (archivos
/// locales, servidores propios) se añaden aquí sin tocar a los consumidores.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MediaSource {
    Remote(RemoteStream),
}

impl MediaSource {
    /// Extrae la fuente reproducible de una resolución.
    ///
    /// `None` si la URI no es un stream HTTP(S): hoy todo lo reproducible lo
    /// es, y devolver `None` evita fabricar una fuente inválida.
    pub fn from_resolution(resolution: &StreamResolution) -> Option<Self> {
        if is_http_uri(&resolution.uri) {
            Some(MediaSource::Remote(RemoteStream {
                url: resolution.uri.clone(),
                headers: resolution.headers.clone(),
            }))
        } else {
            None
        }
    }
}

fn is_http_uri(uri: &str) -> bool {
    uri.starts_with("http://") || uri.starts_with("https://")
}

/// Error de validación estructural de una resolución.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StreamValidationError {
    #[error("la URI de la resolución está vacía")]
    EmptyUri,
    #[error("esquema de URI no soportado: {0}")]
    UnsupportedScheme(String),
}

/// Resultado de resolver el stream de audio de un track.
///
/// Representa el recurso resuelto y sus propiedades, NO la identidad del
/// provider: dos providers distintos producen el mismo tipo. Se construye en
/// el borde del proveedor (mapper) y se consume en el resolver/playback.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StreamResolution {
    /// Origen de la resolución (para métricas, prioridad e invalidación).
    pub provider: Source,
    /// URI del stream. Potencialmente temporal: ver `expires_at`.
    pub uri: String,
    pub mime_type: Option<String>,
    pub codec: Option<String>,
    /// Tasa de bits en bits/s.
    pub bitrate: Option<u32>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u8>,
    /// Duración declarada por el proveedor (el decodificador manda si difiere).
    pub duration: Option<Duration>,
    pub quality: Option<Quality>,
    /// Instante (UTC) a partir del cual la URI debe considerarse muerta.
    /// `None` = sin vencimiento conocido (nunca se asume que caduca).
    pub expires_at: Option<DateTime<Utc>>,
    /// Cabeceras HTTP de contexto necesarias para descargar la URI.
    pub headers: Vec<(String, String)>,
    /// Extras específicos del proveedor (p. ej. `itag` de YouTube), fuera del
    /// contrato común. Nunca contienen secretos.
    pub metadata: Vec<(String, String)>,
}

impl StreamResolution {
    /// Resolución mínima válida: origen + URI. El resto de campos quedan en
    /// `None`/vacío y el proveedor los rellena según lo que conozca.
    pub fn new(provider: Source, uri: impl Into<String>) -> Self {
        Self {
            provider,
            uri: uri.into(),
            mime_type: None,
            codec: None,
            bitrate: None,
            sample_rate: None,
            channels: None,
            duration: None,
            quality: None,
            expires_at: None,
            headers: Vec::new(),
            metadata: Vec::new(),
        }
    }

    /// `true` si la resolución ya caducó respecto a `now`.
    ///
    /// Sin `expires_at` NUNCA caduca: no hay forma segura de saberlo, y la
    /// verificación real de vida corresponde al resolver (sondeo de la URI).
    pub fn is_expired_at(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.is_some_and(|exp| exp <= now)
    }

    /// [`Self::is_expired_at`] contra el reloj actual.
    pub fn is_expired(&self) -> bool {
        self.is_expired_at(Utc::now())
    }

    /// `true` si la resolución caduca dentro de `window` desde `now` (o ya
    /// está caducada). Para decisiones de refresh preventivo.
    pub fn expires_within_at(&self, window: Duration, now: DateTime<Utc>) -> bool {
        self.expires_at.is_some_and(|exp| {
            let deadline = now + chrono::Duration::from_std(window).unwrap_or_default();
            exp <= deadline
        })
    }

    /// [`Self::expires_within_at`] contra el reloj actual.
    pub fn expires_within(&self, window: Duration) -> bool {
        self.expires_within_at(window, Utc::now())
    }

    /// Validación estructural mínima antes de entregar la resolución al motor:
    /// URI presente y con esquema soportado. No hace I/O.
    pub fn validate(&self) -> Result<(), StreamValidationError> {
        if self.uri.trim().is_empty() {
            return Err(StreamValidationError::EmptyUri);
        }
        if !is_http_uri(&self.uri) {
            return Err(StreamValidationError::UnsupportedScheme(
                self.uri.split(':').next().unwrap_or_default().to_string(),
            ));
        }
        Ok(())
    }

    /// Fuente reproducible para el motor (`None` si la URI no es remota).
    pub fn media_source(&self) -> Option<MediaSource> {
        MediaSource::from_resolution(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::TimeZone;

    fn t(h: u32, m: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 22, h, m, s).unwrap()
    }

    fn resolution(expires_at: Option<DateTime<Utc>>) -> StreamResolution {
        StreamResolution {
            expires_at,
            ..StreamResolution::new(Source::YouTube, "https://cdn.example/stream?sig=abc")
        }
    }

    // ------------------------------------------------------------ calidad

    #[test]
    fn quality_classifies_bitrate_bands() {
        assert_eq!(
            Quality::from_bitrate(Some(600_000)),
            Some(Quality::Lossless)
        );
        assert_eq!(Quality::from_bitrate(Some(256_000)), Some(Quality::High));
        assert_eq!(
            Quality::from_bitrate(Some(128_000)),
            Some(Quality::Standard)
        );
        assert_eq!(Quality::from_bitrate(Some(64_000)), Some(Quality::Low));
    }

    #[test]
    fn quality_never_invents_unknown_bitrate() {
        assert_eq!(Quality::from_bitrate(None), None);
        assert_eq!(Quality::from_bitrate(Some(0)), None);
    }

    #[test]
    fn quality_labels_are_stable() {
        assert_eq!(Quality::Standard.label(), "estándar");
        assert_eq!(Quality::Lossless.label(), "sin pérdida");
    }

    // --------------------------------------------------------- expiración

    #[test]
    fn expired_when_deadline_passed() {
        let r = resolution(Some(t(12, 0, 0)));
        assert!(r.is_expired_at(t(12, 0, 1)));
        assert!(!r.is_expired_at(t(11, 59, 59)));
    }

    #[test]
    fn exact_deadline_counts_as_expired() {
        // En la frontera ya no es utilizable: <= y no <.
        let r = resolution(Some(t(12, 0, 0)));
        assert!(r.is_expired_at(t(12, 0, 0)));
    }

    #[test]
    fn without_expiry_never_expires() {
        let r = resolution(None);
        assert!(!r.is_expired_at(t(23, 59, 59)));
        assert!(!r.expires_within(Duration::from_secs(3600)));
    }

    #[test]
    fn expires_within_detects_near_expiry() {
        let r = resolution(Some(t(12, 10, 0)));
        let now = t(12, 5, 0);
        assert!(r.expires_within_at(Duration::from_secs(6 * 60), now));
        assert!(!r.expires_within_at(Duration::from_secs(4 * 60), now));
    }

    #[test]
    fn already_expired_is_always_within_any_window() {
        let r = resolution(Some(t(12, 0, 0)));
        let now = t(13, 0, 0);
        assert!(r.is_expired_at(now));
        assert!(r.expires_within_at(Duration::ZERO, now));
    }

    // -------------------------------------------------------- validación

    #[test]
    fn validate_accepts_http_uris() {
        for uri in ["http://cdn/x", "https://cdn/x?a=b"] {
            let r = StreamResolution::new(Source::YouTube, uri);
            assert!(r.validate().is_ok(), "{uri} debería validar");
        }
    }

    #[test]
    fn validate_rejects_empty_uri() {
        let r = StreamResolution::new(Source::YouTube, "");
        assert_eq!(r.validate(), Err(StreamValidationError::EmptyUri));
    }

    #[test]
    fn validate_rejects_unsupported_scheme() {
        let r = StreamResolution::new(Source::YouTube, "ftp://cdn/x");
        assert_eq!(
            r.validate(),
            Err(StreamValidationError::UnsupportedScheme("ftp".into()))
        );
    }

    // ------------------------------------------------------- media source

    #[test]
    fn media_source_carries_uri_and_headers() {
        let mut r = StreamResolution::new(Source::YouTube, "https://cdn/stream");
        r.headers = vec![("User-Agent".into(), "Tunefold".into())];
        let Some(MediaSource::Remote(stream)) = r.media_source() else {
            panic!("una URI https produce fuente remota");
        };
        assert_eq!(stream.url, "https://cdn/stream");
        assert_eq!(stream.headers.len(), 1);
    }

    #[test]
    fn media_source_none_for_non_http_uri() {
        let mut r = StreamResolution::new(Source::YouTube, "file:///music/song.mp3");
        assert!(r.media_source().is_none());
        r.uri = String::new();
        assert!(r.media_source().is_none());
    }

    // ------------------------------------------------------ serialización

    #[test]
    fn serializes_roundtrip_for_persistence() {
        let mut r = resolution(Some(t(20, 0, 0)));
        r.codec = Some("mp4a.40.2".into());
        r.bitrate = Some(128_000);
        r.quality = Quality::from_bitrate(r.bitrate);
        r.headers = vec![("Referer".into(), "https://www.youtube.com/".into())];
        r.metadata = vec![("itag".into(), "140".into())];

        let json = serde_json::to_string(&r).expect("serializa");
        let back: StreamResolution = serde_json::from_str(&json).expect("deserializa");
        assert_eq!(back, r);
    }
}
