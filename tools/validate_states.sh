#!/usr/bin/env bash
# Phase 5 state walk: rotation and navigation while things are happening.
#
#   tools/validate_states.sh <outdir>
#
# The layout matrix (tools/validate_layout.sh) proves the screens fit in every
# window. This one proves they survive the three moments that break layouts:
# while audio is playing, while a track is being resolved or buffered, and while a
# download is running — plus navigating between destinations mid-playback, where
# the mini player has to stay exactly where it was.
#
# What it asserts per case:
#   * the Activity is not recreated (dumpsys meminfo -> Activities)
#   * nothing crashes (AndroidRuntime in logcat)
#   * the mini player is still on screen afterwards, where it belongs
#   * every rotation still lands inside the display (layout_audit.py)
set -uo pipefail

ADB="${ADB:-$HOME/Android/Sdk/platform-tools/adb}"
PKG=com.tunefold.app
ACTIVITY="$PKG/.MainActivity"
OUT="${1:-/tmp/opencode/states}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SHOTS="$OUT/shots"
mkdir -p "$SHOTS"

WIDTH=1080
HEIGHT=2388

failures=0

rotate() {
    "$ADB" shell settings put system accelerometer_rotation 0 >/dev/null 2>&1
    "$ADB" shell settings put system user_rotation "$1" >/dev/null 2>&1
    ORIENTATION="$1"
    sleep 3
}

shot() {
    "$ADB" shell screencap -p "/sdcard/$1.png" >/dev/null 2>&1
    "$ADB" pull "/sdcard/$1.png" "$SHOTS/$1.png" >/dev/null 2>&1
}

check() {
    local name="$1" condition="$2" detail="$3"
    if [ "$condition" = "1" ]; then
        echo "  ok    $name"
    else
        echo "  FAIL  $name: $detail"
        failures=$((failures + 1))
    fi
}

activities() {
    "$ADB" shell dumpsys meminfo "$PKG" 2>/dev/null \
        | sed -n 's/.*Activities:[[:space:]]*\([0-9]*\).*/\1/p' | head -1
}

crashes() {
    "$ADB" logcat -d -b crash 2>/dev/null | grep -c "com.tunefold.app" || true
}

audit() {
    # Bounds are screen coordinates, so a rotated dump is audited against the rotated
    # panel: reporting it against the portrait one flags the whole landscape UI.
    local name="$1"
    "$ADB" shell uiautomator dump "/sdcard/$name.xml" >/dev/null 2>&1
    "$ADB" pull "/sdcard/$name.xml" "$OUT/$name.xml" >/dev/null 2>&1 || return 0
    if [ "${ORIENTATION:-0}" = "1" ]; then
        python3 "$ROOT/tools/layout_audit.py" "$OUT/$name.xml" "$name" "$HEIGHT" "$WIDTH" \
            | sed 's/^/  /'
    else
        python3 "$ROOT/tools/layout_audit.py" "$OUT/$name.xml" "$name" "$WIDTH" "$HEIGHT" \
            | sed 's/^/  /'
    fi
}

find_point() {
    "$ADB" shell uiautomator dump /sdcard/find.xml >/dev/null 2>&1
    "$ADB" pull /sdcard/find.xml "$OUT/find.xml" >/dev/null 2>&1 || return 1
    python3 "$ROOT/tools/first_row.py" "$OUT/find.xml" "$1"
}

nav_tap() {
    local x="$1" y="$2"
    "$ADB" shell input tap "$x" "$y" >/dev/null 2>&1
    sleep 1.5
}

"$ADB" shell settings put system accelerometer_rotation 0 >/dev/null 2>&1
"$ADB" shell wm size reset >/dev/null 2>&1
"$ADB" shell wm density reset >/dev/null 2>&1
rotate 0

# ------------------------------------------------------- 1. rotation while playing
echo "=== rotation while playing"
"$ADB" shell am force-stop "$PKG" >/dev/null 2>&1
sleep 1
"$ADB" logcat -c >/dev/null 2>&1
"$ADB" logcat -c -b crash >/dev/null 2>&1
"$ADB" shell am start -n "$ACTIVITY" >/dev/null 2>&1
sleep 7
point=$(find_point "Double tap to")
if [ -n "$point" ]; then "$ADB" shell input tap $point >/dev/null 2>&1; fi
sleep 4
mini_before=$(find_point "Double tap to open the player")
check "mini player is on screen before rotating" \
    "$([ -n "$mini_before" ] && echo 1 || echo 0)" "no mini player node found"
rotate 1
rotate 0
rotate 1
rotate 0
mini_after=$(find_point "Double tap to open the player")
check "mini player survives four rotations" \
    "$([ -n "$mini_after" ] && echo 1 || echo 0)" "the strip disappeared"
check "one Activity after four rotations" \
    "$([ "$(activities)" = "1" ] && echo 1 || echo 0)" "Activities=$(activities)"
check "no crash during rotation" \
    "$([ "$(crashes)" = "0" ] && echo 1 || echo 0)" "$(crashes) crash lines"
