# Validation report (Phase 3)

## Tests executed

| Check | Result |
|---|---|
| `cargo fmt --check` | Pre-existing formatting diff in `examples/probe_seek.rs` (rustfmt version/style); no changed Rust files in this work |
| `cargo clippy --all-targets -- -D warnings` | Pass (default) |
| `cargo test --lib` | Pass: 710 passed, 1 ignored |
| `cargo test --lib --features youtube` | Pass: 733 passed, 1 ignored |
| `cargo test --lib --features android` | Pass: 738 passed, 1 ignored |
| `cargo test --test audio_decode` | Pass: 6 passed |
| `cargo check --offline` | Pass |
| `cargo check --offline --features youtube` | Pass |
| `cd android && ./gradlew --offline test` (with ANDROID_HOME set) | Pass: BUILD SUCCESSFUL |
| Context report formats (text/markdown/json) | Pass |
| Context report edge cases (empty/unknown/inferred) | Pass |
| Context report path resolution | Pass (0 unresolved after fixes) |

## Context report metrics (after)

| Task | System | files_included | files_excluded | context_size_bytes | unresolved | errors | time_ms |
|---|---|---:|---:|---:|---:|---:|---:|
| A DSP EQ | dsp | 8 | 3 | 22291 | 0 | 0 | 0.194 |
| B Android playback | android | 11 | 3 | 71019 | 0 | 0 | 0.256 |
| C HTTP transport | networking | 12 | 3 | 25746 | 0 | 0 | 0.240 |
| D UI surface | ui | 10 | 3 | 22688 | 0 | 0 | 0.268 |

## Comparison (before vs after)

Tokens are not available in this environment; before/after measured in context size and file counts where possible.

| Metric | Before | After |
|---|---:|---:|
| Input/output tokens | not available | not available |
| Files included in task package | not measured (manual discovery) | 8-12 |
| Files explicitly excluded | not tracked | 3 per task |
| Unresolved sources | not tracked | 0 |
| Context size (bytes) | not measured | 22-71 KB |
| Generator execution time | n/a | <1 ms |

## Compatibility

- Linux: verified (commands above).
- Windows/macOS: not verified in this environment.
- Cache invalidation: manifest mtime check implemented; `--no-cache` and `--refresh` supported.

## Regressions

- No functional code changed.
- No API changes.
- No dependencies added.
- No secrets committed.

## Remaining limitations

- Android Gradle requires ANDROID_HOME/ANDROID_SDK_ROOT in environment.
- `cargo fmt --check` reports pre-existing example formatting diff.
- Real token counts unavailable; context size bytes are not tokens.
- Windows/macOS not verified.
- Context report does not read code contents; precision depends on manifest accuracy.
