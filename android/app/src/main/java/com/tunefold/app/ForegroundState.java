package com.tunefold.app;

/**
 * When playback owns a foreground service and when it must give it back.
 *
 * <p>Pure policy: the service asks this class and only this class, so a paused
 * track keeps the notification, a resolving track keeps the process alive, and
 * a stopped or failed session never leaves a foreground notification behind.
 * Downloads are the one kind of work that outlives playback, which is why the
 * stop decision takes them into account.
 */
final class ForegroundState {
    enum Transition { NONE, START, STOP }

    private ForegroundState() { }

    /** States in which the user still owns a live playback session. */
    static boolean wantsForeground(PlaybackState state) {
        return state == PlaybackState.PLAYING
                || state == PlaybackState.PAUSED
                || state == PlaybackState.BUFFERING
                || state == PlaybackState.RESOLVING_METADATA
                || state == PlaybackState.RESOLVING_SOURCE;
    }

    /**
     * True when a terminal playback state may release the service.
     *
     * <p>{@code ERROR} is terminal for the notification but not for a running
     * download: cancelling the service there would silently kill work the user
     * explicitly started.
     */
    static boolean mayStop(PlaybackState state, boolean hasActiveDownloads) {
        return mayStop(state, hasActiveDownloads, false);
    }

    /** Variant that knows whether an {@link PlaybackState#ERROR} is recoverable. */
    static boolean mayStop(PlaybackState state, boolean hasActiveDownloads,
            boolean errorRetryable) {
        if (hasActiveDownloads) return false;
        if (state == PlaybackState.IDLE || state == PlaybackState.STOPPED) return true;
        if (state == PlaybackState.ERROR) return !errorRetryable;
        return false;
    }

    /** Convenience variant that takes the error itself. */
    static boolean mayStop(PlaybackState state, boolean hasActiveDownloads,
            PlaybackError error) {
        boolean retryable = error != null && error.retryable;
        return mayStop(state, hasActiveDownloads, retryable);
    }

    static Transition transition(PlaybackState state, boolean foreground) {
        boolean wanted = wantsForeground(state);
        if (wanted && !foreground) return Transition.START;
        if (!wanted && foreground) return Transition.STOP;
        return Transition.NONE;
    }
}
