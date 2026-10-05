# Tunefold Android runtime host

This is a minimal test host for the existing Rust Android playback engine. It
does not provide a production player UI or Android background playback
service.

## Build

Requirements: JDK 17, Android SDK platform 37, Android build tools 36.0.0,
Android NDK 28.2.13676358, and the Rust targets `aarch64-linux-android` and
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

## Runtime smoke test

Install the APK on an Android device or emulator, launch **Tunefold Android
Runtime Test**, and press **Play URL**. The URL field starts with a public CC0
Ogg Vorbis sample; it can be replaced with an HTTP(S) URL to another supported
stream. The screen reports buffering, playing, pause, stop, decoder/output
errors, and the analysis waveform peak after analysis publishes a snapshot.
**Pause / Resume** and **Stop** exercise the JNI controls. For scripted device
validation, launch with `--ez runtime_smoke_test true --es runtime_stream_url
https://samplelib.com/wav/sample-15s.wav`. Closing the Activity stops and
releases the engine; playback is not expected to continue after Activity
destruction in this baseline.

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
