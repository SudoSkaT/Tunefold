# Providers

## Purpose
External source adapters and composition boundary.

## Key files
- `src/providers/` (`mod.rs`, `lyrics.rs`, `youtube/`)
- `src/api/mod.rs`
- `src/catalog/mod.rs`, `src/media/registry.rs`, `src/media/provider.rs`

## Constraints
- YouTube and rustypipe only behind feature `youtube`.
- YouTube code only in `src/providers/youtube/`.
- `api/mod.rs` is sole composition boundary.
- Default build has empty registries and structured `ProviderUnavailable`.
- LRCLIB lyrics always compiled.

## Tests
- `cargo test --lib`
- `cargo test --lib --features youtube`
- `cargo check --offline` / `--features youtube`

## Docs
- `docs/policies.md`
- `README.md`

## Related systems
networking, recommendations, catalog
