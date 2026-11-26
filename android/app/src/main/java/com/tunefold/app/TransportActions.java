package com.tunefold.app;

/**
 * What the primary transport control means in each playback state.
 *
 * <p>The UI must never enable a button that does nothing. Resolving and
 * buffering are waits, not pauses; a stopped track has a real restart; a
 * retryable failure has a real retry; a terminal failure belongs to Next.
 */
final class TransportActions {
    enum Primary { NONE, PLAY, PAUSE, RETRY }

    private TransportActions() { }

    static Primary primaryFor(PlaybackState state, boolean hasTrack, boolean retryable) {
        if (!hasTrack) return Primary.NONE;
        if (state == PlaybackState.PLAYING) return Primary.PAUSE;
        if (state == PlaybackState.PAUSED || state == PlaybackState.STOPPED) {
            return Primary.PLAY;
        }
        if (state == PlaybackState.ERROR) return retryable ? Primary.RETRY : Primary.NONE;
        return Primary.NONE;
    }

    static boolean primaryEnabled(PlaybackState state, boolean hasTrack, boolean retryable) {
        return primaryFor(state, hasTrack, retryable) != Primary.NONE;
    }

    /**
     * Next keeps working on a terminal error and while a track is resolving:
     * skipping is the offered escape, not a no-op the UI should hide.
     */
    static boolean nextEnabled(boolean hasTrack, boolean autoplay) {
        return hasTrack || autoplay;
    }

    /** Previous acts on the current track as soon as one exists. */
    static boolean previousEnabled(boolean hasTrack) {
        return hasTrack;
    }
}
