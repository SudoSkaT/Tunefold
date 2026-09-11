//! Instrumentación diagnóstica del límite de ~1 MiB en streams de YouTube.
//!
//! Sobre UNA sola StreamResolution real ejecuta:
//!   - traza de redirects (¿hay salto? ¿sobrevive el Range?);
//!   - TEST A: GET normal sin Range (bytes hasta EOF/error);
//!   - TEST B/C/D: rangos cerrados [0,1MiB) [1MiB,2MiB) [2MiB,3MiB);
//!   - TEST E: ventanas encadenadas [3MiB,6MiB) sin pausas (encadenamiento).
//!
//! Uso: cargo run --release --example probe_range -- [query]
//!
//! Seguridad: nunca imprime la URL completa ni valores de parámetros
//! sensibles (sig/pot/lsig/ratebypass); solo host y nombres de parámetros,
//! más valores inofensivos (itag/mime/clen/expire/dur).

use std::time::{Duration, Instant};

use futures_util::StreamExt;

use tunefold::catalog::CatalogProvider;
use tunefold::domain::track::Track;
use tunefold::providers::youtube::{context_headers, YouTubeAdapter};

const MIB: u64 = 1024 * 1024;

#[derive(Debug)]
struct RequestLog {
    id: &'static str,
    range: String,
    host: String,
    status: u16,
    content_length: Option<u64>,
    content_range: Option<String>,
    accept_ranges: Option<String>,
    content_type: Option<String>,
    bytes_received: u64,
    elapsed: Duration,
    retries: u32,
    outcome: String,
}

fn classify(status: u16, error: Option<&str>) -> &'static str {
    if let Some(e) = error {
        let m = e.to_ascii_lowercase();
        if m.contains("timed out") || m.contains("timeout") {
            return "Timeout";
        }
        return "NetworkFailure";
    }
    match status {
        206 => "Ok(Range)",
        200 => "Ok(Full)",
        401 | 403 => "AuthenticationRequired",
        404 => "Unsupported",
        416 => "RangeNotSatisfiable",
        429 => "RateLimited",
        500..=599 => "ProviderUnavailable",
        _ => "Unknown",
    }
}

/// Host de una URL (sin path ni query). Nunca imprime la URL completa.
fn host_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or("?")
        .split('/')
        .next()
        .unwrap_or("?")
        .to_string()
}

/// Nombres de parámetros de query + valores inofensivos seleccionados.
fn describe_query(url: &str) -> Vec<(String, Option<String>)> {
    const SAFE: &[&str] = &[
        "itag",
        "mime",
        "clen",
        "dur",
        "expire",
        "alr",
        "keepalive",
        "c",
    ];
    let Some(q) = url.split('?').nth(1) else {
        return Vec::new();
    };
    q.split('&')
        .filter(|kv| !kv.is_empty())
        .map(|kv| {
            let mut parts = kv.splitn(2, '=');
            let k = parts.next().unwrap_or("?").to_string();
            let v = parts.next().map(str::to_string);
            let shown = v.filter(|_| SAFE.contains(&k.as_str()));
            (k, shown)
        })
        .collect()
}

fn print_log(l: &RequestLog) {
    println!(
        "[{id}] range={range} host={host}\n    status={status} CL={cl:?} CR={cr:?} AR={ar:?} CT={ct:?}",
        id = l.id,
        range = l.range,
        host = l.host,
        status = l.status,
        cl = l.content_length,
        cr = l.content_range,
        ar = l.accept_ranges,
        ct = l.content_type,
    );
    println!(
        "    bytes={} elapsed={:.2}s speed={:.0} KB/s retries={} outcome={}",
        l.bytes_received,
        l.elapsed.as_secs_f64(),
        if l.elapsed.as_secs_f64() > 0.0 {
            l.bytes_received as f64 / 1024.0 / l.elapsed.as_secs_f64()
        } else {
            0.0
        },
        l.retries,
        l.outcome,
    );
}

struct Probe<'a> {
    http: reqwest::Client,
    headers: &'a [(String, String)],
}

impl Probe<'_> {
    /// Una petición con registro completo. `range`: None = sin cabecera Range.
    async fn request(
        &self,
        id: &'static str,
        url: &str,
        range: Option<(u64, u64)>,
        max_bytes: u64,
    ) -> RequestLog {
        let started = Instant::now();
        let mut req = self.http.get(url);
        for (k, v) in self.headers {
            req = req.header(k, v);
        }
        if let Some((s, e)) = range {
            req = req.header("Range", format!("bytes={s}-{e}"));
        }
        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                return RequestLog {
                    id,
                    range: range.map_or("none".into(), |(s, e)| format!("{s}-{e}")),
                    host: host_of(url),
                    status: 0,
                    content_length: None,
                    content_range: None,
                    accept_ranges: None,
                    content_type: None,
                    bytes_received: 0,
                    elapsed: started.elapsed(),
                    retries: 0,
                    outcome: format!("Error({})={}", classify(0, Some(&e.to_string())), e),
                };
            }
        };
        let status = resp.status().as_u16();
        let hdr = |name: &str| {
            resp.headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let content_length = resp.content_length();
        let content_range = hdr("content-range");
        let accept_ranges = hdr("accept-ranges");
        let content_type = hdr("content-type");

        let mut body = resp.bytes_stream();
        let mut received: u64 = 0;
        let mut err: Option<String> = None;
        while received < max_bytes {
            match body.next().await {
                Some(Ok(chunk)) => received += chunk.len() as u64,
                Some(Err(e)) => {
                    err = Some(e.to_string());
                    break;
                }
                None => break,
            }
        }
        let outcome = match err {
            Some(e) => format!("Error({})={e}", classify(status, Some(&e))),
            None if status == 206 => "Ok(206)".into(),
            None if status == 200 => {
                if received >= max_bytes {
                    "TruncatedAtProbeLimit".into()
                } else {
                    "EofClean".into()
                }
            }
            None => format!("Http{status}"),
        };
        RequestLog {
            id,
            range: range.map_or("none".into(), |(s, e)| format!("{s}-{e}")),
            host: host_of(url),
            status,
            content_length,
            content_range,
            accept_ranges,
            content_type,
            bytes_received: received,
            elapsed: started.elapsed(),
            retries: 0,
            outcome,
        }
    }

    async fn range(&self, id: &'static str, url: &str, start: u64, len: u64) -> RequestLog {
        self.request(id, url, Some((start, start + len - 1)), len)
            .await
    }
}

