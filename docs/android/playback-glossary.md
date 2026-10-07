# Android playback glossary

Canonical vocabulary for the Android playback, download and UI pipeline.
These names are used consistently in code, tests, logs, benchmarks and reports.

Related documents: [`../../android/README.md`](../../android/README.md) (build and
runtime host) and [`../policies.md`](../policies.md) (project policies).

## Timeline metrics

| Term | Meaning |
|---|---|
| **Play tap** | The instant the user asks to play. It is the single origin of the whole playback timeline; every event is expressed relative to it. |
| **TTFA** (Time To First Audio) | `AUDIOTRACK_FIRST_POSITIVE_WRITE − PLAY_TAP`. The primary playback metric. |
| **Decoder-relative first PCM** | First PCM accepted by the PCM ring, measured from the **decoder entry**, not from the Play tap. Kept separate on purpose so decoder cost can be read without the UI/provider prefix. |

TTFA is a **proxy**: the first positive `AudioTrack.write` is not acoustic
onset. The `AudioTrack` buffer still has to drain.

## Entities

| Term | Meaning |
|---|---|
| **Track** | The logical musical entity. Its identity is `provider + track id`, stable across resolutions and across resolved URLs. |
| **Metadata** | Stable descriptive information about a Track (title, artist/channel, duration, artwork reference). |
| **PlayableSource** | A concrete playable source: either a temporary remote URL with its HTTP headers, or a `file:` URI for an already downloaded track. |
| **Provider** | An adapter to one external source. Isolated behind the provider-neutral Catalog/StreamResolver contracts. |
| **Catalog** | Search and metadata. Answers "which tracks match this query". |
| **StreamResolver** | Resolves `Track → PlayableSource`. Returns a temporary URL; never a permanent media reference. |

## Transport

| Term | Meaning |
|---|---|
| **Range request** | One partial HTTP request (`Range: bytes=start-end`). |
| **Range window** | The byte range requested in a single Range request. |
| **RangePolicy** | Window sizes, retries and per-request timeout. Android uses the defaults: 64 KiB initial window, 512 KiB thereafter, 3 retries, 30 s timeout. |
| **HttpRangeStream** | The *logical* continuous stream built on chained Range requests. Consumers never see HTTP boundaries. |
| **Window reuse** | When a `seek` lands inside the already-downloaded window, the cursor is repositioned and no request is issued. |
| **Seek** | A byte-position change inside the logical stream. Container probes (`Probe`) are the main source of seeks. |
| **Probe** | Symphonia's initial inspection of the container. For MP4 it walks atom tables and therefore seeks heavily. |
| **Reader position** | The logical byte offset the `StreamReader` is currently consuming. Recorded per Range request. |
| **Seek-derived request** | A Range request issued as a direct consequence of a `seek`. |

## Decode and output

| Term | Meaning |
|---|---|
| **Decoder** | Turns compressed packets into PCM. Symphonia in this pipeline; not replaced. |
| **PCM** | Uncompressed f32 audio delivered to the pipeline, interleaved stereo. |
| **PCM ring** | Bounded SPSC buffer between the decoder thread and the audio output. |
| **Analysis tap** | A copy of the PCM *accepted by the ring*, feeding the analysis engine. It never receives PCM the ring rejected. |
| **AudioTrack** | The Android audio output. Written by a Java worker; never called from an audio callback. |
| **ForegroundPlaybackService** | Owner of the persistent playback: the controller, the Rust engine and the AudioTrack output. |
| **PlaybackController** | The command/state boundary on the Android side. Issues commands and exposes state; owns no durable UI state. |
| **JNI bridge** | The Java ↔ Rust frontier (`TunefoldBridge` ↔ `platform::android`). |
| **Preload** | Resolving the next track in advance. Never downloads a complete file. |
| **Buffering** | The state in which playback is waiting for data. |

## Caches

These four are deliberately **separate** and must never be conflated:

| Term | Meaning |
|---|---|
| **Metadata cache** | Stable Track information, keyed by provider id. Contains no signed URLs. |
| **Artwork cache** | Images derived from metadata. Loaded on its own worker; never blocks playback. |
| **Resolution cache** | Temporary playback URLs with their own expiry. **A resolution cache is not downloaded audio.** |
| **LocalMediaStore** | Persistent storage of audio the user explicitly downloaded. Bounded (200 MiB per item, 512 MiB total), atomic, with an identity/size sidecar. |

## Session, queue and library

