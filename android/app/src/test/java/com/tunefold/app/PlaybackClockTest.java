package com.tunefold.app;

import org.junit.Before;
import org.junit.Test;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

/**
 * Stale progress must never be attributed to a different track (§2, §3, §5).
 *
 * <p>Each test drives {@link PlaybackClock} the way the session does, and asserts
 * the position a track is allowed to report. The bug these cover: a new song
 * showing the previous song's elapsed time because the engine's position was read
 * before the new source had produced audio.
 */
public class PlaybackClockTest {

    private PlaybackClock clock;

    @Before public void setUp() {
        clock = new PlaybackClock();
    }

    /** Simulates one track: begin, adopt when output begins, then tick. */
    private long playTrack() {
        long token = clock.beginTrack();
        clock.adoptEngine(token);
        return token;
    }

    @Test public void aNewTrackNeverShowsThePreviousPosition() {
        playTrack();
        clock.positionMs(107_000); // track A is at 1:47

        long b = clock.beginTrack();
        assertEquals("B starts at 0:00, not at A's position",
                0L, clock.positionMs(107_000));
        assertTrue(clock.isCurrent(b));
    }

    @Test public void aTickForTheSupersededTrackIsIgnored() {
        long a = playTrack();
        long b = clock.beginTrack();

        assertFalse("A's late output signal must be refused",
                clock.adoptEngine(a));
        assertEquals(0L, clock.positionMs(107_000));
        assertTrue("B is still the active attempt", clock.isCurrent(b));
    }

    @Test public void quickSuccessionKeepsOnlyTheLastTrack() {
        playTrack();
        long b = clock.beginTrack();
        long c = clock.beginTrack();

        clock.adoptEngine(b);
        assertEquals("C must not inherit B's engine clock", 0L, clock.positionMs(5_000));

        assertTrue(clock.adoptEngine(c));
        assertEquals(3_000L, clock.positionMs(3_000));
    }

    @Test public void positionAdvancesOnlyWhileTheEngineIsPlaying() {
        long a = playTrack();
        assertEquals(1_000L, clock.positionMs(1_000));
        assertEquals(2_000L, clock.positionMs(2_000));
        assertTrue(clock.isCurrent(a));
    }

    @Test public void pauseKeepsTheSameOwnerSoResumeContinues() {
        long a = playTrack();
        clock.positionMs(45_000);

        // Pause and resume do not open a new generation: the same track keeps the
        // engine clock, so the position must still be reported.
        assertTrue(clock.isCurrent(a));
        assertEquals(45_000L, clock.positionMs(45_000));
        assertEquals(48_000L, clock.positionMs(48_000));
    }

    @Test public void endOfTrackFollowedByAutoplayStartsAtZero() {
        playTrack();
        clock.positionMs(212_000); // A finished at 3:32

        long b = clock.beginTrack();
        clock.adoptEngine(b);
        assertEquals("the recommended track begins at 0:00",
                0L, clock.positionMs(0));
    }

    @Test public void restartDropsTheOldPositionUntilOutputResumes() {
        long a = playTrack();
        clock.positionMs(95_000);

        // Previous restarting the same track: same generation, no ownership.
        clock.detachEngine();
        assertEquals("a restarting track reads 0:00", 0L, clock.positionMs(95_000));

        assertTrue("the replay of the same attempt is adopted", clock.adoptEngine(a));
        assertEquals(0L, clock.positionMs(0));
    }

    @Test public void stopEndsEngineOwnership() {
        playTrack();
        clock.release();
        assertEquals(0L, clock.positionMs(120_000));
        assertFalse(clock.engineOwnsTrack());
    }

    @Test public void generationsOnlyEverIncrease() {
        long a = clock.beginTrack();
        long b = clock.beginTrack();
        long c = clock.beginTrack();
        assertTrue(a < b);
        assertTrue(b < c);
        assertFalse("an old token never becomes current again", clock.isCurrent(a));
    }

    @Test public void onlyTheExpectedAttemptMayReportEndOfTrack() {
        long a = clock.beginTrack();
        clock.adoptEngine(a);
        clock.expectEofFrom(7L);

        assertTrue(clock.acceptEof(7L));
        assertFalse("a decoder from another attempt is not our end of track",
                clock.acceptEof(6L));
        assertFalse(clock.acceptEof(8L));
    }

    @Test public void noAttemptYetMeansNoEndOfTrackIsAccepted() {
        long a = clock.beginTrack();
        clock.adoptEngine(a);
        // A new track has been selected but not handed to the engine: an EOF
        // arriving now belongs to the track we just replaced.
        assertFalse(clock.acceptEof(3L));
    }

    @Test public void selectingANewTrackDisarmsTheOldAttemptsEndOfTrack() {
        long a = clock.beginTrack();
        clock.adoptEngine(a);
        clock.expectEofFrom(7L);
        assertTrue(clock.acceptEof(7L));

        clock.beginTrack(); // user selects another track
        assertFalse("the replaced attempt must not advance the queue",
                clock.acceptEof(7L));
    }

    @Test public void stopDisarmsAnyPendingEndOfTrack() {
        long a = clock.beginTrack();
        clock.adoptEngine(a);
        clock.expectEofFrom(7L);
        clock.release();
        assertFalse("a stop must not be followed by a queue advance",
                clock.acceptEof(7L));
    }

    @Test public void negativeEngineReadingsAreClamped() {
        playTrack();
        assertEquals(0L, clock.positionMs(-5));
    }
}