# DSP

## Purpose
Real-time DSP chain for desktop playback (preamp, EQ, stereo, normalize, limiter).

## Key files
- `src/dsp/params.rs`, `control.rs`, `processor.rs`, `chain.rs`
- `src/dsp/rodio_source.rs`
- `src/dsp/gain.rs`, `biquad.rs`, `stereo.rs`, `normalize.rs`, `limiter.rs`, `smooth.rs`

## Contract
- `DspParams` is `Copy`; no Vec/String.
- `DspControl` uses try_lock; audio never waits.
- Chain refreshes params per block; Flat early return bit-exact.
- `prepare` on format change; `reset` on seek/discontinuity.
- Block size 256 frames; limiter lookahead ~5 ms.

## Tests
- `cargo test --lib dsp::`
- `cargo run --release --example bench_dsp`

## Docs
- `docs/architecture/dsp-pipeline.md`

## Known limits
- No UI/persistence yet (Fase 3 pending).
- Android not on DSP chain.
- No measured loudness/peaks for normalization when absent.
- Limiter no queue flush.

## Related systems
audio, analysis
