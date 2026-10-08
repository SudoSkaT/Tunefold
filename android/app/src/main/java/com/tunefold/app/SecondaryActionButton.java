package com.tunefold.app;

import android.content.Context;

/**
 * A secondary action: like, download, queue, autoplay.
 *
 * <p>Identical in structure to {@link IconButton} — one view, one glyph — with one
 * addition: an optional {@link Phase} that decides both the glyph and the tint, and a
 * progress ring for transfers in flight.
 *
 * <p>That addition is what makes dropping the captions safe. An icon-only button has
 * to prove what its last tap did (§26), otherwise cancelling a download becomes an
 * invisible action. Encoding the outcome as a small closed set means the button can
 * be driven straight from {@link DownloadState.Phase} and there is no wording to get
 * out of step with the behaviour.
 */
final class SecondaryActionButton extends IconButton {

    /** What a secondary action is currently doing or offering. */
    enum Phase {
        /** Nothing in progress; the control offers to start. */
        IDLE,
        /** Preparing; cancellable, not yet moving bytes. */
        PENDING,
        /** Determinate or indeterminate progress; cancellable. */
        RUNNING,
        /** Finished and confirmed. */
        DONE,
        /** Rejected or interrupted; the control offers to try again. */
        FAILED,
        /** Deliberately off (an enabled setting that is currently disabled). */
        OFF
    }

    private Phase phase = Phase.IDLE;
    private float progress = IconDrawable.NO_PROGRESS;

    SecondaryActionButton(Context context) {
        super(context);
    }

    /**
     * Applies the current phase: glyph, tint and progress ring.
     *
     * <p>A total no-op when nothing visible would change, which is what makes this safe
     * to call from a notification that happens not to concern this control: no
     * description event, no invalidate, no allocation.
     *
     * <p>The description is deliberately <b>not</b> derived here. A control's wording
     * belongs to whoever owns the meaning (a download speaks about downloads, autoplay
     * about playback), and deriving it from a shared table is how one control ends up
     * announcing another's action. The owner calls {@link #applyDescription} instead.
     *
     * @param icon    the glyph for this phase, or {@code null} to keep the current glyph
     *                and only let the tint change
     * @param value   the phase, which decides tint and progress ring
     * @param percent 0..100 for {@link Phase#RUNNING}, or {@code -1} when the total is
     *                unknown
     */
    void setPhase(Icon icon, Phase value, int percent) {
        Phase next = value == null ? Phase.IDLE : value;
        float ring = ringFor(next, percent);
        boolean ringMoved = Float.compare(ring, progress) != 0;
        phase = next;
        if (icon != null) setIcon(icon);
        if (ringMoved && iconDrawable() != null) iconDrawable().setProgress(ring);
        progress = ring;
        paint();
    }

    Phase phase() { return phase; }

    /** True while the control must stay reachable so it can cancel. */
    boolean isInFlight() { return phase == Phase.PENDING || phase == Phase.RUNNING; }

    /**
     * Ring extent for a phase.
     *
     * <p>An unknown total draws the full ring rather than a fabricated percentage
     * (§7): the action still shows it is working, and it does not claim to know a
     * figure it cannot compute.
     */
    private static float ringFor(Phase value, int percent) {
        if (value != Phase.RUNNING) return IconDrawable.NO_PROGRESS;
        if (percent < 0) return IconDrawable.FULL_PROGRESS;
        return Math.min(100, percent) / 100f;
    }

    @Override int resolveGlyphColor() {
        if (!isEnabled()) {
            return DesignTokens.scaleAlpha(DesignTokens.Palette.ON_SURFACE_FAINT,
                    DesignTokens.Palette.ALPHA_DISABLED);
        }
        switch (phase) {
            case DONE: return DesignTokens.Palette.POSITIVE;
            case FAILED: return DesignTokens.Palette.ERROR;
            case OFF: return DesignTokens.scaleAlpha(DesignTokens.Palette.ON_SURFACE_MUTED,
                    DesignTokens.Palette.ALPHA_DISABLED);
            default: return DesignTokens.controlColor(true, false, isActivated(),
                    ArtworkTheme.accent());
        }
    }

}
