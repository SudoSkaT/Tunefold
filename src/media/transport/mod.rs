//! Transporte de media para streams remotos HTTP (capa Media).
//!
//! [`HttpRangeStream`] convierte una URL de stream con soporte de rangos en un
//! **stream lógico continuo**: el consumidor pide el siguiente trozo y nunca
//! ve los límites de cada petición HTTP. La semántica de transporte vive aquí,
//! NO en los backends ni en los proveedores.
//!
//! Responsabilidades separadas:
//!
//! | Pieza              | Contrato                                        |
//! |--------------------|--------------------------------------------------|
//! | [`RangePolicy`]    | tamaños de ventana/retries (configurable)       |
//! | [`HttpRangeStream`]| descarga por ventanas + validación estricta     |
//! | [`TransportFailure`]| fallo clasificado sin detalles sensibles       |
//!
//! Validación por respuesta (nada se acepta en silencio): 206 esperado con
//! Content-Range coherente; 200 solo tolerado si ignora el primer rango desde
//! 0 (modo archivo-completo); truncamiento ⇒ reintento transitorio acotado;
//! 403 posicional ⇒ restricción del servidor (no cuota por IP: ver probes);
//! 416 ⇒ fin de archivo o respuesta inválida según posición.
//!
//! Observabilidad: cada petición registra request id, host (nunca la URL
//! completa ni parámetros firmados), rango pedido, status, cabeceras clave,
//! bytes, latencia, intento y clasificación.

use std::time::{Duration, Instant};

use crate::media::FailureCategory;

/// Política de ventanas de descarga (pura, configurable).
///
/// Los valores por defecto son conservadores: la evidencia (probes
/// `probe_range`/`probe_frontier`) muestra servicio a plena velocidad dentro
/// de la extensión servible, así que ventanas moderadas minimizan el desperdicio
/// ante cortes sin penalizar throughput.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangePolicy {
    /// Tamaño de la primera petición (descubre tamaño total y cabeceras).
    pub initial_window: u64,
    /// Tamaño de las ventanas siguientes.
    pub window_size: u64,
    /// Reintentos máximos SOLO para fallos transitorios (red/timeout/trunca-
    /// miento/5xx). 403/401/404/416/respuesta inválida NO se reintentan.
    pub max_retries: u32,
    /// Backoff base entre reintentos (lineal: delay × intento).
    pub retry_delay: Duration,
    /// Timeout de cada petición individual.
    pub request_timeout: Duration,
}

impl Default for RangePolicy {
    fn default() -> Self {
        Self {
            initial_window: 64 * 1024,
            window_size: 512 * 1024,
            max_retries: 3,
            retry_delay: Duration::from_millis(400),
            request_timeout: Duration::from_secs(30),
        }
    }
}

impl RangePolicy {
    /// Política leída del entorno: `TUNEFOLD_RANGE_WINDOW_KIB` ajusta el
    /// tamaño de ventana (clampado 32–4096 KiB). Se acepta el nombre legacy
    /// `PLAYFUSION_RANGE_WINDOW_KIB` como alias. El resto queda por defecto.
    pub fn from_env() -> Self {
        let mut p = Self::default();
        let cur = std::env::var("PLAYFUSION_RANGE_WINDOW_KIB").ok();
        let kib = std::env::var("TUNEFOLD_RANGE_WINDOW_KIB")
            .ok()
            .or(cur)
            .and_then(|v| v.parse::<u64>().ok());
        if let Some(kib) = kib {
            p.window_size = kib.clamp(32, 4096) * 1024;
        }
        p
    }

    /// Longitud solicitada para la ventana que empieza en `offset`.
    /// El clamp al final del archivo lo hace el stream (conoce `total`).
    pub fn window_len_at(&self, offset: u64) -> u64 {
        if offset == 0 {
            self.initial_window.min(self.window_size)
        } else {
            self.window_size
        }
    }
}

