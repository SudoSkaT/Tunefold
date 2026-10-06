# Tunefold Android runtime host

This is a minimal test host for the Rust Android playback engine. Playback is
owned by `ForegroundPlaybackService`; `MainActivity` binds to its
`PlaybackController` and does not own engine or output cleanup.

## Build

Requirements: JDK 17, Android SDK platform 37, Android build tools 36.0.0,
Android NDK 28.2.13676358 (or an explicit local `TUNEFOLD_NDK_ROOT`), libclang
for optional QuickJS bindings, and the Rust targets `aarch64-linux-android` and
`x86_64-linux-android`.

Set `JAVA_HOME`, `ANDROID_HOME` (or `ANDROID_SDK_ROOT`) and ensure the SDK
contains the listed platform/build-tools/NDK packages, then run from the
repository root:

```sh
cd android
./gradlew assembleDebug
```

The APK is `android/app/build/outputs/apk/debug/app-debug.apk`. Gradle builds
the Rust `android` feature for ARM64 and x86_64 and packages
`libtunefold.so` into the corresponding APK ABI directories.

### Optional YouTube provider

Custom builds can opt in explicitly with:

```sh
cd android
./gradlew -PtunefoldYoutube=true assembleDebug
```

This selects Rust's `android-youtube` feature. The normal Android build does
not compile the provider. The Android UI calls the provider-neutral Rust
catalog and stream resolver asynchronously, then hands its `Track` and remote
URI plus HTTP context headers to the existing playback controller. YouTube
resolution is supplied by the repository's optional `rustypipe` integration,
which uses YouTube's nonofficial Innertube/player interfaces and direct
Googlevideo audio streams. This is not the YouTube Data API; it may fail when
those interfaces change and remains excluded from official binaries under the
repository's existing YouTube terms restrictions.

## Runtime smoke test

Install the APK on an Android device or emulator and launch **Tunefold**. Enter
a YouTube URL or a search query. Direct HTTP(S) audio
URLs remain supported for decoder smoke tests. The screen reports buffering,
playing, pause, stop, provider metadata, decoder/output errors, and the analysis
waveform peak after analysis publishes a snapshot.
**Pause / Resume** and **Stop** exercise the JNI controls. For scripted device
validation, launch with `--ez runtime_smoke_test true --es runtime_stream_url
https://samplelib.com/wav/sample-15s.wav`. Closing the Activity stops and
releases only its binding; the started foreground service keeps playback alive.
The service posts a minimal Tunefold playback notification and releases its
engine/output when playback stops or reaches EOF. Activity recreation can bind
again to observe the current controller state.

The output uses Android `AudioTrack` in float PCM mode. Rust normalizes decoded
audio to interleaved stereo `f32`, duplicates mono input, keeps the first two
channels for multichannel input, and publishes the decoded source sample rate.
`AudioTrack` is opened at that exact rate. The decoder pushes accepted frames
to the bounded SPSC PCM ring and the analysis tap receives those same accepted
frames. Decoding and network I/O run on the Rust decoder thread; AudioTrack
drains PCM from a Java output worker and never invokes the decoder in an audio
callback.

Android's audio system may convert the stream to the physical device rate. A
successful build or app launch does not demonstrate audible device output;
verify playback on a device/emulator with a functioning audio backend.

## Interface

The screen is organised around real playback states, in this order: artwork,
title/artist, playback state, progress, transport controls, secondary actions,
then search results. Technical diagnostics are **not** part of the normal
surface: the `Diagnostics` button toggles `PlaybackDebugPanel`, which shows the
decoder counters and the ordered playback timeline.

Layout is responsive: the cover is sized from the current window (never a fixed
dimension), portrait stacks it above the details, landscape puts it beside them,
and the page is scrollable so a short window never collapses a control. All
dimensions are expressed in `dp`/`sp` and interactive controls are at least
48dp high.

States surfaced by the UI: Idle, Searching, Resolving metadata, Resolving
source, Buffering, Playing, Paused, Downloading, Downloaded, Error.

## Playback timing and cache

Each play writes an ordered, per-play timeline to Logcat under the `TunefoldPerf`
tag. **Every event is anchored to the Play tap**, so one run can be
reconstructed end to end:

