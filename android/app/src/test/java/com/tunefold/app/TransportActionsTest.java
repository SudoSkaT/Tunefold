package com.tunefold.app;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

public class TransportActionsTest {

    @Test public void primaryReflectsTheActionItWillPerform() {
        assertEquals(TransportActions.Primary.PAUSE,
                TransportActions.primaryFor(PlaybackState.PLAYING, true, false));
        assertEquals(TransportActions.Primary.PLAY,
                TransportActions.primaryFor(PlaybackState.PAUSED, true, false));
        assertEquals(TransportActions.Primary.PLAY,
                TransportActions.primaryFor(PlaybackState.STOPPED, true, false));
        assertEquals(TransportActions.Primary.RETRY,
                TransportActions.primaryFor(PlaybackState.ERROR, true, true));
    }

    @Test public void primaryIsDisabledWhileTheSessionCannotAct() {
        assertFalse(TransportActions.primaryEnabled(
                PlaybackState.RESOLVING_SOURCE, true, false));
        assertFalse(TransportActions.primaryEnabled(
                PlaybackState.RESOLVING_METADATA, true, false));
        assertFalse(TransportActions.primaryEnabled(
                PlaybackState.BUFFERING, true, false));
        assertFalse(TransportActions.primaryEnabled(
                PlaybackState.ERROR, true, false));
        assertFalse(TransportActions.primaryEnabled(
                PlaybackState.PLAYING, false, false));
    }

    @Test public void nextRemainsTheEscapeFromATerminalError() {
        assertTrue(TransportActions.nextEnabled(true, false));
        assertTrue(TransportActions.nextEnabled(false, true));
        assertFalse(TransportActions.nextEnabled(false, false));
    }

    @Test public void previousWaitsForAResolvedTrack() {
        assertTrue(TransportActions.previousEnabled(true));
        assertFalse(TransportActions.previousEnabled(false));
    }
}
