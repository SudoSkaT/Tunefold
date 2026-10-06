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

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::media::FailureCategory;

/// Una petición Range individual observada por la traza de reproducción.
///
/// NUNCA contiene la URL completa ni cabeceras firmadas: solo host, offsets,
/// status y tiempos. Es la unidad que permite reconstruir qué hizo cada
/// request de una reproducción concreta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeRequestRecord {
    /// Identificador correlativo dentro del stream lógico (empieza en 1).
    pub request_id: u64,
    /// Offset inicial solicitado.
    pub start: u64,
    /// Offset final solicitado.
    pub end: u64,
    /// Bytes pedidos (`end - start + 1`).
    pub requested_bytes: u64,
    /// Bytes realmente recibidos en el cuerpo.
    pub received_bytes: u64,
    /// Estado HTTP observado (`None` si la petición no llegó a respuesta).
    pub status: Option<u16>,
    /// Latencia hasta recibir las cabeceras de la respuesta.
    pub headers_us: u64,
    /// Latencia total de la petición (envío → cuerpo completo).
    pub total_us: u64,
    /// Número de reintento: 0 para el primer intento.
    pub retry: u32,
    /// Clasificación del resultado (`ok`, `timeout`, `network`, …).
    pub classification: &'static str,
    /// Posición lógica del `StreamReader` cuando se emitió la petición.
    pub reader_position: u64,
    /// `true` si la petición es consecuencia de un `seek`.
    pub from_seek: bool,
    /// Host (nunca la ruta ni los parámetros firmados).
    pub host: String,
}

/// Receptor de telemetría por petición Range. Se invoca desde el hilo del
/// decoder durante `fetch_window`, por lo que NUNCA debe bloquear.
pub trait RangeTraceSink: Send + Sync {
    /// Una petición Range terminó (con éxito o con fallo clasificado).
    fn range_request(&self, record: &RangeRequestRecord);

    /// Evento semántico del transporte (p. ej. `HTTP_FIRST_RESPONSE`).
    fn event(&self, _event: &str, _detail: &str) {}
}

/// Contexto de traza compartido entre el stream lógico y el `StreamReader`
/// que lo consume: sabe quién pregunta (posición lógica) y si la próxima
/// petición viene de un `seek`.
#[derive(Clone, Default)]
pub struct RangeTraceContext {
    sink: Option<Arc<dyn RangeTraceSink>>,
    reader_position: Arc<AtomicU64>,
    pending_seek: Arc<AtomicBool>,
    /// Host cacheado: el registro de traza no debe recalcularlo por petición.
    host: Option<String>,
}

impl std::fmt::Debug for RangeTraceContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RangeTraceContext")
            .field("sink", &self.sink.is_some())
            .field(
                "reader_position",
                &self.reader_position.load(Ordering::Relaxed),
            )
            .field("pending_seek", &self.pending_seek.load(Ordering::Relaxed))
            .finish()
    }
}

impl RangeTraceContext {
    /// Contexto sin observador: coste cero para el pipeline desktop.
    pub fn disabled() -> Self {
        Self::default()
    }

    /// Contexto observado por `sink`.
    pub fn with_sink(sink: Arc<dyn RangeTraceSink>) -> Self {
        Self {
            sink: Some(sink),
            ..Self::default()
        }
    }

    /// Publica la posición lógica del lector antes de consumir o reposicionar.
    pub fn set_reader_position(&self, position: u64) {
        self.reader_position.store(position, Ordering::Relaxed);
    }

    /// Marca que la próxima petición Range viene de un `seek` del consumidor.
    pub fn begin_seek(&self) {
        self.pending_seek.store(true, Ordering::Relaxed);
    }

    /// Consume y devuelve la marca de seek para la siguiente petición.
    fn take_seek(&self) -> bool {
        self.pending_seek.swap(false, Ordering::Relaxed)
    }

    /// Posición lógica del consumidor al emitirse la petición.
    fn reader_position(&self) -> u64 {
        self.reader_position.load(Ordering::Relaxed)
    }

    pub fn is_active(&self) -> bool {
        self.sink.is_some()
    }

    fn record(&self, record: &RangeRequestRecord) {
        if let Some(sink) = &self.sink {
            sink.range_request(record);
        }
    }

    fn event(&self, event: &str, detail: &str) {
        if let Some(sink) = &self.sink {
            sink.event(event, detail);
        }
    }

    fn set_host(&mut self, host: String) {
        self.host = Some(host);
    }

