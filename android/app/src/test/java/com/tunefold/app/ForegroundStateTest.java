package com.tunefold.app;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

public class ForegroundStateTest {

    @Test public void playbackAndResolutionKeepForeground() {
        assertTrue(ForegroundState.wantsForeground(PlaybackState.PLAYING));
        assertTrue(ForegroundState.wantsForeground(PlaybackState.PAUSED));
        assertTrue(ForegroundState.wantsForeground(PlaybackState.BUFFERING));
        assertTrue(ForegroundState.wantsForeground(PlaybackState.RESOLVING_SOURCE));
        assertTrue(ForegroundState.wantsForeground(PlaybackState.RESOLVING_METADATA));
    }

    @Test public void terminalStatesDoNotKeepForeground() {
        assertFalse(ForegroundState.wantsForeground(PlaybackState.STOPPED));
        assertFalse(ForegroundState.wantsForeground(PlaybackState.ERROR));
        assertFalse(ForegroundState.wantsForeground(PlaybackState.IDLE));
    }

    @Test public void transitionStartsAndStopsExactlyOnce() {
        assertEquals(ForegroundState.Transition.START,
                ForegroundState.transition(PlaybackState.PLAYING, false));
        assertEquals(ForegroundState.Transition.NONE,
                ForegroundState.transition(PlaybackState.PLAYING, true));
        assertEquals(ForegroundState.Transition.STOP,
                ForegroundState.transition(PlaybackState.STOPPED, true));
        assertEquals(ForegroundState.Transition.NONE,
                ForegroundState.transition(PlaybackState.STOPPED, false));
    }

    @Test public void activeDownloadBlocksServiceStop() {
        assertFalse(ForegroundState.mayStop(PlaybackState.STOPPED, true));
        assertFalse(ForegroundState.mayStop(PlaybackState.ERROR, true));
        assertTrue(ForegroundState.mayStop(PlaybackState.STOPPED, false));
        assertTrue(ForegroundState.mayStop(PlaybackState.ERROR, false));
        assertTrue(ForegroundState.mayStop(PlaybackState.IDLE, false));
        assertFalse(ForegroundState.mayStop(PlaybackState.PAUSED, false));
    }
}
