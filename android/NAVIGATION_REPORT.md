# Navigation, Home, Search & Library — Phase 4 report

What changed, why it is built this way, and what was measured. Numbers are either
measured on a real device (labelled, with the command that produces them) or stated
as a bound with the argument that makes it a bound.

---

## 1. The principle, and what it cost to obey

> The navigation must disappear visually while the user is playing music.

That is one sentence and it decided the shape of the screen. The full player is a
view in this Activity, not a second one: opening it hides the strip and the bottom
bar, and one back gesture gives the screen back exactly as it was. An Activity
switch would have torn down the window, rebuilt the strip, re-decoded the cover and
handed the service a new binding — all to show a view that was already built.

## 2. What is on screen now

```
┌──────────────────────────────┐
│  destinations (scroll host)  │   ← one surface visible at a time
│  Home · Search · L1K3D ·     │
│  Descargadas                 │
├──────────────────────────────┤
│  mini player (56dp)          │   ← only while something is loaded
├──────────────────────────────┤
│  bottom navigation (60dp)    │   ← 4 destinations, glyph + caption
└──────────────────────────────┘
```

* **Bottom navigation.** Four destinations built once; a switch flips four
  selected states. Active is the artwork-derived accent through `ArtworkTheme`.
* **Mini player.** Cover, title, artist when there is one, play/pause. 56dp.
  It is fed *only* by a `PlaybackSession.Snapshot` and keeps no state of its own:
  a strip with its own copy of the truth is a strip that is wrong twice a day.
* **Full player.** Tapping the strip opens it; back closes it.
* **Diagnostics.** One 48dp target at the top of Home opens a panel that is hidden
  by default. It was in the top bar until the bottom navigation replaced it.

## 3. The list is a window, not a column of rows

The first version of the track list put every track into a `LinearLayout`. A
library of 500 tracks meant 500 rows — each with a cover, three labels and a button
— built to show the eight that fit, on every render. Recycling does not fix that:
rows that were never built cannot be recycled, and the memory was the problem.

So:

* `ScrollHost` is a ~200-line scroller (`OverScroller`, drag, fling, clamp) that
  measures its content once for its height and then moves it. `ScrollView` cannot be
  used here: it measures its child unbounded, which forces every row to be laid out.
* `TrackList` keeps the **tracks** and owns a window of **rows**: the rows in view
  plus `LIST_PREFETCH_ROWS` above and below. It reports its height from the track
  count, and `windowFor(...)` is a pure function with nine unit tests.
* A row asks for its cover when it is **bound**, through one loader on the pool.
  A row that was never built never asks, which is the same decision seen from the
  other side.

**Measured on the device** (`CRT-LX3`, L1K3D with 15 tracks, Descargadas with 17):

```text
lists=0+0/0  0+14/15  0+0/0  0+0/0  0+14/15  0+9/17
built=47 pooled=10
```

`0+14/15` is "14 rows exist for 15 tracks". `built` counts every row the process has
ever constructed, across all five lists, after scrolling both libraries end to end
twice: **47 rows for 32 tracks on screen and a scrolling history of the same**, not
32 per screen.

## 4. The row

`TrackRow` is one node: cover, title, artist, **duration**, an optional **status**
and one **overflow** button. Tap plays; the button and the long press open the same
menu, whose items are decided by the row's own rendered state (a liked track offers
to unlike it, a running download offers to cancel it, a failed one offers to retry).

The status line is what the track *is*, not what it could become: `SAVING 42%`,
`SAVED`, `LIKED`, `FAILED`, and nothing at all for a plain track. Two always-present
buttons per row were also two always-present focus stops in a list of fifty.

## 5. Search is a destination

A search field that lives on Home is a field that is always there, always in the
way, and belongs to the screen you are not on. Search now owns its field, a paste
action, its results and its own states, with a back arrow that returns to wherever
the user came from.

Nothing here resolves anything: the surface reports a query and the screen sends the
one command the rest of the app already uses, so there is exactly one resolution
path. `isUrl(...)` is a pure function with a narrow rule — a pasted link starts with
a scheme or `www.` **and** points at a shape this app can resolve — because
"how do I post on youtube.com/watch" is a search, not a stream.

## 6. Home is discovery

`Continue listening` (the queue from the current track on), `L1K3D` (what the user
already said yes to) and `Recommended` (one provider request per track, never per
render). The player is deliberately **not** here: it is one tap away in the strip,
always, and embedding it would make Home's main content a control the user already
has.

