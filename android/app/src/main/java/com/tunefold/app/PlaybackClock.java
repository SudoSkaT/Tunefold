package com.tunefold.app;

/**
 * Owns which track the reported playback position belongs to (§3, §5).
 *
 * <p>The bug this exists to prevent: the engine keeps reporting the previous
 * stream's position until a new source has actually produced audio. Pairing that
 * reading with the newly selected track produced a new song showing the old
 * song's elapsed time.
 *
 * <p>Two rules, and nothing else:
 * <ul>
 *   <li>a track change opens a new <em>generation</em>, which invalidates every
 *       asynchronous callback created under the previous one;</li>
 *   <li>the engine's clock may only be reported once output has begun for the
 *       track of the current generation, so before that the position is 0.</li>
 * </ul>
 *
 * <p>Pure logic with no Android types and no I/O, so both rules are verifiable in
 * unit tests instead of by watching a device.
 */
final class PlaybackClock {

    /** Monotonic id of the current playback attempt. */
    private long generation;
    /** True only while the engine is known to be playing the current generation. */
    private boolean engineOwnsTrack;
    /** Play attempt allowed to report the end of the active track. */
    private long eofAttempt = NO_ATTEMPT;

    /** No attempt has claimed the end of the active track. */
    private static final long NO_ATTEMPT = 0L;

    /**
     * Opens a new attempt, invalidating everything belonging to the previous one.
     *
     * @return the generation token to capture in callbacks for this attempt.
     */
    long beginTrack() {
        generation++;
        engineOwnsTrack = false;
        // A new track has not been handed to the engine yet, so no end-of-track
        // may be accepted until it is.
        eofAttempt = NO_ATTEMPT;
        return generation;
    }

    /** True while {@code token} is still the active attempt. */
    boolean isCurrent(long token) { return token == generation; }

    /** The attempt currently in effect. */
    long generation() { return generation; }

    /**
     * Records that output began for {@code token}.
     *
     * @return {@code true} when the token was current, meaning the engine's clock
     *     may now be reported for this track. A stale token is refused, so a
     *     superseded track can never take over the position.
     */
    boolean adoptEngine(long token) {
        if (token != generation) return false;
        engineOwnsTrack = true;
        return true;
    }

    /**
     * Drops engine ownership without ending the attempt.
     *
     * <p>Used when the same track starts over: the position belongs to this track
     * again only once output resumes.
     */
    void detachEngine() { engineOwnsTrack = false; }

    /** Forgets playback entirely: stop, release or a new process. */
    void release() {
        engineOwnsTrack = false;
        eofAttempt = NO_ATTEMPT;
    }

    /**
     * Declares which play attempt may report the end of the active track.
     *
     * <p>Called once a track has been handed to the engine. Any end-of-track
     * signal arriving from a different attempt belongs to a decoder the user has
     * already replaced, and acting on it would skip a song on its own.
     */
    void expectEofFrom(long attempt) {
        eofAttempt = attempt;
    }

    /**
     * Accepts an end-of-track only when it comes from the expected attempt.
     *
     * @return {@code true} when the signal is ours to act on.
     */
    boolean acceptEof(long attempt) {
        return eofAttempt != NO_ATTEMPT && attempt == eofAttempt;
    }

    /** The attempt whose end-of-track would be accepted; 0 when none. */
    long eofAttempt() { return eofAttempt; }

    /** True when the engine's clock currently describes the active track. */
    boolean engineOwnsTrack() { return engineOwnsTrack; }

    /**
     * Position to report for the active track.
     *
     * <p>{@code enginePositionMs} is ignored until this track owns the engine,
     * which is what keeps a previous track's time off a newly selected one.
     */
    long positionMs(long enginePositionMs) {
        if (!engineOwnsTrack) return 0L;
        return Math.max(0L, enginePositionMs);
    }
}