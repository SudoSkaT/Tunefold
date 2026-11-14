# Visualization

## Purpose
TUI visualization: spectrum, lava metaballs, beat pulse, artwork-derived palette.

## Key files
- `src/visualization/` (mod, engine, params, palette, render)
- `src/ui/widgets/`, `src/ui/dashboard/`

## Constraints
- Phase derives from real playback position; no independent visual clocks.
- Render hot path avoids allocations.
- Bars only redraw on new audio.
- Geometry measured once per size.

## Tests
- `cargo test --lib visualization::`
- `cargo test --lib ui::`

## Docs
- `README.md` (visualizer)
- `docs/android/visual-system.md` (Android visual; separate)

## Related systems
analysis, ui
