//! ¿La frontera de servicio de googlevideo es un tope duro o avanza?
//!
//! Sobre UNA resolución real por fase:
//!   F1: umbral de END con START=0 (binaria aproximada).
//!   F2: avance incremental de la frontera en pasos de 16KiB desde el
//!       máximo extent ya servido: si el frente avanza arbitrariamente,
//!       el streaming por rangos pequeños es viable; si se estanca,
//!       la URL tiene un techo duro.
//!   F3: tras estancarse, ¿una ventana pequeña justo al borde sigue
//!       respondiendo (la URL no se mata)?
//!
//! Uso: cargo run --release --example probe_frontier -- [query]

use std::time::Duration;

use tunefold::catalog::CatalogProvider;
use tunefold::domain::track::Track;
use tunefold::providers::youtube::{context_headers, YouTubeAdapter};

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;

struct P {
    http: reqwest::Client,
    headers: Vec<(String, String)>,
    delay: Duration,
}

impl P {
    async fn status(&self, url: &str, start: u64, end: u64) -> (u16, u64) {
        let mut req = self.http.get(url);
        for (k, v) in &self.headers {
            req = req.header(k, v);
        }
        let resp = req
            .header("Range", format!("bytes={start}-{end}"))
            .send()
            .await
            .expect("petición");
        let st = resp.status().as_u16();
        let n = resp.bytes().await.map(|b| b.len() as u64).unwrap_or(0);
        tokio::time::sleep(self.delay).await;
        (st, n)
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
    let query = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "queen bohemian rhapsody".into());
    let results = provider.search_tracks(&query, 1).await?;
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
    println!(
        "video={} clen={clen} ({:.2} MiB)",
        track.identifier(),
        clen as f64 / MIB as f64
    );

    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .build()?;
    let p = P {
        http,
        headers: context_headers(),
        delay: Duration::from_millis(120),
    };

    println!("\n== F1: umbral de END con START=0 (saltos crecientes) ==");
    let mut served_end: u64 = 0;
    for end in [
        256 * KIB,
        512 * KIB,
        768 * KIB,
        MIB - KIB,
        MIB,
        MIB + 64 * KIB,
        MIB + 128 * KIB,
        MIB + 256 * KIB,
        MIB + 512 * KIB,
        2 * MIB,
    ] {
        let (st, n) = p.status(&url, 0, end.min(clen - 1)).await;
        println!("bytes=0-{}/{clen} -> {st} recibidos={n}", end.min(clen - 1));
        if st == 206 && n > 0 {
            served_end = served_end.max(end.min(clen - 1));
        }
    }

    println!("\n== F2: avance incremental de la frontera (pasos de 16KiB) ==");
    let step = 16 * KIB;
    let mut frontier = served_end + 1;
    let mut advances = 0u32;
    let mut consecutive_fails = 0u32;
    let started = std::time::Instant::now();
    while frontier < clen && consecutive_fails < 12 && advances < 400 {
        let end = (frontier + step - 1).min(clen - 1);
        let (st, _) = p.status(&url, frontier, end).await;
        if st == 206 {
            frontier = end + 1;
            advances += 1;
            consecutive_fails = 0;
        } else {
            consecutive_fails += 1;
            if consecutive_fails <= 3 || consecutive_fails.is_multiple_of(4) {
                println!(
                    "fallo en frontera={frontier} ({:.3} MiB) status={st} (fallos seguidos={consecutive_fails})",
                    frontier as f64 / MIB as f64
                );
            }
        }
        if started.elapsed() > Duration::from_secs(240) {
            println!("límite de tiempo");
            break;
        }
    }
    println!(
        "frontier final={} ({:.3} MiB de {:.3} MiB = {:.1}%) avanzado con {} ventanas exitosas",
        frontier,
        frontier as f64 / MIB as f64,
        clen as f64 / MIB as f64,
        100.0 * frontier as f64 / clen as f64,
        advances
    );

    println!("\n== F3: la URL sigue viva tras los fallos ==");
    let (st, n) = p.status(&url, 0, 64 * KIB - 1).await;
    println!("re-petición inicial -> {st} recibidos={n}");
    let (st, n) = p
        .status(&url, frontier.saturating_sub(step), frontier - 1)
        .await;
    println!("última ventana servida -> {st} recibidos={n}");

    Ok(())
}
