//! ¿El techo de ~1 MiB depende del cliente InnerTube usado para resolver?
//!
//! Resuelve el MISMO video por cada ClientType disponible y sondea tres
//! ventanas de cada URL resuelta: [0,64K), [1MiB,+64K), [2.5MiB,+64K).
//! Registra además el parámetro `c=` del googlevideo (contexto real).
//!
//! Uso: cargo run --release --example probe_clients

use std::time::Duration;

use rustypipe::client::{ClientType, RustyPipe};

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;

/// Ventanas de sondeo (start, len) clampadas a clen.
fn windows(clen: u64) -> [(u64, u64); 4] {
    [
        (0, 64 * KIB),
        (MIB, 64 * KIB),
        (2 * MIB + 512 * KIB, 64 * KIB),
        (clen.saturating_sub(256 * KIB), 64 * KIB), // cola del archivo
    ]
}

async fn cap_of(
    http: &reqwest::Client,
    url: &str,
    headers: &[(String, String)],
    clen: u64,
) -> Vec<(u64, u16)> {
    let mut out = Vec::new();
    for (start, len) in windows(clen) {
        if start >= clen {
            out.push((start, 0));
            continue;
        }
        let mut req = http.get(url);
        for (k, v) in headers {
            req = req.header(k, v);
        }
        let resp = match req
            .header("Range", format!("bytes={}-{}", start, start + len - 1))
            .send()
            .await
        {
            Ok(r) => r,
            Err(_) => {
                out.push((start, 0));
                continue;
            }
        };
        let st = resp.status().as_u16();
        let _ = resp.bytes().await;
        tokio::time::sleep(Duration::from_millis(150)).await;
        out.push((start, st));
    }
    out
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

    let rp = RustyPipe::builder()
        .storage_dir(tunefold::infrastructure::dirs::cache_dir().join("rustypipe"))
        .build()?;
    let video_id = "kM0Fpbz0W8U";

    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()?;

    for ct in [ClientType::Visionos, ClientType::Android, ClientType::Ios] {
        print!("\n== {ct:?} ==\n");
        // visitor_data fresco por intento (el pool cacheado suele venir
        // bloqueado y sesgar el resultado del cliente).
        let vd = rp.query().get_visitor_data(true).await?;
        let q = rp.query().visitor_data(vd);
        match q.player_from_clients(video_id, &[ct]).await {
            Ok(p) => {
                println!(
                    "audio_streams={} valid_until={}",
                    p.audio_streams.len(),
                    p.valid_until
                );
                // Candidato mp4 con mayor bitrate (mismo criterio del provider).
                let mut best: Option<&rustypipe::model::AudioStream> = None;
                for s in &p.audio_streams {
                    let mp4 =
                        s.url.contains("mime=audio%2Fmp4") || s.url.contains("mime=audio/mp4");
                    let better = match best {
                        None => true,
                        Some(b) => {
                            (mp4 as u32 * 1_000_000 + s.bitrate)
                                > (b.url.contains("mp4") as u32 * 1_000_000 + b.bitrate)
                        }
                    };
                    if better {
                        best = Some(s);
                    }
                }
                let Some(s) = best else {
                    println!("sin streams de audio");
                    continue;
                };
                let url = &s.url;
                let c_ctx = url
                    .split("c=")
                    .nth(1)
                    .and_then(|v| v.split('&').next())
                    .unwrap_or("?")
                    .to_string();
                println!(
                    "itag={} bitrate={} c={c_ctx} clen={}",
                    s.itag,
                    s.bitrate,
                    url.split("clen=")
                        .nth(1)
                        .and_then(|v| v.split('&').next())
                        .unwrap_or("?")
                );
                let clen: u64 = url
                    .split("clen=")
                    .nth(1)
                    .and_then(|v| v.split('&').next())
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                // Contexto del propio cliente (UA coherente con la resolución).
                let ua: Vec<(String, String)> = match ct {
                    ClientType::Android => vec![(
                        "User-Agent".into(),
                        "com.google.android.youtube/20.36.36 (Linux; U; Android 14; en_US; Pixel 8 Pro) gzip".into(),
                    )],
                    ClientType::Ios => vec![(
                        "User-Agent".into(),
                        "com.google.ios.youtube/20.36.4 (iPhone16,2; U; CPU iOS 20_36_4 like Mac OS X;)".into(),
                    )],
                    _ => vec![(
                        "User-Agent".into(),
                        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36".into(),
                    )],
                };
                let results = cap_of(&http, url, &ua, clen).await;
                for (start, st) in results {
                    println!("  ventana @{:>9} -> {st}", start / KIB);
                }
            }
            Err(e) => println!("player error: {e}"),
        }
    }

    Ok(())
}
