# Architecture (Tunefold)

High-level map. Deep specs remain in `docs/architecture/*`, `docs/android/*`, `docs/recommendations.md`.

## Layers

- `domain`: pure models, no I/O.
- `catalog` / `media`: provider-neutral catalog and stream resolution.
- `providers/*`: isolated adapters; YouTube only behind feature `youtube`.
- `api`: sole composition boundary (`compose_media`).
- `playback`: queue, position, recovery, router.
- `app`: orchestration, audio contract, history, search, updater.
- `infrastructure`: DB, storage, config, XDG dirs, desktop playback engine.
- `ui`: TUI.
- `platform`: Android JNI/decoder/playback.
- `analysis`, `dsp`, `visualization`, `recommendation`: specialized subsystems.

## Main flows

1. Search: UI -> app/search -> catalog registry -> provider adapter.
2. Play: UI/backend -> app/playback -> stream resolver -> media transport -> playback engine.
3. Analysis: decoder tap -> analysis thread -> feature bus -> visualization/recommendations.
4. Recommendations: provider related -> shared ranker (desktop or JNI) -> rerank when profiles available.
5. Android: Kotlin service -> JNI -> Rust decoder -> PCM ring -> AudioTrack; ranking via `android_rank`.

## Critical constraints

- Audio thread never waits on network, disk, SQLite, UI, or blocking locks.
- Events are filtered by generation/epoch; stale sessions cannot publish state.
- Cancel transport token interrupts open/read/retry/body.
- YouTube code and rustypipe only with feature `youtube`.
- Data paths via `dirs::*`; legacy `data/music.db` copied, never deleted.
- Analysis is pre-DSP; DSP chain is desktop-only currently.
- Recommendation weights fixed and tested; impressions never score.

## Contracts

- `PlaybackEngine` (`src/app/audio.rs`).
- Android ranking JSON (`src/platform/android_rank.rs`).
- HTTP range invariants (`docs/architecture/audio-engine.md`).
- XDG paths (`src/infrastructure/dirs.rs`).

## Historical vs current

Current architecture: `docs/architecture/*`, `docs/android/*`, `docs/recommendations.md`.
Historical/audit/proposals: `docs/audit/*`, `docs/roadmap/*`, `android/*_REPORT.md`.
Audit docs are not descriptions of current implementation.
