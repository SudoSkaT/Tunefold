# 0001 — YouTube provider isolated behind feature flag

Status: accepted

## Context
YouTube access uses nonofficial Innertube interfaces and is excluded from official binaries by YouTube ToS.

## Decision
All YouTube-specific code lives in `src/providers/youtube/` and compiles only with feature `youtube`; `src/api/mod.rs` is the sole composition boundary. Default builds have empty registries and return structured `ProviderUnavailable`.

## Alternatives
- Always compile provider with runtime toggle: rejected because symbols and network code would ship in official binary.
- Separate crate: rejected; feature flag keeps single repo and tests.

## Consequences
- `cargo check --offline` and `--features youtube` both must pass.
- Tests split by feature.
- Android custom builds opt in explicitly.

## References
- `src/api/mod.rs`
- `Cargo.toml` features
- `docs/policies.md`
