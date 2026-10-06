package com.tunefold.app;

/**
 * Decides what happens when a track finishes (§14, §16).
 *
 * <p>Pure decision function with no I/O and no Android types, so the policy —
 * including the "no infinite loop" guarantees — is verifiable in unit tests
 * rather than by watching a device.
 *
 * <p>Order of precedence:
 * <ol>
 *   <li>the queue has a next item → play it;</li>
 *   <li>otherwise, autoplay enabled → ask for a recommendation;</li>
 *   <li>otherwise → stop.</li>
 * </ol>
 *
 * <p>Autoplay never downloads anything (§14): a recommendation is resolved and
 * played, never saved.
 */
final class AutoplayPolicy {

    /** What the caller should do next. */
    enum Decision {
        /** Play the queued item that follows the current one. */
        PLAY_NEXT,
        /**
         * Autoplay is on, the queue is exhausted and no candidate has been
         * fetched yet: go ask the provider, once.
         */
        FETCH_CANDIDATE,
        /** A candidate was already fetched: play it. */
        RECOMMEND,
        /** Nothing to play: stop. */
        STOP
    }

    /** One decision plus the optional target that explains it. */
    static final class Outcome {
        final Decision decision;
        final MediaTrack target;
        final String reason;

        Outcome(Decision decision, MediaTrack target, String reason) {
            this.decision = decision;
            this.target = target;
            this.reason = reason;
        }
    }

    private AutoplayPolicy() {}

    /**
     * Chooses the next action for a finished track.
     *
     * <p>A {@code null} {@code recommendation} means "not asked yet", not "there
     * is nothing": the answer is {@link Decision#FETCH_CANDIDATE} so autoplay
     * actually runs. Infinite-loop protection is not this policy's job — it lives
     * in the skip budget of {@link RecoveryPolicy}, which allows one bounded
     * attempt and then stops.
     *
     * @param queue the playback queue, already positioned on the finished track
     * @param autoplayEnabled the user's autoplay setting
     * @param recommendation a candidate already fetched, or {@code null}
     */
    static Outcome afterTrackFinished(PlaybackQueue queue, boolean autoplayEnabled,
                                      MediaTrack recommendation) {
        if (queue != null) {
            MediaTrack next = queue.peekNext();
            if (next != null) {
                return new Outcome(Decision.PLAY_NEXT, next, "queue has a next item");
            }
        }
        if (!autoplayEnabled) {
            return new Outcome(Decision.STOP, null, "queue exhausted and autoplay is off");
        }
        if (recommendation == null) {
            return new Outcome(Decision.FETCH_CANDIDATE, null,
                    "queue exhausted and autoplay is on: fetch one candidate");
        }
        return new Outcome(Decision.RECOMMEND, recommendation, "autoplay filled the queue");
    }
}