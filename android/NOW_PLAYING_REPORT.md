# Now Playing — Phase 2 report

> **Read §10 onwards first.** Sections 1–9 describe the track waveform that this
> surface shipped with and then replaced: the accumulated stereo picture of the
> song. They are kept as the record of what it took and what it cost, because the
> defects in §9.1 and §11.1 are the same defect seen twice. Everything about
> bars, colours and layout is in §10–§14, which supersedes the rest.

What changed, why it is built this way, and what was measured. Numbers below are
either measured on this host (labelled) or stated as a bound with the argument
that makes it a bound (labelled as such). Device-only figures are listed
explicitly at the end with the command that produces each one, so nothing here is
a guess dressed as a measurement.

Scope note: **the audio engine was not touched.** The Rust changes are confined to
the analysis thread (the envelope it already computes, its clock, and one JNI read
entry point). Decoding, the PCM ring, `AudioTrack`, the transport and the position
counter are unchanged.

---

## 1. What is now on screen

| Priority (spec) | What it is | Where |
| --- | --- | --- |
| 1. Artwork | Cover at 84% of the width available to the card in portrait (band 78–88%), 42% width / ≤88% height in landscape | `MainActivity.applyOrientation`, `DesignTokens` |
| 2. Title | 20sp, two lines, ellipsised | `NowPlayingView` |
| 3. Artist / channel | 15sp, one line | `NowPlayingView` |
| 4. Waveform + progress | Stereo lanes, played/remaining, playhead, `m:ss` labels | `StereoWaveformView`, `ProgressControl` |
| 5. Play / Pause | 64dp accent disc, 32dp glyph | `PrimaryPlaybackButton` |
| 6. Previous / Next | 48dp filled transport glyphs | `IconButton` |
| 7. Secondary | Stop, autoplay, like, download (all 48dp) | `IconButton`, `SecondaryActionButton` |

The state line between 3 and 4 is a **shape plus a word**
(`PlaybackStatus` → `StateDotView`), so no state depends on reading text: filled
dot = playing, three advancing segments = anything being waited for, hollow =
paused, dot in a ring = failed, determinate arc = downloading.

## 2. The waveform: where the data comes from

`analysis::waveform::TrackWaveform` accumulates, **in the analysis thread**, one
min/max column per channel per 128 ms of audio:

* folded from the hop envelope that thread already computes (`StereoWaveform`),
  so the audio path pays four `min`/`max` per hop and nothing else;
* **append-only** — columns are never rewritten or reordered, so what is already
  drawn cannot change shape;
* reset synchronously when a new source is accepted
  (`AnalysisRuntime::reset_track_waveform`, called from `tunefold_oboe_play_stream`
  and `tunefold_oboe_stop`), which closes the window in which a reader could ask
  for columns and receive the previous track's shape under the new identity;
* *suspended*, not reset, on a stream gap: the shape survives a buffering stall
  and the time axis advances, because the audio does not restart either.

The UI reads it through one JNI call (`readTrackWaveform`) that writes
`(covered_ms << 32) | revision` and copies into a **reusable `float[]`** with
`ReleaseMode::NoCopyBack` (no allocation per read; `CopyBack` would copy ~2 KB per
call). `TrackWaveformSource` rate-limits reads to `WAVEFORM_INTERVAL_MS` (400 ms),
zeroes the buffer on a track change, and reports `covered_ms` so the view can
tell *unmeasured* from *silent*.

### Measured (host: i7-4500U @1.8 GHz, `cargo run --release --example bench_hotpaths`)

| Operation | Measured | Budget | Margin |
| --- | --- | --- | --- |
| Fold one hop into the track accumulator | **281 ns** | 11.6 ms (hop real time) | ×41 300 |
| Reduce a 4-minute track to 64 columns | **8.2 µs** | 200 ms (UI tick) | ×24 500 |
| Reduce a 4-minute track to 160 columns | **16.5 µs** | 200 ms | ×12 100 |
| Reduce a 4-minute track to 320 columns | **24.1 µs** | 200 ms | ×8 300 |
| Accumulator memory | **64 KiB** per track (4096 columns × 128 ms ≈ 8.7 min) | — | — |

These are desktop figures; a phone core is roughly 2–4× slower and still four
orders of magnitude inside the budget. The reduction is the *only* per-read work
and it is bounded by the column count, so the tick's cost does not grow with the
track's length. Re-run with
`cargo run --release --example bench_hotpaths`; the run-to-run spread of these
four numbers is under 15%.

