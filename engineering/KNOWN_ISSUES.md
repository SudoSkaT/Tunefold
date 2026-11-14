# Known issues and implementation status

Status values: reproduced, fixed, not-reproduced, known-limitation, pending-validation, pending-instrumentation.

## Known limitations

- DSP Fase 3 (UI controls, persistence, loudness/picks analysis) not implemented (`docs/architecture/dsp-pipeline.md`).
- Android has not been validated on hardware; TTFA/AudioTrack stability pending (`docs/roadmap/audio-stabilization.md`).
- `external_id` empty origin not instrumented yet.
- Commit lock covers persistence; slow SQLite can delay new orders (deliberate consistency decision).
- AAC SBR/PS support undecided; fixtures absent.
- Android does not use desktop DSP chain yet.
- Limiter has no queue flush: up to ~5 ms lost at track end.
- Normalization lacks measured loudness/peaks when source data missing.
- Visualizer remains pre-DSP.
- Recommendation acoustic axis dormant on Android v1 (contract ready).

## Fixed (evidence in code/tests)

- HTTP range cancellation and classification.
- Generation/epoch guards for playback.
- PCE AAC decoding.
- Android token cancellation and bounded decode errors.

## Measurement limitations

- Real token counts are not available in this environment; bytes/files/lines/searches are recorded instead.
- Android hardware metrics unavailable without device.
- Timing depends on machine; compare same hardware/build.

## Historical reports

`docs/audit/*`, `docs/roadmap/*`, `android/*_REPORT.md` are historical and must not be read as current implementation state.