/// Traza manual de redirects con Range: ¿el CDN redirige? ¿qué hop sirve?
async fn trace_redirects(http: &reqwest::Client, url: &str, headers: &[(String, String)]) {
    println!("== Redirects ==");
    let mut current = url.to_string();
    for hop in 0..6u32 {
        let mut req = http.get(&current);
        for (k, v) in headers {
            req = req.header(k, v);
        }
        let req = req.header("Range", "bytes=0-1023");
        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                println!("hop{hop}: ERROR {e}");
                return;
            }
        };
        let status = resp.status().as_u16();
        let loc = resp
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        println!(
            "hop{hop}: {} host={} location_host={:?}",
            status,
            host_of(&current),
            loc.as_deref().map(host_of)
        );
        match (status, loc) {
            (300..=399, Some(next)) => current = next,
            _ => {
                // El hop final ya es la respuesta servible: el Range llegó.
                let cr = resp
                    .headers()
                    .get("content-range")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string);
                println!("hop final sirvió el rango: status={status} content-range={cr:?}");
                return;
            }
        }
    }
    println!("demasiados redirects (>5)");
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rustypipe=warn".into()),
        )
        .with_target(false)
        .init();

    let provider = YouTubeAdapter::new();
    let results = provider.search_tracks("Queen Bohemian Rhapsody", 3).await?;
    let track: Track = results
        .first()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("sin resultados"))?;
    println!(
        "pista: {} — video_id={} (única StreamResolution para todo el experimento)",
        track.title,
        track.identifier()
    );

    let resolved_at = Instant::now();
    let url = provider
        .inner()
        .resolve_audio_url(&track)
        .await?
        .ok_or_else(|| anyhow::anyhow!("sin stream resoluble"))?;
    let headers = context_headers();

    println!("\n== StreamResolution ==");
    println!("host inicial: {}", host_of(&url));
    print!("parámetros (valores sensibles omitidos):");
    for (k, v) in describe_query(&url) {
        match v {
            Some(val) => print!(" {k}={val}"),
            None => print!(" {k}=…"),
        }
    }
    println!();
    // Vigencia declarada por la propia URL (param `expire`, epoch UTC).
    let expire = url
        .split("expire=")
        .nth(1)
        .and_then(|v| v.split('&').next())
        .and_then(|v| v.parse::<i64>().ok());
    match expire {
        Some(ts) => {
            let exp = chrono::DateTime::from_timestamp(ts, 0)
                .map(|d| d.to_rfc3339())
                .unwrap_or_else(|| "?".into());
            let mins = (ts - chrono::Utc::now().timestamp()) / 60;
            println!("expiración declarada por la URL: {exp} (en ~{mins} min)");
        }
        None => println!("la URL no declara `expire`"),
    }

    let probe_http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .build()?;
    let probe = Probe {
        http: probe_http.clone(),
        headers: &headers,
    };

    trace_redirects(&probe_http, &url, &headers).await;

    println!("\n== TEST A: GET normal (sin Range) ==");
    let a = probe.request("A", &url, None, 8 * MIB).await;
    print_log(&a);

    println!("\n== TEST B: Range 0..1MiB ==");
    let b = probe.range("B", &url, 0, MIB).await;
    print_log(&b);

    println!("\n== TEST C: Range 1MiB..2MiB ==");
    let c = probe.range("C", &url, MIB, MIB).await;
    print_log(&c);

    println!("\n== TEST D: Range 2MiB..3MiB ==");
    let d = probe.range("D", &url, 2 * MIB, MIB).await;
    print_log(&d);

    println!("\n== TEST E: ventanas encadenadas sin pausa (3..4, 4..5, 5..6 MiB) ==");
    let e1 = probe.range("E1", &url, 3 * MIB, MIB).await;
    print_log(&e1);
    let e2 = probe.range("E2", &url, 4 * MIB, MIB).await;
    print_log(&e2);
    let e3 = probe.range("E3", &url, 5 * MIB, MIB).await;
    print_log(&e3);

    println!("\n== TEST F: re-petición del primer bloque (idempotencia/cuota) ==");
    let f = probe.range("F", &url, 0, MIB).await;
    print_log(&f);

    println!(
        "\nresolución vivió {:.1}s durante el experimento",
        resolved_at.elapsed().as_secs_f64()
    );
    Ok(())
}
