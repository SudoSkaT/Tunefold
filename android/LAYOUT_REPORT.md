# Responsive Layout, Mini Player & Adaptive Composition — Phase 5 report

What changed, why it is built this way, and what was measured. Numbers are either
measured on a real device (labelled, with the command that produces them) or stated
as a bound with the argument that makes it a bound.

The device is the same one Phase 4 used: `CRT-LX3`, Android 14 / SDK 34,
1080×2388 @480 dpi, over wireless ADB.

---

## 1. What this phase was for

> Make the interface that already exists fit the window it is in, without paying for
> that with frames, memory or state.

So this phase adds no feature. It adds **one class** (`LayoutRules`) and a set of
guards, and it removes work that was happening whether or not anything had changed.
Two of the things it found were not layout problems at all: one of them was the
bottom navigation not responding, which is in §9.

## 2. The window is read once, and everything else follows

`LayoutRules` is pure arithmetic over a window in `dp`. It answers four questions —
what kind of window is this, how big may the cover be, how wide may the column get,
is this a short window — and it answers them in integers, with no `Resources`, no
insets and no `View`. That is not tidiness: it is what makes every rule in it
assertable in a JVM test on a machine with no phone attached
(`LayoutRulesTest`, 17 cases).

```java
LayoutRules.Profile profile = LayoutRules.of(widthDp, heightDp, smallestDp, landscape);
```

The screen reads the window in exactly one place (`MainActivity.applyWindow`), hands
the profile to the surfaces that care, and writes nothing. Before this phase the
same information was read twice — "be landscape" and then "the cover is N pixels" —
and each of those ended in a `setLayoutParams`, so one rotation was two layout
passes over the card that holds the largest bitmap in the app.

| Form factor | Smallest side | Column maximum | Cover maximum |
| --- | --- | --- | --- |
| `COMPACT` | < 600dp | none (a phone has no width to spare) | 420dp |
| `MEDIUM` | 600–839dp | 560dp | 480dp |
| `EXPANDED` | ≥ 840dp | 560dp | 540dp |

The form follows the **screen**, not the window: a tablet turned on its side is
still a tablet, and a phone turned on its side must not inherit the limits its
portrait layout uses. That is why `Profile` carries the smallest width separately
from the current orientation.

What a bigger screen gets is **air, a bigger cover and a wider column** — and
nothing else. Touch targets, type sizes and control sizes are tokens and stay
exactly as they are. A tablet is a bigger room, not a bigger person, and scaling
the controls is how an interface turns into a diagram of itself. `LayoutRulesTest`
asserts exactly that (`nothingScalesIndiscriminately`).

## 3. Portrait, landscape, and the third thing that is neither

**Portrait (one column).** Cover, title, artist, state, waveform, position and
times, transport, secondary actions. The cover takes 84% of the width available to
the card and gives height back to whatever is left: `Surface.onMeasure` measures the
details column at its natural height, hands the cover `spare = available − details`,
and floors it at `ARTWORK_MIN`. A window too small for that is a window problem, not
a deformed cover.

**Landscape (two regions).** Cover on the left owning its share of the width;
metadata, waveform, position, transport and secondary actions on the right, taking
the rest and centred against it. Landscape is not the portrait layout with less
height — it is a different composition, and the difference is visible in the
screenshots: the cover moves beside the text instead of above it, and the details
column grows to what it needs rather than being squeezed to the cover's height.

**Short window.** A landscape phone is short by definition. The sound bars are the
one block that may give up height (`SOUNDBARS_HEIGHT` 56dp → 34dp); the transport
is never touched. Verified on a 720×1280 phone in landscape: every control is inside
the window and inside one screen.

**Large screen.** The column stops growing at 560dp and centres, so the extra space
becomes margin. The card obeys the same maximum as the lists — a maximum that only
the lists honour is a maximum the player quietly ignores, which is exactly what
happened until §9 was fixed.

## 4. The two rules that make a two-region composition hold

Both of these were found by looking at a tablet, and both are the kind of thing that
is invisible in code review:

