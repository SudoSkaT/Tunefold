#!/usr/bin/env bash
# Phase 5 measurement battery for the Tunefold Android UI.
#
#   tools/measure_layout.sh <label>
#
# Runs the same sequence twice (once per build) so "it got better" is a number and
# not an impression. Everything it prints comes from the device; nothing is
# estimated. Coordinates are derived from the layout constants of this app at
# 1080x2388 @480dpi and are documented next to each use.
set -uo pipefail

ADB="${ADB:-$HOME/Android/Sdk/platform-tools/adb}"
PKG=com.tunefold.app
ACTIVITY="$PKG/.MainActivity"
LABEL="${1:-run}"
OUT="${OUT:-/tmp/opencode/layout-$LABEL.txt}"
LOG="${LOG:-/tmp/opencode/layout-$LABEL.log}"

# --- geometry of this device (1080x2388 @480dpi, gesture nav, status bar 108px)
NAV_Y=2202          # centre of the bottom navigation strip
NAV_X=(135 405 675 945)
MINI_Y=2028         # centre of the mini player, when something is loaded
PLAYER_CLOSE=(132 204)

step() { printf '\n=== %s\n' "$1" >> "$OUT"; }
note() { printf '%s\n' "$1" >> "$OUT"; }

tap() { "$ADB" shell input tap "$1" "$2" >/dev/null 2>&1; sleep "${3:-0.6}"; }

rotate() { # 0 = portrait, 1 = landscape (90 degrees)
    "$ADB" shell settings put system accelerometer_rotation 0 >/dev/null 2>&1
    "$ADB" shell settings put system user_rotation "$1" >/dev/null 2>&1
    sleep 2
}

wake() {
    "$ADB" shell input keyevent KEYCODE_WAKEUP >/dev/null 2>&1
    "$ADB" shell wm dismiss-keyguard >/dev/null 2>&1
    "$ADB" shell svc power stayon true >/dev/null 2>&1
    sleep 1
}

: > "$OUT"
: > "$LOG"

gfx() { # total frames, janky frames, percentiles
    "$ADB" shell dumpsys gfxinfo "$PKG" 2>/dev/null \
        | grep -E "Total frames rendered|Janky frames|percentile|Number Missed Vsync|Number High input|Number Slow UI thread|Number Slow bitmap|Number Slow issue" \
        | sed 's/^[[:space:]]*//'
}

mem() {
    "$ADB" shell dumpsys meminfo "$PKG" 2>/dev/null \
        | grep -E "^ *(TOTAL|TOTAL PSS|Java Heap|Native Heap|Graphics|Code|TOTAL RSS|Views|Activities|AppContexts|Objects):"
}

threads() {
    local pid
    pid=$("$ADB" shell pidof "$PKG" 2>/dev/null | tr -d '\r')
    [ -z "$pid" ] && { note "threads=?"; return; }
    "$ADB" shell cat "/proc/$pid/status" 2>/dev/null | grep -E "^Threads:" >> "$OUT"
}

# ---------------------------------------------------------------- 1. startup
step "1. cold start (am start -W, 5 runs)"
"$ADB" shell am force-stop "$PKG" >/dev/null 2>&1
sleep 1
for run in 1 2 3 4 5; do
    "$ADB" shell am start -W -n "$ACTIVITY" 2>/dev/null \
        | grep -E "TotalTime|WaitTime" | tr '\n' ' ' >> "$OUT"
    note ""
    "$ADB" shell am force-stop "$PKG" >/dev/null 2>&1
    sleep 1
done

# --------------------------------------------------------- 2. steady state
step "2. idle on Home (nothing playing)"
"$ADB" shell am force-stop "$PKG" >/dev/null 2>&1
sleep 1
"$ADB" logcat -c >/dev/null 2>&1
"$ADB" shell am start -n "$ACTIVITY" >/dev/null 2>&1
sleep 5
note "--- memory"
mem >> "$OUT"
threads >> "$OUT"

# The churn test: a screen that draws while nothing changes is spending battery
# and waking the GPU for nothing. Frames counted over a quiet ten seconds.
step "2b. frames drawn while nothing happens (10s on Home, nothing playing)"
"$ADB" shell dumpsys gfxinfo "$PKG" reset >/dev/null 2>&1
sleep 10
note "--- gfx"
gfx >> "$OUT"

# ----------------------------------------------------------- 3. navigation
step "3. navigation, 12 switches over the 4 destinations"
"$ADB" shell dumpsys gfxinfo "$PKG" reset >/dev/null 2>&1
for pass in 1 2 3; do
    for index in 0 1 2 3; do
        tap "${NAV_X[$index]}" "$NAV_Y"
    done
done
note "--- gfx"
gfx >> "$OUT"
note "--- memory after navigating"
mem >> "$OUT"
threads >> "$OUT"

# --------------------------------------------------------- 4. rotation x6
step "4. rotation, 3 round trips, nothing playing"
"$ADB" shell dumpsys gfxinfo "$PKG" reset >/dev/null 2>&1
for pass in 1 2 3; do
    rotate 1
    rotate 0
done
note "--- gfx across 6 rotations"
gfx >> "$OUT"
note "--- Activity instances after rotating"
"$ADB" shell dumpsys meminfo "$PKG" 2>/dev/null | grep -E "Activities" >> "$OUT"
note "--- configuration-changed deliveries (logcat)"
"$ADB" logcat -d -s Tunefold:* AndroidRuntime:E 2>/dev/null | tail -5 >> "$OUT"

# ------------------------------------------- 5. rotation while the player plays
step "5. rotation with the player open (no audio source attached)"
"$ADB" shell input tap 540 "$MINI_Y" >/dev/null 2>&1 || true
sleep 2
"$ADB" shell dumpsys gfxinfo "$PKG" reset >/dev/null 2>&1
rotate 1
rotate 0
rotate 1
rotate 0
note "--- gfx across 4 rotations with the player open"
gfx >> "$OUT"
note "--- Activity instances"
"$ADB" shell dumpsys meminfo "$PKG" 2>/dev/null | grep -E "Activities" >> "$OUT"
note "--- memory"
mem >> "$OUT"
threads >> "$OUT"

# ------------------------------------------------------------- 6. GC + churn
step "6. garbage collection while navigating"
"$ADB" logcat -c >/dev/null 2>&1
for pass in 1 2 3; do
    for index in 0 1 2 3 2 1; do
        tap "${NAV_X[$index]}" "$NAV_Y"
    done
done
sleep 2
GC_LINES=$("$ADB" logcat -d -s art 2>/dev/null | grep -c "GC freed")
GC_BYTES=$("$ADB" logcat -d -s art 2>/dev/null | grep "GC freed" \
    | grep -oE "[0-9]+K" | tr -d 'K' | awk '{s+=$1} END {print s+0}')
note "gc_events=$GC_LINES gc_freed_kb=$GC_BYTES"
note "--- memory after navigating"
mem >> "$OUT"

# ---------------------------------------------------------------- 7. summary
step "7. summary"
note "label=$LABEL"
"$ADB" shell dumpsys meminfo "$PKG" 2>/dev/null \
    | grep -E "TOTAL PSS|TOTAL RSS|Views|Activities" >> "$OUT"
threads >> "$OUT"

rotate 0
"$ADB" shell settings put system accelerometer_rotation 1 >/dev/null 2>&1

echo "wrote $OUT"
cat "$OUT"