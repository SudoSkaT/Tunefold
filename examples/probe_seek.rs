//! E2E de reproducción completa: una canción larga (> 65 s, muy por encima
//! del antiguo corte de ~1 MiB) debe llegar a `Finished` de forma natural,
//! sin `Cut` ni `Error`.
//!
//! Uso: cargo run --release --example probe_seek

use std::time::{Duration, Instant};
use tunefold::app::audio::{EventBus, PlaybackEvent};
use tunefold::app::playback::PlaybackRouter;
use tunefold::catalog::CatalogProvider;
use tunefold::domain::stream::MediaSource;
use tunefold::domain::track::Track;

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
    let results = provider.search_tracks("Queen Bohemian Rhapsody", 5).await?;
    let track = results.first().cloned().unwrap_or_else(|| {
        Track::new(
            "I Want To Break Free".to_string(),
            vec![tunefold::domain::artist::Artist::new(
                "Queen".to_string(),
                None,
                None,
                None,
            )],
            tunefold::domain::source::Source::YouTube,
        )
    });
    println!("pista: {} ({})", track.title, track.identifier());

    let (bus, joined) = EventBus::channel();
    let (engine_config, _features) = tunefold::infrastructure::playback::build_engines(
        &tunefold::infrastructure::config::Config::default(),
        bus,
        reqwest::Client::new(),
    );
    let router = PlaybackRouter::new(engine_config, joined);

    let url = provider
        .inner()
        .resolve_audio_url(&track)
        .await?
        .expect("stream resoluble");
    let source = MediaSource::Remote(tunefold::domain::stream::RemoteStream {
        url,
        headers: context_headers(),
    });
    router.play(&track, Some(source)).await?;

    // Presupuesto: la duración declarada + margen de arranque/red.
    let budget = track
        .duration
        .map_or(Duration::from_secs(240), |d| d + Duration::from_secs(60));
    let started = Instant::now();
    let mut events = router.subscribe();
    let mut max_pos = 0.0f32;
    loop {
        tokio::select! {
            ev = events.recv() => match ev {
                Ok(PlaybackEvent::Cut(msg)) => {
                    anyhow::bail!("CORTE prematuro (stream restringido): {msg}");
                }
                Ok(PlaybackEvent::Error(msg)) => anyhow::bail!("ERROR de reproducción: {msg}"),
                Ok(PlaybackEvent::Finished) => break,
                _ => {}
            },
            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                if started.elapsed() > budget {
                    anyhow::bail!("sin Finished en {:?}", budget);
                }
                if started.elapsed().as_secs().is_multiple_of(5) {
                    let status = router.status().await;
                    max_pos = max_pos.max(status.position.as_secs_f32());
                    print!(
                        "\r{t:>4}s pos={pos:>6.1}s/{dur:?}",
                        t = started.elapsed().as_secs(),
                        pos = status.position.as_secs_f32(),
                        dur = status.duration,
                    );
                    use std::io::Write as _;
                    let _ = std::io::stdout().flush();
                }
            }
        }
    }
    println!();
    println!(
        "OK: reproducción COMPLETA ({:.1}s reproducidos en {:.1}s reales)",
        max_pos,
        started.elapsed().as_secs_f64()
    );
    router.stop().await?;
    Ok(())
}

use tunefold::providers::youtube::{context_headers, YouTubeAdapter};