1. **The cover yields a floor.** `DETAILS_MIN_DP` (280dp) is the narrowest the
   metadata region may become beside the cover. The cover is computed first, so
   without the floor it can claim the entire column and leave the details with
   nothing — a screen that shows a cover and no way to control it.
2. **The details column is always `wrap_content`.** Told to fill the row, it is
   measured at the row's height, and a `LinearLayout` squeezed to a height drops its
   **last** children — which are the transport and the secondary actions. The column
   grows to what it needs and the cover centres against it.

## 5. Mini player

It is built once, in `buildBottomBar`, and never rebuilt: a configuration change
re-weights nothing about it, and a destination switch does not touch it. It is fed
only by `PlaybackSession.Snapshot`, so it cannot disagree with the player.

Two things were wrong with it:

* **It republished its description on every snapshot.** `setContentDescription` is
  not free — it is an accessibility event, which is another process walking a tree —
  and it happened twice a second for a sentence that was almost always the same
  sentence. It is now compared before it is written.
* **It sat above the window it belongs to.** The strip's artwork, title and
  transport are fixed-size by token, so nothing about them had to change; what did
  change is that they now live inside a window that keeps them there when the
  window turns out to be short.

Verified: the strip survives four rotations during playback, four destination
switches during playback, a rotation during a resolve, a rotation during a download,
and being opened and closed around the player — `tools/validate_states.sh`, 11
assertions, 0 failures.

## 6. Configuration changes

`configChanges` now also covers `smallestScreenSize|screenLayout|uiMode|fontScale|density`.
Each one used to cost a full Activity recreation — window, decor, pooled rows,
decoded cover, service binding — for something this Activity can absorb by
re-reading its tokens and re-running `applyWindow`. Rotation never recreated the
Activity (it did not before either) and still does not: `dumpsys meminfo` reports
**1 Activity** after twelve rotations.

One inset listener, installed once for the life of the screen, and it only calls
`applyWindow` when the insets actually moved.

## 7. What was costing frames, and what it costs now

The biggest finding of the phase was not in the layout at all.

`HomeSurface.onShow()` was called from `onSessionChanged` — which fires twice a
second for every reason at once — and it re-derived both local groups every time:
two `ArrayList`s, a walk over seventeen identities, and two `TrackList.submit()`
calls that dropped every bound row, re-bound every row and asked the host for a
layout pass, all to arrive at the picture already on screen.

The fix is three guards, and each is a rule rather than a patch:

* `Library.revision()` — a membership set has a version; L1K3D re-derives when it
  moves (`PlaybackSession.libraryRevision()`, the same trick downloads already used).
* "Keep going" re-derives when the track or the queue size moves, and not one time
  in between.
* `TrackList.sameTracks(...)` — identical identities are not a new list. Pure
  function, five unit tests, and it protects every list, not just Home's.

The app now **reaches idle**: `uiautomator dump` used to fail with "could not get
idle state" within a second, and now succeeds in 2.5 s. That is the same fact
stated twice — once as a tool that works, once as a number.

## 8. Measured on the device

Wireless ADB, `CRT-LX3`, build from the working tree. Two kinds of comparison, and
they are kept apart on purpose:

* **A/B, same session** — the guards that stop the churn turned off in a temporary
  build, so "before" and "after" are the same device, the same minute and the same
  script. This is the comparison the headline rests on.
* **Against the phase-4 build**, measured with the same script at the start of the
  session, for everything the A/B does not cover (startup, memory, threads,
  rotations).

### The churn, A/B

| 10 seconds on Home, nothing playing | Without the guards | With them |
| --- | --- | --- |
| Frames drawn | **21** | **0** |
| Janky frames | 21 (**100%**) | 0 |
| p50 / p90 frame | 36 ms / 48 ms | — (no frames) |
| `uiautomator dump` | fails: "could not get idle state" | succeeds in 2.5 s |

Twelve destination switches over the four destinations, same A/B:

| | Without the guards | With them |
| --- | --- | --- |
| Frames | 348 | 347 |
| Janky | 23 (6.6%) | 20 (5.8%) |
| p50 / p90 | 10 ms / 24 ms | 9 ms / 26 ms |