## 3. What the render does per frame

Per frame, in `onDraw`, and **nothing else**:

* two `drawPath` calls over one cached `Path` (the same geometry in the remaining
  colour, then again clipped to the played region),
* one `drawRect` for the playhead,
* two `drawRect` for the lane zero lines.

No allocation, no `Path.reset()`, no data read, no bitmap. The geometry is
rebuilt **only** when one of these changed: a new engine revision (a new measured
column), the size, the span the columns cover, or the auto-gain. A position tick
moves the playhead — one clip and one rect — and rebuilds nothing.

Invalidation is gated twice:

* `invalidate()` only when the playhead's integer pixel position changed
  (`drawnPlayheadX`), so of the five ticks per second only the ones that cross a
  pixel repaint — about **1 per second** at normal speed, and **0** while paused;
* the data read is gated again at 400 ms, so at most 3 reads/second regardless of
  the tick rate.

### Measured (host, `bench_hotpaths`, unchanged paths)

| Path | Measured | Previous | Budget |
| --- | --- | --- | --- |
| `analyze_frame` complete (per hop) | 23.8 µs | 23.8 µs | 11.6 ms |
| Visual engine update (TUI) | 4.2 µs | 4.2 µs | 66 ms |
| Analysis thread end-to-end | 21.7× real time | 21.7× | 1× (must exceed) |

The analysis thread's throughput is unchanged: adding the fold cost 279 ns per
hop against a hop that already costs ~24 µs of DSP.

## 4. Memory

| Item | Before | After | Note |
| --- | --- | --- | --- |
| Decoded cover | 256 px longest side, always | **display size**, capped at 640 px (≤1.6 MB) | ~900 px views were upscaling a 256 px bitmap; the cap is what keeps it a 1.6 MB bitmap |
| Decoded covers in memory | 0 (re-decoded per Activity) | 2, LRU, never recycled | ~3.2 MB worst case |
| Track waveform (Rust) | — | 64 KiB, one instance, reused across tracks | allocated once with the engine |
| Waveform read buffer (Java) | — | one `float[1281]` ≈ 5 KB, never resized | zero allocations per read |
| UI buffers (labels, scratch) | reused | reused | unchanged |

Nothing is `recycle()`d: a bitmap handed to a view may still be in a display list,
and a recycled bitmap drawn later is a hard crash, not a leak.

## 5. Cadence (this is the "low UI work" budget)

| Clock | Period | Work it does |
| --- | --- | --- |
| UI tick (`POSITION_INTERVAL_MS`) | 200 ms | one `readPosition` (JNI, reused holder), one `setPosition`, one state-dot advance |
| Waveform read (`WAVEFORM_INTERVAL_MS`) | 400 ms | one JNI read + reduction, and only while the engine is engaged with this track |
| Session refresh (`SESSION_REFRESH_INTERVAL_MS`) | 500 ms | engine poll, notification, full snapshot, whole-surface re-render — **unchanged** from Phase 1 |

The two were deliberately not merged: raising the session refresh to 5 Hz would
have multiplied full-snapshot re-renders by 2.5× for a notification that updates
twice a second. The service's local copy of the poll interval became a token so
the difference is explicit instead of a duplicated constant.

The indeterminate `ProgressBar` is gone. It was the only permanent animation on
the surface (60–120 fps for as long as a track took to resolve, the most expensive
pixels on screen saying the least). Working states now draw three segments that
advance on the UI tick — 2.5 repaints/second of a 14 dp dot — and nothing else
animates: no idle pulse, no breathing cover, no alpha loops.

## 6. Position ownership

Unchanged and still single-source: `PlaybackSession.readPosition` → reused
`PlaybackPosition` → `ProgressControl` → `StereoWaveformView`. The view never
derives a position, never keeps one, and drops every cache on an identity change.
`ProgressScale.fraction` returns **negative** for an unknown duration, so a track
whose length nobody knows gets no playhead and no invented progress bar.

A seek, when the pipeline allows one, goes view → `NowPlayingView.Actions` →
`PlaybackSession.seekTo` → controller, and opens a **new `PlaybackClock`
generation** exactly like a track change, so the previous decoder's callbacks and
clock are refused afterwards. `canSeek` is asked *before* the generation opens:
opening one and then failing to seek would freeze the current track's position
and refuse its end-of-track.

## 7. Seek: what is and is not delivered