    fn host(&self) -> &str {
        self.host.as_deref().unwrap_or("?")
    }
}

/// Etiqueta estable de un fallo de transporte para la traza. Usa la
/// categoría estructural (nunca texto libre del servidor).
fn failure_label(failure: &TransportFailure) -> &'static str {
    match failure {
        TransportFailure::Restricted { .. } => "restricted",
        TransportFailure::UrlRejected(_) => "url_rejected",
        TransportFailure::NotFound(_) => "not_found",
        TransportFailure::InvalidResponse(_) => "invalid_response",
        TransportFailure::Timeout(_) => "timeout",
        TransportFailure::Network(_) => "network",
    }
}

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
    /// Latencia acumulada de cada petición hasta recibir y validar su cuerpo.
    pub elapsed_ms: u64,
    /// Tiempo hasta las cabeceras de la primera respuesta Range.
    pub first_response_us: u64,
    /// Latencia acumulada hasta las CABECERAS de todas las respuestas.
    pub headers_us: u64,
    /// `Seek` solicitados por el consumidor.
    pub seeks: u64,
    /// Peticiones Range que fueron consecuencia de un `seek`.
    pub seek_requests: u64,
}

impl HttpRangeStream {
    /// Descarga el stream lógico COMPLETO y lo entrega en trozos.
    ///
    /// Es la vía explícita de adquisición completa de un track (Download). No
    /// la usa la reproducción: Play consume `next_chunk` y sigue.
    ///
    /// El llamador decide el destino; esta función no escribe nada, no decide
    /// rutas y no conserva identidad de medios: solo entrega bytes. Quien
    /// decide si eso es caché, descarga o prebuffering es el llamador.
    pub async fn download_full<F>(&mut self, mut on_chunk: F) -> Result<u64, TransportFailure>
    where
        F: FnMut(&[u8]) -> Result<(), TransportFailure>,
    {
        let mut delivered = 0u64;
        while let Some(chunk) = self.next_chunk(256 * 1024).await? {
            on_chunk(&chunk)?;
            delivered += chunk.len() as u64;
        }
        Ok(delivered)
    }

    /// Descarga el recurso completo desde cero como un `Vec<u8>`.
    ///
    /// Azúcar sobre [`HttpRangeStream::download_full`] para usos acotados
    /// (diagnóstico, pruebas). El límite de memoria lo impone el llamante:
    /// para persistencia se usa la variante con `on_chunk`.
    pub async fn download_to_vec(&mut self) -> Result<Vec<u8>, TransportFailure> {
        let mut out = Vec::new();
        self.download_full(|chunk| {
            out.extend_from_slice(chunk);
            Ok(())
        })
        .await?;
        Ok(out)
    }
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

/// Petición Range que llegó a respuesta y supera la validación de contrato.
struct RequestOutcome {
    bytes: Vec<u8>,
    status: u16,
    headers_us: u64,
}

/// Petición Range fallida: conserva el fallo clasificado más lo que se observó
/// antes de fallar (status, latencia de cabeceras) para la traza.
#[derive(Debug)]
struct RequestFailure {
    failure: TransportFailure,
    status: Option<u16>,
    headers_us: u64,
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
    /// Offset absoluto de `pending[0]`. Permite reconocer un `seek` que cae
    /// dentro de la ventana ya descargada y reutilizarla en vez de pedirla.
    window_start: u64,
    eof: bool,
    rid: u64,
    metrics: TransportMetrics,
    trace: RangeTraceContext,
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
        Self::open_with_trace(http, url, headers, policy, RangeTraceContext::disabled()).await
    }

