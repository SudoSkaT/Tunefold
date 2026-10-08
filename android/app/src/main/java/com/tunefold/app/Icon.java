package com.tunefold.app;

/**
 * The Tunefold icon family.
 *
 * <p>Every glyph is authored once, on the same
 * {@value DesignTokens#ICON_GRID}&times;{@value DesignTokens#ICON_GRID} grid, in the
 * restricted {@link IconPath} dialect. There are no Unicode glyphs and no platform
 * drawables: a text button was the previous way to express an action, which cost a
 * layout pass for every label, made translation a layout problem and made the
 * action width depend on its wording.
 *
 * <h3>Two sub-families, one rule each</h3>
 * <ul>
 *   <li><b>Transport</b> ({@link #PLAY}, {@link #PAUSE}, {@link #PREVIOUS},
 *       {@link #NEXT}, {@link #STOP}) are <i>filled</i> solids. They are the loudest
 *       thing on the screen and they read at 20&nbsp;dp, where an outline would
 *       break up.</li>
 *   <li><b>Utility</b> (everything else) are <i>stroked</i> outlines at exactly
 *       {@value DesignTokens#ICON_STROKE} grid units with round caps and round joins,
 *       so a mixed-weight set cannot happen: the weight is a property of the paint,
 *       not of the individual glyph.</li>
 * </ul>
 *
 * <h3>States</h3>
 * A state is expressed by geometry where a paired glyph exists ({@link #HEART} /
 * {@link #HEART_FILLED}, {@link #DOWNLOAD} / {@link #DOWNLOADED},
 * {@link #AUTOPLAY} / {@link #AUTOPLAY_OFF}) and by tint everywhere else, through
 * {@link DesignTokens#controlColor}. Keeping the pair identical in outline and only
 * different in fill is what stops a selected icon from looking like a different
 * product.
 *
 * <p>No glyph carries a {@code contentDescription}: a description must describe the
 * <i>action</i> ("Pause playback"), not the picture ("pause icon"), so it is supplied
 * by the control together with the icon, where they cannot drift apart.
 */
enum Icon {

    // ---- transport: filled solids ------------------------------------------

    /** Triangle pointing right. */
    PLAY(true, "M 8 5 L 19 12 L 8 19 Z"),

    /** Two vertical bars. */
    PAUSE(true,
            "M 7.4 5 L 10 5 C 10.5 5 10.9 5.4 10.9 5.9 L 10.9 18.1 C 10.9 18.6 10.5 19 10 19"
                    + " L 7.4 19 C 6.9 19 6.5 18.6 6.5 18.1 L 6.5 5.9 C 6.5 5.4 6.9 5 7.4 5 Z"
                    + " M 13.9 5 L 16.5 5 C 17 5 17.4 5.4 17.4 5.9 L 17.4 18.1"
                    + " C 17.4 18.6 17 19 16.5 19 L 13.9 19 C 13.4 19 13 18.6 13 18.1"
                    + " L 13 5.9 C 13 5.4 13.4 5 13.9 5 Z"),

    /** Bar plus a triangle pointing left. */
    PREVIOUS(true,
            "M 6 5.6 L 9.8 5.6 L 9.8 18.4 L 6 18.4 Z M 10.4 12 L 18.6 5.6 L 18.6 18.4 Z"),

    /** Triangle pointing right plus a bar. */
    NEXT(true,
            "M 14.2 5.6 L 18 5.6 L 18 18.4 L 14.2 18.4 Z M 13.6 12 L 5.4 5.6 L 5.4 18.4 Z"),

    /** Rounded square. */
    STOP(true,
            "M 8 5.6 L 16 5.6 C 16.9 5.6 17.6 6.3 17.6 7.2 L 17.6 16.8"
                    + " C 17.6 17.7 16.9 18.4 16 18.4 L 8 18.4 C 7.1 18.4 6.4 17.7 6.4 16.8"
                    + " L 6.4 7.2 C 6.4 6.3 7.1 5.6 8 5.6 Z"),

    // ---- utility: stroked outlines ----------------------------------------

