# Dependencies (Tunefold)

Major crates and where they are used. Source of truth: `Cargo.toml`, `Cargo.lock`.

| Crate | Purpose | Used in |
|---|---|---|
| tokio | async runtime | media, app, providers |
| reqwest | HTTP client (rustls) | media/transport, providers |
| rodio | desktop audio output | infrastructure/playback |
| symphonia | decoding | infrastructure/playback, media, tests |
| sqlx | SQLite + migrations | infrastructure |
| serde / serde_json | serialization | domain, media, platform JNI |
| thiserror / anyhow | errors | all |
| tracing | logging | all |
| rustfft | FFT | analysis |
| ratatui / crossterm | TUI | ui |
| image | artwork | ui, app |
| arboard | clipboard (optional) | ui |
| jni | JNI bridge (optional) | platform |
| rustypipe | YouTube (optional, vendored) | providers/youtube only |
| rquickjs-core | QuickJS bindings (optional) | providers/youtube android |
| chrono | time | domain, infrastructure |
| unicode-normalization | text normalization | providers/search |
| sha2 | hashing | media/cache |
| futures-util | stream utilities | media |
| dotenvy | .env loading | infrastructure/config |

## Rules
- No new dependency without justification; prefer small, maintained, rustls-based crates.
- GPL crates only for vendored rustypipe.
- MIT/Apache-2.0/MPL allowed; AGPL/proprietary/unlicensed rejected.
- Update `THIRD_PARTY.md` and `NOTICE` on new dependency.

## Feature gating
- `youtube`: rustypipe
- `android`: jni, symphonia
- `android-youtube`: above + rquickjs-core
- `clipboard`: arboard
- `rodio`: rodio
