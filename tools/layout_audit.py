#!/usr/bin/env python3
"""Audit an uiautomator dump against the window the app was actually given.

    layout_audit.py <dump.xml> <label> <width_px> <height_px>

Prints the nodes whose bounds leave that window, and the ones that are drawn with
no area at all. Both are invisible to a compiler and obvious only on a device: a
control that does not fit is a control the user cannot press, and a control with
no area is a control that looks present and is not.
"""
import re
import sys
import xml.etree.ElementTree as ET

BOUNDS = re.compile(r"\[(-?\d+),(-?\d+)\]\[(-?\d+),(-?\d+)\]")


def box(node):
    match = BOUNDS.match(node.get("bounds", ""))
    return tuple(int(value) for value in match.groups()) if match else None


def label_of(node):
    return node.get("content-desc") or node.get("text") or ""


def main() -> int:
    if len(sys.argv) < 5:
        print("usage: layout_audit.py <dump.xml> <label> <width_px> <height_px>")
        return 1
    path, label = sys.argv[1], sys.argv[2]
    width, height = int(sys.argv[3]), int(sys.argv[4])
    root = ET.parse(path).getroot()
    outside = []
    empty = []
    total = 0
    for node in root.iter("node"):
        if node.get("package") != "com.tunefold.app":
            continue
        name = label_of(node)
        bounds = box(node)
        if not name or bounds is None:
            continue
        total += 1
        node_width, node_height = bounds[2] - bounds[0], bounds[3] - bounds[1]
        if node_width <= 0 or node_height <= 0:
            empty.append("EMPTY %-44s %dx%d" % (name[:44], node_width, node_height))
        elif (bounds[0] < -1 or bounds[1] < -1
              or bounds[2] > width + 1 or bounds[3] > height + 1):
            outside.append("OUT   %-44s [%d,%d][%d,%d] %dx%d"
                           % (name[:44], bounds[0], bounds[1], bounds[2], bounds[3],
                              node_width, node_height))
    print("  %s: window=%dx%d labelled=%d outside=%d empty=%d"
          % (label, width, height, total, len(outside), len(empty)))
    for line in outside + empty:
        print("   ", line)
    return 0


if __name__ == "__main__":
    sys.exit(main())