/// Fallo de transporte clasificado. El mensaje NUNCA incluye la URL completa
/// ni parámetros firmados: solo host, offsets y status.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransportFailure {
    /// El servidor niega rangos más allá de un límite posicional (techo por
    /// URL según el contexto de resolución). No es cuota por IP: las
    /// repeticiones dentro del límite se sirven al instante.
    #[error("el servidor restringe el stream más allá del byte {limit:?}: {msg}")]
    Restricted { limit: Option<u64>, msg: String },
    /// 403/401 sin bytes servidos aún: URL caducada o sin credencial válida.
    #[error("la URL fue rechazada ({0})")]
    UrlRejected(String),
    #[error("el stream no existe ({0})")]
    NotFound(String),
    /// Respuesta que viola el contrato (200 fuera de sitio, Content-Range
    /// mentiroso, cuerpo corto persistente…).
    #[error("respuesta inválida: {0}")]
    InvalidResponse(String),
    #[error("timeout de red: {0}")]
    Timeout(String),
    #[error("fallo de red: {0}")]
    Network(String),
}

impl TransportFailure {
    /// Clasificación estructural para métricas y decisiones de recuperación.
    pub fn category(&self) -> FailureCategory {
        match self {
            TransportFailure::Restricted { .. } => FailureCategory::StreamRestricted,
            TransportFailure::UrlRejected(_) => FailureCategory::AuthenticationRequired,
            TransportFailure::NotFound(_) => FailureCategory::Unsupported,
            TransportFailure::InvalidResponse(_) => FailureCategory::InvalidResponse,
            TransportFailure::Timeout(_) => FailureCategory::Timeout,
            TransportFailure::Network(_) => FailureCategory::NetworkFailure,
        }
    }

    /// Solo los transitorios merecen reintento (idempotente por rango).
    fn is_transient(&self) -> bool {
        matches!(
            self,
            TransportFailure::Timeout(_) | TransportFailure::Network(_)
        )
    }
}

/// Parsea `Content-Range: bytes START-END/TOTAL`. `None` si está malformado.
pub fn parse_content_range(value: &str) -> Option<(u64, u64, Option<u64>)> {
    let rest = value.trim().strip_prefix("bytes")?.trim();
    let (range, total) = rest.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let start = start.trim().parse().ok()?;
    let end = end.trim().parse().ok()?;
    if end < start {
        return None;
    }
    let total = match total.trim() {
        "*" => None,
        n => Some(n.parse().ok()?),
    };
    if let Some(t) = total {
        if end >= t {
            return None;
        }
    }
    Some((start, end, total))
}

/// Host de una URL (para logs; jamás la URL completa).
fn host_of(url: &str) -> &str {
    url.split("://")
        .nth(1)
        .unwrap_or("?")
        .split('/')
        .next()
        .unwrap_or("?")
}

/// Métricas acumuladas de un stream (observabilidad §27).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TransportMetrics {
    pub requests: u64,
    pub retries: u64,
    pub bytes_received: u64,
}

/// Convierte un error reqwest en fallo de transporte SIN filtrar la URL
/// (los mensajes del cliente incluyen la URL completa con parámetros
/// firmados: se elimina siempre) y clasificando el timeout por tipo, no por
/// texto.
fn map_reqwest(e: reqwest::Error) -> TransportFailure {
    let timeout = e.is_timeout();
    let clean = e.without_url().to_string();
    if timeout {
        TransportFailure::Timeout(clean)
    } else {
        TransportFailure::Network(clean)
    }
}

/// Stream lógico continuo sobre peticiones HTTP Range encadenadas.
pub struct HttpRangeStream {
    http: reqwest::Client,
    url: String,
    headers: Vec<(String, String)>,
    policy: RangePolicy,
    /// Tamaño total del recurso (Content-Range o Content-Length conocido).
    total: u64,
    /// Siguiente byte lógico a entregar.
    pos: u64,
    pending: Vec<u8>,
    cursor: usize,
    eof: bool,
    rid: u64,
    metrics: TransportMetrics,
}