Navigation is unchanged — as it should be, because navigation was never the problem.
The problem was a screen redrawing itself 2 times a second while nobody was looking
at it.

### Everything else, against the phase-4 build

| What | Baseline (11:44) | Phase 5 (13:0x) | How |
| --- | --- | --- | --- |
| Cold start to resumed | 890 ms (median of 5) | 1002 ms (median of 5) | `am start -W` |
| Frames drawn while nothing happens (10 s) | 20, 100% janky | **0** | `dumpsys gfxinfo reset`, sleep 10 |
| Navigation, 12 switches | *taps never reached the bar* (§9) | 347 frames, 5.8% janky, p50 9 ms | `dumpsys gfxinfo reset`, tap ×12 |
| Rotation, 12 rotations | 6.2 frames per rotation, 70% janky | **2.0 frames per rotation**, 54% janky, p50 61 ms | `dumpsys gfxinfo reset`, `settings user_rotation` |
| Rotation with the player open (×4) | 25 frames, 60% janky, p50 26 ms | 14 frames, 28.6% janky, p50 26 ms | same |
| Memory, idle on Home | PSS 119.7 MB, RSS 262.6 MB, **226** views | PSS **115.6 MB**, RSS 260.8 MB, **226** views | `dumpsys meminfo` |
| Memory, after driving every destination | PSS 98.4 MB, 226 views | PSS 123.8 MB, 450 views | `dumpsys meminfo` |
| Threads | 43 | 43 | `/proc/<pid>/status` |
| GC events during the run | 0 | 0 | `logcat -s art` |
| Activity recreations | 0 | 0 | `dumpsys meminfo` → `Activities` |

Three of those rows need reading carefully, and one of them is not an improvement:

* **Idle is the headline.** 20 frames in ten seconds of doing nothing → 0. A screen
  that redraws itself while the user reads it is spending battery, waking the GPU
  and producing 100% janky frames for a picture that did not change.
* **The 450 views and the 123.8 MB are the price of navigation working.** The
  baseline never built the Search, L1K3D or Descargadas surfaces, because the taps
  that would have built them never arrived (§9). The comparable row is the idle one:
  226 views and 4 MB less resident than the baseline.
