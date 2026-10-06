//! Persistent cache for provider-neutral YouTube track metadata.
//!
//! Entries contain stable track metadata only. Temporary stream URLs, request
//! headers, and audio bytes are deliberately stored elsewhere (or not stored).

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::domain::{source::Source, track::Track};

const MAX_ENTRIES: usize = 512;
const TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Debug, Serialize, Deserialize)]
struct Entry {
    cached_at: u64,
    track: Track,
}

/// Bounded disk cache keyed by the stable provider ID.
pub(crate) struct MetadataCache {
    directory: PathBuf,
    gate: Mutex<()>,
}

impl MetadataCache {
    pub(crate) fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            gate: Mutex::new(()),
        }
    }

    pub(crate) fn get(&self, id: &str) -> Option<Track> {
        let _guard = self.gate.lock().ok()?;
        let path = self.path(id)?;
        let bytes = fs::read(&path).ok()?;
        let entry: Entry = match serde_json::from_slice(&bytes) {
            Ok(entry) => entry,
            Err(_) => {
                let _ = fs::remove_file(path);
                return None;
            }
        };
        let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
        if now.saturating_sub(entry.cached_at) > TTL.as_secs()
            || entry.track.source != Source::YouTube
            || entry.track.external_id.as_deref() != Some(id)
        {
            let _ = fs::remove_file(path);
            return None;
        }
        Some(entry.track)
    }

    pub(crate) fn put(&self, track: &Track) {
        let Some(id) = track.external_id.as_deref() else {
            return;
        };
        if track.source != Source::YouTube {
            return;
        }
        let Some(path) = self.path(id) else { return };
        let Ok(_guard) = self.gate.lock() else { return };
        let Ok(cached_at) = SystemTime::now().duration_since(UNIX_EPOCH) else {
            return;
        };
        let entry = Entry {
            cached_at: cached_at.as_secs(),
            track: track.clone(),
        };
        let Ok(bytes) = serde_json::to_vec(&entry) else {
            return;
        };
        if fs::create_dir_all(&self.directory).is_err() {
            return;
        }
        let temp = path.with_extension(format!("tmp-{}", std::process::id()));
        if fs::write(&temp, bytes).is_ok() {
            let _ = fs::rename(&temp, &path);
            let _ = fs::remove_file(temp);
            self.evict_old_entries();
        }
    }

    fn path(&self, id: &str) -> Option<PathBuf> {
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        {
            return None;
        }
        Some(self.directory.join(format!("{id}.json")))
    }

    fn evict_old_entries(&self) {
        let Ok(entries) = fs::read_dir(&self.directory) else {
            return;
        };
        let mut files: Vec<_> = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                if path
                    .extension()
                    .is_some_and(|extension| extension == "json")
                {
                    Some((entry.metadata().ok()?.modified().ok()?, path))
                } else {
                    None
                }
            })
            .collect();
        if files.len() <= MAX_ENTRIES {
            return;
        }
        files.sort_by_key(|(modified, _)| *modified);
        let evict_count = files.len().saturating_sub(MAX_ENTRIES);
        for (_, path) in files.into_iter().take(evict_count) {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory() -> PathBuf {
        std::env::temp_dir().join(format!(
            "tunefold-metadata-cache-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn track(id: &str) -> Track {
        let mut track = Track::new("Known title".into(), Vec::new(), Source::YouTube);
        track.external_id = Some(id.into());
        track
    }

    #[test]
    fn metadata_cache_roundtrips_and_misses_cleanly() {
        let root = directory();
        let cache = MetadataCache::new(root.clone());
        cache.put(&track("video_01"));
        assert_eq!(cache.get("video_01").unwrap().title, "Known title");
        assert!(cache.get("missing").is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn metadata_cache_rejects_unsafe_keys() {
        let root = directory();
        let cache = MetadataCache::new(root.clone());
        assert!(cache.get("../video").is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn metadata_cache_expires_stale_entry() {
        let root = directory();
        let cache = MetadataCache::new(root.clone());
        let path = cache.path("old").unwrap();
        fs::create_dir_all(&root).unwrap();
        let old = Entry {
            cached_at: 0,
            track: track("old"),
        };
        fs::write(path, serde_json::to_vec(&old).unwrap()).unwrap();
        assert!(cache.get("old").is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn metadata_cache_removes_corrupt_entry() {
        let root = directory();
        let cache = MetadataCache::new(root.clone());
        let path = root.join("corrupt.json");
        fs::create_dir_all(&root).unwrap();
        fs::write(&path, b"incomplete json").unwrap();
        assert!(cache.get("corrupt").is_none());
        assert!(!path.exists());
        let _ = fs::remove_dir_all(root);
    }
}
