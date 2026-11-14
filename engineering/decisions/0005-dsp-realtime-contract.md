# 0005 — DSP real-time contract

Status: accepted

## Context
Audio thread must not block; DSP must be sample-accurate enough without allocations.

## Decision
Processor composition and buffers are built on control thread before append. Audio thread only copies `Copy` params via `try_lock` and processes preallocated blocks in place. Flat path returns early bit-exact. No lock(), await, I/O, SQLite, network in audio path.

## Alternatives
- Lock-based param updates in audio: rejected; risk of blocking.
- Per-sample allocation: rejected; hot path budget.

## Consequences
- `DspControl` uses versioned `Arc<Mutex<Published>>`.
- Block size 256 frames; limiter lookahead.
- Android DSP not yet implemented.

## References
- `docs/architecture/dsp-pipeline.md`
- `src/dsp/`
