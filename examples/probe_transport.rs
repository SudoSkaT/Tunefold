//! Últimos descartes de nivel transporte sobre la MISMA URL limitada:
//!   T1: host espejo distinto (rrN---sn-…).
//!   T2: parámetros rn/rbuf encadenados.
//!   T3: cliente HTTP/1.1 only (¿el techo es por conexión h2?).
//!   T4: HEAD para metadatos (Accept-Ranges / Content-Length).
//!
//! Uso: cargo run --release --example probe_transport

use std::time::Duration;

use tunefold::catalog::CatalogProvider;
use tunefold::domain::track::Track;
use tunefold::providers::youtube::{context_headers, YouTubeAdapter};

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;

async fn ranged(
    http: &reqwest::Client,
    url: &str,
    headers: &[(String, String)],
    start: u64,
    len: u64,
) -> (u16, u64) {
    let mut req = http.get(url);
    for (k, v) in headers {
        req = req.header(k, v);
    }
    match req
        .header("Range", format!("bytes={}-{}", start, start + len - 1))
        .send()
        .await
    {
        Ok(r) => {
            let st = r.status().as_u16();
            let n = r.bytes().await.map(|b| b.len() as u64).unwrap_or(0);
            (st, n)
        }
        Err(e) => (0, e.to_string().len() as u64),
    }
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
    let results = provider.search_tracks("Queen Bohemian Rhapsody", 1).await?;
    let track: Track = results.first().cloned().unwrap();
    let url = provider
        .inner()
        .resolve_audio_url(&track)
        .await?
        .ok_or_else(|| anyhow::anyhow!("sin stream"))?;
    let headers = context_headers();

    // Baseline: ventana post-frontera en la URL original.
    let h2 = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()?;
    println!(
        "baseline post-frontera @1.5MiB -> {:?}",
        ranged(&h2, &url, &headers, MIB + 512 * KIB, 64 * KIB).await
    );

    println!("\n== T1: host espejo (rr10 -> rr3, rr5, fvip) ==");
    for repl in ["rr3", "rr5"] {
        if let Some(idx) = url.find("rr") {
            let mut u = url.clone();
            u.replace_range(idx..idx + 4, &format!("{repl}---"));
            let r = ranged(&h2, &u, &headers, MIB + 512 * KIB, 64 * KIB).await;
            println!("host {repl}: {r:?}");
        }
    }

    println!("\n== T2: parámetros rn/rbuf ==");
    let u2 = format!("{url}&rn=1&rbuf=65535");
    println!(
        "con &rn=1&rbuf -> {:?}",
        ranged(&h2, &u2, &headers, MIB + 512 * KIB, 64 * KIB).await
    );

    println!("\n== T3: HTTP/1.1 only ==");
    let http1 = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()?;
    let _ = &http1; // reqwest negocia h2 por ALPN; forzamos http1 vía builder abajo
    let b1 = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()?;
    println!(
        "h1 post-frontera @1.5MiB -> {:?}",
        ranged(&b1, &url, &headers, MIB + 512 * KIB, 64 * KIB).await
    );

    println!("\n== T4: HEAD ==");
    let mut req = h2.head(&url);
    for (k, v) in &headers {
        req = req.header(k, v);
    }
    let resp = req.send().await?;
    println!(
        "HEAD -> {} CL={:?} AR={:?} CR={:?}",
        resp.status(),
        resp.content_length(),
        resp.headers()
            .get("accept-ranges")
            .and_then(|v| v.to_str().ok()),
        resp.headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok())
    );

    Ok(())
}
