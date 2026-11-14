# Audio (desktop)

## Purpose
Desktop playback engine: decoding, buffering, monitor, cancellation, DSP integration.

## Key files
- `src/infrastructure/playback/` (mod, output, rodio_backend)
- `src/app/audio.rs` (`PlaybackEngine` contract)
- `src/app/playback.rs`
- `src/media/transport/`

## Flow
`HTTP Range -> StreamBuffer -> Decoder -> TapSource (pre-DSP analysis) -> DspSource -> ProcessorChain -> rodio player`.

## Constraints
- Audio thread never blocks on network/disk/locks.
- Generation/epoch guards prevent stale events.
- Transport cancellation and generation are separate mechanisms.
- Buffering policy adaptive by `bps_ema`.
- EOF vs Cut are distinct.

## Tests
- `cargo test --lib infrastructure::playback`
- `cargo test --lib media::transport`
- `cargo test --test audio_decode`

## Docs
- `docs/architecture/audio-engine.md`
- `docs/architecture/playback-lifecycle.md`
- `docs/testing/audio-validation.md`

## Related systems
dsp, playback, networking, analysis
