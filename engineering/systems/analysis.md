# Analysis

## Purpose
Acoustic analysis: FFT, bands, chroma, features, tap, ring, aggregation.

## Key files
- `src/analysis/` (mod, engine, fft, bands, chroma, features, tap, ring, beat, onset, rms, smoother, waveform, test_support)
- `src/recommendation/acoustic_aggregator.rs`, `profile.rs`

## Constraints
- Analysis is pre-DSP; features must not change with EQ.
- Dedicated thread; drop-newest when lagging.
- Audio thread never blocked.
- Candidate pre-analysis uses HTTP Range without opening audio device.

## Tests
- `cargo test --lib analysis::`
- `cargo test --lib recommendation::`

## Docs
- `docs/architecture/dsp-pipeline.md`
- `docs/recommendations.md`

## Related systems
dsp, visualization, recommendations