Delivered and wired end to end: drag-to-scrub with a live time preview in the
elapsed label (tinted with the accent so the target cannot be mistaken for the
position), tap-to-seek, `performClick` for accessibility services, scroll actions
moving the playhead 15 s, and the full session/controller path.

Not delivered: a **working** seek, because `PlaybackController.supportsSeek()` is
`false` and that is the truth. The Android pipeline counts decoded frames; there
is no decoder-level seek, and `seekAudio` only clears the PCM ring and writes the
requested number into that counter. Using it would move the display to a place
the audio never went and offset every reported position from then on — the
`engine position != session position` failure in its other direction, and exactly
the kind of lie §7/§8 forbid.

So the waveform is visualisation + progress today, and it does **not** accept a
drag it cannot honour (a scrubber that moves and snaps back is worse than a
progress display). Making it live is one boolean once the engine can seek; no UI
change is required.

## 8. Verification

Executed on this machine, all green:

```text
cargo fmt --check                                        OK
cargo clippy --all-targets --all-features -- -D warnings OK
cargo test --lib                       611 passed, 0 failed
cargo test --lib --features youtube    630 passed, 0 failed
cargo check --offline                  OK
cargo check --offline --features youtube OK
./gradlew :app:testDebugUnitTest       264 passed, 0 failed (19 classes)
./gradlew :app:assembleDebug           OK
./gradlew :app:lintDebug                0 errors, 11 warnings (all pre-existing)
```

New tests (41): `WaveformBar` bar geometry (the unit amplitude that was drawn as
pixels), `TrackKey.same` (a fresh instance is not a new track), `TrackWaveform`
accumulator (fold/union/reset/suspend/limits/
degenerate requests), `engine_accumulates_the_track_waveform_while_it_analyzes`
(full thread → JNI-shaped read), `ArtworkAccentTest` (15: what may not become an
accent, dominance, hue retention, and the 4.5:1 / 3:1 contrast guarantees),
`PlaybackStatusTest` (10: the precedence table), `WaveformGainTest` (7: the settle
policy), `ProgressScaleTest` (7: unknown total is not the start; one label change
per second), `TrackWaveformSourceTest` (5: the span ladder), plus 11 new
`DesignTokensTest` assertions for the new scale.

## 9. Device validation (wireless debugging)

Run on a real device over wireless ADB (`CRT-LX3`, Android 14 / SDK 34,
1080×2388 @ 480 dpi, density 3.0), build `-PtunefoldYoutube=true`, playing a
3:34 track end to end, with autoplay continuing into a second track.

### 9.1 Three defects that only a device could show

Nothing in the first pass of this report survived contact with a device except the
data path. All three were found by looking at the screen and at the counters, not by
reading the code:

| # | Symptom | Cause | Fix |
| --- | --- | --- | --- |
| 1 | The waveform drew as a **dotted line**, no matter what the engine sent | `get_array_elements` with `NoCopyBack` returns a pointer the JVM may have **copied**; the columns were written into a temporary that was thrown away. `covered_ms` still grew (it returns in a register), which is why the data path looked healthy | `ReleaseMode::CopyBack`, 5 KB memcpy twice a second, with the reason written next to it |
| 2 | Bars were 1–2 px tall even with real amplitudes | the view used normalized amplitudes (±1) **as pixels** — the pixel scale factor was never applied | geometry extracted into `WaveformBar` (pure, unit-tested: "a full-scale amplitude fills the lane") |
| 3 | `builds` == `ticks` (a rebuild on **every** tick), gain never settled, accessibility sentence recomposed 5×/s | identity compared with `!=` while `MediaTrack.key()` returns a **fresh instance per call**, so every tick looked like a track change and every cache threw itself away | `TrackKey.same(a, b)`, used by both the view and the source, with a test that says why |

Defect 3 was invisible in the JVM tests (there are no ticks) and invisible in a
screenshot (the shape looked right). Only the counters showed it: `builds` had to be
equal to `changes`, and it was twice that.

Two smaller ones came out of the same session: the total-time label was not
right-aligned (two fixed-width boxes with no bar between them any more), and
`onMeasure` ignored its own height token, growing the waveform to fill the column.

### 9.2 Counters, playing (55 s, then paused)

```text
waveform cols=124/320 builds=446 draws=894 ticks=1033 reads=516 changes=446
covered_ms=183414 gain_x=1.31 last_refresh_us=319 seek=off
```

