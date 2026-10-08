package com.tunefold.app;

/**
 * A parsed, framework-free icon outline.
 *
 * <p>The icon family is authored as a restricted command dialect instead of SVG
 * path data, for two reasons:
 *
 * <ul>
 *   <li>the framework has no public SVG path parser, and reaching for the hidden
 *       one would make an icon a reflection risk rather than a constant;</li>
 *   <li>keeping the geometry as plain arrays makes it inspectable, so a malformed
 *       icon (an empty outline, a coordinate outside the grid) is a unit test
 *       failure instead of an invisible blank button discovered on a device.</li>
 * </ul>
 *
 * <h3>The dialect</h3>
 * Absolute commands only, uppercase, each introduced by its own letter:
 * <pre>
 *   M x y                 move to
 *   L x y                 line to
 *   C x1 y1 x2 y2 x y     cubic bezier
 *   Q x1 y1 x y           quadratic bezier
 *   Z                     close the current contour
 * </pre>
 * Numbers are separated by spaces or commas and may be negative; a sign always
 * starts the next number, so {@code 1-2} is two numbers. There are no relative
 * commands: an icon is authored once, and absolute coordinates are what make it
 * reviewable next to the {@value DesignTokens#ICON_GRID}&times;
 * {@value DesignTokens#ICON_GRID} grid.
 *
 * <p>Circles are four cubics using the standard {@code k = r * 0.5522847} handle,
 * which is visually indistinguishable from a true arc at icon sizes and needs no
 * arc-to-bezier conversion at load time.
 *
 * <p>Immutable after parsing, and safe to share: {@link IconDrawable} only reads.
 */
final class IconPath {

    /** Move to a new point. Carries {@link #POINTS_PER_SIMPLE} values. */
    static final int MOVE = 0;
    /** Straight segment. Carries {@link #POINTS_PER_SIMPLE} values. */
    static final int LINE = 1;
    /** Cubic bezier. Carries {@link #POINTS_PER_CUBIC} values. */
    static final int CUBIC = 2;
    /** Quadratic bezier. Carries {@link #POINTS_PER_QUAD} values. */
    static final int QUAD = 3;
    /** Close the current contour. Carries no values. */
    static final int CLOSE = 4;

    static final int POINTS_PER_SIMPLE = 2;
    static final int POINTS_PER_CUBIC = 6;
    static final int POINTS_PER_QUAD = 4;

    private final byte[] ops;
    private final int[] offsets;
    private final float[] values;
    private final float minX;
    private final float minY;
    private final float maxX;
    private final float maxY;

    private IconPath(byte[] ops, int[] offsets, float[] values,
                     float minX, float minY, float maxX, float maxY) {
        this.ops = ops;
        this.offsets = offsets;
        this.values = values;
        this.minX = minX;
        this.minY = minY;
        this.maxX = maxX;
        this.maxY = maxY;
    }

    /** An outline with no geometry; also what a malformed definition degrades to. */
    static IconPath empty() {
        return new IconPath(new byte[0], new int[0], new float[0],
                Float.MAX_VALUE, Float.MAX_VALUE, -Float.MAX_VALUE, -Float.MAX_VALUE);
    }

    /** Number of commands in the outline. */
    int size() { return ops.length; }

    /** Operation code of command {@code index}. */
    int opAt(int index) { return ops[index]; }

    /** Index into {@link #values()} where command {@code index} begins. */
    int offsetOf(int index) { return offsets[index]; }

    /** Value of coordinate {@code index}. */
    float valueAt(int index) { return values[index]; }

    float minX() { return minX; }

    float minY() { return minY; }

    float maxX() { return maxX; }

    float maxY() { return maxY; }

    /** True when the outline contains nothing to draw. */
    boolean isEmpty() { return ops.length == 0; }

    /** How many values a command consumes. */
    static int valueCount(int op) {
        switch (op) {
            case CUBIC: return POINTS_PER_CUBIC;
            case QUAD: return POINTS_PER_QUAD;
            case MOVE:
            case LINE: return POINTS_PER_SIMPLE;
            default: return 0;
        }
    }

