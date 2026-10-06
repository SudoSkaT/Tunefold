package com.tunefold.app;

/**
 * Bounded recovery policy (§20).
 *
 * <p>Retries exist to survive a transient hiccup, never to loop. Every recovery
 * path in the app asks this object how many attempts it has left, and a budget
 * that reaches zero turns "try again" into a terminal outcome.
 *
 * <p>Pure and side-effect free, so the policy can be unit-tested exhaustively
 * without a device or a network.
 */
final class RecoveryPolicy {
    /** How many times a source resolution may be retried automatically. */
    static final int MAX_SOURCE_RETRIES = 1;
    /** How many times a download may retry itself automatically. */
    static final int MAX_DOWNLOAD_RETRIES = 1;
    /** How many times playback may re-resolve a source by itself. */
    static final int MAX_PLAYBACK_SOURCE_RETRIES = 1;
    /**
     * How many consecutive autoplay candidates may fail before autoplay gives
     * up. Without this, a permanently broken provider loops forever.
     */
    static final int MAX_AUTOPLAY_SKIPS = 2;

    private int sourceRetries;
    private int downloadRetries;
    private int playbackSourceRetries;
    private int autoplaySkips;

    /** Attempts left for the current source resolution. */
    int sourceRetriesLeft() { return Math.max(0, MAX_SOURCE_RETRIES - sourceRetries); }

    /** Consumes one source-retry attempt; {@code false} when none remain. */
    boolean takeSourceRetry() {
        if (sourceRetries >= MAX_SOURCE_RETRIES) return false;
        sourceRetries++;
        return true;
    }

    int downloadRetriesLeft() { return Math.max(0, MAX_DOWNLOAD_RETRIES - downloadRetries); }

    boolean takeDownloadRetry() {
        if (downloadRetries >= MAX_DOWNLOAD_RETRIES) return false;
        downloadRetries++;
        return true;
    }

    int playbackSourceRetriesLeft() {
        return Math.max(0, MAX_PLAYBACK_SOURCE_RETRIES - playbackSourceRetries);
    }

    boolean takePlaybackSourceRetry() {
        if (playbackSourceRetries >= MAX_PLAYBACK_SOURCE_RETRIES) return false;
        playbackSourceRetries++;
        return true;
    }

    int autoplaySkipsLeft() { return Math.max(0, MAX_AUTOPLAY_SKIPS - autoplaySkips); }

    /** Records a failed autoplay candidate; {@code false} once the cap is hit. */
    boolean takeAutoplaySkip() {
        if (autoplaySkips >= MAX_AUTOPLAY_SKIPS) return false;
        autoplaySkips++;
        return true;
    }

    /** Called when playback starts cleanly, to refresh the transient budgets. */
    void onPlaybackStarted() {
        sourceRetries = 0;
        playbackSourceRetries = 0;
    }

    /** Called when a download finishes (either way) to refresh that budget. */
    void onDownloadFinished() { downloadRetries = 0; }

    /** Called when an autoplay candidate plays, so the skip budget resets. */
    void onAutoplaySucceeded() { autoplaySkips = 0; }

    /** Full reset between tracks, so one bad track cannot poison the next. */
    void resetForNextTrack() {
        sourceRetries = 0;
        playbackSourceRetries = 0;
        autoplaySkips = 0;
    }

    /** A short human summary of every budget, for the diagnostics panel. */
    String describe() {
        return "source=" + sourceRetriesLeft()
                + " playbackSource=" + playbackSourceRetriesLeft()
                + " download=" + downloadRetriesLeft()
                + " autoplaySkips=" + autoplaySkipsLeft();
    }
}