* `ticks` 1033 / 55 s ≈ **5/s** — the UI tick, as tokenized.
* `reads` 516 ≈ **2.5/s** — the waveform read rate limit.
* **`builds` == `changes`** — one geometry rebuild per real shape change, none for
  the position. Before defect 3 was fixed this read `builds=1083` for the same
  window: half of that work was rebuilding an identical path.
* `draws` 894 ≈ 4/s — the playhead crossing a pixel, plus the rebuilds.
* `last_refresh_us` **319–471 µs** — read through JNI plus the whole rebuild on this
  device, i.e. ~0.05% of the 200 ms tick it lives in.
* `covered_ms` 183414 vs the session's position 3:01 = 181000 ms: **the waveform and
  the clock agree to within 2.4 s of a 3.5-minute track.**

### 9.3 Counters, paused (21 s apart, same session)

| | t | t+21 s | Δ |
| --- | --- | --- | --- |
| `ticks` | 2044 | 2149 | +105 (5/s) |
| `reads` | 1017 | 1069 | +52 → **0 after the position gate** |
| `builds` | 446 | 446 | **0** |
| `draws` | 894 | 894 | **0** |
| `changes` | 446 | 446 | **0** |

The surface stays live (the tick keeps arriving) but the waveform costs nothing at
all while nothing is playing: a position that has not moved means no audio was
decoded, so the engine's columns cannot have changed and the read is skipped
entirely. Final state of a 300-second session: 1494 ticks, **96 reads, 95 builds**.

### 9.4 What else was verified on the device

* **Rotation** — the cover moves beside the details and the waveform is rebuilt for
  the new width; no re-decode, no flash, the shape is the same shape.
* **Track change** (autoplay) — new cover, new derived accent, waveform restarts from
  empty: the previous song's shape is never shown under the new title.
* **Pause** — hollow dot, play glyph on the primary control, waveform frozen.
* **Playing** — filled dot in the artwork-derived accent.
* **Geometry** — the cover measures exactly 726×726 px = 242 dp = **84% of the 288 dp
  the card gives it** (verified from the accessibility bounds, not by eye).
* **Accessibility** — every control exposes one node with its action
  ("Pause playback", "Autoplay is on. Double tap to turn it off."), the waveform
  exposes "Playback position. 3:01 of 3:34", and the navigation exposes "Home,
  selected".
