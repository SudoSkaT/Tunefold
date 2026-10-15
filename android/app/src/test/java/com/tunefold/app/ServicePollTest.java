package com.tunefold.app;

import static org.junit.Assert.assertEquals;

import org.junit.Test;

/**
 * Service activity model: poll cadence per playback state (Fase 2).
 *
 * <p>500ms while audio can be advancing (positions on the lock screen have 1s
 * display precision and buffering must flip to playing promptly); 2s when the
 * position is static and commands arrive synchronously as intents.
 */
public class ServicePollTest {

    @Test public void advancingStatesPollFast() {
        assertEquals(DesignTokens.SESSION_REFRESH_INTERVAL_MS,
                ForegroundPlaybackService.intervalFor(PlaybackState.PLAYING));
        assertEquals(DesignTokens.SESSION_REFRESH_INTERVAL_MS,
                ForegroundPlaybackService.intervalFor(PlaybackState.BUFFERING));
        assertEquals(DesignTokens.SESSION_REFRESH_INTERVAL_MS,
                ForegroundPlaybackService.intervalFor(PlaybackState.RESOLVING_SOURCE));
        assertEquals(DesignTokens.SESSION_REFRESH_INTERVAL_MS,
                ForegroundPlaybackService.intervalFor(PlaybackState.RESOLVING_METADATA));
    }

    @Test public void staticStatesPollSlow() {
        assertEquals(ForegroundPlaybackService.IDLE_POLL_MS,
                ForegroundPlaybackService.intervalFor(PlaybackState.PAUSED));
        assertEquals(ForegroundPlaybackService.IDLE_POLL_MS,
                ForegroundPlaybackService.intervalFor(PlaybackState.STOPPED));
        assertEquals(ForegroundPlaybackService.IDLE_POLL_MS,
                ForegroundPlaybackService.intervalFor(PlaybackState.IDLE));
        assertEquals(ForegroundPlaybackService.IDLE_POLL_MS,
                ForegroundPlaybackService.intervalFor(PlaybackState.ERROR));
    }

    @Test public void idleCadenceIsSlowerThanActive() {
        assertEquals(true, ForegroundPlaybackService.IDLE_POLL_MS
                > DesignTokens.SESSION_REFRESH_INTERVAL_MS);
    }
}
