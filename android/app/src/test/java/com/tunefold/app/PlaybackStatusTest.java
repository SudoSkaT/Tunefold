package com.tunefold.app;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNotEquals;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

/**
 * The state → presentation table of the Now Playing surface.
 *
 * <p>The alternative is a {@code switch} in a view, which is where "Buffering" and
 * "Downloading" end up fighting over one label. These assertions are the table.
 */
public class PlaybackStatusTest {

    @Test public void everyWaitingStateIsWorkInProgress() {
        PlaybackState[] waiting = {PlaybackState.SEARCHING, PlaybackState.RESOLVING_METADATA,
                PlaybackState.RESOLVING_SOURCE, PlaybackState.BUFFERING};
        for (PlaybackState state : waiting) {
            assertEquals("state " + state, PlaybackStatus.Phase.WORKING,
                    PlaybackStatus.phaseOf(state, false));
        }
    }

    @Test public void playbackStatesMapToTheirOwnPhase() {
        assertEquals(PlaybackStatus.Phase.ACTIVE, PlaybackStatus.phaseOf(PlaybackState.PLAYING, false));
        assertEquals(PlaybackStatus.Phase.PAUSED, PlaybackStatus.phaseOf(PlaybackState.PAUSED, false));
        assertEquals(PlaybackStatus.Phase.IDLE, PlaybackStatus.phaseOf(PlaybackState.IDLE, false));
        assertEquals(PlaybackStatus.Phase.IDLE, PlaybackStatus.phaseOf(PlaybackState.STOPPED, false));
    }

    /**
     * A failure must never be softened by anything else happening.
     *
     * <p>Otherwise a download running during a failure takes the status line and
     * the user never learns that playback broke.
     */
    @Test public void errorOutranksEverythingIncludingADownload() {
        assertEquals(PlaybackStatus.Phase.ALERT,
                PlaybackStatus.phaseOf(PlaybackState.ERROR, true));
        assertEquals("Error", PlaybackStatus.labelOf(PlaybackState.ERROR, true));
    }

    /** While audio is playing, the transfer is reported on its own control. */
    @Test public void playingOutranksATransfer() {
        assertEquals(PlaybackStatus.Phase.ACTIVE,
                PlaybackStatus.phaseOf(PlaybackState.PLAYING, true));
        assertEquals("Playing", PlaybackStatus.labelOf(PlaybackState.PLAYING, true));
    }

    /** With playback silent, a running download takes the line. */
    @Test public void aTransferTakesTheLineWhenPlaybackHasNothingToSay() {
        assertEquals(PlaybackStatus.Phase.TRANSFERRING,
                PlaybackStatus.phaseOf(PlaybackState.IDLE, true));
        assertEquals(PlaybackStatus.Phase.TRANSFERRING,
                PlaybackStatus.phaseOf(PlaybackState.STOPPED, true));
        assertEquals("Downloading", PlaybackStatus.labelOf(PlaybackState.IDLE, true));
        assertTrue(PlaybackStatus.isTransfer(PlaybackStatus.phaseOf(PlaybackState.IDLE, true)));
    }

    /**
     * The dot and the word describe the same fact.
     *
     * <p>A paused track with a download running says "Paused": the transfer is
     * still visible on its own control, and two competing words on one line is
     * how a status row stops being readable at all.
     */
    @Test public void aPauseIsNotStolenByATransfer() {
        assertEquals(PlaybackStatus.Phase.PAUSED,
                PlaybackStatus.phaseOf(PlaybackState.PAUSED, true));
        assertEquals("Paused", PlaybackStatus.labelOf(PlaybackState.PAUSED, true));
        assertFalse(PlaybackStatus.isTransfer(PlaybackStatus.phaseOf(PlaybackState.PAUSED, true)));
    }

    @Test public void bufferingIsNeverReportedAsATransfer() {
        assertFalse(PlaybackStatus.isTransfer(PlaybackStatus.phaseOf(PlaybackState.BUFFERING, true)));
        assertEquals("Buffering", PlaybackStatus.labelOf(PlaybackState.BUFFERING, true));
    }

    /** Every state must differ from idle somewhere, or the dot says nothing. */
    @Test public void eachPhaseHasItsOwnColour() {
        int accent = DesignTokens.Palette.ACCENT;
        int previous = 0;
        for (PlaybackStatus.Phase phase : PlaybackStatus.Phase.values()) {
            int color = PlaybackStatus.colorOf(phase, accent);
            if (phase != PlaybackStatus.Phase.TRANSFERRING) {
                assertNotEquals("phase " + phase + " must not share idle's colour",
                        previous, color);
            }
            previous = color;
        }
    }

    /** The accent belongs to playback, never to a state the user is waiting on. */
    @Test public void onlyPlaybackPhasesUseTheArtworkColour() {
        int accent = DesignTokens.Palette.ACCENT;
        assertEquals(accent, PlaybackStatus.colorOf(PlaybackStatus.Phase.ACTIVE, accent));
        assertNotEquals(accent, PlaybackStatus.colorOf(PlaybackStatus.Phase.WORKING, accent));
        assertNotEquals(accent, PlaybackStatus.colorOf(PlaybackStatus.Phase.ALERT, accent));
        assertNotEquals(accent, PlaybackStatus.colorOf(PlaybackStatus.Phase.PAUSED, accent));
        assertNotEquals(accent, PlaybackStatus.colorOf(PlaybackStatus.Phase.IDLE, accent));
    }

    @Test public void nullStateIsIdleNotACrash() {
        assertEquals(PlaybackStatus.Phase.IDLE, PlaybackStatus.phaseOf(null, false));
        assertEquals("Idle", PlaybackStatus.labelOf(null, false));
    }
}