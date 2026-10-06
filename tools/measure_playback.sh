#!/usr/bin/env bash
# Reproducible Android playback measurement for Tunefold.
#
#   ./tools/measure_playback.sh cold 5   # Case A: no useful caches
#   ./tools/measure_playback.sh warm 5   # Case B: caches already populated
#
# Extracts the ordered TunefoldPerf timeline for every run and reports
# min / median / p95 / max for TTFA and each stage.
set -uo pipefail

ADB="${ADB:-$HOME/Android/Sdk/platform-tools/adb}"
PKG=com.tunefold.app
ACTIVITY="$PKG/.MainActivity"
VIDEO="${TUNEFOLD_VIDEO:-dQw4w9WgXcQ}"
URL="${TUNEFOLD_URL:-https://www.youtube.com/watch?v=$VIDEO}"
MODE="${1:-cold}"
RUNS="${2:-5}"
SETTLE="${SETTLE:-9}"
OUT="${OUT:-/tmp/opencode/playback-$MODE-$RUNS.log}"

# Honor/Huawei builds filter INFO logs per tag by default; without this the
# app emits nothing to logcat and every run looks like a silent hang.
for tag in TunefoldPerf Tunefold TunefoldCache; do
    "$ADB" shell setprop "log.tag.$tag" I >/dev/null 2>&1
done

: > "$OUT"
for i in $(seq 1 "$RUNS"); do
    "$ADB" shell am force-stop "$PKG" >/dev/null 2>&1
    if [ "$MODE" = cold ]; then
        "$ADB" shell pm clear "$PKG" >/dev/null 2>&1
    fi
    sleep 2
    "$ADB" logcat -c >/dev/null 2>&1
    "$ADB" shell am start -n "$ACTIVITY" --ez runtime_smoke_test true \
        --es runtime_stream_url "$URL" >/dev/null 2>&1
    sleep "$SETTLE"
    {
        echo "### run=$i mode=$MODE"
        "$ADB" logcat -d -s TunefoldPerf:* 2>/dev/null \
            | sed -n 's/.*TunefoldPerf: *//p' | sed 's/^track=[^ ]* *//'
        echo
    } >> "$OUT"
done

python3 - "$OUT" "$MODE" <<'PY'
import re, sys, statistics

path, mode = sys.argv[1], sys.argv[2]
runs, current = [], None
for line in open(path):
    if line.startswith("### run="):
        if current: runs.append(current)
        current = {"events": {}, "ranges": []}
        continue
    stripped = line.strip()
    m = re.match(r"^TTFA (\d+)ms$", stripped)
    if m and current is not None:
        current["events"]["TTFA"] = int(m.group(1))
        continue
    m = re.match(r"^(\S+)(?: (.*?))? @(\d+)ms", stripped)
    if not m or current is None: continue
    event, detail, at = m.group(1), m.group(2) or "", int(m.group(3))
    if event == "RANGE_REQUEST":
        d = dict(re.findall(r"(\w+)=(\S+)", detail))
        current["ranges"].append((at, d))
    else:
        current["events"].setdefault(event, at)
if current: runs.append(current)

def at(run, name):
    return run["events"].get(name)

def pct(values, p):
    if not values: return None
    values = sorted(values)
    if len(values) == 1: return values[0]
    k = (len(values) - 1) * p
    lo, hi = int(k), min(int(k) + 1, len(values) - 1)
    return values[lo] + (values[hi] - values[lo]) * (k - lo)

def stats(values):
    values = [v for v in values if v is not None]
    if not values: return None
    return dict(n=len(values), min=min(values), med=round(statistics.median(values)),
                p95=round(pct(values, 0.95)), max=max(values))

def gap(run, a, b):
    x, y = at(run, a), at(run, b)
    return None if x is None or y is None else y - x

stages = [
    ("TTFA", lambda r: r["events"].get("TTFA")),
    ("metadata", lambda r: gap(r, "METADATA_RESOLUTION_START", "METADATA_AVAILABLE")),
    ("source_resolution", lambda r: gap(r, "SOURCE_RESOLUTION_START", "PLAYABLE_SOURCE_AVAILABLE")),
    ("http_open", lambda r: gap(r, "HTTP_OPEN_START", "HTTP_FIRST_RESPONSE")),
    ("probe", lambda r: gap(r, "SYMPHONIA_PROBE_START", "SYMPHONIA_PROBE_END")),
    ("first_pcm", lambda r: at(r, "DECODER_FIRST_PCM")),
    ("first_write", lambda r: at(r, "AUDIOTRACK_FIRST_POSITIVE_WRITE")),
    ("output_startup", lambda r: gap(r, "DECODER_FIRST_PCM", "AUDIOTRACK_FIRST_POSITIVE_WRITE")),
    ("range_requests", lambda r: len(r["ranges"])),
    ("range_bytes", lambda r: sum(int(d.get("received", 0)) for _, d in r["ranges"])),
    ("seek_requests", lambda r: sum(1 for _, d in r["ranges"] if d.get("from_seek") == "true")),
    ("range_elapsed_ms", lambda r: sum(int(d.get("total_us", 0)) for _, d in r["ranges"]) // 1000),
]

print(f"== {mode} ({len(runs)} runs) ==")
print(f"{'stage':<20}{'min':>9}{'median':>9}{'p95':>9}{'max':>9}")
for name, fn in stages:
    s = stats([fn(r) for r in runs])
    if not s:
        print(f"{name:<20}{'-':>9}{'-':>9}{'-':>9}{'-':>9}")
        continue
    print(f"{name:<20}{s['min']:>9}{s['med']:>9}{s['p95']:>9}{s['max']:>9}")
PY
echo "raw trace: $OUT"