There is room for a fourth group and **no placeholder for it**: a heading with
nothing under it is a promise the product has not made yet. The seam is a
`TrackGroup` in `HomeSurface`.

## 7. States

`SectionState` owns the four — `LOADING`, `CONTENT`, `EMPTY`, `ERROR` — and
`phaseOf(loading, failed, count)` is a pure function, because "which of the four am
I" is the part that has to be right and the part a reviewer can check without a
device. A failure outranks everything: showing "Loading…" over a request that
already failed is the kind of small lie a state machine makes when its branches are
in the wrong order.

An empty list is not an empty group. A group with nothing in it is a heading
floating over a hole, so `TrackGroup.show()` hides it and `showEmpty()` is a
separate, explicit call.

## 8. Artwork

One cache, two bounds, because a cover is not one thing:

* the player's covers: **2 slots**, each ~1.6 MB — the case this exists for is a
  rotation re-reading the same file;
* list thumbnails: **4 MB**, about fifty at 48dp on a 3x screen, so a flick through
  a list is not a re-decode from disk on every pass. It is a byte budget, not a
  count, so the bound holds at any density.

A decode smaller than what the destination asked for is not a hit, and **only a
player-sized request derives the palette** — deriving colours from a 48dp row
thumbnail would repaint the whole surface every time a different row scrolled past.

## 9. Measured on the device

Wireless ADB, `CRT-LX3`, Android 14 / SDK 34, 1080×2388 @480 dpi, build
`-PtunefoldYoutube=true`.

| What | How | Result |
| --- | --- | --- |
| Cold start to resumed | `am start` → `dumpsys activity` resumed | **~498 ms** |
| Navigation, 12 switches over the 4 destinations | `dumpsys gfxinfo` reset, tap×12 | 364 frames, **4.1% janky**, p50 **10 ms**, p90 **15 ms**, p95 23 ms |
| Scroll, 10 flings through two libraries | `dumpsys gfxinfo` | 58 frames, 10.3% janky, p50 8 ms, p90 34 ms |
| Rows built while scrolling | diagnostics panel | **47** for a 32-track library across five lists |
| Memory, after all of the above | `dumpsys meminfo` | PSS **120.8 MB**, RSS 268.9 MB, 450 views, **1 Activity** |
| Activity recreations while navigating | `dumpsys meminfo` → `Activities` | **1** — destinations are views |

The scroll numbers are worse than the navigation numbers, and the reason is known:
a fling decodes covers on the artwork worker and uploads them on the frame that
lands them. The fix is not to draw faster, it is to predict the next window; the
prefetch rows already narrow it to one decode ahead.

## 10. Three defects this phase produced, and how they were found

| # | Symptom | Cause | Fix |
| --- | --- | --- | --- |
| 1 | Lists filled their whole height and showed **nothing** | `submit()` released the rows but left the window memo intact, so the "the viewport did not move" shortcut skipped the rebind against data that no longer existed | `submit()` invalidates every memo of the old window, and asks the host for a fresh one |
| 2 | **Nothing inside the scroller was clickable** | `ScrollHost.onInterceptTouchEvent` claimed `ACTION_DOWN` whenever the content could scroll, so every button in the content was a dead pixel | the host watches the gesture and decides at the first move past the slop |
| 3 | Every row's overflow button did nothing | the pool had an artwork loader but no overflow host, so rows were built with a button that reached nothing | the pool carries both, and a row without them would have a control that looks functional and is not |

The first one is the reason this report has a `windowFor(...)` unit test, and the
second is the reason the intercept rule is documented where it is: both are invisible
to a compiler and obvious only on a device.

## 11. Verified by hand, on the device

```
Home → Search → type "radiohead" → results (20, with covers, durations, overflow)
      → play → mini player appears, destination unchanged
      → tap strip → full player, navigation gone
      → back → exactly the same screen
Home → L1K3D (15) → Descargadas (17) → Search → the 20 results are still there
Search → back → returns to the destination it was opened from
Overflow on a liked + downloaded track → "Remove from L1K3D", "Remove download"
Rotation → portrait and landscape both lay out, navigation stays at the bottom
```

## 12. Open

* Scroll jank is dominated by cover decoding during a fling; the next step is to
  predict the next window rather than widen the prefetch.
* `MAX_TRACKS` (500) truncates a library rather than paginating it. A destination
  that shows the first 500 of 2,000 should say so instead of ending quietly.
* Recommendations are hidden when a provider has none for the current track, which
  is honest but silent: "nothing to suggest" deserves a line of its own.
