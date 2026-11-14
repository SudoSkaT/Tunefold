# Android

## Purpose
Android playback engine and app: JNI, Oboe/AudioTrack, foreground service, ring buffer, decoder, UI.

## Key files
- `src/platform/` (android, android_decoder, android_playback, android_download, android_rank)
- `android/` app

## Flow
`Kotlin playAudio -> JNI -> decoder thread -> PCM ring (SPSC) -> AudioTrack; analysis tap receives accepted PCM`.

## Constraints
- Ring is SPSC; never cleared from JNI while producer lives.
- Decoder is forward-only; JNI rejects seek.
- Cancel token interrupts transport.
- Generation guards callbacks.
- Ranking fail-open to provider order.
- Resolution cache is not downloaded audio.

## Tests
- `cargo test --lib --features android`
- `cd android && ./gradlew --offline test`

## Docs
- `docs/android/playback-glossary.md`
- `docs/android/visual-system.md`
- `android/README.md`

## Related systems
audio, networking, recommendations, ui
