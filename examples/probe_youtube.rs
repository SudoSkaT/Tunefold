//! Probe de desarrollo: pipeline real YouTube vía rustypipe (stream, letras,
//! recomendados). Ejecutar con: `cargo run --example probe_youtube`

use tunefold::catalog::CatalogProvider;
use tunefold::domain::source::Source;
use tunefold::providers::youtube::{context_headers, YouTubeAdapter};

/// Descarga el inicio de un stream y devuelve (status, content_length).
async fn download_status(url: String, ua: bool) -> anyhow::Result<(u16, u64)> {
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let mut req = client.get(&url);
    if ua {
        req = req.header("User-Agent", "Tunefold/1.5.3");
    }
    let resp = req.send().await?;
    Ok((resp.status().as_u16(), resp.content_length().unwrap_or(0)))
}

/// Prueba un GET con cabeceras de cliente Android/iOS real (UA + Referer +
/// Origin + Range), para descartar que el 403 sea solo por falta de contexto.
async fn probe_headers(url: String) -> anyhow::Result<(u16, u64)> {
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(20))
        .build()?;
    let resp = client
        .get(&url)
        .header(
            "User-Agent",
            "com.google.android.youtube/20.36.36 (Linux; U; Android 14; en_US; Pixel 8 Pro) gzip",
        )
        .header("Referer", "https://www.youtube.com/")
        .header("Origin", "https://www.youtube.com")
        .header("Range", "bytes=0-65535")
        .send()
        .await?;
    Ok((resp.status().as_u16(), resp.content_length().unwrap_or(0)))
}

/// Igual que [`probe_headers`] pero sin `Range`, para ver si el CDN exige el
/// rango parcial o sirve también el cuerpo completo.
async fn probe_headers_no_range(url: String) -> anyhow::Result<(u16, u64)> {
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(20))
        .build()?;
    let resp = client
        .get(&url)
        .header(
            "User-Agent",
            "com.google.android.youtube/20.36.36 (Linux; U; Android 14; en_US; Pixel 8 Pro) gzip",
        )
        .header("Referer", "https://www.youtube.com/")
        .header("Origin", "https://www.youtube.com")
        .send()
        .await?;
    Ok((resp.status().as_u16(), resp.content_length().unwrap_or(0)))
}

