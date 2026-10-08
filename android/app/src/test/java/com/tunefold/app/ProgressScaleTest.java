package com.tunefold.app;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

/**
 * Guards the redraw gate on the position tick.
 *
 * <p>This is the arithmetic that decides whether a 500&nbsp;ms tick touches the view
 * hierarchy at all. If it regressed, playback would keep working and the screen would
 * quietly stop updating, so the monotonicity and clamping cases matter more than they
 * look.
 */
public class ProgressScaleTest {

    @Test public void unknownDurationNeverFakesProgress() {
        assertEquals(ProgressScale.NO_PROGRESS, ProgressScale.permille(10_000L, -1L));
        assertEquals(ProgressScale.NO_PROGRESS, ProgressScale.permille(10_000L, 0L));
    }

    @Test public void progressIsPermilleOfTheDuration() {
        assertEquals(ProgressScale.AT_START, ProgressScale.permille(0L, 10_000L));
        assertEquals(250, ProgressScale.permille(2_500L, 10_000L));
        assertEquals(500, ProgressScale.permille(5_000L, 10_000L));
    }

    @Test public void progressIsClampedToTheDuration() {
        assertEquals(ProgressScale.AT_END, ProgressScale.permille(30_000L, 10_000L));
        assertEquals(ProgressScale.AT_START, ProgressScale.permille(-5_000L, 10_000L));
    }

    @Test public void progressNeverSitsOnEitherExtreme() {
        assertTrue(ProgressScale.permille(0L, 10_000L) > ProgressScale.NO_PROGRESS);
        assertTrue(ProgressScale.permille(10_000L, 10_000L) < ProgressScale.PERMILLE);
    }

    @Test public void progressIsMonotonic() {
        int previous = -1;
        for (long ms = 0L; ms <= 20_000L; ms += 37L) {
            int value = ProgressScale.permille(ms, 20_000L);
            assertTrue("progress went backwards at " + ms, value >= previous);
            previous = value;
        }
    }

    @Test public void pixelStepTracksTheDisplayedWidth() {
        assertEquals(1, ProgressScale.pixelStep(ProgressScale.PERMILLE));
        assertEquals(2, ProgressScale.pixelStep(500));
        assertEquals(10, ProgressScale.pixelStep(100));
        assertEquals(ProgressScale.PERMILLE, ProgressScale.pixelStep(0));
    }

    /** A zero step would redraw on every tick; an absent bar must not divide by zero. */
    @Test public void pixelStepIsNeverZero() {
        assertEquals(1, ProgressScale.pixelStep(10_000));
        assertEquals(ProgressScale.PERMILLE, ProgressScale.pixelStep(0));
        assertEquals(ProgressScale.PERMILLE, ProgressScale.pixelStep(-5));
    }

    /** An unchanged bar must not be re-set: that is the whole point of the gate. */
    @Test public void unchangedBarIsNeverAdvanced() {
        assertFalse(ProgressScale.shouldAdvanceBar(500, 500, 1));
    }

    @Test public void subPixelMovementIsSkipped() {
        assertFalse(ProgressScale.shouldAdvanceBar(500, 501, 5));
        assertTrue(ProgressScale.shouldAdvanceBar(500, 505, 5));
    }

    /** Reaching the end always paints, however fine the step. */
    @Test public void reachingTheEndAlwaysAdvances() {
        assertTrue(ProgressScale.shouldAdvanceBar(ProgressScale.AT_END - 1,
                ProgressScale.AT_END, 1000));
    }

    @Test public void goingBackwardsAlwaysAdvances() {
        assertTrue(ProgressScale.shouldAdvanceBar(600, 100, 5));
    }

    @Test public void displayedSecondOnlyMovesOncePerSecond() {
        assertEquals(0L, ProgressScale.displayedSecond(0L));
        assertEquals(0L, ProgressScale.displayedSecond(999L));
        assertEquals(1L, ProgressScale.displayedSecond(1_000L));
        assertEquals(1L, ProgressScale.displayedSecond(1_999L));
        assertEquals(60L, ProgressScale.displayedSecond(60_000L));
        assertEquals(0L, ProgressScale.displayedSecond(-500L));
    }
}