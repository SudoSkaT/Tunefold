package com.tunefold.app;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.os.SystemClock;
import android.view.View;

/**
 * The state dot: what Tunefold is doing, as a shape rather than as a sentence.
 *
 * <p>Beside the status word it answers the question the word alone cannot — is this
 * still working, is it moving, has it failed, is nothing happening — at a glance and
 * from across the room. A label is read; this is seen.
 *
 * <h3>Why it does not animate by itself</h3>
 * A waiting state draws three segments and moves the lit one every
 * {@link DesignTokens#STATE_SEGMENT_INTERVAL_MS}. The clock is the UI's own tick
 * ({@link #advance()}), not a frame callback and not a
 * {@code ValueAnimator}: an indeterminate spinner redraws 60–120 times a second for
 * as long as a track takes to resolve, which is the most expensive pixel on the
 * screen saying the least. Five redraws a second of a 14dp dot says the same thing
 * for a hundredth of the cost.
 *
 * <p>Every other mode is completely static: there is no idle pulse, no breathing
 * cover, and nothing that repaints for its own sake.
 */
final class StateDotView extends View {

    private static final int SEGMENTS = 3;

    private final Paint fill = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint ring = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint dim = new Paint(Paint.ANTI_ALIAS_FLAG);

    private PlaybackStatus.Phase phase = PlaybackStatus.Phase.IDLE;
    private int color = DesignTokens.Palette.ON_SURFACE_FAINT;
    private int activeSegment;
    private int downloadPercent = -1;
    private long lastSegmentNanos;

    StateDotView(Context context) {
        super(context);
        DesignTokens.init(context);
        int size = DesignTokens.dp(DesignTokens.STATE_DOT_SIZE);
        ring.setStyle(Paint.Style.STROKE);
        ring.setStrokeWidth(DesignTokens.dp(DesignTokens.SPACE_1));
        ring.setStrokeCap(Paint.Cap.ROUND);
        setMinimumWidth(size);
        setMinimumHeight(size);
        // Decorative: the status word beside it is the accessible text, so the dot
        // must not be announced as a second, vaguer node.
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
        // Painted from the first frame: the constructor runs before any state
        // arrives, and a dot drawn with an unset paint is a black smudge.
        paint(PlaybackStatus.colorOf(phase, ArtworkTheme.accent()));
    }

    /** Tints the three paints this indicator draws with. */
    private void paint(int tint) {
        color = tint;
        fill.setColor(tint);
        ring.setColor(tint);
        dim.setColor(DesignTokens.scaleAlpha(tint, DesignTokens.ALPHA_MUTED_SHAPE));
    }

    /**
     * Applies a state.
     *
     * <p>A no-op when nothing visible would change, so the poll that drives the rest
     * of the surface costs one comparison for this control.
     *
     * @param next          the phase to draw
     * @param accent        the current artwork-derived accent
     * @param percent       download progress 0..100, or -1 when unknown; only read
     *                      while the phase is {@link PlaybackStatus.Phase#TRANSFERRING}
     */
    void apply(PlaybackStatus.Phase next, int accent, int percent) {
        int tint = PlaybackStatus.colorOf(next, accent);
        int arcPercent = next == PlaybackStatus.Phase.TRANSFERRING ? percent : -1;
        if (phase == next && tint == color && arcPercent == downloadPercent) return;
        phase = next;
        downloadPercent = arcPercent;
        paint(tint);
        invalidate();
    }

    /**
     * Advances the working indicator, if a segment is due.
     *
     * <p>Called from the UI tick. Nothing is scheduled, nothing runs between ticks,
     * and a state that is not {@link PlaybackStatus.Phase#WORKING} costs one
     * comparison.
     */
    void advance() {
        if (phase != PlaybackStatus.Phase.WORKING) return;
        long now = SystemClock.uptimeMillis() * 1_000_000L;
        if (now - lastSegmentNanos < DesignTokens.STATE_SEGMENT_INTERVAL_MS * 1_000_000L) {
            return;
        }
        lastSegmentNanos = now;
        activeSegment = (activeSegment + 1) % SEGMENTS;
        invalidate();
    }

    @Override protected void onDraw(Canvas canvas) {
        int width = getWidth();
        int height = getHeight();
        if (width <= 0 || height <= 0) return;
        float cx = width / 2f;
        float cy = height / 2f;
        float radius = Math.min(width, height) * 0.22f;
        switch (phase) {
            case WORKING:
                drawSegments(canvas, width, height);
                return;
            case ALERT:
                canvas.drawCircle(cx, cy, radius, fill);
                canvas.drawCircle(cx, cy, radius * 1.9f, ring);
                return;
            case PAUSED:
                // A hollow dot: present, not moving.
                canvas.drawCircle(cx, cy, radius, ring);
                return;
            case TRANSFERRING:
                drawTransfer(canvas, cx, cy, radius);
                return;
            case ACTIVE:
                canvas.drawCircle(cx, cy, radius, fill);
                return;
            default:
                // Idle says nothing, on purpose: an empty slot is the honest shape
                // of "nothing is happening".
                return;
        }
    }

    private void drawSegments(Canvas canvas, int width, int height) {
        float dot = Math.min(width, height) * 0.15f;
        float gap = Math.min(width, height) * 0.34f;
        float startX = (width - (gap * (SEGMENTS - 1) + dot * 2f)) / 2f;
        float cy = height / 2f;
        for (int index = 0; index < SEGMENTS; index++) {
            float cx = startX + dot + index * gap;
            canvas.drawCircle(cx, cy, dot, index == activeSegment ? fill : dim);
        }
    }

    private void drawTransfer(Canvas canvas, float cx, float cy, float radius) {
        canvas.drawCircle(cx, cy, radius, dim);
        if (downloadPercent < 0) {
            // An unknown total draws the whole ring rather than a made-up
            // percentage (§7): it still says a transfer is running.
            canvas.drawCircle(cx, cy, radius * 1.9f, ring);
            return;
        }
        canvas.drawArc(cx - radius * 1.9f, cy - radius * 1.9f,
                cx + radius * 1.9f, cy + radius * 1.9f,
                -90f, Math.min(100, downloadPercent) * 3.6f, false, ring);
    }
}