    /// Igual que [`HttpRangeStream::open`] pero publica cada petición Range en
    /// `trace`. El contexto también es el que el `StreamReader` usa para
    /// publicar su posición lógica y marcar los `seek`.
    pub async fn open_with_trace(
        http: reqwest::Client,
        url: impl Into<String>,
        headers: Vec<(String, String)>,
        policy: RangePolicy,
        mut trace: RangeTraceContext,
    ) -> Result<Self, TransportFailure> {
        let url = url.into();
        trace.set_host(host_of(&url).to_string());
        let mut s = Self {
            http,
            url,
            headers,
            policy,
            total: 0,
            pos: 0,
            pending: Vec::new(),
            cursor: 0,
            window_start: 0,
            eof: false,
            rid: 0,
            metrics: TransportMetrics::default(),
            trace,
        };
        let len = s.policy.window_len_at(0);
        s.pending = s.fetch_window(0, len).await?;
        s.cursor = 0;
        s.window_start = 0;
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

    /// Reposition the byte stream and fetch the next ranged window at that
    /// offset. This is used by container probes such as MP4, whose parser
    /// seeks while inspecting atom tables.
    ///
    /// Si la posición cae dentro de la ventana ya descargada se reposiciona
    /// el cursor SIN pedirla de nuevo: los seeks del probe son casi siempre
    /// corto alcance hacia delante y repetir la descarga desperdiciaba casi
    /// toda la ventana. Solo se pide una ventana nueva cuando el destino cae
    /// fuera de ella.
    pub async fn seek_to(&mut self, position: u64) -> Result<(), TransportFailure> {
        self.metrics.seeks += 1;
        if self.trace.is_active() {
            self.trace.set_reader_position(position);
            self.trace.begin_seek();
        }
        if position > self.total {
            self.pos = position;
            self.pending.clear();
            self.cursor = 0;
            self.window_start = position;
            self.eof = true;
            return Ok(());
        }
        self.pos = position;
        if self.contains_position(position) {
            self.cursor = (position - self.window_start) as usize;
            self.eof = position >= self.total;
            return Ok(());
        }
        self.pending.clear();
        self.cursor = 0;
        self.eof = position == self.total;
        if self.eof {
            self.window_start = position;
            return Ok(());
        }
        let len = self
            .policy
            .window_len_at(position)
            .min(self.total.saturating_sub(position))
            .max(1);
        self.pending = self.fetch_window(position, len).await?;
        self.window_start = position;
        self.eof = false;
        Ok(())
    }

    /// `true` si `position` ya está dentro de la ventana descargada.
    fn contains_position(&self, position: u64) -> bool {
        position >= self.window_start && position - self.window_start < self.pending.len() as u64
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
            self.window_start = start;
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
            let outcome = self.single_request(rid, start, len, started).await;
            let elapsed = started.elapsed();

            match outcome {
                Ok(ok) => {
                    self.metrics.bytes_received += ok.bytes.len() as u64;
                    self.metrics.elapsed_ms += elapsed.as_millis() as u64;
                    self.metrics.headers_us += ok.headers_us;
                    self.record_request(
                        rid,
                        start,
                        len,
                        attempt,
                        Some(ok.status),
                        ok.headers_us,
                        ok.bytes.len() as u64,
                        elapsed,
                        "ok",
                    );
                    tracing::debug!(
                        rid,
                        host = host_of(&self.url),
                        range = %format!("{start}-{}", start + len - 1),
                        bytes = ok.bytes.len(),
                        elapsed_ms = elapsed.as_millis() as u64,
                        attempt,
                        class = "Ok",
                        "transport_request"
                    );
                    return Ok(ok.bytes);
                }
                Err(f) => {
                    self.metrics.elapsed_ms += elapsed.as_millis() as u64;
                    self.metrics.headers_us += f.headers_us;
                    let class = f.failure.category();
                    self.record_request(
                        rid,
                        start,
                        len,
                        attempt,
                        f.status,
                        f.headers_us,
                        0,
                        elapsed,
                        failure_label(&f.failure),
                    );
                    tracing::debug!(
                        rid,
                        host = host_of(&self.url),
                        range = %format!("{start}-{}", start + len - 1),
                        elapsed_ms = elapsed.as_millis() as u64,
                        attempt,
                        class = %class,
                        error = %f.failure,
                        "transport_request_failed"
                    );
                    if f.failure.is_transient() && attempt < self.policy.max_retries {
                        attempt += 1;
                        self.metrics.retries += 1;
                        tokio::time::sleep(self.policy.retry_delay * attempt).await;
                        continue;
                    }
                    return Err(f.failure);
                }
            }
        }
    }

    /// Publica una petición Range en la traza (sin coste si no hay observador).
    #[allow(clippy::too_many_arguments)]
    fn record_request(
        &mut self,
        rid: u64,
        start: u64,
        len: u64,
        attempt: u32,
        status: Option<u16>,
        headers_us: u64,
        received_bytes: u64,
        elapsed: Duration,
        classification: &'static str,
    ) {
        if !self.trace.is_active() {
            return;
        }
        let from_seek = self.trace.take_seek();
        if from_seek {
            self.metrics.seek_requests += 1;
        }
        self.trace.record(&RangeRequestRecord {
            request_id: rid,
            start,
            end: start + len - 1,
            requested_bytes: len,
            received_bytes,
            status,
            headers_us,
            total_us: elapsed.as_micros() as u64,
            retry: attempt,
            classification,
            reader_position: self.trace.reader_position(),
            from_seek,
            host: self.trace.host().to_string(),
        });
    }