```text
PLAY_TAP @0ms
CONTROLLER_RECEIVED_PLAY @13ms
METADATA_RESOLUTION_START @26ms
METADATA_AVAILABLE @826ms
SOURCE_RESOLUTION_START hit=none @850ms
PLAYABLE_SOURCE_AVAILABLE kind=http @1733ms
HTTP_OPEN_START @1740ms
HTTP_FIRST_RESPONSE req=1 range=0-65535 status=206 headers_us=29809 @1770ms
RANGE_REQUEST req=1 host=… range=0-65535 requested=65536 received=65536 status=206
            headers_us=29809 total_us=41072 retry=0 class=ok reader_pos=0 from_seek=false @1781ms
SYMPHONIA_PROBE_START @1781ms
RANGE_REQUEST req=2 … from_seek=true @1809ms      (seeks reuse the window: no request)
SYMPHONIA_PROBE_END probe_us=243000 @2024ms
DECODER_FIRST_PACKET bytes=371 ts=0 @2027ms
DECODER_FIRST_PCM first_pcm_us=248000 frames=1024 @2027ms
AUDIOTRACK_START sample_rate=44100 @2090ms
AUDIOTRACK_FIRST_POSITIVE_WRITE frames=8192 @2092ms
TTFA 2092ms
```

* `TTFA` = first positive `AudioTrack` write − Play tap. It is the reproducible
  proxy for audible output; it does **not** measure acoustic onset.
* `first_pcm_us` keeps its original meaning: first PCM accepted by the ring,
  relative to **decoder entry**. Both are reported so decoder cost can be read
  without the UI/provider prefix.
* `RANGE_REQUEST` records one request: id, host, offsets, requested/received
  bytes, HTTP status, header latency, total latency, retry number, result
  classification, reader position and whether a `seek` caused it. Signed URLs
  and request headers are never logged.

Honour/Huawei builds filter `Log.i` per tag by default, which makes the app look
silently hung. Enable it once before measuring:

```sh
adb shell setprop log.tag.TunefoldPerf I
```

`tools/measure_playback.sh <cold|warm> <runs>` replays the reference track and
reports min/median/p95/max for TTFA and every stage.

Cache ownership stays separate by data type:

- Rustypipe's own provider cache stays below `cacheDir/rustypipe`.
- Tunefold track metadata is stored separately by provider ID for seven days,
  with a 512-entry limit. Signed stream URLs and request headers are excluded.
- Artwork is fetched from the metadata reference on a separate worker, validated
  as an image response, capped at 10 MiB per item and 128 MiB total, then shown
  after decoding off the UI thread.
- Resolved source URIs remain in the provider/resolver memory caches with their
  existing 20-minute expiry and live validation; they are not persisted as
  permanent media references.
- `LocalMediaStore` is the persistent store of audio the user explicitly
  downloaded. Atomic files, sidecar identity/size/timestamp checks, a 200 MiB
  per-item limit and a 512 MiB total limit. Normal Play never fills it.

## Explicit download

A download is a capability of its own, never a side effect of playback. It never
runs during Play, metadata resolution, artwork fetch, preload or buffering; it
starts only from the **Download** action.

```text
MediaTrack → resolve PlayableSource → HTTP Range (full) → .part
           → validate final size → atomic commit → LocalMediaStore → file:
```

* It reuses the existing Range transport; there is no second HTTP stack and no
  second player. A downloaded track is played as `file:` through the **same**
  Symphonia → PCM ring → analysis tap → AudioTrack path.
* Progress, cancellation and byte counts are surfaced in the UI
  (`Downloading 34% · 42.1 MB / 123.8 MB`).
* Cancellation removes the `.part` file; no half-written file is ever committed.
* Identity is `provider + track id`. The resolved URL is never used as the file
  name or persisted as media identity.
* Abandoned `.part` files from interrupted downloads are cleaned when the store
  is created, not during playback.
* `resolution cache != downloaded audio`: a resolved temporary URL is never
  converted into permanent storage.

Scripted validation:

```sh
adb shell am start -n com.tunefold.app/.MainActivity \
  --ez runtime_smoke_test true --ez runtime_download true \
  --es runtime_stream_url https://www.youtube.com/watch?v=dQw4w9WgXcQ
```