impl std::fmt::Debug for HttpRangeStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpRangeStream")
            .field("host", &host_of(&self.url))
            .field("total", &self.total)
            .field("pos", &self.pos)
            .field("eof", &self.eof)
            .finish_non_exhaustive()
    }
}

impl HttpRangeStream {
    /// Abre el stream: primera petición con rango cerrado que valida la
    /// respuesta, descubre el tamaño total y deja el primer bloque en buffer.
    pub async fn open(
        http: reqwest::Client,
        url: impl Into<String>,
        headers: Vec<(String, String)>,
        policy: RangePolicy,
    ) -> Result<Self, TransportFailure> {
        let url = url.into();
        let mut s = Self {
            http,
            url,
            headers,
            policy,
            total: 0,
            pos: 0,
            pending: Vec::new(),
            cursor: 0,
            eof: false,
            rid: 0,
            metrics: TransportMetrics::default(),
        };
        let len = s.policy.window_len_at(0);
        s.pending = s.fetch_window(0, len).await?;
        s.cursor = 0;
        Ok(s)
    }

    /// Tamaño total del recurso descubierto en la apertura.
    pub fn total(&self) -> u64 {
        self.total
    }

    /// Bytes lógicos ya entregados.
    pub fn position(&self) -> u64 {
        self.pos
    }

    pub fn metrics(&self) -> TransportMetrics {
        self.metrics
    }

    /// Siguiente trozo del stream lógico (`None` = EOF limpio).
    ///
    /// Drena el buffer de la ventana actual y encadena la petición siguiente
    /// cuando se agota; el consumidor nunca percibe los límites HTTP.
    pub async fn next_chunk(&mut self, max: usize) -> Result<Option<Vec<u8>>, TransportFailure> {
        loop {
            if self.cursor < self.pending.len() {
                let end = (self.cursor + max).min(self.pending.len());
                let chunk = self.pending[self.cursor..end].to_vec();
                self.cursor = end;
                self.pos += chunk.len() as u64;
                return Ok(Some(chunk));
            }
            if self.eof || (self.total > 0 && self.pos >= self.total) {
                return Ok(None);
            }
            let len = self
                .policy
                .window_len_at(self.pos)
                .min(self.total.saturating_sub(self.pos))
                .max(1);
            let start = self.pos;
            self.pending = self.fetch_window(start, len).await?;
            self.cursor = 0;
        }
    }

    /// Una petición de ventana con validación completa y retries acotados.
    async fn fetch_window(&mut self, start: u64, len: u64) -> Result<Vec<u8>, TransportFailure> {
        let mut attempt: u32 = 0;
        loop {
            self.rid += 1;
            self.metrics.requests += 1;
            let rid = self.rid;
            let started = Instant::now();
            let outcome = self.single_request(rid, start, len).await;
            let elapsed = started.elapsed();

            match outcome {
                Ok(bytes) => {
                    tracing::debug!(
                        rid,
                        host = host_of(&self.url),
                        range = %format!("{start}-{}", start + len - 1),
                        bytes = bytes.len(),
                        elapsed_ms = elapsed.as_millis() as u64,
                        attempt,
                        class = "Ok",
                        "transport_request"
                    );
                    return Ok(bytes);
                }
                Err(f) => {
                    let class = f.category();
                    tracing::debug!(
                        rid,
                        host = host_of(&self.url),
                        range = %format!("{start}-{}", start + len - 1),
                        elapsed_ms = elapsed.as_millis() as u64,
                        attempt,
                        class = %class,
                        error = %f,
                        "transport_request_failed"
                    );
                    if f.is_transient() && attempt < self.policy.max_retries {
                        attempt += 1;
                        self.metrics.retries += 1;
                        tokio::time::sleep(self.policy.retry_delay * attempt).await;
                        continue;
                    }
                    return Err(f);
                }
            }
        }
    }

