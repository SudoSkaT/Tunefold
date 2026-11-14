# 0002 — XDG paths and non-destructive migration

Status: accepted

## Context
User data must survive updates and never be CWD-relative.

## Decision
Data/config/cache/state paths resolved via `src/infrastructure/dirs.rs` (XDG). Legacy `data/music.db` is copied once to canonical path; never deleted. Canonical takes priority if both exist.

## Alternatives
- Keep relative `data/`: rejected; breaks when run from different CWD.
- Move (delete legacy): rejected; data-loss risk.

## Consequences
- All new code uses `dirs::*`.
- Migration is idempotent and non-destructive.

## References
- `src/infrastructure/dirs.rs`
- `README.md` (local data)