    /**
     * Parses {@code source}; returns {@link #empty()} for anything unparseable.
     *
     * <p>Never throws: a bad icon must degrade to a blank glyph rather than take
     * a whole view tree down while a screen is being built.
     */
    static IconPath parse(String source) {
        if (source == null || source.isEmpty()) return empty();
        try {
            return new Parser(source).parse();
        } catch (RuntimeException malformed) {
            return empty();
        }
    }

    /** Single-pass scanner over the restricted dialect. */
    private static final class Parser {
        private final String source;
        private int cursor;

        Parser(String source) { this.source = source; }

        IconPath parse() {
            ByteList ops = new ByteList();
            IntList offsets = new IntList();
            FloatList values = new FloatList();
            float minX = Float.MAX_VALUE, minY = Float.MAX_VALUE;
            float maxX = -Float.MAX_VALUE, maxY = -Float.MAX_VALUE;
            boolean started = false;
            boolean hasGeometry = false;

            while (true) {
                skipSeparators();
                if (cursor >= source.length()) break;
                char letter = source.charAt(cursor++);
                int op;
                int count;
                switch (letter) {
                    case 'M': op = MOVE; count = POINTS_PER_SIMPLE; break;
                    case 'L': op = LINE; count = POINTS_PER_SIMPLE; break;
                    case 'C': op = CUBIC; count = POINTS_PER_CUBIC; break;
                    case 'Q': op = QUAD; count = POINTS_PER_QUAD; break;
                    case 'Z': case 'z': op = CLOSE; count = 0; break;
                    default: throw new IllegalArgumentException("unknown command " + letter);
                }
                if (op == CLOSE) {
                    if (!started) throw new IllegalArgumentException("close before any move");
                    ops.add((byte) op);
                    offsets.add(values.size());
                    continue;
                }
                offsets.add(values.size());
                for (int i = 0; i < count; i++) {
                    float value = readNumber();
                    values.add(value);
                    if ((i & 1) == 0) {
                        if (value < minX) minX = value;
                        if (value > maxX) maxX = value;
                    } else {
                        if (value < minY) minY = value;
                        if (value > maxY) maxY = value;
                    }
                }
                started = true;
                hasGeometry = true;
                ops.add((byte) op);
            }
            if (!hasGeometry) return empty();
            return new IconPath(ops.toArray(), offsets.toArray(), values.toArray(),
                    minX, minY, maxX, maxY);
        }

        private void skipSeparators() {
            while (cursor < source.length()) {
                char c = source.charAt(cursor);
                if (c == ' ' || c == ',' || c == '\t' || c == '\n' || c == '\r') cursor++;
                else return;
            }
        }

        private float readNumber() {
            skipSeparators();
            int start = cursor;
            int end = cursor;
            if (end < source.length()
                    && (source.charAt(end) == '-' || source.charAt(end) == '+')) end++;
            boolean digits = false;
            while (end < source.length()) {
                char c = source.charAt(end);
                if (c >= '0' && c <= '9') { digits = true; end++; continue; }
                if (c == '.' && digits) { end++; continue; }
                break;
            }
            if (!digits || end == start) throw new IllegalArgumentException("number expected");
            cursor = end;
            return Float.parseFloat(source.substring(start, end));
        }
    }

    /** Grows-by-double byte buffer; a parsed outline is a handful of commands. */
    private static final class ByteList {
        private byte[] items = new byte[16];
        private int size;

        void add(byte value) {
            if (size == items.length) items = java.util.Arrays.copyOf(items, size * 2);
            items[size++] = value;
        }

        byte[] toArray() { return java.util.Arrays.copyOf(items, size); }
    }

    /** Grows-by-double int buffer holding per-command value offsets. */
    private static final class IntList {
        private int[] items = new int[16];
        private int size;

        void add(int value) {
            if (size == items.length) items = java.util.Arrays.copyOf(items, size * 2);
            items[size++] = value;
        }

        int[] toArray() { return java.util.Arrays.copyOf(items, size); }
    }

    /** Grows-by-double float buffer. */
    private static final class FloatList {
        private float[] items = new float[16];
        private int size;

        int size() { return size; }

        void add(float value) {
            if (size == items.length) items = java.util.Arrays.copyOf(items, size * 2);
            items[size++] = value;
        }

        float[] toArray() { return java.util.Arrays.copyOf(items, size); }
    }
}