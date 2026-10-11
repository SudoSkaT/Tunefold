package com.tunefold.app;

/**
 * Turns a playback position into the two numbers the progress surface draws.
 *
 * <p>Pure logic with no Android types, so it is unit-testable and so the hot path
 * is a handful of arithmetic operations.
 *
 * <h3>The redraw gate</h3>
 * A position tick arrives five times a second. The elapsed label only changes once
 * a second, and the playhead only moves when it crosses a pixel. Computing both
 * here lets the surface ask "did anything I can see change?" and skip the
 * {@code setText} / {@code invalidate} entirely — which is what keeps a tick from
 * costing a measure, layout and draw pass for as long as a track plays.
 */
final class ProgressScale {

    /** Returned by {@link #fraction} when the total length is not known. */
    static final float UNKNOWN = -1f;

    private ProgressScale() { }

    /**
     * How far along the track {@code positionMs} is, as a fraction of
     * {@code durationMs}.
     *
     * <p>A non-positive duration yields {@link #UNKNOWN}, never zero: a display
     * that cannot know its progress must not claim to be at the start of a track it
     * has not measured (§7). The result is also clamped, because a position briefly
     * beyond the end — the last frame before an end-of-track — must not draw a
     * playhead outside its own surface.
     */
    static float fraction(long positionMs, long durationMs) {
        if (durationMs <= 0L) return UNKNOWN;
        if (positionMs <= 0L) return 0f;
        if (positionMs >= durationMs) return 1f;
        return (float) positionMs / (float) durationMs;
    }

    /**
     * The whole seconds a timestamp label shows for a position.
     *
     * <p>Two ticks one second apart produce two different strings; two ticks in the
     * same second produce the same one. Returning the second is what lets the label
     * skip the {@code setText} call, and therefore the allocation.
     */
    static long displayedSecond(long positionMs) {
        if (positionMs <= 0L) return 0L;
        return positionMs / 1000L;
    }
}