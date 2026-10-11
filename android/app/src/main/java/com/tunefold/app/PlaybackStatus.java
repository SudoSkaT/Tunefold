package com.tunefold.app;

/**
 * What the Now Playing surface shows about "what is Tunefold doing", decided in one
 * place from two facts.
 *
 * <p>The surface has to say seven things at once — idle, resolving, buffering,
 * playing, paused, error, downloading — and it has to say them <em>without</em>
 * leaving it to a line of text. A label alone cannot be read at a glance, cannot be
 * distinguished when the label is long, and is invisible to a glance that never
 * happens.
 *
 * <p>So each state resolves to a {@link Phase}, and a {@link Phase} resolves to a
 * shape and a colour, not just words:
 *
 * <ul>
 *   <li>{@link Phase#WORKING} — three segments that advance on the UI tick.
 *       Something is being waited for, and the user can see that it has not
 *       stalled;</li>
 *   <li>{@link Phase#ACTIVE} — a solid dot in the artwork-derived accent:</li>
 *   <li>{@link Phase#PAUSED} — a hollow dot: present, not moving;</li>
 *   <li>{@link Phase#ALERT} — a solid dot inside a ring: something failed and has
 *       an action attached to it;</li>
 *   <li>{@link Phase#TRANSFERRING} — a dot with a progress arc:</li>
 *   <li>{@link Phase#IDLE} — nothing at all.</li>
 * </ul>
 *
 * <p>Pure logic with no Android types: the whole precedence table is a unit test
 * rather than something to be verified by watching a device go from buffering to
 * playing.
 *
 * <h3>Precedence</h3>
 * A failure outranks everything, then work in progress, then audible playback, then
 * a download. A download deliberately sits at the bottom: while audio is playing,
 * the transfer is already reported on its own control (a determinate ring plus a
 * description that says how to cancel it), and the status line belongs to
 * playback. It takes the line only when playback has nothing to say.
 */
final class PlaybackStatus {

    /** The visual class of a state; what the indicator draws and tints. */
    enum Phase {
        /** Nothing loaded, nothing playing, nothing moving. */
        IDLE,
        /** Work in progress before audio exists: resolving, searching, buffering. */
        WORKING,
        /** Audio is being written to the output. */
        ACTIVE,
        /** Output paused; the position is retained. */
        PAUSED,
        /** Failed; an action is attached to the failure. */
        ALERT,
        /** A download is running and playback has nothing to report. */
        TRANSFERRING
    }

    private PlaybackStatus() { }

    /**
     * The phase for one combination of facts.
     *
     * @param state       the session's own state, the only authority on playback
     * @param transferring whether a download is resolving or running
     */
    static Phase phaseOf(PlaybackState state, boolean transferring) {
        if (state == null) return Phase.IDLE;
        switch (state) {
            // A failure is sticky and must not be softened by anything below it:
            // an error the user cannot see is a dead end with no action.
            case ERROR: return Phase.ALERT;
            case SEARCHING:
            case RESOLVING_METADATA:
            case RESOLVING_SOURCE:
            case BUFFERING: return Phase.WORKING;
            case PLAYING: return Phase.ACTIVE;
            case PAUSED: return Phase.PAUSED;
            default: return transferring ? Phase.TRANSFERRING : Phase.IDLE;
        }
    }

    /**
     * Whether this phase is a transfer (a download), which is the one case where
     * the indicator also draws an arc.
     */
    static boolean isTransfer(Phase phase) { return phase == Phase.TRANSFERRING; }

    /**
     * The colour role for a phase.
     *
     * @param accent the current artwork-derived accent; only {@link Phase#ACTIVE}
     *               uses it, so the accent never leaks into a state that is not
     *               the one the user is acting on
     */
    static int colorOf(Phase phase, int accent) {
        switch (phase) {
            case ACTIVE: return accent;
            case WORKING: return DesignTokens.Palette.STATE_WORKING;
            case ALERT: return DesignTokens.Palette.ERROR;
            case PAUSED: return DesignTokens.Palette.ON_SURFACE_MUTED;
            case TRANSFERRING: return accent;
            default: return DesignTokens.Palette.ON_SURFACE_FAINT;
        }
    }

    /**
     * The word for a phase, for the line beside the indicator.
     *
     * <p>Not translatable prose and not a string resource on purpose: it names a
     * state of this pipeline, the same way {@link PlaybackState#label()} does, and
     * it is short enough that it never wraps.
     */
    static String labelOf(PlaybackState state, boolean transferring) {
        if (state == PlaybackState.ERROR) return PlaybackState.ERROR.label();
        if (transferring && (state == PlaybackState.IDLE
                || state == PlaybackState.STOPPED)) {
            return PlaybackState.DOWNLOADING.label();
        }
        return state == null ? PlaybackState.IDLE.label() : state.label();
    }
}