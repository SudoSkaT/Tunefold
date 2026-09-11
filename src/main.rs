//! CLI de desarrollo: valida el flujo end-to-end (providers + SQLite + TUI).

use std::collections::HashMap;

use tunefold::app::aggregator::MetadataAggregator;
use tunefold::infrastructure::config::Config;
use tunefold::infrastructure::db::Db;
use tunefold::infrastructure::dirs;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if matches!(
        args.get(1).map(String::as_str),
        Some("--version") | Some("-V")
    ) {
        println!("tunefold {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let config = Config::load();
    // Migración no destructiva: si existía `data/music.db`, se copia a
    // `~/.local/share/tunefold/music.db` y se mantiene el original.
    dirs::migrate_legacy_data();
    let db = Db::connect(dirs::db_path().to_str().unwrap()).await?;

    // El CLI respeta el flag del provider: con YouTube apagado el agregador
    // queda vacío y los comandos responden "sin resultados" sin tocar red.
    let aggregator = MetadataAggregator::new(if config.flags.youtube_provider {
        tunefold::api::build_providers()
    } else {
        Default::default()
    });

    match args.get(1).map(String::as_str) {
        Some("--update") => {
            tunefold::app::updater::run(false).await?;
        }
        Some("--update-check") => {
            tunefold::app::updater::run(true).await?;
        }
        Some("--search") => {
            let query = args
                .get(2)
                .cloned()
                .unwrap_or_else(|| "queen bohemian rhapsody".to_string());
            search_and_save(&db, &aggregator, &query).await?;
        }
        Some("--sources") => {
            let sources = config
                .available_sources()
                .iter()
                .map(|s| s.label())
                .collect::<Vec<_>>()
                .join(", ");
            println!("Fuentes activas: {sources}");
        }
        Some("--history") => {
            let history = tunefold::app::history::History::new(db);
            let entries = history.recent(20).await?;
            for e in entries {
                let artist = e.artist_name.unwrap_or_else(|| "?".to_string());
                println!(
                    "[{}] {artist} - {} (track {})",
                    e.played_at, e.title, e.track_id
                );
            }
        }
        Some("--tui") | None => {
            tunefold::ui::run().await?;
        }
        _ => print_help(),
    }

    Ok(())
}

async fn search_and_save(
    db: &Db,
    aggregator: &MetadataAggregator,
    query: &str,
) -> anyhow::Result<()> {
    println!("Buscando «{query}» en todas las fuentes...\n");
    let outcome = aggregator.search_tracks(query, 5).await;

    if outcome.items.is_empty() {
        println!("Sin resultados.");
    }

    for track in &outcome.items {
        let artist = track.primary_artist_name().unwrap_or("Desconocido");
        let duration = track
            .duration
            .map(|d| format!("{}s", d.as_secs()))
            .unwrap_or_else(|| "-".to_string());
        let external = track.external_id.as_deref().unwrap_or("-");

        let mut ids = HashMap::new();
        if external != "-" {
            ids.insert(track.source, external.to_string());
        }
        let internal_id = db.upsert_track(track, &ids).await?;

        println!(
            "[{source}] {artist} - {title} ({duration}) ext={external} -> track_id={internal_id}",
            source = track.source,
            title = track.title,
        );
    }

    for e in &outcome.errors {
        eprintln!("aviso: {e}");
    }

    Ok(())
}

fn print_help() {
    println!("Tunefold — cliente TUI de YouTube / YouTube Music (vía rustypipe).");
    println!();
    println!("  tunefold [--tui]              Lanza la interfaz de terminal");
    println!(
        "  tunefold --search <consulta>  Busca en YouTube Music y guarda en {}",
        dirs::db_path().display()
    );
    println!("  tunefold --sources            Lista las fuentes activas");
    println!("  tunefold --history            Muestra las últimas reproducciones");
    println!("  tunefold --update             Actualiza a la última release oficial");
    println!("  tunefold --update-check       Solo informa si hay release más nueva");
    println!("  tunefold --version            Muestra la versión");
    println!();
    println!(
        "Configuración en {} (o .env legacy).",
        dirs::env_path().display()
    );
    println!("Variables: PLAYBACK_POLICY (auto | rodio), YOUTUBE_PROVIDER_ENABLED, PROXY_ENABLED.");
}
