# AGENTS.md

Compact entry point for agents working on TuneFold. Read this first, then load only the specialized docs needed for the task.

## Identity

Tunefold is a TUI music player and recommendation engine in Rust (GPL-3.0). The YouTube provider is optional behind the Cargo feature `youtube` and is excluded from official binaries.

## Non-negotiable rules

1. Rebrand: no `playfusion`/`PlayFusion` in new code, visible text, or paths (legacy env alias `PLAYFUSION_RANGE_WINDOW_KIB` excepted).
2. YouTube only in `src/providers/youtube/` and feature `youtube`; default builds must not compile rustypipe or touch YouTube network.
3. GPL-3.0; dependencies only per `docs/policies.md`.
4. User data and DB via `src/infrastructure/dirs.rs::*`; never CWD-relative paths.
5. User data migration copies, never deletes.

## Essential commands

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --lib
cargo test --lib --features youtube
cargo check --offline
cargo check --offline --features youtube
```

See `engineering/VERIFICATION.md` for the full matrix.

## Context discovery

- Level 1 (always): this file + `engineering/CORE.md`.
- Level 2 (by task): `engineering/MODULE_INDEX.md`, `engineering/ARCHITECTURE.md`, `engineering/VERIFICATION.md`, `engineering/KNOWN_ISSUES.md`, relevant `engineering/systems/*.md`.
- Level 3 (after locating modules): specific source files.
- Generate task package: `scripts/engineering/context-report --task "..." --system <system> [--format text|markdown|json]`.

## Where to look

| Need | Doc |
|---|---|
| Modules/deps | `engineering/MODULE_INDEX.md`, `engineering/DEPENDENCIES.md` |
| Architecture | `engineering/ARCHITECTURE.md`, `docs/architecture/*` |
| Android | `engineering/systems/android.md`, `docs/android/*` |
| DSP | `engineering/systems/dsp.md`, `docs/architecture/dsp-pipeline.md` |
| Recommendations | `engineering/systems/recommendations.md`, `docs/recommendations.md` |
| Verification | `engineering/VERIFICATION.md`, `docs/testing/audio-validation.md` |
| Known issues | `engineering/KNOWN_ISSUES.md` |
| Decisions | `engineering/decisions/*` |

## Historical docs

`docs/audit/*`, `docs/roadmap/*`, and `android/*_REPORT.md` are historical. Do not treat them as current implementation state.

## Notes

- `examples/probe_*` require feature `youtube`; `bench_hotpaths` does not.
- Version source: `Cargo.toml` (`CARGO_PKG_VERSION`); releases use numeric tags without `v`.
- Policies: `docs/policies.md`, `PRIVACY.md`.
