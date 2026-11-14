//! External playlist import and generation helpers.
//!
//! Small, deterministic orchestration over the existing persistence layer.
//! Provider/network retrieval stays in the provider layer; this module only
//! consumes already-resolved tracks.

use std::collections::HashMap;

use anyhow::Result;

use crate::domain::external_playlist::{
    ExternalPlaylist, ExternalPlaylistReference, GenerationStrategy, ImportStatus,
};
use crate::domain::track::Track;
use crate::infrastructure::db::Db;

pub async fn persist_reference(db: &Db, playlist: &ExternalPlaylist) -> Result<i64> {
    let reference = ExternalPlaylistReference::from_playlist(playlist);
    db.create_external_playlist_reference(&reference).await
}

pub async fn import_tracks(db: &Db, name: &str, tracks: &[Track]) -> Result<i64> {
    let playlist_id = db.create_playlist(name).await?;
    for track in tracks {
        let mut ids = HashMap::new();
        if let Some(external_id) = &track.external_id {
            ids.insert(track.source, external_id.clone());
        }
        let track_id = db.upsert_track(track, &ids).await?;
        db.add_to_playlist(playlist_id, track_id).await?;
    }
    Ok(playlist_id)
}

pub async fn associate_reference(
    db: &Db,
    reference_id: i64,
    native_playlist_id: i64,
    strategy: GenerationStrategy,
) -> Result<()> {
    db.associate_native_playlist(reference_id, native_playlist_id, Some(strategy.as_str()))
        .await
}

pub async fn generate_copy(db: &Db, name: &str, tracks: &[Track]) -> Result<i64> {
    import_tracks(db, name, tracks).await
}

pub async fn generate_variation(db: &Db, name: &str, tracks: &[Track]) -> Result<i64> {
    // Deterministic variation: reverse order, preserving membership.
    let mut varied: Vec<Track> = tracks.to_vec();
    varied.reverse();
    import_tracks(db, name, &varied).await
}

pub async fn refresh_reference_metadata(
    db: &Db,
    reference_id: i64,
    playlist: &ExternalPlaylist,
) -> Result<()> {
    if let Some(mut existing) = db.get_external_playlist_reference(reference_id).await? {
        existing.title = playlist.title.clone();
        existing.description = playlist.description.clone();
        existing.thumbnail = playlist.thumbnail.as_ref().map(|t| t.url.clone());
        existing.owner = playlist.owner.clone();
        existing.last_refreshed_at = Some(chrono::Utc::now().to_rfc3339());
        existing.import_status = if existing.native_playlist_id.is_some() {
            ImportStatus::Imported
        } else {
            existing.import_status
        };
        db.update_external_playlist_reference(&existing).await?;
    }
    Ok(())
}

pub fn related_discovery_available() -> bool {
    // Conservative: only true if a provider implements `related`. The default
    // provider capability is a no-op, so report unavailable by default.
    false
}

// Re-export for convenience.
pub use crate::domain::external_playlist::ExternalPlaylistReference as Reference;
