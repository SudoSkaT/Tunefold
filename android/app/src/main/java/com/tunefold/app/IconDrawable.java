package com.tunefold.app;

import android.graphics.Canvas;
import android.graphics.ColorFilter;
import android.graphics.Matrix;
import android.graphics.Paint;
import android.graphics.Path;
import android.graphics.PixelFormat;
import android.graphics.Rect;
import android.graphics.drawable.Drawable;

/**
 * Renders one {@link Icon} at whatever size its view gives it.
 *
 * <p>Deliberately a single {@code Drawable} rather than a per-icon resource set:
 * the geometry is parsed once per constant, the transform is computed only when the
 * bounds change, and a repaint is two {@code drawPath} calls. There is no bitmap, no
 * shader and no allocation in {@link #draw(Canvas, Rect, Paint)}, so an icon can be
 * invalidated as often as its state changes without producing garbage.
 *
 * <p>The glyph is scaled from the {@value DesignTokens#ICON_GRID}-unit grid to the
 * view's bounds uniformly and centred, so one geometry serves a 16&nbsp;dp inline
 * affordance and a 40&nbsp;dp brand mark with the same visual weight.
 *
 * <h3>State feedback without text</h3>
 * An icon-only button used to need a caption to prove what its last tap did. Instead,
 * {@link #setProgress(float)} paints a ring around the glyph, which is how the
 * download action reports its own progress (§26) without a label. The ring is
 * <em>not</em> animated: a determinate arc redraws only when the percentage moves,
 * and an unknown total draws a full track ring, so a download in flight costs no
 * frames at all.
 */
final class IconDrawable extends Drawable {

    /** No progress ring. */
    static final float NO_PROGRESS = -1f;
    /** Progress ring drawn at full extent, for an indeterminate transfer. */
    static final float FULL_PROGRESS = 1f;
    /** Radius of the progress ring as a fraction of the drawn side. */
    private static final float RING_RADIUS = 0.46f;
    /** Gap between the glyph and the ring, as a fraction of the ring radius. */
    private static final float RING_GAP = 1.28f;
    private static final float START_ANGLE = -90f;

    private final Icon icon;
    private final Paint glyph = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint ring = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Matrix transform = new Matrix();
    private final Path path = new Path();

    /** Transform inputs, so the matrix is only recomputed when the size changes. */
    private float builtForSize = -1f;
    private int color = DesignTokens.Palette.ON_SURFACE;
    private int ringColor = DesignTokens.Palette.ACCENT;
    private float progress = NO_PROGRESS;

    IconDrawable(Icon icon) {
        this.icon = icon == null ? Icon.MORE : icon;
        glyph.setAntiAlias(true);
        glyph.setColor(color);
        if (this.icon.isFilled()) {
            glyph.setStyle(Paint.Style.FILL);
        } else {
            glyph.setStyle(Paint.Style.STROKE);
            glyph.setStrokeCap(Paint.Cap.ROUND);
            glyph.setStrokeJoin(Paint.Join.ROUND);
        }
        ring.setAntiAlias(true);
        ring.setStyle(Paint.Style.STROKE);
        ring.setStrokeCap(Paint.Cap.ROUND);
        buildPath();
    }

    /** Which icon this drawable shows. */
    Icon icon() { return icon; }

    /**
     * Parses the outline into a {@link Path}, once.
     *
     * <p>The result is only read at draw time, so it is shared by every drawable
     * built from the same {@link Icon}.
     */
    private void buildPath() {
        IconPath outline = icon.path();
        path.reset();
        for (int i = 0; i < outline.size(); i++) {
            int offset = outline.offsetOf(i);
            switch (outline.opAt(i)) {
                case IconPath.MOVE:
                    path.moveTo(outline.valueAt(offset), outline.valueAt(offset + 1));
                    break;
                case IconPath.LINE:
                    path.lineTo(outline.valueAt(offset), outline.valueAt(offset + 1));
                    break;
                case IconPath.CUBIC:
                    path.cubicTo(outline.valueAt(offset), outline.valueAt(offset + 1),
                            outline.valueAt(offset + 2), outline.valueAt(offset + 3),
                            outline.valueAt(offset + 4), outline.valueAt(offset + 5));
                    break;
                case IconPath.QUAD:
                    path.quadTo(outline.valueAt(offset), outline.valueAt(offset + 1),
                            outline.valueAt(offset + 2), outline.valueAt(offset + 3));
                    break;
                case IconPath.CLOSE:
                    path.close();
                    break;
                default:
                    break;
            }
        }
    }

