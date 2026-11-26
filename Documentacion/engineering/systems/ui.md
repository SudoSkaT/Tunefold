# UI (TUI)

Product and interface requirements: [`Documentacion/standards/TPS/README.md`](../../standards/TPS/README.md). Android UI is implemented separately under `android/app/src/main/java/com/tunefold/app/`; this system describes Rust `src/ui/`.

## Purpose
Terminal UI: app loop, backend commands, views, widgets, navigation, settings.

## Key files
- `src/ui/` (app, backend, event, view, layout, navigation, settings, search/*, metadata/*, dashboard/*, history/*, widgets/*)
- `src/main.rs`

## Constraints
- Backend events are epoch-aware.
- No blocking I/O on UI thread beyond documented paths.
- Local-first privacy.

## Tests
- `cargo test --lib ui::`
- `cargo test --lib app::`

## Docs
- `README.md`

## Related systems
playback, visualization, persistence
