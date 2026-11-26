# AGENTS.md

Compact entry point for agents working on TuneFold. Read this first, then load only the specialized docs needed for the task.

## Identity

Tunefold is a TUI music player and recommendation engine in Rust (GPL-3.0). The YouTube provider is optional behind the Cargo feature `youtube` and is excluded from official binaries.

## Non-negotiable rules

1. Rebrand: no `playfusion`/`PlayFusion` in new code, visible text, or paths (legacy env alias `PLAYFUSION_RANGE_WINDOW_KIB` excepted).
2. YouTube only in `src/providers/youtube/` and feature `youtube`; default builds must not compile rustypipe or touch YouTube network.
3. GPL-3.0; dependencies only per `Documentacion/docs/policies.md`.
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

See `Documentacion/engineering/VERIFICATION.md` for the full matrix.

## Context discovery

- Level 1 (always): this file + `Documentacion/engineering/CORE.md`.
- Level 2 (by task): `Documentacion/engineering/MODULE_INDEX.md`, `Documentacion/engineering/ARCHITECTURE.md`, `Documentacion/engineering/VERIFICATION.md`, `Documentacion/engineering/KNOWN_ISSUES.md`, relevant `Documentacion/engineering/systems/*.md`.
- Level 3 (after locating modules): specific source files.
- Generate task package: `scripts/engineering/context-report --task "..." --system <system> [--format text|markdown|json]`.

## Where to look

| Need | Doc |
|---|---|
| Modules/deps | `Documentacion/engineering/MODULE_INDEX.md`, `Documentacion/engineering/DEPENDENCIES.md` |
| Architecture | `Documentacion/engineering/ARCHITECTURE.md`, `Documentacion/docs/architecture/*` |
| Android | `Documentacion/engineering/systems/android.md`, `Documentacion/docs/android/*` |
| DSP | `Documentacion/engineering/systems/dsp.md`, `Documentacion/docs/architecture/dsp-pipeline.md` |
| Recommendations | `Documentacion/engineering/systems/recommendations.md`, `Documentacion/docs/recommendations.md` |
| Verification | `Documentacion/engineering/VERIFICATION.md`, `Documentacion/docs/testing/audio-validation.md` |
| Known issues | `Documentacion/engineering/KNOWN_ISSUES.md` |
| Decisions | `Documentacion/engineering/decisions/*` |

## Historical docs

`Documentacion/docs/audit/*`, `Documentacion/docs/roadmap/*`, and `Documentacion/android/*_REPORT.md` are historical. Do not treat them as current implementation state.

## Notes

- `examples/probe_*` require feature `youtube`; `bench_hotpaths` does not.
- Version source: `Cargo.toml` (`CARGO_PKG_VERSION`); releases use numeric tags without `v`.
- Policies: `Documentacion/docs/policies.md`, `Documentacion/PRIVACY.md`.
