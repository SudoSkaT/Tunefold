# Module index (Tunefold)

Compact index of architecturally relevant modules. Paths are relative to repo root.
For task-specific selection use `scripts/Documentacion/engineering/context-report`.

Product/UI standard: [`Documentacion/standards/TPS/README.md`](../standards/TPS/README.md). Read it for interface, interaction, motion, accessibility, visualizer, or UI performance changes. Android Views and TUI are separate implementations.

| Subsystem | Module | Path | Responsibility | Key dependents | Boundary | Tests |
|---|---|---|---|---|---|---|
| Core | lib crate | `src/lib.rs` | Crate root and module declarations | all | no provider details | `cargo test --lib` |
| Composition | api | `src/api/mod.rs` | Sole composition boundary for providers/media | app, ui | no network/provider impl | `cargo test --lib` |
| Domain | domain | `src/domain/` | Pure models (Track, Source, Stream, Playlist) | all | no I/O | `cargo test --lib domain::` |
| Catalog | catalog | `src/catalog/mod.rs` | Provider-neutral catalog registry | api, app | no provider-specific code | `cargo test --lib catalog::` |
| Media | media | `src/media/` | Stream registry/resolver/router/cache/failure | api, playback | no UI | `cargo test --lib media::` |
| Transport | media/transport | `src/media/transport/` | HTTP Range stream, retries, cancellation | media, audio | no track semantics | `cargo test --lib media::transport` |
| Providers | providers | `src/providers/` | Adapters; YouTube behind feature `youtube` | api only | no direct refs outside api | `cargo test --lib --features youtube` |
| Playback | playback | `src/playback/` | Queue, position clock, preload, recovery | app, ui | no provider details | `cargo test --lib playback::` |
| App | app | `src/app/` | Playback orchestration, audio, history, search, updater | ui | no provider details | `cargo test --lib app::` |
| Audio (desktop) | infrastructure/playback | `src/infrastructure/playback/` | rodio/symphonia engine, buffering, monitor | app | no SQLite/UI | `cargo test --lib infrastructure::playback` |
| DSP | dsp | `src/dsp/` | Params, control, processors, chain, rodio adapter | audio | no rodio in core dsp | `cargo test --lib dsp::` |
| Analysis | analysis | `src/analysis/` | FFT, bands, chroma, features, tap, ring | recommendation, visualization | no UI | `cargo test --lib analysis::` |
| Visualization | visualization | `src/visualization/` | Spectrum, lava, beat pulse, palette | ui | no audio thread | `cargo test --lib visualization::` |
| Recommendations | recommendation | `src/recommendation/` | Weights, scoring, signals, sessions, JNI rank | ui, platform | no provider details | `cargo test --lib recommendation::` |
| Android JNI rank | platform/android_rank | `src/platform/android_rank.rs` | Stateless JSON ranking contract | Android app | no youtube dep | `cargo test --lib --features android` |
| Platform | platform | `src/platform/` | Android decoder/playback/download/JNI | android app | no TUI | `cargo test --lib --features android` |
| Persistence | infrastructure | `src/infrastructure/` | DB, storage, config, XDG dirs | app, media | no UI | `cargo test --lib infrastructure::` |
| UI | ui | `src/ui/` | TUI views, widgets, backend, navigation | app | no provider details | `cargo test --lib ui::` |
| Android app | android | `android/` | Java app, service, UI, tests | JNI | no Rust internals beyond JNI | `ANDROID_HOME=... ./gradlew --offline test` |

## Feature flags

| Flag | Purpose | Default |
|---|---|---|
| `youtube` | YouTube provider (rustypipe) | off |
| `android` | JNI + AudioTrack backend | off |
| `android-youtube` | android + youtube + rquickjs | off |
| `clipboard` | Desktop clipboard | on |
| `rodio` | Desktop audio backend | on |

## Contracts shared across platforms

- `PlaybackEngine` (`src/app/audio.rs`).
- Recommendation scoring (`src/recommendation/ranker.rs`).
- Android ranking JSON (`src/platform/android_rank.rs`).
- XDG dirs (`src/infrastructure/dirs.rs`).
- Provider composition (`src/api/mod.rs`).