/// Range abierto `bytes=0-`: sirve el cuerpo completo con 206. Es lo que haría
/// la reproducción si solo añadimos la cabecera Range a la descarga actual.
async fn probe_headers_range_open(url: String) -> anyhow::Result<(u16, u64)> {
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(20))
        .build()?;
    let resp = client
        .get(&url)
        .header(
            "User-Agent",
            "com.google.android.youtube/20.36.36 (Linux; U; Android 14; en_US; Pixel 8 Pro) gzip",
        )
        .header("Referer", "https://www.youtube.com/")
        .header("Origin", "https://www.youtube.com")
        .header("Range", "bytes=0-")
        .send()
        .await?;
    Ok((resp.status().as_u16(), resp.content_length().unwrap_or(0)))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rustypipe=debug,tunefold=debug".into()),
        )
        .with_target(false)
        .init();
    let provider = YouTubeAdapter::new();

    let query = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "queen bohemian rhapsody".to_string());
    let tracks = provider.search_tracks(&query, 3).await?;
    if tracks.is_empty() {
        anyhow::bail!("sin resultados para «{query}»");
    }

    for t in &tracks {
        println!(
            "[{}] {} - {} (id={}, dur={:?})",
            t.source,
            t.primary_artist_name().unwrap_or("?"),
            t.title,
            t.external_id.as_deref().unwrap_or("-"),
            t.duration,
        );
        assert_eq!(t.source, Source::YouTube);
    }

    let track = &tracks[0];
    let video_id = track.external_id.clone().unwrap_or_default();
    println!("\n== Letras y recomendados de «{}» ==", track.title);
    let synced = provider.synced_lyrics(track).await?;
    let related = provider.related(&video_id).await?;
    println!("letras sincronizadas (LRC): {}", synced.is_some());
    if let Some(s) = &synced {
        println!(
            "LRC (primeras 120 chars): {}...",
            s.chars().take(120).collect::<String>()
        );
    }
    println!("recomendados: {} canciones", related.len());

    println!(
        "\n== Stream de «{}» vía StreamProvider::resolve ==",
        track.title
    );
    let resolved = provider.inner().resolve_audio_url(track).await;
    match resolved {
        Ok(Some(url)) => {
            let stream = tunefold::domain::stream::RemoteStream {
                url,
                headers: context_headers(),
            };
            // Descarga del primer bloque replicando al motor de reproducción:
            // cabeceras del StreamRef + Range cerrado.
            let client = reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .timeout(std::time::Duration::from_secs(30))
                .build()?;
            let mut req = client.get(&stream.url);
            for (k, v) in &stream.headers {
                req = req.header(k, v);
            }
            let resp = req.header("Range", "bytes=0-65535").send().await?;
            let total = resp
                .headers()
                .get("content-range")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.rsplit('/').next())
                .map(|v| v.to_string())
                .unwrap_or_else(|| "?".into());
            let status = resp.status();
            let bytes = resp.bytes().await?.len();
            println!(
                "stream resuelto; descarga primer bloque -> status={} bytes={} total={}",
                status, bytes, total
            );
        }
        Ok(None) => println!("sin stream resoluble"),
        Err(fail) => println!("fallo de resolución: {fail}"),
    }

    // Diagnóstico fino: pide el player crudo con la API pública para ver qué
    // devuelve YouTube (streamingData presente, playabilityStatus, errores).
    println!("\n== Diagnóstico: player crudo ==");
    {
        use rustypipe::client::{ClientType, RustyPipe};
        let rp = RustyPipe::builder()
            .storage_dir(tunefold::infrastructure::dirs::cache_dir().join("rustypipe"))
            .build()
            .expect("cliente válido");
        let vd = rp.query().get_visitor_data(true).await?;
        let q = rp.query().visitor_data(vd);
        for ct in [ClientType::Android, ClientType::Ios] {
            match q.player_from_clients(video_id.clone(), &[ct]).await {
                Ok(p) => {
                    println!(
                        "[{ct:?}] ok: audio={} video={} video_only={} hls={:?} dash={:?} valid_until={}",
                        p.audio_streams.len(),
                        p.video_streams.len(),
                        p.video_only_streams.len(),
                        p.hls_manifest_url,
                        p.dash_manifest_url,
                        p.valid_until,
                    );
                    for a in p.audio_streams.iter().take(3) {
                        let u = a.url.split('?').next().unwrap_or("?");
                        println!(
                            "  audio: itag={} bitrate={} size={:?} url={}",
                            a.itag,
                            a.bitrate,
                            a.size,
                            &u[..u.len().min(80)]
                        );
                    }
                    if let Some(a) = p.audio_streams.first() {
                        let st = download_status(a.url.clone(), false).await?;
                        println!(
                            "  primer audio con GET plano -> status={} bytes={}",
                            st.0, st.1
                        );
                        let st2 = download_status(a.url.clone(), true).await?;
                        println!(
                            "  primer audio con GET + UA -> status={} bytes={}",
                            st2.0, st2.1
                        );
                        let st3 = probe_headers(a.url.clone()).await?;
                        println!(
                            "  primer audio con cabeceras completas -> status={} bytes={}",
                            st3.0, st3.1
                        );
                        let st4 = probe_headers_no_range(a.url.clone()).await?;
                        println!(
                            "  primer audio con cabeceras SIN Range -> status={} bytes={}",
                            st4.0, st4.1
                        );
                        let st5 = probe_headers_range_open(a.url.clone()).await?;
                        println!(
                            "  primer audio con Range abierto 0- -> status={} bytes={}",
                            st5.0, st5.1
                        );
                    }
                }
                Err(e) => println!("[{ct:?}] error: {e}"),
            }
        }
    }

    Ok(())
}
