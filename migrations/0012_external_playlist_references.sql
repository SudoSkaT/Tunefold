-- 0012: referencias a playlists externas (YouTube) y procedencia de playlists generadas.

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS external_playlist_references (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source TEXT NOT NULL,
    external_id TEXT NOT NULL,
    canonical_url TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    description TEXT,
    thumbnail TEXT,
    owner TEXT,
    last_refreshed_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    import_status TEXT NOT NULL DEFAULT 'not_imported',
    native_playlist_id INTEGER REFERENCES playlists(id) ON DELETE SET NULL,
    UNIQUE (source, external_id)
);

CREATE INDEX IF NOT EXISTS idx_external_playlist_references_url
    ON external_playlist_references(canonical_url);
CREATE INDEX IF NOT EXISTS idx_external_playlist_references_native
    ON external_playlist_references(native_playlist_id);

ALTER TABLE playlists ADD COLUMN source_reference_id INTEGER
    REFERENCES external_playlist_references(id) ON DELETE SET NULL;
ALTER TABLE playlists ADD COLUMN generation_strategy TEXT;
