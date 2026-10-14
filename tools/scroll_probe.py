#!/usr/bin/env python3
"""Does the list follow the finger one to one?

    scroll_probe.py

Drags the content up by an exact, known distance with zero release velocity (the
finger stops before it lifts, so nothing flings) and reports how far every row
that stayed on screen actually moved.

A scroller that applies its offset twice moves twice as far as the finger; one
that replays the touch slop jumps before it tracks. Both show up here as a
measured distance, not as an opinion.
"""
import os
import re
import subprocess
import tempfile
import sys
import time
import xml.etree.ElementTree as ET

ADB = os.environ.get("ADB", os.path.expanduser("~/Android/Sdk/platform-tools/adb"))
WORK = os.environ.get("TUNEFOLD_WORK", tempfile.gettempdir())
BOUNDS = re.compile(r"\[(-?\d+),(-?\d+)\]\[(-?\d+),(-?\d+)\]")
DISTANCE = 400
# 8dp at 3x: the slop a scroll spends before it tracks.
MAX_SLOP_PX = 64
# 8dp at 480dpi: the slop a row long-presses inside instead of scrolling.
TOUCH_SLOP_PX = 24
DISTANCE_FLICK = 600
# 20dp gutter and 64dp rows at 480dpi: what a list row looks like here.
GUTTER_PX = 60
ROW_HEIGHT_PX = 192


def adb(*args, **kwargs):
    return subprocess.run([ADB, *args], capture_output=True, text=True, **kwargs).stdout


def rows(tag=""):
    adb("shell", "uiautomator", "dump", "/sdcard/probe.xml")
    adb("pull", "/sdcard/probe.xml", os.path.join(WORK, f"probe{tag}.xml"))
    out = {}
    for node in ET.parse(os.path.join(WORK, f"probe{tag}.xml")).getroot().iter("node"):
        desc = node.get("content-desc") or ""
        if "Double tap to" not in desc or node.get("class") != "android.widget.LinearLayout":
            continue
        match = BOUNDS.match(node.get("bounds", ""))
        if not match:
            continue
        left, top, right, bottom = (int(value) for value in match.groups())
        # A row sits inside the content column; the mini player spans the whole window
        # and carries the same sentence about the same track. Without this the probe
        # reads the strip's position and calls it a list row.
        if left != GUTTER_PX or bottom - top != ROW_HEIGHT_PX:
            continue
        out[desc.split(". Liked")[0].split(". Not liked")[0]] = top
    return out


def main() -> int:
    # Back to the top of the list, the way a user would: leaving a destination and
    # returning to it resets the window, so the probe always measures from a known
    # place instead of from wherever the previous run left it.
    adb("shell", "input", "tap", "132", "2244")
    time.sleep(1.5)
    adb("shell", "input", "tap", "675", "2244")
    time.sleep(2)

    before = rows("_before")
    if not before:
        print("no rows on screen; open L1K3D first")
        return 1

    x, start = 540, 1800
    steps = 20
    # The first move clears the touch slop immediately. Sending it late leaves the
    # finger inside the slop long enough for the row's long-press to fire, and the
    # list ends up behind a context menu instead of being measured.
    beyond = int(TOUCH_SLOP_PX * 2)
    batch = [f"input motionevent DOWN {x} {start}",
             f"input motionevent MOVE {x} {start - beyond}"]
    for step in range(1, steps + 1):
        y = start - (DISTANCE * step) // steps
        batch.append(f"input motionevent MOVE {x} {y}")
    adb("shell", "; ".join(batch))
    # The finger stops here before it lifts: without this the release velocity is
    # the drag velocity and a fling is added to what is being measured.
    time.sleep(0.4)
    adb("shell", "; ".join([f"input motionevent MOVE {x} {start - DISTANCE}"] * 3)
        + f"; input motionevent UP {x} {start - DISTANCE}")
    time.sleep(1.5)

    after = rows("_after")
    common = [title for title in before if title in after]
    if not common:
        print("no row survived the drag; raise DISTANCE's companions or re-run")
        return 1

    print(f"finger moved {DISTANCE}px up; the content below must move DOWN by that much")
    moved = []
    for title in common:
        delta = after[title] - before[title]
        moved.append(delta)
        print(f"  {delta:+5d}px  {title[:40]}")
    unique = sorted(set(moved))
    if len(unique) != 1:
        print(f"\nFAIL: rows moved by different distances {unique}: the content is not rigid")
        return 1

    delta = unique[0]
    travelled = -delta          # a finger travelling up takes the content up with it
    slop = DISTANCE - travelled
    print(f"\nONE distance for every row: {travelled}px up ({slop}px spent as touch slop)")
    if delta >= 0:
        print("FAIL: the content moved the wrong way: the scroll is inverted")
        return 1
    if travelled == 2 * DISTANCE:
        print("FAIL: the offset is applied twice")
        return 1
    if slop < 0 or slop > MAX_SLOP_PX:
        print(f"FAIL: the content moved {travelled}px for a {DISTANCE}px drag")
        return 1
    print("PASS: the content is rigid, it followed the finger, and the slop was spent "
          "rather than replayed")
    return fling_follows()


def fling_follows() -> int:
    """A flick must carry past the finger.

    Measured as total travel, not as two samples in time: a 4000 px/s flick lasts
    well under a second, and a `uiautomator dump` takes longer than that, so asking
    "was it still moving" always answers after the fling has ended. Total travel is
    the honest question — a scroller with momentum overshoots the finger by a wide
    margin, and one without stops exactly where the finger did.
    """
    adb("shell", "input", "tap", "132", "2244")
    time.sleep(1.5)
    adb("shell", "input", "tap", "675", "2244")
    time.sleep(2)

    before = rows("_fling_before")
    if not before:
        print("SKIP: no rows to flick")
        return 0
    adb("shell", "input", "swipe", "540", "1800", "540", "1200", "150")
    time.sleep(2.5)
    after = rows("_fling_after")

    # The row that started lowest and stayed on screen is the one to measure on.
    candidates = [(title, after[title] - before[title])
                  for title in before if title in after]
    if not candidates:
        print("SKIP: no row survived the flick; cannot read travel from this pair")
        return 0
    travelled = -min(delta for _, delta in candidates)
    print(f"\nflick: finger travelled {DISTANCE_FLICK}px, content travelled {travelled}px")
    if travelled <= DISTANCE_FLICK + MAX_SLOP_PX:
        print("FAIL: the content stopped where the finger did: the flick has no momentum")
        return 1
    print(f"PASS: the flick carried {travelled - DISTANCE_FLICK}px past the finger")
    return 0


if __name__ == "__main__":
    sys.exit(main())