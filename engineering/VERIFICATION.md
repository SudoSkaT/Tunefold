# Verification matrix (Tunefold)

Single source for which checks to run by change type. Commands come from repository/CI.

| Change type | Required | Recommended | Notes |
|---|---|---|---|
| Rust general | `cargo fmt --check`; `cargo clippy --all-targets -- -D warnings`; `cargo test --lib` | `cargo check --offline` | Both default and `youtube` when affected |
| DSP | `cargo test --lib dsp::` | `cargo run --release --example bench_dsp` | Android not on DSP chain yet |
| Audio decode | `cargo test --test audio_decode`; `cargo test --lib media::transport` | `cargo test --lib` | Integration test needs symphonia dev-deps |
| Android | `cargo test --lib --features android`; `cd android && ./gradlew --offline test` | device validation | JNI ranking included |
| JNI contract | `cargo test --lib --features android` | `cargo check --offline --features android` | `platform/android_rank.rs` |
| Networking/transport | `cargo test --lib media::transport` | `cargo test --lib` | FakeServer scenarios |
| Providers | `cargo check --offline`; `cargo check --offline --features youtube`; `cargo test --lib --features youtube` | license check | No YouTube outside provider |
| UI | `cargo test --lib ui::`; `cargo test --lib app::` | `cargo test --lib` | TUI only |
| Recommendations | `cargo test --lib recommendation::`; `cargo test --lib --features android platform::android_rank` | Android tests | Shared ranker |
| Dependencies | `cargo check --offline`; `cargo clippy --all-targets --all-features -- -D warnings` | `cargo license --all-features --avoid-dev-deps` | Update THIRD_PARTY/NOTICE |
| Docs only | link/path check | n/a | No code change |

## Hardware / external

- Android audio validation requires a physical device/emulator; see `docs/testing/audio-validation.md`.
- YouTube features require network and feature flag; not part of official verification.

## CI source

`.github/workflows/ci.yml`: fmt, clippy (default + youtube), test (default + youtube), licenses.
`cargo test --lib --features android` and Android Gradle tests are documented in policies but not in default CI matrix.
