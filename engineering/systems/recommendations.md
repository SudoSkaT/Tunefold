# Recommendations

## Purpose
Shared ranking engine and signals for desktop and Android.

## Key files
- `src/recommendation/` (ranker, signals, types, session, scoring/*, acoustic_aggregator, profile, greeting)
- `src/platform/android_rank.rs`

## Model
Weights sum 1.0: meta .10, acoustic .30, harmonic .20, structure .10, affinity .20, recency .05, popularity .05.
Impressions never score; rec_click collapses with play.

## Contracts
- Android: stateless JSON `{"candidates",...}` -> `{"ok":true,"order":[indices]}`.
- Fail-open to provider order on any failure/non-permutation.
- Acoustic axis dormant on Android v1 (contract ready).

## Tests
- `cargo test --lib recommendation::`
- `cargo test --lib --features android platform::android_rank`

## Docs
- `docs/recommendations.md`

## Related systems
analysis, providers, persistence, android