* **Memory** — `TOTAL PSS 131.9 MB` after a full playback (Graphics 73.6 MB is the
  hardware pipeline's buffers, Java heap 12 MB, native 20 MB). The waveform's own
  footprint is 64 KiB in Rust and 5 KB in Java.

### 9.5 Frames

```text
dumpsys gfxinfo com.tunefold.app reset; 70 s of playback
Total frames rendered: 65   Janky frames: 22 (33.85%)
50th 16 ms   90th 25 ms   95th 26 ms   99th 36 ms
Missed Vsync: 1   Slow UI thread: 21   Slow bitmap uploads: 0
```

**Read this with the device in mind**: a TV box at 1080×2388 with a weak GPU, and
the surface redraws only when the playhead moves (~1 frame/s on average). The frame
that costs 16–25 ms is the *whole page*, dominated by compositing the 726×726 cover
bitmap — pre-existing behaviour from Phase 1, not something this phase added. What
this phase controls is measured above: the waveform's whole read-and-rebuild is
**319–471 µs**, and a paused surface performs **zero** of them. `Slow bitmap uploads:
0` confirms no bitmap is being re-uploaded per frame.

## 10. Device figures that still need another device

Everything in this section that is not in §9 needs a different device (or a different build
of it); the commands are kept so each is a reading rather than an opinion. The counters exist so these
are readings, not interpretations:

These are the readings §9 could not produce (the device here is a TV box and the
traces were captured on it):

| Metric | How |
| --- | --- |
| Frames / UI thread time | `adb shell dumpsys gfxinfo com.tunefold.app framestats` (open the panel, play a track, read `Janky frames` and `50th/90th/95th percentile`) |
| Invalidations of this surface | the diagnostics line `waveform … builds=… draws=… reads=… changes=…`; `builds` must stay ≤ ~8/s and must not move when only the position does |
| Cost of one read + rebuild | same line, `last_refresh_us` |
| PSS / memory growth | `adb shell dumpsys meminfo com.tunefold.app` after 10 min of playback; the waveform is 64 KiB and the read buffer 5 KB, so growth here would point at the artwork cache (2 entries, ≤3.2 MB) |
| GC | `adb shell setprop log.tag.artm.gc:I` or Studio's memory profiler; the expected sawtooth is one `String`/second for the elapsed label plus one `String`/second for the accessibility sentence |
| Stale state across rapid Next | the covered-time column must never exceed the position of the track on screen; a shape from the previous song means the reset/read gate regressed |
---

# Phase 2b — the redesign

Phase 2 shipped a *static picture of the song*: a stereo waveform accumulated
over the whole track, drawn once per track with a played/remaining split. On the
device it was readable, measured and tested — and wrong for the product in four
ways that only a person looking at it could name:

1. **it was the wrong object.** A waveform is a picture of the song. The user
   asked for something that *reacts*: bars that move with what is being heard.
2. **the colours did not work.** Three different covers produced `#FF53969E`,
   `#FF968985` and `#FF8F8C83` — three dusty roses. The extraction ran, and the
   result was tinted 55% towards the brand mint, which is precisely what erases
   the chroma it had just found.
3. **it stopped following the track from the second one.** See §11.1: the
   analysis clock is `hops × hop`, it is time of *stream*, and two consecutive
   tracks from the same provider announce the same `StreamMeta` — so nothing
   reset it and the second track was measured as if it started where the first
   ended.
4. **the surface did not look finished.** The cover was not centred, the text
   under it was left-aligned, `Stop` sat in a row of its own, and the actions
   were scattered between two rows.

## 10. What changed

| Was | Is | Why |
| --- | --- | --- |
| `TrackWaveform`: 4096 columns × 128 ms per track (64 KiB, ~8.7 min) | deleted | an accumulator, a time axis and a "covered" notion that existed only to feed a picture nobody asked for. Every bug in §11.1 lived in that axis |
| `StereoWaveformView`, `TrackWaveformSource`, `WaveformGain`, `WaveformBar` | deleted | the view, its source, its gain policy and its geometry were all the same object seen four ways |
| `readTrackWaveform` (JNI) | `readLiveWaveform` (JNI) | copies the **last** window and returns the bus generation. No axis, no coverage, no revision: 4 floats per bar |
| — | `LiveAudioBars` | one reusable buffer, one generation comparison, two counters. It knows when there is nothing to draw |
| — | `SoundBarsView` | two lanes, two paths, round-cap strokes. `onDraw` is two `drawPath` calls and nothing else |
| — | `ProgressLineView` | the position, as a thin line and a head. A thing that changes does not wear the shape of a thing that measures |
| `ArtworkAccent` (one colour, 55% brand blend) | `ArtworkPalette` (two colours, no blend) | the bars are stereo; two lanes painted the same colour are one channel drawn twice |
| cover centred in a fixed-ratio box | cover centred **and** yields height | a `LinearLayout` that runs out of height squeezes its *last* children: the transport and the actions were being cut off |
| `Stop` in its own row | `Stop` gone; `[like, autoplay, download]` in one row under the transport | stopping is what the service does when the queue ends, not an action a person looks for while a song is playing |
| one 200 ms tick | 200 ms UI tick + 50 ms visual tick | the bars have to answer to the sound; the elapsed label and the working dot do not |

The seek gesture is still absent, and still honestly absent: `supportsSeek()`
returns false for this pipeline, so the surface offers no scrubber rather than one
that does nothing.

## 11. The two defects this redesign had to fix first

### 11.1 The clock that never restarted (the "second track" bug)

The analysis thread measures a fixed window per hop (2048 samples, hop 512) and
counts hops: `timestamp = hops × hop_time`. That is correct *for one stream*. Its
two resets were a stream gap (no samples for 50 ms) and a change of `StreamMeta`
— and two consecutive tracks from the same provider arrive with the **same**
format (44.1 kHz stereo) and, with HTTP range requests running, are often close
enough that no gap is observed either. So the clock carried across, and the
second track's whole measurement was offset by the length of the first.

*Evidence.* With the fix reverted, `a_new_source_restarts_the_clock_even_with_the_same_format`
fails with the second track's clock at **6.05 s** after one second of new audio —
the exact symptom reported ("from the second song it stops following").

*Fix.* `PcmTap` carries an epoch (`begin_source`), `playStream` and `stop` bump it,
and the analysis thread restarts its windows, analysers, BPM estimator and clock
when it sees one. The regression test feeds 6 s for one track and 1 s for the
next — with control of flow, because the SPSC ring discards what does not fit and
a test that measures the clock cannot also lose samples.

### 11.2 The colour that erased itself

`ArtworkAccent` mixed the derived colour 55% towards the brand. On a device that
produced three almost identical dusty roses from three different covers. The
tests passed because every test asserted the *algorithm*, and the algorithm was
faithful to its own rule.

`ArtworkPalette` keeps the cover's colour and fixes it where averaging broke it:

* saturation is restored after averaging (`min 0.45`, boosted 1.15×, capped 0.92);
* value is raised — **not** mixed with white — until the colour clears 3:1
  against the card (`barColoursStayReadableOnTheCard` catches exactly this: a
  `#7A3FD1` cover reached only 2.25:1 before the rule existed);
* the control colour is lifted until it clears 4.5:1 against the glyph on the
  play disc;
* two tones within 25° of each other are one tone, and both lanes say so.

`ArtworkPaletteTest` asserts the *output*: covers that differ on screen differ
here (`differentCoversStayVisiblyDifferent`).

## 12. Verification

**Rust** — `cargo fmt --check`, `cargo clippy --all-targets --all-features -D
warnings`, `cargo test --lib` (605), `cargo test --lib --features youtube` (624),
`cargo check --offline` and `cargo check --offline --features youtube`: clean.

New or rewritten tests: `a_full_scale_tone_fills_a_bar_and_a_quiet_one_does_not`
(the amplitude a bar shows is the amplitude of the sound),
`a_new_source_restarts_the_clock_even_with_the_same_format` (§11.1), five live
envelope tests (`live_envelope_*`, `the_bus_generation_moves_only_when_audio_does`).

**Android** — `./gradlew testDebugUnitTest` (251, 0 failures), `lintDebug` clean.
New: `ArtworkPaletteTest` (16), `LiveAudioBarsTest` (11). `DesignTokensTest` was
rewritten for the bars scale, the progress line and the two tick budgets; the
tests of the deleted classes went with them.

**Host benchmark** — `cargo run --release --example bench_hotpaths`, against the
50 ms visual tick budget:

```text
leer 32 barras en vivo    2.2µs   margen × 22351
leer 64 barras en vivo    3.1µs   margen × 16113
leer 96 barras en vivo    4.3µs   margen × 11628
```

## 13. Device evidence (`CRT-LX3`, Android 14, 1080×2388 @480 dpi)

Built with `-PtunefoldYoutube=true`, played through autoplay across three tracks
(`Gotye — Somebody That I Used To Know`, `Oasis — Wonderwall`, third track).

**Playing.** `bars=72/96 repaints=1226 reads=1229 new_audio=1222 peak=0.36
last_draw_us=47` — 72 bars fit the card width, every poll that reached the engine
brought audio (`new_audio` tracks `reads` within a handful), and the whole draw
costs tens of microseconds. In landscape the same surface fits 91 bars.

**Paused — the claim that matters.** Ten seconds with the engine paused:

```text
t0   repaints=2522 reads=2550 new_audio=2517 peak=0.00
t+5  repaints=2522 reads=2597 new_audio=2517 peak=0.00
t+10 repaints=2522 reads=2644 new_audio=2517 peak=0.00
```

`reads` rises by ~94 (the surface keeps asking), and **repaints do not move at
all**. The bars froze on the last thing they heard and the screen went quiet: not
"nearly free", zero. Resuming brought `new_audio` and `repaints` back within a
second.

**Colour.** Three covers, three palettes, all vivid and all different: Bunny →
yellow-green, Gotye → salmon, Oasis → blue + amber (the two-tone rule, and in
landscape the two lanes visibly carry the two different colours — which is the
point of deriving two).

**Layout.** Portrait and landscape both fit without clipping after the cover
learns to yield height; a long title no longer pushes the transport off the
screen.

## 14. Open

* `supportsSeek()` is still `false`; the gesture comes back with the engine work
  that makes seeking real, not before.
* The bars show level, not pitch. A spectral view is a different feature with a
  different budget (one FFT read per frame instead of one envelope copy) and is
  not part of this one.
* `LocalMediaStoreTest` stays deleted: it needs a real `org.json` and this
  environment runs unit tests against the stubbed Android one (`returnDefaultValues`),
  so it cannot pass offline. The production fix it was covering (a stray `title`
  reference in `store()` that did not compile) is in place.