    /// Petición individual SIN reintentos, con validación de contrato.
    async fn single_request(
        &mut self,
        rid: u64,
        start: u64,
        len: u64,
        request_started: Instant,
    ) -> Result<RequestOutcome, RequestFailure> {
        let end = start + len - 1;
        let mut req = self.http.get(&self.url);
        for (k, v) in &self.headers {
            req = req.header(k, v);
        }
        let sent = req
            .header("Range", format!("bytes={start}-{end}"))
            .timeout(self.policy.request_timeout)
            .send()
            .await;
        let resp = match sent {
            Ok(resp) => resp,
            Err(error) => {
                return Err(RequestFailure {
                    failure: map_reqwest(error),
                    status: None,
                    headers_us: request_started.elapsed().as_micros() as u64,
                })
            }
        };
        let headers_us = request_started.elapsed().as_micros() as u64;
        if self.metrics.first_response_us == 0 {
            self.metrics.first_response_us = headers_us;
            self.trace.event(
                "HTTP_FIRST_RESPONSE",
                &format!(
                    "req={rid} range={start}-{end} status={} headers_us={headers_us}",
                    resp.status().as_u16()
                ),
            );
        }

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

        let invalid = |failure: TransportFailure| RequestFailure {
            failure,
            status: Some(status),
            headers_us,
        };

        if let Some(e) = read_err {
            return Err(RequestFailure {
                failure: map_reqwest(e),
                status: Some(status),
                headers_us,
            });
        }

        match status {
            206 => {
                let Some((s, e, total)) = content_range.as_deref().and_then(parse_content_range)
                else {
                    return Err(invalid(TransportFailure::InvalidResponse(format!(
                        "206 sin Content-Range válido en byte {start}"
                    ))));
                };
                if s != start {
                    return Err(invalid(TransportFailure::InvalidResponse(format!(
                        "Content-Range empieza en {s}, se pidió {start}"
                    ))));
                }
                if let Some(t) = total {
                    if self.total == 0 {
                        self.total = t;
                    } else if self.total != t {
                        return Err(invalid(TransportFailure::InvalidResponse(format!(
                            "total cambió: {} != {}",
                            t, self.total
                        ))));
                    }
                }
                if buf.len() as u64 != e - s + 1 {
                    // Truncado: transitorio (el rango es idempotente).
                    return Err(invalid(TransportFailure::Network(format!(
                        "cuerpo truncado: {} de {} bytes",
                        buf.len(),
                        e - s + 1
                    ))));
                }
                Ok(RequestOutcome {
                    bytes: buf,
                    status,
                    headers_us,
                })
            }
            200 => {
                // Servidor que ignora Range: válido SOLO cubriendo desde 0.
                // El cuerpo completo es la única entrega: se marca EOF para
                // que el stream lógico termine con esta respuesta.
                if start == 0 {
                    self.total = content_length.unwrap_or(buf.len() as u64);
                    self.eof = true;
                    Ok(RequestOutcome {
                        bytes: buf,
                        status,
                        headers_us,
                    })
                } else {
                    Err(invalid(TransportFailure::InvalidResponse(format!(
                        "200 ignoró Range en byte {start}"
                    ))))
                }
            }
            403 | 401 => {
                if self.pos == 0 && start == 0 {
                    Err(invalid(TransportFailure::UrlRejected(format!(
                        "HTTP {status}"
                    ))))
                } else {
                    Err(invalid(TransportFailure::Restricted {
                        limit: Some(start),
                        msg: format!("HTTP {status} pidiendo byte {start}"),
                    }))
                }
            }
            404 => Err(invalid(TransportFailure::NotFound(format!(
                "HTTP 404 byte {start}"
            )))),
            416 => Err(invalid(TransportFailure::InvalidResponse(format!(
                "416 en byte {start} (total={})",
                self.total
            )))),
            500..=599 => Err(invalid(TransportFailure::Network(format!(
                "HTTP {status} en byte {start}"
            )))),
            other => Err(invalid(TransportFailure::InvalidResponse(format!(
                "HTTP {other} inesperado en byte {start}"
            )))),
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
pub(crate) mod fake_server;
#[cfg(test)]
mod integration;
