# 0003 — Stateless Android ranking contract

Status: accepted

## Context
Android and desktop must share recommendation scoring without diverging.

## Decision
Desktop ranker is exposed to Android via JNI as `TunefoldBridge.rankRelated` with stateless JSON request/response (indices over input array). Android owns data; `.so` keeps no state. Fail-open to provider order on any failure/non-permutation.

## Alternatives
- Duplicate scoring in Kotlin: rejected; divergence risk.
- Stateful native cache: rejected; lifecycle/ownership complexity.

## Consequences
- Contract tested both sides.
- Android v1 sends meta/history/signals only; acoustic axis dormant but contract-ready.

## References
- `src/platform/android_rank.rs`
- `docs/recommendations.md`
- `docs/android/playback-glossary.md`