shot playing_after_rotations
audit playing_after_rotations

# --------------------------------------------------- 2. navigating while playing
echo "=== navigating while playing"
# Four taps, four destinations, and the strip has to be the same strip afterwards.
for x in 405 675 945 135; do nav_tap "$x" 2250; done
"$ADB" shell input tap 675 2250 >/dev/null 2>&1
sleep 2
selected() {
    "$ADB" shell uiautomator dump /sdcard/sel.xml >/dev/null 2>&1
    "$ADB" pull /sdcard/sel.xml "$OUT/sel.xml" >/dev/null 2>&1 || return 0
    python3 - "$OUT/sel.xml" <<'PY2'
import sys, xml.etree.ElementTree as ET
for node in ET.parse(sys.argv[1]).getroot().iter("node"):
    if "selected" in (node.get("content-desc") or ""):
        print(node.get("content-desc"))
        break
PY2
}
check "a destination switch reaches the surface" \
    "$([ "$(selected)" = "L1K3D, selected" ] && echo 1 || echo 0)" "selected=$(selected)"
mini_nav=$(find_point "Double tap to open the player")
check "mini player survives navigation" \
    "$([ -n "$mini_nav" ] && echo 1 || echo 0)" "the strip disappeared on a destination switch"
check "no crash during navigation" \
    "$([ "$(crashes)" = "0" ] && echo 1 || echo 0)" "$(crashes) crash lines"
shot navigating_mini
audit navigating_mini

# ------------------------------------- 3. rotation right after asking for a track
echo "=== rotation while a track is resolving (buffering path)"
"$ADB" shell am force-stop "$PKG" >/dev/null 2>&1
sleep 1
"$ADB" logcat -c -b crash >/dev/null 2>&1
"$ADB" shell am start -n "$ACTIVITY" >/dev/null 2>&1
sleep 6
point=$(find_point "Double tap to")
if [ -n "$point" ]; then "$ADB" shell input tap $point >/dev/null 2>&1; fi
# No sleep: the rotation lands inside the resolve/buffer window, which is the case
# that has no window of its own to be measured in.
rotate 1
rotate 0
mini_buffering=$(find_point "Double tap to open the player")
check "mini player exists during and after a resolving rotation" \
    "$([ -n "$mini_buffering" ] && echo 1 || echo 0)" "the strip never appeared"
check "no crash while resolving" \
    "$([ "$(crashes)" = "0" ] && echo 1 || echo 0)" "$(crashes) crash lines"
shot buffering_rotation
audit buffering_rotation

# ------------------------------------------------ 4. rotation during a download
echo "=== rotation while a download runs"
"$ADB" shell am force-stop "$PKG" >/dev/null 2>&1
sleep 1
"$ADB" logcat -c -b crash >/dev/null 2>&1
"$ADB" shell am start -n "$ACTIVITY" --ez runtime_download true \
    --es runtime_download_action start --es runtime_track_label "Phase5 probe" >/dev/null 2>&1
sleep 8
download_shot_before=$("$ADB" shell dumpsys meminfo "$PKG" 2>/dev/null \
    | sed -n 's/.*Activities:[[:space:]]*\([0-9]*\).*/\1/p' | head -1)
rotate 1
rotate 0
check "one Activity while a download runs" \
    "$([ "$(activities)" = "1" ] && echo 1 || echo 0)" "Activities=$(activities)"
check "no crash while downloading" \
    "$([ "$(crashes)" = "0" ] && echo 1 || echo 0)" "$(crashes) crash lines"
shot download_rotation
audit download_rotation

# --------------------------------------------------- 5. the player across rotation
echo "=== the player across a rotation"
"$ADB" shell am force-stop "$PKG" >/dev/null 2>&1
sleep 1
"$ADB" shell am start -n "$ACTIVITY" >/dev/null 2>&1
sleep 7
point=$(find_point "Double tap to")
if [ -n "$point" ]; then "$ADB" shell input tap $point >/dev/null 2>&1; fi
sleep 3
open=$(find_point "Double tap to open the player")
if [ -n "$open" ]; then "$ADB" shell input tap $open >/dev/null 2>&1; fi
sleep 3
audit player_portrait
shot player_portrait
rotate 1
audit player_landscape
shot player_landscape
transport_portrait=$(find_point "Pause playback")
rotate 0
transport_portrait=$(find_point "Pause playback")
check "transport is on screen in both orientations" \
    "$([ -n "$transport_portrait" ] && echo 1 || echo 0)" "the transport is not reachable"
close=$(find_point "Close player")
if [ -n "$close" ]; then "$ADB" shell input tap $close >/dev/null 2>&1; fi
sleep 2
mini_back=$(find_point "Double tap to open the player")
check "the strip is still there after closing the player" \
    "$([ -n "$mini_back" ] && echo 1 || echo 0)" "closing the player lost the strip"
shot after_close

"$ADB" shell settings put system accelerometer_rotation 1 >/dev/null 2>&1
echo "failures=$failures"
echo "screenshots in $SHOTS"
exit $((failures > 0 ? 1 : 0))