    /** Glyph colour; the control owns the state-to-colour mapping. */
    void setColor(int value) {
        if (color == value) return;
        color = value;
        glyph.setColor(value);
        invalidateSelf();
    }

    int color() { return color; }

    /**
     * Progress ring, 0..1, or {@link #NO_PROGRESS}.
     *
     * <p>Out-of-range and NaN input collapse to "no ring": an icon must never show a
     * nonsensical percentage.
     */
    void setProgress(float value) {
        float clamped = Float.isNaN(value) || value < 0f ? NO_PROGRESS : Math.min(1f, value);
        if (Float.compare(clamped, progress) == 0) return;
        progress = clamped;
        invalidateSelf();
    }

    float progress() { return progress; }

    /** Ring colour, used for the indeterminate state and the filled sweep. */
    void setRingColor(int value) {
        if (ringColor == value) return;
        ringColor = value;
        invalidateSelf();
    }

    @Override protected void onBoundsChange(Rect bounds) {
        // The transform depends on the side length only, and it is recomputed lazily
        // against the current bounds, so an unrelated bounds change on the same size
        // costs nothing beyond this flag.
        builtForSize = -1f;
    }

    @Override public int getIntrinsicWidth() {
        return DesignTokens.dp(DesignTokens.ICON_SIZE_MEDIUM);
    }

    @Override public int getIntrinsicHeight() {
        return DesignTokens.dp(DesignTokens.ICON_SIZE_MEDIUM);
    }

    @Override public int getOpacity() { return PixelFormat.TRANSLUCENT; }

    @Override public void setAlpha(int alpha) { glyph.setAlpha(alpha); ring.setAlpha(alpha); }

    @Override public void setColorFilter(ColorFilter filter) {
        glyph.setColorFilter(filter);
        ring.setColorFilter(filter);
    }

    @Override public void draw(Canvas canvas) {
        Rect bounds = getBounds();
        int side = Math.min(bounds.width(), bounds.height());
        if (side <= 0) return;
        float scale = side / DesignTokens.ICON_GRID;
        if (scale != builtForSize) {
            // Centre the grid inside the bounds so a non-square view still renders
            // the glyph undistorted.
            float dx = bounds.left + (bounds.width() - side) / 2f;
            float dy = bounds.top + (bounds.height() - side) / 2f;
            transform.reset();
            transform.postTranslate(dx, dy);
            transform.postScale(scale, scale);
            glyph.setStrokeWidth(icon.strokeWidth());
            builtForSize = scale;
        }
        int save = canvas.save();
        canvas.concat(transform);
        canvas.drawPath(path, glyph);
        canvas.restoreToCount(save);
        if (progress != NO_PROGRESS) {
            drawRing(canvas, bounds, side);
        }
    }

    private void drawRing(Canvas canvas, Rect bounds, int side) {
        float radius = (side / 2f) / RING_RADIUS;
        float stroke = Math.max(DesignTokens.dp(1.5f), radius * 0.12f);
        radius = radius - stroke / 2f;
        ring.setStrokeWidth(stroke);
        float cx = bounds.left + bounds.width() / 2f;
        float cy = bounds.top + bounds.height() / 2f;
        // The glyph is authored for a box that leaves room for the ring; shrink it
        // so an icon with a ring is never cropped by its own feedback.
        float inner = radius / RING_GAP;
        int save = canvas.save();
        canvas.scale(inner / (side / 2f), inner / (side / 2f), cx, cy);
        ring.setColor(DesignTokens.scaleAlpha(ringColor, DesignTokens.Palette.ALPHA_HAIRLINE * 3f));
        canvas.drawCircle(cx, cy, radius, ring);
        if (progress > 0f && progress < FULL_PROGRESS) {
            ring.setColor(ringColor);
            canvas.drawArc(cx - radius, cy - radius, cx + radius, cy + radius,
                    START_ANGLE, progress * 360f, false, ring);
        } else if (progress >= FULL_PROGRESS) {
            ring.setColor(ringColor);
            canvas.drawCircle(cx, cy, radius, ring);
        }
        canvas.restoreToCount(save);
    }
}