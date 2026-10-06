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

Install the APK on an Android device or emulator, launch **Tunefold Android
Runtime Test**, and enter a YouTube URL or search query. Direct HTTP(S) audio
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

## Playback timing and cache

The Android host writes per-play monotonic events to Logcat under
`TunefoldPerf`: the Play tap, controller receipt, metadata/source readiness,
engine start, AudioTrack start, and first positive AudioTrack write. Decoder
diagnostics report HTTP open and first-response-header durations, Symphonia
probe time, first decoded packet, first PCM ring write, range request count,
accepted range bytes, and accumulated request time. The first positive output
write is the reproducible TTFA proxy; it does not measure acoustic onset.

Cache ownership is separate by data type:

- Rustypipe's own provider cache stays below `cacheDir/rustypipe`.
- Tunefold track metadata is stored separately by provider ID for seven days,
  with a 512-entry limit. Signed stream URLs and request headers are excluded.
- Artwork is fetched from the metadata reference on a separate worker, validated
  as an image response, capped at 10 MiB per item and 128 MiB total, then shown
  after decoding off the UI thread.
- Resolved source URIs remain in the provider/resolver memory caches with their
  existing 20-minute expiry and live validation; they are not persisted as
  permanent media references.
- `LocalMediaStore` is a separate explicit-save API with atomic files, sidecar
  identity/size/timestamp checks, a 200 MiB per-item limit, and a 512 MiB total
  limit. Normal Play never fills it automatically. A cache hit uses a `file:`
  source handled by the same Symphonia, PCM ring, analysis tap, and AudioTrack
  path; a miss streams from the provider.

No audio file is downloaded before playback. The store accepts bytes only when
an explicit caller supplies them; this Android runtime host currently exposes
the storage hook to the provider layer but has no download/offline UI.