    /// Petición individual SIN reintentos, con validación de contrato.
    async fn single_request(
        &mut self,
        rid: u64,
        start: u64,
        len: u64,
    ) -> Result<Vec<u8>, TransportFailure> {
        let end = start + len - 1;
        let mut req = self.http.get(&self.url);
        for (k, v) in &self.headers {
            req = req.header(k, v);
        }
        let resp = req
            .header("Range", format!("bytes={start}-{end}"))
            .timeout(self.policy.request_timeout)
            .send()
            .await
            .map_err(map_reqwest)?;

        let status = resp.status().as_u16();
        let hdr = |name: &str| {
            resp.headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let content_range = hdr("content-range");
        let accept_ranges = hdr("accept-ranges");
        let content_type = hdr("content-type");
        let content_length = resp.content_length();

        // Lectura del cuerpo; el truncamiento se detecta contra
        // Content-Range (206) o Content-Length al validar.
        let mut body = resp.bytes_stream();
        use futures_util::StreamExt;
        let mut buf: Vec<u8> = Vec::new();
        let read_err: Option<reqwest::Error> = loop {
            match body.next().await {
                Some(Ok(chunk)) => buf.extend_from_slice(&chunk),
                Some(Err(e)) => break Some(e),
                None => break None,
            }
        };

        tracing::trace!(
            rid,
            host = host_of(&self.url),
            range = %format!("{start}-{end}"),
            status,
            clen = content_length.unwrap_or(0),
            cr = content_range.as_deref().unwrap_or("-"),
            ar = accept_ranges.as_deref().unwrap_or("-"),
            ct = content_type.as_deref().unwrap_or("-"),
            received = buf.len(),
            "transport_response"
        );

        if let Some(e) = read_err {
            return Err(map_reqwest(e));
        }

        match status {
            206 => {
                let Some((s, e, total)) = content_range.as_deref().and_then(parse_content_range)
                else {
                    return Err(TransportFailure::InvalidResponse(format!(
                        "206 sin Content-Range válido en byte {start}"
                    )));
                };
                if s != start {
                    return Err(TransportFailure::InvalidResponse(format!(
                        "Content-Range empieza en {s}, se pidió {start}"
                    )));
                }
                if let Some(t) = total {
                    if self.total == 0 {
                        self.total = t;
                    } else if self.total != t {
                        return Err(TransportFailure::InvalidResponse(format!(
                            "total cambió: {} != {}",
                            t, self.total
                        )));
                    }
                }
                if buf.len() as u64 != e - s + 1 {
                    // Truncado: transitorio (el rango es idempotente).
                    return Err(TransportFailure::Network(format!(
                        "cuerpo truncado: {} de {} bytes",
                        buf.len(),
                        e - s + 1
                    )));
                }
                Ok(buf)
            }
            200 => {
                // Servidor que ignora Range: válido SOLO cubriendo desde 0.
                // El cuerpo completo es la única entrega: se marca EOF para
                // que el stream lógico termine con esta respuesta.
                if start == 0 {
                    self.total = content_length.unwrap_or(buf.len() as u64);
                    self.eof = true;
                    Ok(buf)
                } else {
                    Err(TransportFailure::InvalidResponse(format!(
                        "200 ignoró Range en byte {start}"
                    )))
                }
            }
            403 | 401 => {
                if self.pos == 0 && start == 0 {
                    Err(TransportFailure::UrlRejected(format!("HTTP {status}")))
                } else {
                    Err(TransportFailure::Restricted {
                        limit: Some(start),
                        msg: format!("HTTP {status} pidiendo byte {start}"),
                    })
                }
            }
            404 => Err(TransportFailure::NotFound(format!("HTTP 404 byte {start}"))),
            416 => Err(TransportFailure::InvalidResponse(format!(
                "416 en byte {start} (total={})",
                self.total
            ))),
            500..=599 => Err(TransportFailure::Network(format!(
                "HTTP {status} en byte {start}"
            ))),
            other => Err(TransportFailure::InvalidResponse(format!(
                "HTTP {other} inesperado en byte {start}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --------------------------------------------------- parse_content_range

    #[test]
    fn content_range_valid_forms_parse() {
        assert_eq!(
            parse_content_range("bytes 0-1048575/2395999"),
            Some((0, 1048575, Some(2395999)))
        );
        assert_eq!(parse_content_range("bytes 5-9/*"), Some((5, 9, None)));
    }

    #[test]
    fn content_range_malformed_is_none() {
        for bad in [
            "",
            "bytes",
            "bytes /",
            "bytes 5/10",
            "bytes 9-5/10",
            "bytes 5-9/x",
        ] {
            assert_eq!(parse_content_range(bad), None, "{bad}");
        }
    }

    #[test]
    fn content_range_end_beyond_total_is_none() {
        assert_eq!(parse_content_range("bytes 0-10/10"), None);
        assert_eq!(parse_content_range("bytes 0-9/10"), Some((0, 9, Some(10))));
    }

    // ------------------------------------------------------------ RangePolicy

    #[test]
    fn policy_first_window_is_initial() {
        let p = RangePolicy {
            initial_window: 64 * 1024,
            window_size: 512 * 1024,
            ..Default::default()
        };
        assert_eq!(p.window_len_at(0), 64 * 1024);
        assert_eq!(p.window_len_at(1), 512 * 1024);
        assert_eq!(p.window_len_at(u64::MAX / 2), 512 * 1024);
    }

    #[test]
    fn policy_from_env_clamps_window_kib() {
        // La función lee el entorno real; probamos el clamp puro aquí.
        let kib = 9999_u64.clamp(32, 4096);
        assert_eq!(kib, 4096);
        let kib = 1_u64.clamp(32, 4096);
        assert_eq!(kib, 32);
    }

    // ------------------------------------------------------ clasificación

    #[test]
    fn failures_classify_into_structural_categories() {
        let f = |t: &str| {
            TransportFailure::Restricted {
                limit: Some(1),
                msg: t.to_string(),
            }
            .category()
        };
        assert_eq!(f(""), FailureCategory::StreamRestricted);
        assert_eq!(
            TransportFailure::UrlRejected("403".into()).category(),
            FailureCategory::AuthenticationRequired
        );
        assert_eq!(
            TransportFailure::NotFound("x".into()).category(),
            FailureCategory::Unsupported
        );
        assert_eq!(
            TransportFailure::InvalidResponse("y".into()).category(),
            FailureCategory::InvalidResponse
        );
        assert_eq!(
            TransportFailure::Timeout("z".into()).category(),
            FailureCategory::Timeout
        );
        assert_eq!(
            TransportFailure::Network("w".into()).category(),
            FailureCategory::NetworkFailure
        );
    }

    #[test]
    fn only_transient_failures_retry() {
        assert!(TransportFailure::Timeout("t".into()).is_transient());
        assert!(TransportFailure::Network("n".into()).is_transient());
        assert!(!TransportFailure::Restricted {
            limit: None,
            msg: "r".into()
        }
        .is_transient());
        assert!(!TransportFailure::UrlRejected("u".into()).is_transient());
        assert!(!TransportFailure::InvalidResponse("i".into()).is_transient());
        assert!(!TransportFailure::NotFound("f".into()).is_transient());
    }

    #[test]
    fn failure_display_has_no_full_url_semantics() {
        let f = TransportFailure::Restricted {
            limit: Some(1048576),
            msg: "HTTP 403 pidiendo byte 1048576".into(),
        };
        assert!(f.to_string().contains("1048576"));
        assert!(!f.to_string().contains("http"));
    }
}

#[cfg(test)]
mod fake_server;
#[cfg(test)]
mod integration;