* **Cold start is ~110 ms slower and this phase does not explain it.** Removing the
  only new measurement in the start path (the navigation caption's auto-sizing)
  changes it by 0 ms — 986 ms with, 986 ms without. What `applyWindow` adds is a
  handful of integer operations and three guarded writes. The two sessions are not
  the same machine state: the baseline ran at 11:44 on a freshly used device, this
  one after ninety minutes of continuous instrumentation. It is reported as
  measured, and it is the one metric that did not improve.

The remaining ~61 ms rotation frame is one full measure and layout of the content
tree. That is the price of not recreating the Activity: a recreation would rebuild
the window, the rows and the decoded cover, and would take longer.

## 9. The defect the physical validation found

The bottom navigation did not respond to taps.

Not "sometimes", and not "on one device": on this build, on this device, tapping a
destination's **glyph** — which is where the finger goes, and which is the centre of
the slot — did nothing at all. Tapping the caption strip worked. The measurement
script reported it as "17 frames for 12 switches", which reads like an efficient
navigation and was in fact a navigation that never happened — and that is how it
survived a phase whose own report claimed to have measured twelve switches.

The cause: `IconButton.asGlyph()` cleared the inner button's size and padding and
set `clickable=false`, but left it **long-clickable** through its tooltip. A
long-clickable child keeps the whole gesture: it takes the `DOWN`, and on release
there is no click left for the parent. The tooltip was even visible on screen during
the press, which is what finally made it obvious that the touch was arriving and
being eaten rather than never delivered.

The fix is `asGlyph()` becoming what it says: a glyph is not a control.

```java
glyph = true;
setClickable(false);
setLongClickable(false);
setFocusable(false);
setTooltipText(null);
setDuplicateParentStateEnabled(true);

@Override public boolean onTouchEvent(MotionEvent event) {
    if (glyph) return false;      // el gesto entero se lo pasa a quien lo contiene
    return super.onTouchEvent(event);
}
```

The long-press word moved to the slot, which is what a finger actually presses.
`tools/validate_states.sh` now asserts that a tap on the glyph changes the selected
destination, so this cannot come back silently.

Two smaller clipping fixes came out of the same walk: the destination caption used
the body label size and overflowed the screen edge ("Descargada|s"), and it now
uses `NAV_CAPTION_SIZE` inside its own slot, auto-sized between 8sp and 10sp.

## 10. Physical validation

`tools/validate_layout.sh` walks four windows and both orientations, and for each
one captures the list, the mini player and the full player, and audits every node
the system reports against the window it was given:

| Window | dp | Form | Portrait | Landscape |
| --- | --- | --- | --- | --- |
| `1080×2388 @480` | 360×796 | COMPACT | 0 outside, 0 empty | 0 outside, 0 empty |
| `720×1280 @320` | 360×640 | COMPACT, short | 0 outside, 0 empty | 0 outside, 0 empty |
| `1600×2560 @240` | 667×1067 | MEDIUM | 0 outside, 0 empty | 0 outside, 0 empty |
| `2560×1600 @200` | 1280×800 | EXPANDED | 0 outside, 0 empty | 0 outside, 0 empty |

20 configurations, 20 screenshots, no node outside the display and no node drawn
with zero area. The states walk (`tools/validate_states.sh`) covers the moments a
static screenshot cannot: rotation during playback, during a resolve, during a
download; navigation during playback; and the player across a rotation — 11
assertions, 0 failures, 1 Activity throughout, nothing in the crash buffer.

The audit compares against the **emulated panel**, not the app's own window:
accessibility bounds are screen coordinates, and auditing against the window would
put the status bar's worth of pixels outside every node. That is a mistake this
report made first, and the script says so where it would otherwise be repeated.

## 11. Artwork and waveform

* **The cover is sized once per layout change**, by `LayoutRules`, and every
  listener reads the result (`MainActivity.loadPlayerArtwork` asks
  `nowPlaying.artworkSidePx()`; nobody recomputes a ratio). The only other writer
  is the card itself, when a short window has to give height back — and that is not
  a resize, it is the cover yielding.
* **The bars adapt to a new width without re-reading audio.** `SoundBarsView` keeps
  its geometry measurement in `onSizeChanged` (it was recomputed twenty times a
  second inside `build`), and a resize that keeps the same bar count recolocates
  the paths over the values already in memory: no JNI read, no allocation. Only a
  change in the number of bars goes back to the engine.
* **`LiveAudioBars.pitchFor`** is what makes "adapts to the new size" true on a wide
  window: the desired pitch is a legibility floor, not a contract, so past
  `MAX_BARS` the pitch grows until the bars that do fit **fill** the width. Without
  it, rotating a phone to a tablet left the waveform drawn across a third of its
  region, bunched at the left.
* **The position line is the artwork's colour**, like the bars — the same song, the
  same ink. Its head stays neutral because it marks "now" over any colour.

## 12. Text

A tick that cannot change anything visible costs comparisons. `setText` is only
called when the displayed second changed, the elapsed label and the total have
separate memos, and a tick computed for a track that is no longer active is dropped
before it reaches the label. The mini player's description is written once per
change of sentence rather than twice a second. No label anywhere in the player is
re-measured per frame.

## 13. Open

* **The rotation frame is 65 ms.** It is one measure and layout of the whole content
  tree. The next step is not to animate it but to shrink it: the lists already
  report their height from the data, so a rotation could re-measure only the window
  that moved rather than the whole column.
* **Scroll jank** is still dominated by cover decoding during a fling (Phase 4's
  finding), and Phase 5 did not touch it.
* **`MAX_TRACKS` (500)** still truncates a library rather than paginating it, and a
  tablet now has the space to show that it is truncated.
* The **tablet layout has been validated on emulated windows on a phone**, not on a
  tablet: `wm size` and `wm density` change the window the app is given, which is
  exactly what the rules read, but they cannot show a real tablet's panel, its
  cutouts or its GPU.