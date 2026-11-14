# Baseline report (Phase 0)

## Repository state

- Crate: `tunefold` 0.19.4, GPL-3.0, Rust 2021.
- Source: 140 `.rs` files under `src/`.
- Tests: 699 `#[test]` attributes in src/tests; integration `tests/audio_decode.rs`.
- Docs: 27 markdown files; docs size ~108 KB; largest `docs/android/visual-system.md` (14.3 KB) and `docs/android/playback-glossary.md` (16.1 KB).
- Android app present under `android/`.

## Documentation inventory

Current technical specs:
- `docs/architecture/audio-engine.md`, `dsp-pipeline.md`, `playback-lifecycle.md`
- `docs/android/playback-glossary.md`, `visual-system.md`
- `docs/recommendations.md`
- `docs/testing/audio-validation.md`

Historical/roadmap/audit:
- `docs/audit/*`
- `docs/roadmap/audio-stabilization.md`
- `android/*_REPORT.md`

Policies:
- `docs/policies.md`, `CONTRIBUTING.md`, `PRIVACY.md`, `SECURITY.md`

## Findings

| ID | Finding | Priority |
|---|---|---|
| F1 | Global instructions (`AGENTS.md`) mix architecture, rules, commands; no compact task discovery | P1 |
| F2 | No central module index; module discovery requires reading many files | P1 |
| F3 | Audit/roadmap docs can be mistaken for current state | P1 |
| F4 | Verification commands scattered across AGENTS/CONTRIBUTING/policies/testing | P2 |
| F5 | No task-specific context generator; agents must traverse repo | P1 |
| F6 | No known-issues register separating fixed vs pending | P2 |
| F7 | No ADR set for key architectural decisions | P2 |
| F8 | Android and desktop boundaries documented but not indexed compactly | P2 |

## Measurement limitations

- Real token counters unavailable: record bytes, files, lines, searches, repeated reads, times when available.
- Android hardware metrics unavailable without device.

## Baseline metrics (honest)

| Metric | Value |
|---|---|
| Input/output tokens | not available |
| Rust source files | 140 |
| Markdown docs | 27 |
| Test attributes | 699 |
| Docs size | ~108 KB |

## Reference tasks (discovery only, no code changes)

### Task A — DSP
Goal: locate how to modify an EQ parameter and its tests.
Expected modules: `src/dsp/biquad.rs`, `src/dsp/params.rs`, `src/dsp/control.rs`, `src/dsp/chain.rs`.
Tests: `cargo test --lib dsp::`, `bench_dsp`.
Baseline pain: must read `dsp-pipeline.md` (8.6 KB) plus source.

### Task B — Android
Goal: locate playback flow, responsible service, verification.
Expected modules: `src/platform/android_playback.rs`, `src/platform/android_decoder.rs`, `android/` service.
Tests: `cargo test --lib --features android`, Gradle tests.
Baseline pain: glossary (16 KB) + visual system (14 KB) + README.

### Task C — Networking
Goal: locate HTTP transport, cancellation, tests.
Expected modules: `src/media/transport/`, `src/media/stream.rs`, `src/media/resolver.rs`.
Tests: `cargo test --lib media::transport`, integration FakeServer.
Baseline pain: README streaming section + audio validation.

### Task D — UI
Goal: locate an interface surface, composition rules, tests.
Expected modules: `src/ui/`, `src/ui/widgets/`, `src/ui/backend.rs`.
Tests: `cargo test --lib ui::`, `app::`.
Baseline pain: scattered docs; Android visual doc not applicable to TUI.

## Initial estimate
Before infrastructure, an agent likely inspects many docs and source files; objective is >=30% reduction in irrelevant files inspected without precision loss.
