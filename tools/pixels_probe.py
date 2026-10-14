#!/usr/bin/env python3
"""Where did the content actually land on screen?

    pixels_probe.py

Accessibility bounds are computed from the view tree and its scroll offsets; the
pixels are what the user sees. They are not required to agree, so the only way to
know whether scrolling works is to read the framebuffer.

Profiles the brightness of every pixel row and reports, for each content landmark,
the screen row where it appears. Used to answer: after a known drag of the finger,
how far did the drawn content move?
"""
import os
import struct
import subprocess
import tempfile
import sys
import zlib

ADB = os.environ.get("ADB", os.path.expanduser("~/Android/Sdk/platform-tools/adb"))
WORK = os.environ.get("TUNEFOLD_WORK", tempfile.gettempdir())


def adb(*args):
    return subprocess.run([ADB, *args], capture_output=True, text=True).stdout


def shot(name):
    adb("shell", "screencap", "-p", f"/sdcard/{name}.png")
    adb("pull", f"/sdcard/{name}.png", os.path.join(WORK, f"{name}.png"))
    return os.path.join(WORK, f"{name}.png")


def read_png(path):
    data = open(path, "rb").read()
    i, idat = 8, b""
    width = height = color = None
    while i < len(data):
        length = struct.unpack(">I", data[i:i + 4])[0]
        kind = data[i + 4:i + 8]
        if kind == b"IHDR":
            width, height, _, color = struct.unpack(">IIBB", data[i + 8:i + 18])
        elif kind == b"IDAT":
            idat += data[i + 8:i + 8 + length]
        elif kind == b"IEND":
            break
        i += 12 + length
    raw = zlib.decompress(idat)
    channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[color]
    stride = width * channels
    out = bytearray()
    previous = bytearray(stride)
    pos = 0
    for _ in range(height):
        filt = raw[pos]
        pos += 1
        line = bytearray(raw[pos:pos + stride])
        pos += stride
        for x in range(stride):
            a = line[x - channels] if x >= channels else 0
            b = previous[x]
            c = previous[x - channels] if x >= channels else 0
            if filt == 1:
                line[x] = (line[x] + a) & 255
            elif filt == 2:
                line[x] = (line[x] + b) & 255
            elif filt == 3:
                line[x] = (line[x] + (a + b) // 2) & 255
            elif filt == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pred = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[x] = (line[x] + pred) & 255
        out += line
        previous = line
    return width, height, channels, bytes(out)


def landmarks(path):
    """Screen rows of the header text, of the artwork tiles and of the nav bar."""
    width, height, channels, pixels = read_png(path)
    # Bright pixels are text; the artwork tiles are the only saturated blocks.
    rows_text = []
    rows_art = []
    for y in range(0, height, 2):
        bright = 0
        art = 0
        base = y * width * channels
        for x in range(0, width, 6):
            offset = base + x * channels
            r, g, b = pixels[offset], pixels[offset + 1], pixels[offset + 2]
            if r + g + b > 300:
                bright += 1
            # A cover is colourful and mid-bright; the empty placeholder is flat.
            if max(r, g, b) - min(r, g, b) > 28 and r + g + b > 150:
                art += 1
        rows_text.append(bright)
        rows_art.append(art)
    # The nav bar is the one thing that never scrolls: the bottom-most bright band.
    nav = None
    for y in range(height // 2, height, 2):
        if rows_text[y // 2] > 3:
            nav = y
    return {"header": first_band(rows_text, 0, nav or height),
            "covers": cover_rows(rows_art, 0, nav or height),
            "nav": nav}


def first_band(profile, low, high, threshold=3):
    runs = []
    start = None
    for y in range(low // 2, high // 2):
        if profile[y] >= threshold:
            if start is None:
                start = y * 2
        elif start is not None:
            runs.append((start, y * 2))
            start = None
    if start is not None:
        runs.append((start, high))
    return [r for r in runs if r[1] - r[0] >= 6][:6]


def cover_rows(profile, low, high):
    runs = []
    start = None
    for y in range(low // 2, high // 2):
        if profile[y] > 3:
            if start is None:
                start = y * 2
        elif start is not None:
            if y * 2 - start >= 40:
                runs.append(start)
            start = None
    return runs


def main() -> int:
    print("A:", landmarks(shot("px_a")))
    adb("shell", "input", "swipe", "540", "1800", "540", "1400", "900")
    import time
    time.sleep(2.5)
    print("B:", landmarks(shot("px_b")))
    return 0


if __name__ == "__main__":
    sys.exit(main())