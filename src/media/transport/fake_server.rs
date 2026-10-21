//! Servidor HTTP fake para los tests de integración del transporte.
//!
//! Simula los comportamientos observados en el diagnóstico: rangos cerrados
//! con Content-Range honesto, servidores que ignoran Range, techos
//! posicionales (403 más allá de un umbral), respuestas truncadas,
//! Content-Range mentiroso, 404 y lentitud (timeout).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Escenario del servidor fake.
pub enum Scenario {
    /// Sirve rangos cerrados con 206 + Content-Range honesto; 416 más allá
    /// del final.
    Normal(Arc<Vec<u8>>),
    /// Ignora Range: siempre 200 con el cuerpo completo.
    IgnoreRange(Arc<Vec<u8>>),
    /// 200 sin Content-Length (radio en directo): cuerpo secuencial.
    LiveNoLength(Arc<Vec<u8>>),
    /// Sirve el prefijo hasta `threshold` y responde 403 a cualquier rango
    /// que empiece en o después de él (techo posicional).
    ForbiddenBeyond { data: Arc<Vec<u8>>, threshold: u64 },
    /// 403 a todo.
    AlwaysForbidden,
    /// Las primeras `count` respuestas llegan truncadas a la mitad (206 con
    /// Content-Range correcto pero cuerpo corto = truncamiento detectable).
    TruncateFirst {
        data: Arc<Vec<u8>>,
        count: usize,
        served: Arc<AtomicUsize>,
    },
    /// 206 con Content-Range MALFORMADO.
    BadContentRange(Arc<Vec<u8>>),
    /// Retardo fijo antes de cada respuesta (timeout).
    Slow(Duration, Arc<Vec<u8>>),
}

pub struct FakeServer {
    pub addr: String,
    task: tokio::task::JoinHandle<()>,
    requests: Arc<AtomicUsize>,
}

impl FakeServer {
    /// Arranca el servidor con el escenario dado en un puerto efímero.
    pub async fn start(scenario: Scenario) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr").to_string();
        let requests = Arc::new(AtomicUsize::new(0));
        let req_counter = requests.clone();
        let scenario = Arc::new(scenario);
        let task = tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else {
                    break;
                };
                let sc = scenario.clone();
                let rc = req_counter.clone();
                tokio::spawn(async move {
                    let _ = serve(socket, sc, rc).await;
                });
            }
        });
        Self {
            addr,
            task,
            requests,
        }
    }

    pub fn url(&self) -> String {
        format!("http://{}/audio.mp4", self.addr)
    }

    pub fn request_count(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve(
    mut socket: TcpStream,
    scenario: Arc<Scenario>,
    requests: Arc<AtomicUsize>,
) -> std::io::Result<()> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = socket.read(&mut byte).await?;
        if n == 0 {
            return Ok(());
        }
        head.push(byte[0]);
        if head.ends_with(b"\r\n\r\n") {
            break;
        }
        if head.len() > 16 * 1024 {
            return Ok(());
        }
    }
    let text = String::from_utf8_lossy(&head);
    let range = text
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case("range")
                .then(|| v.trim().to_string())
        })
        .unwrap_or_default();

    let (start, end) = parse_range(&range).unwrap_or((0, u64::MAX));
    requests.fetch_add(1, Ordering::SeqCst);

    let (status_line, extra_headers, body): (&str, String, Vec<u8>) = match &*scenario {
        Scenario::Normal(data) => reply_range(start, end, data),
        Scenario::IgnoreRange(data) => (
            "HTTP/1.1 200 OK",
            format!("Content-Length: {}\r\n", data.len()),
            data.as_ref().clone(),
        ),
        Scenario::LiveNoLength(data) => (
            "HTTP/1.1 200 OK",
            "Content-Type: audio/aac\r\n".to_string(),
            data.as_ref().clone(),
        ),
        Scenario::ForbiddenBeyond { data, threshold } => {
            if start >= *threshold {
                (
                    "HTTP/1.1 403 Forbidden",
                    "Content-Length: 0\r\n".to_string(),
                    Vec::new(),
                )
            } else {
                reply_range(start, end, data)
            }
        }
        Scenario::AlwaysForbidden => (
            "HTTP/1.1 403 Forbidden",
            "Content-Length: 0\r\n".to_string(),
            Vec::new(),
        ),
        Scenario::TruncateFirst {
            data,
            count,
            served,
        } => {
            let (status, headers, mut body) = reply_range(start, end, data);
            if served.fetch_add(1, Ordering::SeqCst) < *count {
                let keep = body.len() / 2;
                body.truncate(keep);
                let headers = headers.replace(
                    &format!("Content-Length: {}\r\n", end - start + 1),
                    &format!("Content-Length: {}\r\n", keep),
                );
                return send(socket, status, &headers, &body).await;
            }
            return send(socket, status, &headers, &body).await;
        }
        Scenario::BadContentRange(data) => {
            let total = data.len() as u64;
            if start < total {
                let end2 = end.min(total - 1);
                let slice = data[start as usize..=(end2 as usize)].to_vec();
                (
                    "HTTP/1.1 206 Partial Content",
                    format!(
                        "Content-Range: nonsense\r\nContent-Length: {}\r\n",
                        slice.len()
                    ),
                    slice,
                )
            } else {
                (
                    "HTTP/1.1 416 Range Not Satisfiable",
                    format!("Content-Range: bytes */{total}\r\nContent-Length: 0\r\n"),
                    Vec::new(),
                )
            }
        }
        Scenario::Slow(delay, data) => {
            tokio::time::sleep(*delay).await;
            reply_range(start, end, data)
        }
    };

    send(socket, status_line, &extra_headers, &body).await
}

async fn send(
    mut socket: TcpStream,
    status_line: &str,
    headers: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let response = format!("{status_line}\r\n{headers}Connection: close\r\n\r\n");
    socket.write_all(response.as_bytes()).await?;
    socket.write_all(body).await?;
    socket.flush().await
}

fn reply_range(start: u64, end: u64, data: &[u8]) -> (&'static str, String, Vec<u8>) {
    let total = data.len() as u64;
    if start >= total {
        return (
            "HTTP/1.1 416 Range Not Satisfiable",
            format!("Content-Range: bytes */{total}\r\nContent-Length: 0\r\n"),
            Vec::new(),
        );
    }
    let end = end.min(total - 1);
    let body = data[start as usize..=(end as usize)].to_vec();
    (
        "HTTP/1.1 206 Partial Content",
        format!(
            "Content-Range: bytes {start}-{end}/{total}\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nContent-Type: audio/mp4\r\n",
            body.len()
        ),
        body,
    )
}

fn parse_range(range: &str) -> Option<(u64, u64)> {
    let rest = range.strip_prefix("bytes=")?;
    let (s, e) = rest.split_once('-')?;
    Some((s.parse().ok()?, e.parse().ok()?))
}
