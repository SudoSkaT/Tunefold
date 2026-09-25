//! Saludo diario "seguir el hilo" (Nivel A/B, sin ML).
//!
//! Puro y testeable: a partir del historial reciente + stats + señales
//! agregadas decide si mostrar el saludo (una vez por fecha local) y qué
//! contenido sugerir (continuar + top artistas). El ranking pesado (`rank()`)
//! lo hace el llamador con `all_tracks`; aquí solo la heurística de portada.

use std::collections::HashMap;

use crate::infrastructure::storage::{HistoryEntry, TrackListeningStats};

/// Resumen listo para pintar en `View::Home`/modal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MorningBrief {
    /// `buenos días|buenas tardes|buenas noches`.
    pub franja: &'static str,
    /// Nombre para personalizar (`None` = genérico).
    pub display_name: Option<String>,
    /// Track para "continuar donde quedaste" (último no-skipped).
    pub continue_track_id: Option<i64>,
    /// Top artistas ponderados por la heurística (máx 3).
    pub top_artists: Vec<String>,
    /// Motivo explicable (`porque escuchaste X ×N`).
    pub reason: Option<String>,
}

/// Franja por hora local (0-23). Pura.
pub fn franja_for_hour(hour: u32) -> &'static str {
    match hour {
        6..=12 => "buenos días",
        13..=20 => "buenas tardes",
        _ => "buenas noches",
    }
}

/// ¿Toca saludar hoy? `last` = `YYYY-MM-DD` guardado, `today` = hoy local.
pub fn should_greet(last: Option<&str>, today: &str) -> bool {
    last != Some(today)
}

/// Fecha local `YYYY-MM-DD` de hoy (zona del usuario, no UTC).
pub fn today_local() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Hora local 0-23.
pub fn hour_local() -> u32 {
    use chrono::Timelike;
    chrono::Local::now().hour()
}

/// Construye el brief desde datos ya cargados (sin IO).
///
/// - `continue_track_id`: primer `recent` cuyo track no esté en `skipped_ids`.
/// - `top_artists`: frecuencia en `recent` (x1) + `play_count` en stats (x1),
///   ordenado desc, top 3 con nombre no vacío.
/// - `reason`: `porque escuchaste {artista} ×{plays}` del top 1 si hay plays.
pub fn build_brief(
    recent: &[HistoryEntry],
    stats: &[TrackListeningStats],
    skipped_ids: &std::collections::HashSet<i64>,
    display_name: Option<String>,
    hour: u32,
) -> MorningBrief {
    let continue_track_id = recent
        .iter()
        .find(|e| !skipped_ids.contains(&e.track_id))
        .map(|e| e.track_id);

    let mut counts: HashMap<String, i64> = HashMap::new();
    for e in recent {
        if let Some(a) = e.artist_name.clone() {
            if !a.is_empty() {
                *counts.entry(a).or_default() += 1;
            }
        }
    }
    for s in stats {
        if let Some(a) = s.artist_name.clone() {
            if !a.is_empty() {
                *counts.entry(a).or_default() += s.play_count;
            }
        }
    }
    let mut ranked: Vec<(String, i64)> = counts.into_iter().collect();
    ranked.sort_by_key(|b| std::cmp::Reverse(b.1));
    let top_artists: Vec<String> = ranked.iter().take(3).map(|(k, _)| k.clone()).collect();
    let reason = ranked
        .first()
        .map(|(k, n)| format!("porque escuchaste {k} ×{n}"));

    MorningBrief {
        franja: franja_for_hour(hour),
        display_name,
        continue_track_id,
        top_artists,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn franja_covers_day() {
        assert_eq!(franja_for_hour(7), "buenos días");
        assert_eq!(franja_for_hour(15), "buenas tardes");
        assert_eq!(franja_for_hour(23), "buenas noches");
        assert_eq!(franja_for_hour(3), "buenas noches");
    }

    #[test]
    fn greet_once_per_day() {
        assert!(should_greet(None, "2026-09-25"));
        assert!(!should_greet(Some("2026-09-25"), "2026-09-25"));
        assert!(should_greet(Some("2026-09-24"), "2026-09-25"));
    }

    fn entry(id: i64, artist: &str) -> HistoryEntry {
        HistoryEntry {
            track_id: id,
            played_at: "2026-09-25".into(),
            source: crate::domain::source::Source::YouTube,
            duration: None,
            title: format!("T{id}"),
            artist_name: Some(artist.into()),
            play_count: 1,
        }
    }

    fn stat(id: i64, artist: &str, plays: i64) -> TrackListeningStats {
        TrackListeningStats {
            track_id: id,
            key: format!("{id}"),
            artist_name: Some(artist.into()),
            play_count: plays,
            last_played: "2026-09-25".into(),
            recently_played: true,
        }
    }

    #[test]
    fn brief_skips_skipped_and_ranks_artists() {
        let recent = vec![entry(1, "A"), entry(2, "B"), entry(3, "A")];
        let stats = vec![stat(9, "B", 5)];
        let skipped: std::collections::HashSet<i64> = [1].into_iter().collect();
        let b = build_brief(&recent, &stats, &skipped, Some("Ska".into()), 9);
        assert_eq!(b.continue_track_id, Some(2));
        assert_eq!(b.top_artists[0], "B");
        assert!(b.reason.unwrap().contains('B'));
        assert_eq!(b.franja, "buenos días");
        assert_eq!(b.display_name.as_deref(), Some("Ska"));
    }
}
