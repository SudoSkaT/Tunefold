#!/usr/bin/env python3
"""Centre of the first node whose description contains a marker, as "X Y".

    first_row.py <dump.xml> [marker]

Used by tools/validate_layout.sh to tap the same thing on a 720x1280 phone and on a
2560x1600 tablet. A row announces itself as "... Double tap to play"; the player
closes through "... Close player". Finding both through the accessibility tree is
what keeps the validation walk independent of any window's coordinates.
"""
import re
import sys
import xml.etree.ElementTree as ET

BOUNDS = re.compile(r"\[(-?\d+),(-?\d+)\]\[(-?\d+),(-?\d+)\]")
DEFAULT_MARKER = "Double tap to play"


def main() -> int:
    if len(sys.argv) < 2:
        print("", end="")
        return 1
    marker = sys.argv[2] if len(sys.argv) > 2 else DEFAULT_MARKER
    root = ET.parse(sys.argv[1]).getroot()
    for node in root.iter("node"):
        if node.get("package") != "com.tunefold.app":
            continue
        if marker not in (node.get("content-desc") or ""):
            continue
        match = BOUNDS.match(node.get("bounds", ""))
        if not match:
            continue
        x1, y1, x2, y2 = (int(value) for value in match.groups())
        print((x1 + x2) // 2, (y1 + y2) // 2)
        return 0
    return 1


if __name__ == "__main__":
    sys.exit(main())