# Playback (session and queue)

## Purpose
Queue, position clock, preload, recovery, router, app orchestration.

## Key files
- `src/playback/` (mod, queue, position_clock, preload, recovery, karaoke_tests)
- `src/app/playback.rs`
- `src/infrastructure/playback/` (engine)
- `src/media/router.rs`

## Flow
Intention -> epoch -> resolve -> epoch check -> commit -> engine play -> persist -> monitor.

## Constraints
- `begin_playback_epoch` invalidates in-flight work.
- `recover_current` captures epoch and rechecks before play.
- Stop increments epoch; late play cannot revive stop.
- Persist only after successful start.
- Monitor filters by generation.

## Tests
- `cargo test --lib playback::`
- `cargo test --lib app::playback`

## Docs
- `docs/architecture/playback-lifecycle.md`

## Related systems
audio, networking, ui, persistence
