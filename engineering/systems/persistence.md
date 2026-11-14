# Persistence

## Purpose
SQLite, models, history, stats, caches, XDG dirs, migration.

## Key files
- `src/infrastructure/db.rs`, `storage.rs`, `config.rs`, `dirs.rs`, `mod.rs`
- `src/app/history.rs`, `aggregator.rs`
- `migrations/`

## Constraints
- XDG paths only; never CWD-relative data.
- Legacy `data/music.db` copied, never deleted.
- Migration non-destructive.
- Config `.env` in config dir.

## Tests
- `cargo test --lib infrastructure::`
- `cargo test --lib app::history`

## Docs
- `docs/policies.md`
- `README.md` (local data)

## Related systems
playback, recommendations, ui
