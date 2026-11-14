# Networking

## Purpose
HTTP transport and stream resolution: Range requests, buffering, retries, cancellation.

## Key files
- `src/media/transport/mod.rs`, `integration.rs`, `fake_server.rs`
- `src/media/stream.rs`, `resolver.rs`, `cache.rs`, `expiration.rs`

## Invariants
- `total` only 0 -> value or fail.
- `200` valid only full body from byte 0.
- `206` must have coherent Content-Range and body size.
- Cancel token interrupts open, next_chunk, retries, body.
- No blind retries on 403/416/200-forced/bad Content-Range.

## Tests
- `cargo test --lib media::transport`
- `cargo test --test audio_decode`

## Docs
- `docs/testing/audio-validation.md`
- `README.md` (streaming limits)

## Related systems
audio, playback, providers
