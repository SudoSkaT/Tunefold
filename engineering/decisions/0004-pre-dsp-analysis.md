# 0004 — Analysis remains pre-DSP

Status: accepted

## Context
DSP modifies output sound; recommendations/visualization need stable acoustic features.

## Decision
Analysis tap is before DSP (`TapSource` pre-DSP). Features feed recommendations/visualization and must not change when EQ moves. A future optional post-DSP tap may be added, but no second analysis engine.

## Alternatives
- Analyze post-DSP: rejected; user EQ would alter recommendations.
- Two engines: rejected; complexity and inconsistency.

## Consequences
- `docs/architecture/dsp-pipeline.md` documents pre-DSP tap.
- Android analysis tap receives PCM accepted by ring.

## References
- `docs/architecture/dsp-pipeline.md`
- `src/analysis/tap.rs`
