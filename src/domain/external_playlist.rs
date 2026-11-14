//! External (provider-hosted) playlist references and previews.

use crate::domain::source::Source;
use crate::domain::track::{Thumbnail, Track};

/// Import state of an external playlist reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum ImportStatus {
    #[default]
    NotImported,
    Imported,
    Partial,
}

impl ImportStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotImported => "not_imported",
            Self::Imported => "imported",
            Self::Partial => "partial",
        }
    }
}

impl std::str::FromStr for ImportStatus {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(match value {
            "imported" => Self::Imported,
            "partial" => Self::Partial,
            _ => Self::NotImported,
        })
    }
}

/// Deterministic generation strategies supported for new playlists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GenerationStrategy {
    Copy,
    Variation,
    RelatedDiscovery,
}

impl GenerationStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::Variation => "variation",
            Self::RelatedDiscovery => "related_discovery",
        }
    }
}

impl std::str::FromStr for GenerationStrategy {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(match value {
            "variation" => Self::Variation,
            "related_discovery" => Self::RelatedDiscovery,
            _ => Self::Copy,
        })
    }
}

/// Preview/metadata retrieved from an external provider before persistence.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExternalPlaylist {
    pub source: Source,
    pub external_id: String,
    pub canonical_url: String,
    pub title: String,
    pub description: Option<String>,
    pub thumbnail: Option<Thumbnail>,
    pub owner: Option<String>,
    pub track_count: Option<u64>,
    pub tracks: Vec<Track>,
}

impl ExternalPlaylist {
    pub fn new(source: Source, external_id: String) -> Self {
        let canonical_url = format!("https://www.youtube.com/playlist?list={external_id}");
        Self {
            source,
            external_id,
            canonical_url,
            title: String::new(),
            description: None,
            thumbnail: None,
            owner: None,
            track_count: None,
            tracks: Vec::new(),
        }
    }
}

/// Persisted external playlist reference.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExternalPlaylistReference {
    pub id: i64,
    pub source: Source,
    pub external_id: String,
    pub canonical_url: String,
    pub title: String,
    pub description: Option<String>,
    pub thumbnail: Option<String>,
    pub owner: Option<String>,
    pub last_refreshed_at: Option<String>,
    pub created_at: String,
    pub import_status: ImportStatus,
    pub native_playlist_id: Option<i64>,
}

impl ExternalPlaylistReference {
    pub fn from_playlist(playlist: &ExternalPlaylist) -> Self {
        Self {
            id: 0,
            source: playlist.source,
            external_id: playlist.external_id.clone(),
            canonical_url: playlist.canonical_url.clone(),
            title: playlist.title.clone(),
            description: playlist.description.clone(),
            thumbnail: playlist.thumbnail.as_ref().map(|t| t.url.clone()),
            owner: playlist.owner.clone(),
            last_refreshed_at: None,
            created_at: String::new(),
            import_status: ImportStatus::NotImported,
            native_playlist_id: None,
        }
    }
}