| Term | Meaning |
|---|---|
| **TrackKey** | The stable logical identity of a track: `provider + provider_track_id`. The only key used for downloads, likes and history. |
| **PlaybackSession** | Service-owned source of truth for playback state, queue, library and download references. The Activity renders it and never owns it, so state survives recreation. |
| **PlaybackQueue** | Ordered list of TrackKeys with a cursor, repeat mode and the autoplay switch. Pure logic, unit-tested without Android. |
| **Current** | The queue item at the cursor. `playNow` moves the cursor and truncates what was after it. |
| **Previous rule** | Within the first seconds, Previous moves the cursor back; after that it restarts the current track and leaves the cursor alone. |
| **Autoplay** | When the queue is exhausted at end of track, one provider recommendation is fetched and played. Never downloads. |
| **PlaybackState** | `IDLE`, `RESOLVING`, `BUFFERING`, `PLAYING`, `PAUSED`, `STOPPED`, `ERROR`. Derived from the engine, never guessed by the UI. |
| **PlaybackClock** | Decides which track a reported position belongs to. Pure logic, no Android types. |
| **Generation** | Monotonic id of the current playback attempt. A track change opens a new one; callbacks capture it and drop themselves when it no longer matches. |
| **Engine ownership** | The engine's position may only be reported once output has begun for the current generation. Until then the position is 0, because the engine is still describing the previous stream. |
| **Output began** | The first positive `AudioTrack` write for a source: the earliest moment the engine can be said to be playing *that* source. `AudioTrackOutput` joins the previous worker before starting a new one, so the signal cannot arrive for a superseded source. |
| **PlaybackError** | Classified failure with stage, message and whether a retry is allowed. Shown with Retry/Skip actions. |
| **RecoveryPolicy** | Bounded budgets for retries and autoplay skips, so a failing item cannot loop forever. |
| **L1K3D** | The user's liked tracks. Membership only, persisted atomically; it is **not** the download list. Records carry the title as a trailing field so the list still reads as music after a restart instead of as bare provider ids. |
| **Descargadas** | Derived from the LocalMediaStore's valid entries. A track appears because its file exists, not because it was liked. |
| **Recommendations** | Provider "related" list, deduplicated against current track, queue, likes and recent history. A failure yields an empty list, never an error. |
| **MediaSession** | System integration for lock-screen and headset controls. |
| **Transport command** | One dispatch point, `ForegroundPlaybackService.dispatchCommand`, used by the notification intents, the system and the MediaSession callback alike. Every external control therefore reaches the same `PlaybackController`. |
| **Explicit control intent** | Notification actions name the service component. The service declares no intent-filter, so an implicit intent would resolve to nothing and the control would silently do nothing. |
| **Resolution thread** | Source resolution runs on its own single thread, separate from the provider thread that serves search and metadata, so a resolution that hangs cannot take the rest of the app with it. |

## Download

| Term | Meaning |
|---|---|
| **Download** | A complete copy of a track into the LocalMediaStore, started only by an explicit user action. |
| **Explicit download** | The only way audio becomes persistent. Never triggered by Play, metadata resolution, artwork fetch, preload or buffering. |
| **`.part` file** | The staging file a download writes before committing. A cancelled or failed download leaves no committed file. |
| **Commit** | The atomic rename from `.part` to the final file, performed only after the final size is validated. |
| **Downloaded** | A Track with a valid file in the LocalMediaStore. Played as `file:` through the same decoder and output. |
| **DownloadRegistry** | Authoritative per-TrackKey download state. One identity can hold one download, so duplicates are impossible. |
| **Resolve deadline** | 30 s budget for a download's source resolution. Without it a resolver that never answers would leave the button reading "Resolving source" forever. |
| **Store location** | `filesDir/media/local_media`, **not** the cache: an explicit download is user data, and Android may purge a cache directory at any time. |
| **DownloadState** | `IDLE`, `RESOLVING_SOURCE`, `DOWNLOADING`, `COMPLETED`, `FAILED`, `CANCELLED`, `ALREADY_DOWNLOADED`. |
| **Deduplication** | A second request for a downloaded or in-flight track is answered from state and starts no work. |
| **Store scan** | The one background enumeration of the store that seeds the registry, so the UI never walks the filesystem to render `Descargadas`. |

## Trace events

Emitted to Logcat under the `TunefoldPerf` tag, anchored to the Play tap:

