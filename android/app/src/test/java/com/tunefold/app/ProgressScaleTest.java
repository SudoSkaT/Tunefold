package com.tunefold.app;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

/**
 * The progress arithmetic of the waveform and its labels.
 *
 * <p>The invariants that matter are that an unknown total is never rendered as
 * progress, and that a position just past the end cannot draw outside the surface.
 */
public class ProgressScaleTest {

    @Test public void fractionOfAKnownTrackIsBounded() {
        assertEquals(0f, ProgressScale.fraction(0L, 200_000L), 0f);
        assertEquals(0.5f, ProgressScale.fraction(100_000L, 200_000L), 1e-6f);
        assertEquals(1f, ProgressScale.fraction(200_000L, 200_000L), 0f);
    }

    /**
     * An unknown duration is not a position of zero.
     *
     * <p>Reporting zero would draw a playhead at the start of a track whose length
     * nobody knows, which reads as "not started" rather than "unknown" (§7).
     */
    @Test public void anUnknownDurationIsNotTheStart() {
        assertEquals(ProgressScale.UNKNOWN, ProgressScale.fraction(30_000L, 0L), 0f);
        assertEquals(ProgressScale.UNKNOWN, ProgressScale.fraction(30_000L, -1L), 0f);
        assertTrue(ProgressScale.fraction(30_000L, -1L) < 0f);
    }

    /** The last frame before an end-of-track must stay inside the surface. */
    @Test public void aPositionPastTheEndClampsToTheEnd() {
        assertEquals(1f, ProgressScale.fraction(200_500L, 200_000L), 0f);
        assertEquals(0f, ProgressScale.fraction(-50L, 200_000L), 0f);
    }

    @Test public void aShortTrackStillDivides() {
        // A one-second track is the case where an integer scale loses its only
        // pixel: the fraction, not a permille, is what the playhead is drawn from.
        assertEquals(0.25f, ProgressScale.fraction(250L, 1_000L), 1e-6f);
        assertEquals(0.999f, ProgressScale.fraction(999L, 1_000L), 1e-6f);
    }

    @Test public void displayedSecondChangesOncePerSecond() {
        assertEquals(0L, ProgressScale.displayedSecond(0L));
        assertEquals(0L, ProgressScale.displayedSecond(999L));
        assertEquals(1L, ProgressScale.displayedSecond(1_000L));
        assertEquals(1L, ProgressScale.displayedSecond(1_999L));
        assertEquals(2L, ProgressScale.displayedSecond(2_000L));
    }

    /**
     * The gate the label relies on: two ticks in the same second are the same tick.
     *
     * <p>Four of every five ticks of a position refresh land inside one second, and
     * each of them must cost a comparison instead of a layout.
     */
    @Test public void onlyOneTickPerSecondChangesTheLabel() {
        int changes = 0;
        int ticks = 0;
        for (long ms = 0L; ms <= 6_000L; ms += DesignTokens.POSITION_INTERVAL_MS) {
            ticks++;
            long before = ms - DesignTokens.POSITION_INTERVAL_MS;
            if (ProgressScale.displayedSecond(ms) != ProgressScale.displayedSecond(before)) {
                changes++;
            }
        }
        assertEquals("thirty-one ticks, six label changes", 6, changes);
        assertTrue("the gate has to be worth having: " + ticks + " ticks", ticks > 24);
    }

    @Test public void negativePositionsReadAsZero() {
        assertEquals(0L, ProgressScale.displayedSecond(-1_000L));
    }
}