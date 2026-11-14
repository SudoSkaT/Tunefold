# Core engineering contract (Tunefold)

Canonical short contract. `AGENTS.md` points here; do not duplicate long specs.

## Identity

Tunefold is a TUI music player and recommendation engine in Rust. YouTube provider is optional behind Cargo feature `youtube` and excluded from official binaries.

## Non-negotiable rules

1. Rebrand: no `playfusion`/`PlayFusion` in new code, visible text, or paths (legacy env alias `PLAYFUSION_RANGE_WINDOW_KIB` excepted).
2. YouTube only in `src/providers/youtube/` and feature `youtube`; default build must not compile rustypipe or touch YouTube network.
3. GPL-3.0; dependencies only per `docs/policies.md`.
4. User data and DB via `src/infrastructure/dirs.rs::*`; never CWD-relative paths.
5. User data migration copies, never deletes.

## Context discovery

- Level 1 (always): `AGENTS.md`, this file.
- Level 2 (by task): `engineering/MODULE_INDEX.md`, `engineering/ARCHITECTURE.md`, `engineering/VERIFICATION.md`, `engineering/KNOWN_ISSUES.md`, and relevant `engineering/systems/*.md`.
- Level 3 (only after locating modules): specific source files.
- Use `scripts/engineering/context-report --task "..." --system <system>`.

## Verification

See `engineering/VERIFICATION.md`. Minimum for Rust changes:
`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --lib`.

## Agent consumption

Any agent can invoke:
`scripts/engineering/context-report --task "..." --system <system> --format json`

Supported consumers: Codex, Claude Code, Cursor, GitHub Copilot. Use Level 1 always, then Level 2 docs from the package, then only Level 3 paths listed.

## Documentation ownership

- `docs/architecture/*`, `docs/android/*`, `docs/recommendations.md`: current technical specs.
- `docs/audit/*`, `docs/roadmap/*`, `android/*_REPORT.md`: historical; do not treat as current state.
- `engineering/*`: compact, current, linked to code.
