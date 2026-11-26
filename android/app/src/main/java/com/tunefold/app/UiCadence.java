package com.tunefold.app;

/**
 * When the UI is allowed to wake up and when the bars are allowed to read audio.
 *
 * <p>Two independent questions, kept pure so they can be tested without an
 * Activity: a stopped screen has no reason to poll the session, and only a track
 * that is actually producing audio has anything new for the bars to read.
 */
final class UiCadence {

    private UiCadence() {}

    /** The Activity poll runs only between {@code onStart} and {@code onStop}. */
    static boolean uiPollEnabled(boolean started, boolean destroyed) {
        return started && !destroyed;
    }

    /**
     * A live window exists only while the engine is producing audio.
     *
     * <p>Paused and buffering keep the last painted frame; touching the engine
     * there only confirms that the generation has not moved.
     */
    static boolean soundProducesBars(PlaybackState state) {
        return state == PlaybackState.PLAYING;
    }

    /** The visual tick runs only for a visible player whose audio can advance. */
    static boolean visualTickEnabled(boolean started, boolean playerOpen,
                                     boolean soundPlayable) {
        return started && playerOpen && soundPlayable;
    }
}
