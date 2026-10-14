# Tunefold Android runtime host

Phase 2 (Now Playing, artwork, live sound bars) is documented in
[`NOW_PLAYING_REPORT.md`](NOW_PLAYING_REPORT.md), **Phase 4 (navigation,
Home, Search, library and lists)** in [`NAVIGATION_REPORT.md`](NAVIGATION_REPORT.md)
and **Phase 5 (responsive layout, mini player, adaptive composition)** in
[`LAYOUT_REPORT.md`](LAYOUT_REPORT.md): what was measured, what is a bound, and what
still needs a device.

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
playing, pause, provider metadata and decoder/output errors, and draws the
stereo level of the audio **as it is heard**: two lanes, one per channel, in the
colours taken from the cover. The bars move with the sound and freeze when the
sound stops; the thin line under them is the session's position.
The transport controls (previous, play/pause, next) and the actions below them
(like, autoplay, download) exercise the JNI controls; they are icon-only, so
their meaning is carried by `contentDescription` and by the long-press tooltip. For scripted device validation, launch with
`--ez runtime_smoke_test true --es runtime_stream_url
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

## Validating Now Playing on a device

```sh
adb shell setprop log.tag.TunefoldPerf I
adb shell am start -n com.tunefold.app/.MainActivity
```

Open the diagnostics panel (pulse icon in the top bar). Its **first** line belongs
to this surface:

```text
waveform cols=160/320 builds=7 draws=91 ticks=45 reads=12 changes=7 covered_ms=91000 gain_x=1.74 last_refresh_us=38 seek=off
```

* `builds` must equal `changes` and **must not move when only the position moves**
  — that is the geometry cache. (Measured on a device: `builds == changes == 446` in
  a 55-second window; while paused, all three stay at 446 for 21 s while `ticks`
  grows by 105.)
* `reads` grows at most 2.5 per second while audio plays and **not at all while
  paused** — a position that has not moved means no audio was decoded.
* `draws` tracks `builds` plus the playhead crossing a pixel, nothing else;
* `covered_ms` must agree with the elapsed label: it is the audio actually measured
  by the engine, so it is the honest check that the waveform belongs to this track;
* `seek=off` is expected on the streaming pipeline; see *Waveform, progress and
  seek* above.

What to exercise: a very short track (the shape must complete and the last bar
must reach the playhead), a long one, rapid Next (the shape must empty rather than
show the previous song), Previous (restart), Pause/Resume (nothing moves),
buffering (the shape survives, the state dot shows three segments), rotation (the
shape is rebuilt, not re-decoded), autoplay, and a download while playing (the
download control's arc moves; the status line keeps saying "Playing").

## Validating the layout on a device

Two scripts walk the surfaces and leave evidence, instead of asking a reviewer to
look at six screens and remember what looked wrong:

```sh
tools/measure_layout.sh <label>   # startup, idle frames, navigation, rotation, memory
tools/validate_layout.sh <dir>    # 4 windows x 2 orientations, screenshots + audit
tools/validate_states.sh <dir>    # rotation/navigation while playing, buffering, downloading
```

`validate_layout.sh` emulates the windows it cannot hold (`wm size` + `wm density`):
a large phone, a small phone, a tablet and a large-screen tablet, each in both
orientations, capturing the list, the mini player and the full player. For every one
of them it audits the accessibility tree against the window and reports the nodes
that fall outside it or that are drawn with no area — which is how "the transport is
off the bottom of a landscape phone" becomes a line of output instead of a memory.

`validate_states.sh` covers what a screenshot cannot: rotation during playback,
during a resolve and during a download; navigation during playback; and the player
across a rotation. It asserts the Activity is never recreated, nothing lands in the
crash buffer, the mini player survives, and the transport stays reachable in both
orientations.

Each configuration change logs one line:

```text
I Tunefold: window 1080x2232 @3.0 -> 360x744/360/0P gutter=20dp column=full cover=360dp space
```

That is the window the app read and the rules it derived from it — the difference
between a layout that "looked wrong on that tablet" and a number.

## Interface

Now Playing is the primary surface, and its hierarchy is fixed: **artwork,
title, artist/channel, waveform & progress, play/pause, previous/next, secondary
actions.** Nothing in that order is a style preference — it is the order the eye
is meant to travel, and the token tests assert the parts of it that have numbers
(the cover's share of the width, the touch targets, the seek step).

Technical diagnostics are **not** part of the normal surface: the pulse icon in
the top bar toggles `PlaybackDebugPanel`, which shows the decoder counters, the
ordered playback timeline and the waveform's own counters.

### Artwork

The cover is the loudest object on the screen and is sized from the current
window, never from a fixed dimension:

* **portrait** — `ARTWORK_PORTRAIT_WIDTH_RATIO` (84%, inside the required
  78–88% band) of the width available to the card, capped at
  `ARTWORK_PORTRAIT_HEIGHT_RATIO` (42%) of the window height so a very tall phone
  cannot push every control below the fold;
* **landscape** — the cover sits beside the metadata and controls, at 42% of the
  width and at most 88% of the height, and it always yields at least
  `LayoutRules.DETAILS_MIN_DP` to the region beside it: the cover is computed
  first, so without that floor it could take the whole column and leave the
  transport nowhere to live.

Both numbers come out of `LayoutRules.artworkSideDp`, which also keeps the cover
inside `[ARTWORK_MIN, artworkMaxDp]` on every screen, and the whole column inside
`LayoutRules.CONTENT_MAX_DP` once the window is a tablet's.

Covers are decoded **at the size they are drawn at** (capped at
`ARTWORK_DECODE_MAX_PX`, 640 px ≈ 1.6 MB as ARGB_8888), on the artwork worker,
with an `inSampleSize` chosen from the file's real bounds so no full-size bitmap
is ever allocated and thrown away. The two most recent decodes are kept in memory
(`ArtworkCache`), so a rotation or an Activity recreation re-paints a cover
without touching the disk or the decoder again. Nothing is recycled: the view may
still be drawing the bitmap it was handed.

### Live bars, progress and seek

The bars are the stereo level of the audio **as it is being heard**: two lanes
(left above, right below), each bar the minimum and maximum of its slice of the
last measured window, each lane in its own colour taken from the cover. They are
not a picture of the song: there is no time axis, no played/remaining split and
nothing to scroll. Progress is a separate, thinner line below them, because a
thing that changes has no business wearing the same shape as a thing that
measures the music.

The data comes from Rust and is already there: the analysis thread publishes one
stereo window per hop (46 ms) on the `WaveformBus`. `read_live_envelope` copies
the last one into `4 × bars` floats and returns the bus **generation**. That
number is the whole budget: while it does not move there is no new audio, and the
view does not invalidate, does not allocate and does not draw. A paused track
therefore costs exactly one paint (the silence that arrived when it stopped) and
nothing after that.

Three honesty rules the render is built around:

* a bar is the amplitude that was measured — no auto-gain, no smoothing and no
  remembered past, so a quiet track looks quiet (`a_full_scale_tone_fills_a_bar_and_a_quiet_one_does_not`);
* the same value drives both lanes' scale, so the channels can be compared;
* a track with **no known duration** gets no progress fill and no invented total:
  the line shows the position as a dot and the total stays `0:00`.

**The clock is restarted per track, not per format.** The analysis clock is
`hops × hop`: two consecutive tracks from the same provider announce the same
`StreamMeta`, so neither a stream gap nor a format change resets it, and the second
track is measured as if it started where the first one ended. `PcmTap` carries an
epoch that `playStream` bumps, and the analysis thread restarts its windows,
analysers and clock when it sees a new one
(`a_new_source_restarts_the_clock_even_with_the_same_format`). Without it every
consumer that draws against that axis is off by the length of the previous track.

**Seek.** The gesture layer is complete — drag to scrub, tap to seek, and
accessibility scroll actions that move the playhead 15 s at a time — and it is
gated on `PlaybackController.supportsSeek()`, which is **false today**. The
Android pipeline counts frames it has decoded; there is no decoder-level seek, and
`seekAudio` only clears the PCM ring and writes a number into that counter, which
would move the *display* to a place the audio never went and offset every reported
position afterwards. Until the engine can seek, the waveform shows progress and
reaches the session only when a seek is actually possible; flipping that one flag
is all it takes to make it live.

### States

Every state is a shape **and** a word, never a word alone: a filled dot in the
accent for playing, three segments that advance on the UI tick for anything being
waited for, a hollow dot for paused, a dot inside a ring for a failure, and a
determinate arc for a running download. `PlaybackStatus` owns the precedence
(failure → work in progress → playback → transfer) and is a unit test, because
"Buffering" and "Downloading" fighting over one label is a bug nobody notices
until both are wrong.

States surfaced by the UI: Idle, Searching, Resolving metadata, Resolving
source, Buffering, Playing, Paused, Stopped, Downloading, Downloaded, Error.

### Colour comes from the artwork

The chain ARTWORK → COLOUR → BARS → PLAYBACK is now closed.
`ArtworkPalette` reads a 32×32 downsample of the cover on the artwork worker and
returns **two** colours, because the bars are stereo: the dominant hue, and the
second one if the cover has a distinct second hue (two tones closer than 25° are
one tone, and the control says so instead of pretending). The rules are unit
tests rather than taste:

* neutral, very dark, very bright and transparent pixels do not vote — a white
  cover is a highlight, not a colour;
* the winner's saturation is **restored** after averaging (averaging colours of the
  same hue is what turns them to dust) and never lands below 0.45;
* **no blending towards the brand.** The first version tinted the result 55% to
  stay "on brand", and on a device three different covers came out as three
  almost identical dusty roses: the extraction worked and was invisible;
* the bar colours are then lifted in *value* — not mixed with white, which would
  take the croma back — until they clear 3:1 against the card, and the control
  colour is lifted until it clears 4.5:1 against the content drawn on the play
  disc;
* a cover with no colour to give (grey, black, white, empty) leaves the brand
  mint exactly where it was, and `ArtworkTheme.setPalette` is a no-op when nothing
  moved.

### UI layer

Every dimension, colour, glyph and component in the surface comes from one place.

| Concern | Owner |
| --- | --- |
| Dimensions, type scale, motion, colour roles | `DesignTokens` |
| Glyph geometry | `Icon`, parsed once by `IconPath` |
| Glyph rendering | `IconDrawable` |
| Accent colour, and who repaints when it moves | `ArtworkTheme` |
| The rule that turns a cover into those colours | `ArtworkPalette` |
| States: shape, colour, precedence, wording | `PlaybackStatus`, `StateDotView` |
| Progress arithmetic and the redraw gate | `ProgressScale` |
| Live bar data: one read, one buffer, one generation | `LiveAudioBars` |
| Bar rendering: two lanes, two colours, no work in `onDraw` | `SoundBarsView`, `ProgressControl` |
| The position line and its head | `ProgressLineView` |
| Controls | `IconButton`, `PrimaryPlaybackButton`, `SecondaryActionButton`, `NavigationItem`, `SectionHeader`, `ArtworkView`, `TrackRow`, `TrackList` |

`DesignTokens` holds raw `dp`/`sp` constants with no Android types, so the scale
itself is asserted in `DesignTokensTest` — including the 48dp accessibility floor,
which is a build failure rather than a review comment.

**Iconography.** Actions are icons, not labels. Two sub-families with one rule
each: transport glyphs (`PLAY`, `PAUSE`, `PREVIOUS`, `NEXT`, `STOP`) are filled
solids because they are the loudest thing on screen and must read at 20dp; every
other glyph is a stroked outline at exactly 2 grid units with round caps and
joins, so a mixed-weight set cannot happen. A glyph is authored on a 24-unit grid
in a restricted command dialect (`M`/`L`/`C`/`Q`/`Z`), parsed by `IconPath` into
plain arrays — which is what lets `IconFamilyTest` reject an empty or out-of-grid
outline as a failing build instead of a dead button found on a device. There are
no Unicode glyphs and no per-icon drawable resources.

Accessibility is structural rather than reviewed: `setAction(icon, description)`
sets both in one call, the touch target comes from a token and cannot be set
below 48dp, and a row exposes one description node naming the track and what both
of its actions will do.

**Rendering.** `NowPlayingView` has one entry point per kind of change —
`applyTrack`, `applyPlaybackState`, `applyLibraryState`, `applyDownloadState`,
`applyError`, `setPosition` — and each compares against what it last rendered
before it writes. A position tick reads through `PlaybackSession.readPosition`
into a reused `PlaybackPosition` (no snapshot, no queue copy, no allocation),
`ProgressControl` only calls `setText` when the displayed second changed, and
`SoundBarsView` only calls `invalidate` when the generation moved — that is, when
there was audio it had not drawn. A tick that cannot change anything visible costs
a handful of integer comparisons.

**Cadence.** Three clocks, deliberately. The UI tick runs at
`POSITION_INTERVAL_MS` (200 ms) and feeds the playhead, the elapsed label and the
working indicator; the visual tick runs at `VISUAL_INTERVAL_MS` (50 ms) and exists
only for the bars, which have to answer to the sound; the engine refresh that
notifies every observer runs at `SESSION_REFRESH_INTERVAL_MS` (500 ms), because it
takes a full session snapshot and re-renders the whole surface. The bars are not
read at all unless the engine is engaged with the current track.

**Where the counters live.** `SoundBarsView` counts repaints, the last draw in
microseconds and the highest amplitude it painted; `LiveAudioBars` counts reads
and the reads that brought new audio. They are printed by the diagnostics panel,
which is what makes "the bars repaint 20 times a second" and "the bars cost
nothing while paused" measurements instead of claims.

**Lists.** `TrackList` does not build the list; it builds the **window**. It keeps
the tracks, reports its height from their count, and owns only the rows that can be
seen plus `LIST_PREFETCH_ROWS` above and below, recycled through a shared `RowPool`.
`ScrollHost` exists because `ScrollView` cannot be used here: it measures its child
unbounded, which forces every row of every list to be laid out at once. The pool is
owned by the screen, not the process: a static pool would keep a whole Activity
reachable through a parked row.

**Navigation.** Home, Search, L1K3D and Descargadas are four `SectionSurface`
views behind a bottom bar, and the player is a view that covers them. A switch is a
visibility change, so no navigation recreates the Activity and no destination loses
what the user did in it. `MiniPlayer` is fed only by `PlaybackSession.Snapshot` and
holds no playback state of its own. A destination's glyph is **not a control**:
`IconButton.asGlyph()` makes it invisible to touch entirely, because a long-clickable
child keeps the whole gesture and the destination stops responding exactly where the
finger lands (Phase 5, §9 of the layout report).

**Adaptive layout.** One class decides everything about a window: `LayoutRules`
answers, in pure `dp` arithmetic with no `View` and no `Resources`, which kind of
window this is, how large the cover may be, how wide the column may get, and whether
the window is short enough that the sound bars should give up height. The screen
reads the window in exactly one place and hands the result to the surfaces that
care. Touch targets, type and control sizes never move: a tablet gets more air, a
bigger cover and a wider column, and nothing else.

| Concern | Owner |
| --- | --- |
| The rules that turn a window into a layout | `LayoutRules` |
| Applying them to the player, once per change | `NowPlayingView.setWindow` |
| Applying them to the content column | `MainActivity.applyWindow` |
| The same content is not a new list | `TrackList.sameTracks` |
| Whether L1K3D has changed since the last render | `Library.revision` |

**A cover is not one thing.** The player's covers get two slots in the decoded cache
(1.6 MB each); list thumbnails get a separate 4 MB byte budget, about fifty of them
at 48dp, so a fling is not a re-decode from disk on every pass. Only a player-sized
request derives the palette — deriving colours from a 48dp row thumbnail would
repaint the whole surface every time a different row scrolled past.

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
  as an image response, capped at 10 MiB per item and 128 MiB total, then decoded
  **at its display size** off the UI thread and kept (last two decodes, ≤1.6 MB
  each) so a rotation does not decode it again. The accent derived from it is
  computed on the same worker and applied on the main thread.
- The stereo waveform is never cached: it is a measurement of the audio that has
  actually played, accumulated in Rust as one min/max column per channel per
  128 ms (64 KiB per track, ≈8.7 min of coverage) and re-read on demand.
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