    /** Magnifier: a ring and a handle. */
    SEARCH(false,
            "M 4.5 10.5 C 4.5 7.1863 7.1863 4.5 10.5 4.5"
                    + " C 13.8137 4.5 16.5 7.1863 16.5 10.5"
                    + " C 16.5 13.8137 13.8137 16.5 10.5 16.5"
                    + " C 7.1863 16.5 4.5 13.8137 4.5 10.5 Z M 15.1 15.1 L 20.4 20.4"),

    /** Heart outline; the filled twin is {@link #HEART_FILLED}. */
    HEART(false,
            "M 12 20.6 C 12 20.6 3.6 14.6 3.6 9.3 C 3.6 6.4 5.9 4.1 8.8 4.1"
                    + " C 10.4 4.1 11.5 4.9 12 5.7 C 12.5 4.9 13.6 4.1 15.2 4.1"
                    + " C 18.1 4.1 20.4 6.4 20.4 9.3 C 20.4 14.6 12 20.6 12 20.6 Z"),

    /** Same outline as {@link #HEART}, filled. */
    HEART_FILLED(true,
            "M 12 20.6 C 12 20.6 3.6 14.6 3.6 9.3 C 3.6 6.4 5.9 4.1 8.8 4.1"
                    + " C 10.4 4.1 11.5 4.9 12 5.7 C 12.5 4.9 13.6 4.1 15.2 4.1"
                    + " C 18.1 4.1 20.4 6.4 20.4 9.3 C 20.4 14.6 12 20.6 12 20.6 Z"),

    /** Arrow into a tray: the download affordance. */
    DOWNLOAD(false,
            "M 12 3.4 L 12 14.2 M 7.4 9.6 L 12 14.2 L 16.6 9.6"
                    + " M 4 15.4 L 4 17.6 C 4 19 5.1 20.1 6.5 20.1 L 17.5 20.1"
                    + " C 18.9 20.1 20 19 20 17.6 L 20 15.4"),

    /** Box with a check: the track already has a committed file. */
    DOWNLOADED(false,
            "M 5.2 4.6 L 18.8 4.6 C 19.9 4.6 20.8 5.5 20.8 6.6 L 20.8 17.4"
                    + " C 20.8 18.5 19.9 19.4 18.8 19.4 L 5.2 19.4 C 4.1 19.4 3.2 18.5 3.2 17.4"
                    + " L 3.2 6.6 C 3.2 5.5 4.1 4.6 5.2 4.6 Z"
                    + " M 8 12.2 L 11 15.2 L 16 9.6"),

    /** Stacked rules with a down arrow: the downloads destination. */
    DOWNLOADS(false,
            "M 4 6.5 L 20 6.5 M 4 12 L 20 12 M 16.5 15.2 L 16.5 20.2"
                    + " M 13.6 17.3 L 16.5 20.2 L 19.4 17.3"),

    /** Outline house. */
    HOME(false,
            "M 4 11.6 L 12 4.6 L 20 11.6"
                    + " M 6.6 10.1 L 6.6 18.4 C 6.6 19.2 7.2 19.8 8 19.8 L 16 19.8"
                    + " C 16.8 19.8 17.4 19.2 17.4 18.4 L 17.4 10.1"),

    /** Three stacked rules: a collection. */
    LIBRARY(false, "M 4.5 7 L 19.5 7 M 4.5 12 L 19.5 12 M 4.5 17 L 14.5 17"),

    /** Three dots. */
    MORE(false, "M 12 5.3 L 12 5.7 M 12 11.9 L 12 12.3 M 12 18.3 L 12 18.7"),

    /** Rules plus a play marker: the upcoming-queue affordance. */
    QUEUE(false,
            "M 4 7 L 14.5 7 M 4 12 L 14.5 12 M 4 17 L 10 17 M 17.5 13.6 L 21.5 17 L 17.5 20.4 Z"),

    /** Loop with a play inside: autoplay is on. */
    AUTOPLAY(false,
            "M 4.4 12 C 4.4 7.8026 7.8026 4.4 12 4.4 C 16.1974 4.4 19.6 7.8026 19.6 12"
                    + " C 19.6 16.1974 16.1974 19.6 12 19.6 C 7.8026 19.6 4.4 16.1974 4.4 12 Z"
                    + " M 10.2 9.4 L 15.4 12 L 10.2 14.6 Z"),

