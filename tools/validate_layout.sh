#!/usr/bin/env bash
# Phase 5 validation walk: the same screens on every window this app has to survive.
#
#   tools/validate_layout.sh <outdir>
#
# Emulated windows (wm size / wm density), because the phase is about adapting to a
# window and a phone cannot show you a tablet:
#
#   phone-large   1080x2388 @480 ->  360 x 796 dp   COMPACT
#   phone-small    720x1280 @320 ->  360 x  640 dp   COMPACT and short
#   tablet         1600x2560 @240 ->  667 x 1067 dp   MEDIUM
#   tablet-wide    2560x1600 @200 -> 1280 x  800 dp   EXPANDED
#
# For each window it captures the list and the player in portrait and in landscape,
# and then reports the nodes the system says fall outside that window: a control
# whose bounds leave the window is the defect this phase is about.
set -uo pipefail

ADB="${ADB:-$HOME/Android/Sdk/platform-tools/adb}"
PKG=com.tunefold.app
ACTIVITY="$PKG/.MainActivity"
OUT="${1:-/tmp/opencode/validate}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SHOTS="$OUT/shots"

mkdir -p "$SHOTS"

# name -> "WxH @density dp_width dp_height"
WINDOWS=(
    "phone_large|1080x2388 @480|1080|2388"
    "phone_small|720x1280 @320|720|1280"
    "tablet|1600x2560 @240|1600|2560"
    "tablet_wide|2560x1600 @200|2560|1600"
)

apply_window() {
    local spec="$1" size density
    size="${spec% @*}"; density="${spec#*@}"
    "$ADB" shell wm size "$size" >/dev/null 2>&1
    "$ADB" shell wm density "$density" >/dev/null 2>&1
    sleep 3
}

rotate() {
    "$ADB" shell settings put system accelerometer_rotation 0 >/dev/null 2>&1
    "$ADB" shell settings put system user_rotation "$1" >/dev/null 2>&1
    ORIENTATION="$1"
    sleep 3
}

shot() {
    "$ADB" shell screencap -p "/sdcard/$1.png" >/dev/null 2>&1
    "$ADB" pull "/sdcard/$1.png" "$SHOTS/$1.png" >/dev/null 2>&1
    echo "  shot $1.png"
}

# Dumps the tree and reports what leaves the display.
#
# The reference is the emulated panel, not the app's own window: accessibility bounds
# are screen coordinates, so auditing against the window (the panel minus the system
# bars) would put the status bar's worth of pixels outside every node. A rotated panel
# is audited with its sides swapped, which is the point of walking both orientations.
offscreen_report() {
    local name="$1" width="$2" height="$3"
    "$ADB" shell uiautomator dump "/sdcard/$name.xml" >/dev/null 2>&1
    "$ADB" pull "/sdcard/$name.xml" "$OUT/$name.xml" >/dev/null 2>&1 || return 0
    if [ "${ORIENTATION:-0}" = "1" ]; then
        python3 "$ROOT/tools/layout_audit.py" "$OUT/$name.xml" "$name" "$height" "$width"
    else
        python3 "$ROOT/tools/layout_audit.py" "$OUT/$name.xml" "$name" "$width" "$height"
    fi
}

start() {
    "$ADB" shell am force-stop "$PKG" >/dev/null 2>&1
    sleep 1
    "$ADB" logcat -c >/dev/null 2>&1
    "$ADB" shell am start -n "$ACTIVITY" >/dev/null 2>&1
    sleep 7
}

# Plays whatever is in the first row of the visible list: with a downloaded library
# that is a local file, so this works without the YouTube provider. The row is found
# through its description rather than through coordinates, so the same code plays on
# a 720x1280 phone and on a 2560x1600 tablet.
play_first_row() {
    "$ADB" shell uiautomator dump /sdcard/row.xml >/dev/null 2>&1
    "$ADB" pull /sdcard/row.xml "$OUT/row.xml" >/dev/null 2>&1 || return 1
    local point
    point=$(python3 "$ROOT/tools/first_row.py" "$OUT/row.xml" "Double tap to")
    if [ -z "$point" ]; then echo "  no playable row found"; return 1; fi
    "$ADB" shell input tap $point >/dev/null 2>&1
    sleep 4
}

# Opens the full player the way the user does: by tapping the strip. A row only
# starts playback — the strip is what opens the player — and a validation walk that
# assumed otherwise was measuring Home with a different file name.
open_player() {
    "$ADB" shell uiautomator dump /sdcard/row.xml >/dev/null 2>&1
    "$ADB" pull /sdcard/row.xml "$OUT/row.xml" >/dev/null 2>&1 || return 1
    local point
    point=$(python3 "$ROOT/tools/first_row.py" "$OUT/row.xml" "Double tap to open the player")
    if [ -z "$point" ]; then echo "  mini player not found"; return 1; fi
    "$ADB" shell input tap $point >/dev/null 2>&1
    sleep 3
}

# Closes the player with the control the user would press, never with a raw BACK:
# a BACK from a destination that is not the player leaves the app, and the walk would
# go on to measure the launcher.
close_player() {
    "$ADB" shell uiautomator dump /sdcard/row.xml >/dev/null 2>&1
    "$ADB" pull /sdcard/row.xml "$OUT/row.xml" >/dev/null 2>&1 || return 1
    local point
    point=$(python3 "$ROOT/tools/first_row.py" "$OUT/row.xml" "Close player")
    [ -z "$point" ] && { echo "  player was not open"; return 1; }
    "$ADB" shell input tap $point >/dev/null 2>&1
    sleep 2
}

"$ADB" shell settings put system accelerometer_rotation 0 >/dev/null 2>&1

for entry in "${WINDOWS[@]}"; do
    IFS='|' read -r preset spec width height <<< "$entry"
    echo "=== $preset ($spec)"
    apply_window "$spec"
    rotate 0
    start
    offscreen_report "${preset}_portrait_home" "$width" "$height"
    shot "${preset}_portrait_home"

    play_first_row
    offscreen_report "${preset}_portrait_mini" "$width" "$height"
    shot "${preset}_portrait_mini"

    open_player
    offscreen_report "${preset}_portrait_player" "$width" "$height"
    shot "${preset}_portrait_player"

    rotate 1
    offscreen_report "${preset}_landscape_player" "$width" "$height"
    shot "${preset}_landscape_player"

    close_player
    offscreen_report "${preset}_landscape_home" "$width" "$height"
    shot "${preset}_landscape_home"

    rotate 0
done

# Leave the device as it was found.
"$ADB" shell wm size reset >/dev/null 2>&1
"$ADB" shell wm density reset >/dev/null 2>&1
"$ADB" shell settings put system accelerometer_rotation 1 >/dev/null 2>&1
echo "screenshots in $SHOTS"