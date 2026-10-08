package com.tunefold.app;

/**
 * Turns a playback position into the few numbers a progress display actually
 * draws.
 *
 * <p>Pure logic with no Android types, so it is unit-testable and so the hot
 * path is a handful of integer operations.
 *
 * <p>The point is the <em>redraw gate</em>. A position tick arrives twice a
 * second; the elapsed label only changes once a second, and the bar only changes
 * once its quantised value moves. Computing both here means the display can ask
 * "did anything I can see change?" and skip the {@code setProgress}/{@code
 * setText} call entirely — which is what avoids a measure/layout pass per tick
 * while a track plays.
 */
final class ProgressScale {

    private ProgressScale() { }

    /** Resolution of the progress bar's own scale. */
    static final int PERMILLE = 1000;
    /** Used when the duration is unknown: the bar stays empty, never faked. */
    static final int NO_PROGRESS = 0;
    /** Below this the bar is at the very start. */
    static final int AT_START = 1;
    /** At or above this the bar is full. */
    static final int AT_END = PERMILLE - 1;

    /**
     * Progress as permille of {@code durationMs}.
     *
     * <p>An unknown or non-positive duration yields {@link #NO_PROGRESS}: the bar
     * must not invent progress it cannot know (§7).
     */
    static int permille(long positionMs, long durationMs) {
        if (durationMs <= 0L) return NO_PROGRESS;
        long clamped = positionMs;
        if (clamped < 0L) clamped = 0L;
        if (clamped > durationMs) clamped = durationMs;
        int value = (int) (clamped * PERMILLE / durationMs);
        if (value < AT_START) return AT_START;
        if (value > AT_END) return AT_END;
        return value;
    }

    /**
     * One step of the bar for a display {@code widthPx} wide.
     *
     * <p>A permille value cannot move the bar by less than a pixel, so anything
     * finer than a pixel is invisible work. Returns the permille gap that
     * corresponds to one pixel, so a tick whose permille value has not crossed a
     * pixel boundary can be dropped without the bar ever appearing to stutter.
     */
    static int pixelStep(int widthPx) {
        if (widthPx <= 0) return PERMILLE;
        int step = PERMILLE / widthPx;
        return step < 1 ? 1 : step;
    }

    /**
     * True when {@code next} is far enough from {@code previous} to be visible.
     *
     * @param previous the value currently on screen, or {@link #NO_PROGRESS}
     * @param next     the value this tick wants to show
     */
    static boolean shouldAdvanceBar(int previous, int next, int step) {
        if (next == previous) return false;
        if (next > previous) return next - previous >= step || next == AT_END;
        // Going backwards (a seek, or a previous-track restart) is always shown.
        return true;
    }

    /**
     * The whole seconds a timestamp label shows for a position.
     *
     * <p>Two ticks one second apart produce two different strings; two ticks in
     * the same second produce the same one. Returning the second is what lets the
     * label skip the {@code setText} call, and therefore the allocation.
     */
    static long displayedSecond(long positionMs) {
        long seconds = positionMs / 1000L;
        return seconds < 0L ? 0L : seconds;
    }
}