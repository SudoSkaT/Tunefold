# Core engineering contract (Tunefold)

Canonical short contract. `AGENTS.md` points here; do not duplicate long specs.

## Identity

Tunefold is a TUI music player and recommendation engine in Rust. YouTube provider is optional behind Cargo feature `youtube` and excluded from official binaries.

## Non-negotiable rules

1. Rebrand: no `playfusion`/`PlayFusion` in new code, visible text, or paths (legacy env alias `PLAYFUSION_RANGE_WINDOW_KIB` excepted).
2. YouTube only in `src/providers/youtube/` and feature `youtube`; default build must not compile rustypipe or touch YouTube network.
3. GPL-3.0; dependencies only per `Documentacion/docs/policies.md`.
4. User data and DB via `src/infrastructure/dirs.rs::*`; never CWD-relative paths.
5. User data migration copies, never deletes.

## Context discovery

- Level 1 (always): `AGENTS.md`, this file.
- Level 2 (by task): `Documentacion/engineering/MODULE_INDEX.md`, `Documentacion/engineering/ARCHITECTURE.md`, `Documentacion/engineering/VERIFICATION.md`, `Documentacion/engineering/KNOWN_ISSUES.md`, and relevant `Documentacion/engineering/systems/*.md`.
- Level 3 (only after locating modules): specific source files.
- Use `scripts/Documentacion/engineering/context-report --task "..." --system <system>`.
- For product/UI/interaction/accessibility/visualizer work, consult `Documentacion/standards/TPS/README.md` and the relevant section before editing. Android Views are in `android/app/src/main/java/com/tunefold/app/`; TUI widgets are in `src/ui/`.

## Verification

See `Documentacion/engineering/VERIFICATION.md`. Minimum for Rust changes:
`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --lib`.

## Agent consumption

Any agent can invoke:
`scripts/Documentacion/engineering/context-report --task "..." --system <system> --format json`

Supported consumers: Codex, Claude Code, Cursor, GitHub Copilot. Use Level 1 always, then Level 2 docs from the package, then only Level 3 paths listed.

## Documentation ownership

- `Documentacion/docs/architecture/*`, `Documentacion/docs/android/*`, `Documentacion/docs/recommendations.md`: current technical specs.
- `Documentacion/docs/audit/*`, `Documentacion/docs/roadmap/*`, `Documentacion/android/*_REPORT.md`: historical; do not treat as current state.
- `Documentacion/engineering/*`: compact, current, linked to code.
- `Documentacion/standards/TPS/*`: normative product and interface standard; detailed playback/DSP and platform contracts remain in their technical source documents.
