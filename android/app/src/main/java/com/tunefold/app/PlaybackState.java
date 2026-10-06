package com.tunefold.app;

import java.util.Locale;

/**
 * The one product-level playback state, derived from the real pipeline.
 *
 * <p>Deliberately distinct from the decoder's own ints, the AudioTrack's own
 * state and the download state (§10). Those are lower-level facts; the UI needs
 * a single answer to "what is Tunefold doing right now", and this is it.
 *
 * <p>{@link #SEARCHING}, {@link #RESOLVING_METADATA} and {@link #RESOLVING_SOURCE}
 * describe work that happens *before* a source exists; {@link #BUFFERING} and
 * {@link #PLAYING} describe output that has started. Keeping them apart is what
 * lets the UI explain a slow start instead of showing one vague "Loading".
 */
enum PlaybackState {
    /** Nothing loaded, nothing playing. */
    IDLE("Idle"),
    /** A catalog query is in flight. */
    SEARCHING("Searching"),
    /** Turning a URL into a Track. */
    RESOLVING_METADATA("Resolving metadata"),
    /** Turning a Track into a PlayableSource. */
    RESOLVING_SOURCE("Resolving source"),
    /** A source exists but no audio has been written yet. */
    BUFFERING("Buffering"),
    /** Audio is being written to the output. */
    PLAYING("Playing"),
    /** Output paused; the position is retained. */
    PAUSED("Paused"),
    /** Playback finished or was stopped on purpose. */
    STOPPED("Stopped"),
    /** A download is the user's current intent, and is not playback. */
    DOWNLOADING("Downloading"),
    /** Failed; see {@link PlaybackError} for the class and recovery. */
    ERROR("Error");

    private final String label;

    PlaybackState(String label) { this.label = label; }

    String label() { return label; }

    /** Maps the decoder's raw state onto a product state. */
    static PlaybackState fromEngine(int engineState) {
        switch (engineState) {
            case PlaybackController.PLAYING: return PLAYING;
            case PlaybackController.PAUSED: return PAUSED;
            case PlaybackController.LOADING: return BUFFERING;
            case PlaybackController.STOPPED: return STOPPED;
            case PlaybackController.ERROR: return ERROR;
            default: return IDLE;
        }
    }

    @Override public String toString() {
        return name().toLowerCase(Locale.ROOT);
    }
}