| Event | Meaning |
|---|---|
| `PLAY_TAP` | User asked to play. Origin of the timeline. |
| `CONTROLLER_RECEIVED_PLAY` | `PlaybackController` received the order. |
| `METADATA_RESOLUTION_START` / `METADATA_AVAILABLE` | Metadata request start / Track available. |
| `SOURCE_RESOLUTION_START` | `Track → PlayableSource` resolution start. Carries `hit=none` or `hit=local_media`. |
| `PLAYABLE_SOURCE_AVAILABLE` | A playable source exists (`kind=http` or `kind=local_media`). |
| `HTTP_OPEN_START` | The HTTP stream is being opened. |
| `HTTP_FIRST_RESPONSE` | Headers of the first Range response arrived. |
| `RANGE_REQUEST` | One Range request completed: request id, host, offsets, requested/received bytes, status, header latency, total latency, retry, classification, reader position, `from_seek`. |
| `SYMPHONIA_PROBE_START` / `SYMPHONIA_PROBE_END` | Container probe start / end, with its duration. |
| `DECODER_FIRST_PACKET` | First compressed packet. |
| `DECODER_FIRST_PCM` | First PCM accepted by the ring. |
| `AUDIOTRACK_START` | `AudioTrack` created and playing. |
| `AUDIOTRACK_FIRST_POSITIVE_WRITE` | First positive `AudioTrack` write: the TTFA proxy. |
| `TTFA` | Emitted with the final TTFA value in milliseconds. |
| `RECOMMENDATIONS_START` / `RECOMMENDATIONS_COMPLETE` / `RECOMMENDATIONS_END` | Recommendation request start, provider answer, and candidates kept after deduplication. |
| `AUTOPLAY_TRIGGER` | End of track with an empty queue and autoplay enabled. |
| `AUTOPLAY_GAVE_UP` / `AUTOPLAY_NO_CANDIDATE` | Autoplay stopped instead of looping. |
| `QUEUE_PREVIOUS` | Previous action; `restart` when it restarted the current track instead of moving the cursor. |
| `RESOLVE_ENTER` / `RESOLVE_EXIT` | see Download |
| `DOWNLOAD_START` / `DOWNLOAD_CANCELLED` | Explicit download started / cancelled by the user. |
| `DOWNLOAD_COMPLETE` | The transfer committed; carries the byte count. |
| `DOWNLOAD_RESOLVE_TIMEOUT` | Source resolution exceeded its deadline and the download failed visibly instead of hanging. |
| `DOWNLOAD_SUPERSEDED` | The attempt was cancelled or failed while resolving, so the resolved source was discarded. |
| `RESOLVE_ENTER` / `RESOLVE_EXIT` | Entry and exit of one source resolution, with its duration. Present so a resolution that never returns is distinguishable from one never requested. |
| `DOWNLOAD_ALREADY_EXISTS` / `DOWNLOAD_ALREADY_RUNNING` | A duplicate download request was refused. |
| `DOWNLOAD_CLEAN_PARTS` | Abandoned `.part` files removed at construction. |

Never logged: signed stream URLs, request headers, or full resource paths of
remote sources.

## UI states

`Idle`, `Searching`, `Resolving metadata`, `Resolving source`, `Buffering`,
`Playing`, `Paused`, `Downloading`, `Downloaded`, `Error`.

Technical diagnostics never appear in the normal Now Playing surface; they live
in the `Diagnostics` panel (`PlaybackDebugPanel`).

Every control label states the **action** it will perform, not the current state:
a playing track offers `Pause`, and the autoplay button reads `Autoplay: on`.

## Validation hooks

Scripted device validation only; never used by the UI. They call the same
public commands the buttons call, so there is no second code path.

| Hook | Purpose |
|---|---|
| `runtime_smoke_test` | Play the URL given in `runtime_stream_url` on launch. |
| `runtime_stream_url` | The track or direct audio URL to use. |
| `runtime_download` | Drive the download flows for `runtime_track_label` without screen taps. |
| `runtime_download_action` | `start`, `cancel`, `retry`, `like` or `remove`. |
| `runtime_track_label` | Track id used as the identity for the download hook. |

Example:

```sh
adb shell am start -n com.tunefold.app/.MainActivity \
  --ez runtime_smoke_test true \
  --es runtime_stream_url https://www.youtube.com/watch?v=dQw4w9WgXcQ

adb shell am start -n com.tunefold.app/.MainActivity \
  --ez runtime_download true --es runtime_download_action start \
  --es runtime_track_label dQw4w9WgXcQ
```

## Benchmarking

`tools/measure_playback.sh <cold|warm> <runs>` replays the reference track and
reports min / median / p95 / max for TTFA and each stage.

Some Honor/Huawei builds filter `Log.i` per tag, so app output never reaches
logcat and every run looks like a silent hang. The script sets
`log.tag.TunefoldPerf=I` first; if a trace is empty, check that before
suspecting a playback problem.