    /** Same loop with the play crossed out: autoplay is off. */
    AUTOPLAY_OFF(false,
            "M 4.4 12 C 4.4 7.8026 7.8026 4.4 12 4.4 C 16.1974 4.4 19.6 7.8026 19.6 12"
                    + " C 19.6 16.1974 16.1974 19.6 12 19.6 C 7.8026 19.6 4.4 16.1974 4.4 12 Z"
                    + " M 10.2 9.4 L 15.4 12 L 10.2 14.6 Z M 9.4 8.8 L 15.8 15.2"),

    /** Chevron pointing left. */
    BACK(false, "M 14.5 5.5 L 8 12 L 14.5 18.5"),

    /** Chevron pointing right. */
    CHEVRON_RIGHT(false, "M 9.5 5.5 L 16 12 L 9.5 18.5"),

    /** Circular arrow: retry a failed step. */
    RETRY(false,
            "M 19.4 9.2 L 19.4 4.6 L 14.8 4.6"
                    + " M 19 5.9 C 17.4 4.6 15.4 4 13.1 4 C 8.2 4 4.2 8 4.2 12.9"
                    + " C 4.2 17.8 8.2 21.8 13.1 21.8"),

    /** Pulse: the technical diagnostics panel. */
    DIAGNOSTICS(false, "M 2.8 12 L 6.4 12 L 8.6 6.4 L 11.6 17.6 L 14.4 8.4 L 16.6 12 L 21.2 12"),

    /** Sliders: settings. */
    SETTINGS(false,
            "M 4 7 L 9.6 7 M 14.4 7 L 20 7 M 4 12 L 14.8 12 M 18.8 12 L 20 12"
                    + " M 4 17 L 11.2 17 M 15.2 17 L 20 17"
                    + " M 10 7 C 10 5.8954 10.8954 5 12 5 C 13.1046 5 14 5.8954 14 7"
                    + " C 14 8.1046 13.1046 9 12 9 C 10.8954 9 10 8.1046 10 7 Z"
                    + " M 15 12 C 15 10.8954 15.8954 10 17 10 C 18.1046 10 19 10.8954 19 12"
                    + " C 19 13.1046 18.1046 14 17 14 C 15.8954 14 15 13.1046 15 12 Z"
                    + " M 11.2 17 C 11.2 15.8954 12.0954 15 13.2 15 C 14.3046 15 15.2 15.8954 15.2 17"
                    + " C 15.2 18.1046 14.3046 19 13.2 19 C 12.0954 19 11.2 18.1046 11.2 17 Z"),

    /** Triangle with an exclamation: a failure. */
    WARNING(false, "M 12 4 L 21 19.5 L 3 19.5 Z M 12 10 L 12 14.4 M 12 16.4 L 12 16.6"),

    /** Cross: dismiss. */
    CLOSE(false, "M 6.4 6.4 L 17.6 17.6 M 17.6 6.4 L 6.4 17.6");

    private final boolean filled;
    private final String geometry;
    /** Parsed once per enum constant, on first use. */
    private volatile IconPath path;

    Icon(boolean filled, String geometry) {
        this.filled = filled;
        this.geometry = geometry;
    }

    /** True for the filled transport family. */
    boolean isFilled() { return filled; }

    /** Stroke weight for a stroked glyph, in grid units. */
    float strokeWidth() { return filled ? 0f : DesignTokens.ICON_STROKE; }

    /**
     * The parsed outline, built on first use and then shared.
     *
     * <p>Parsing is a few microseconds and happens once per control at
     * construction; the result is immutable and read-only afterwards, so sharing it
     * between drawables is safe.
     */
    IconPath path() {
        IconPath local = path;
        if (local == null) {
            local = IconPath.parse(geometry);
            path = local;
        }
        return local;
    }

    /** Raw geometry, for the icon family unit test. */
    String geometry() { return geometry; }
}