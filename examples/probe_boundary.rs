//! Frontera exacta del bloqueo de rangos en googlevideo.
//!
//! Sobre UNA resolución real determina:
//!   - el offset mínimo desde el que los rangos responden 403;
//!   - si una ventana que CRUZA la frontera se trunca o se rechaza;
//!   - si el archivo completo cabe en una sola petición `bytes=0-clen-1`;
//!   - si el contexto HTTP (UA/Referer/Origin) altera el límite;
//!   - si una NUEVA resolución (URL nueva) restaura la ventana servible.
//!
//! Uso: cargo run --release --example probe_boundary

use std::time::Duration;

use tunefold::catalog::CatalogProvider;
use tunefold::domain::track::Track;
use tunefold::providers::youtube::{context_headers, YouTubeAdapter};

const KIB: u64 = 1024;

struct P {
    http: reqwest::Client,
    headers: Vec<(String, String)>,
}

impl P {
    /// Status + bytes recibidos para un rango cerrado.
    async fn probe(
        &self,
        tag: &str,
        url: &str,
        start: u64,
        len: u64,
    ) -> (u16, u64, Option<String>) {
        let mut req = self.http.get(url);
        for (k, v) in &self.headers {
            req = req.header(k, v);
        }
        let resp = req
            .header("Range", format!("bytes={}-{}", start, start + len - 1))
            .send()
            .await
            .expect("petición");
        let status = resp.status().as_u16();
        let cr = resp
            .headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let n = match resp.bytes().await {
            Ok(b) => b.len() as u64,
            Err(e) => {
                println!("{tag}: ERROR de lectura {e}");
                return (status, 0, cr);
            }
        };
        println!(
            "{tag}: bytes={start}..{end} -> {status} recibidos={n} cr={cr:?}",
            end = start + len - 1
        );
        (status, n, cr)
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
    let track: Track = results
        .first()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("sin resultados"))?;

    let url = provider
        .inner()
        .resolve_audio_url(&track)
        .await?
        .ok_or_else(|| anyhow::anyhow!("sin stream"))?;
    let clen: u64 = url
        .split("clen=")
        .nth(1)
        .and_then(|v| v.split('&').next())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    println!("video={} clen={clen}", track.identifier());

    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .build()?;
    let p = P {
        http,
        headers: context_headers(),
    };

    println!("\n== 1) barrido del offset inicial (ventanas de 64KiB dentro del archivo) ==");
    for start in [
        0u64,
        512 * KIB,
        1000 * KIB,
        1023 * KIB,
        1024 * KIB - 1,
        1024 * KIB,
        1024 * KIB + 1,
        1536 * KIB,
        2048 * KIB,
    ] {
        let len = 64 * KIB;
        // Clamp al final del archivo para no mezclar con 416.
        let len = len.min(clen.saturating_sub(start));
        if len == 0 {
            continue;
        }
        let _ = p.probe("barrido", &url, start, len).await;
    }

    println!("\n== 2) ventana que CRUZA la frontera de 1MiB ==");
    let _ = p.probe("cruce", &url, 1000 * KIB, 128 * KIB).await;

    println!("\n== 3) archivo completo en una petición bytes=0-(clen-1) ==");
    let _ = p.probe("completo", &url, 0, clen).await;

    println!("\n== 4) mismo rango post-frontera repetido x3 (¿transitorio?) ==");
    for i in 1..=3 {
        let _ = p
            .probe(&format!("rep{i}"), &url, 1024 * KIB, 64 * KIB)
            .await;
    }

    println!("\n== 5) sin cabeceras de contexto (reqwest pelado) ==");
    let bare = P {
        http: p.http.clone(),
        headers: Vec::new(),
    };
    let _ = bare.probe("sinctx-pre", &url, 0, 64 * KIB).await;
    let _ = bare.probe("sinctx-post", &url, 1024 * KIB, 64 * KIB).await;

    println!("\n== 6) UA de navegador (Chrome desktop) ==");
    let chrome = P {
        http: p.http.clone(),
        headers: vec![(
            "User-Agent".to_string(),
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36".to_string(),
        )],
    };
    let _ = chrome.probe("chrome-pre", &url, 0, 64 * KIB).await;
    let _ = chrome
        .probe("chrome-post", &url, 1024 * KIB, 64 * KIB)
        .await;

    println!("\n== 7) NUEVA resolución (otra instancia = URL nueva) ==");
    let provider2 = YouTubeAdapter::new();
    let url2 = provider2
        .inner()
        .resolve_audio_url(&track)
        .await?
        .ok_or_else(|| anyhow::anyhow!("sin stream #2"))?;
    println!("url nueva distinta: {}", url != url2);
    let p2 = P {
        http: p.http.clone(),
        headers: context_headers(),
    };
    let _ = p2.probe("nueva-url-pre", &url2, 0, 64 * KIB).await;
    let _ = p2
        .probe("nueva-url-post", &url2, 1024 * KIB, 64 * KIB)
        .await;
    let _ = p2.probe("nueva-url-mas", &url2, 2048 * KIB, 64 * KIB).await;

    Ok(())
}
