package com.tunefold.app;

import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.LinearGradient;
import android.graphics.Outline;
import android.graphics.Paint;
import android.graphics.Rect;
import android.graphics.Shader;
import android.view.View;
import android.view.ViewOutlineProvider;

/**
 * The cover: square, corner-rounded, and correct before, during and after loading.
 *
 * <p>Owns the whole artwork concern the surface used to spread across the Activity
 * and {@code NowPlayingView}: the placeholder, the swap, the identity guard against a
 * late decode, and the square constraint.
 *
 * <h3>Why it is cheap</h3>
 * <ul>
 *   <li>the placeholder is one {@link Paint} with a gradient rebuilt only when the
 *       width changes, so a scroll or a rotation does not allocate per frame;</li>
 *   <li>the rounded corners are an <em>outline clip</em>, so there is no
 *       round-rect {@code BitmapShader} matrix to recompute and no transparent bitmap
 *       overdraw;</li>
 *   <li>{@link #setCover} refuses a bitmap whose identity is not the one the view is
 *       waiting for, so a decode that finishes after the user skipped cannot repaint
 *       the previous cover over the new one — the failure mode every timed callback
 *       had before this guard existed.</li>
 * </ul>
 */
final class ArtworkView extends View {

    private final Paint placeholder = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint cover = new Paint(Paint.ANTI_ALIAS_FLAG | Paint.FILTER_BITMAP_FLAG);
    private final Rect destination = new Rect();
    private final Rect source = new Rect();

    /** Identity the displayed cover belongs to; empty while showing the placeholder. */
    private String coverId = "";
    /** Identity the next {@link #setCover} is allowed to paint. */
    private String expectedCoverId = "";
    private Bitmap bitmap;
    private int gradientForWidth = -1;
    private int cornerRadius;
    private boolean square = true;

    ArtworkView(Context context) {
        super(context);
        DesignTokens.init(context);
        cornerRadius = DesignTokens.dp(DesignTokens.RADIUS_LARGE);
        placeholder.setColor(DesignTokens.Palette.SURFACE_ARTWORK);
        setOutlineProvider(new ViewOutlineProvider() {
            @Override public void getOutline(View view, Outline outline) {
                outline.setRoundRect(0, 0, view.getWidth(), view.getHeight(), cornerRadius);
            }
        });
        setClipToOutline(true);
        setContentDescription("Artwork");
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
    }

    /**
     * Declares which identity the next {@link #setCover} is allowed to paint.
     *
     * <p>Called before a decode starts, and kept separate from {@link #coverId} so a
     * load that never arrives cannot make the view look current.
     */
    void expectCover(String identity) {
        expectedCoverId = identity == null ? "" : identity;
    }

    /**
     * Shows {@code value} for {@code identity}, or the placeholder for {@code null}.
     *
     * @param value {@code null} clears the cover; any bitmap whose identity is not the
     *             expected one is discarded
     */
    void setCover(String identity, Bitmap value) {
        if (value == null) {
            clearCover();
            return;
        }
        if (identity == null || !identity.equals(expectedCoverId)) return;
        if (value == bitmap && identity.equals(coverId)) return;
        bitmap = value;
        coverId = identity;
        invalidate();
    }

    /** True when {@code identity} is what the view is currently showing. */
    boolean isShowing(String identity) {
        return identity != null && identity.equals(coverId);
    }

    /** Identity currently displayed, or the empty string for the placeholder. */
    String shownCoverId() { return coverId; }

    /** Drops the cover and returns to the placeholder. */
    void clearCover() {
        if (bitmap == null && coverId.isEmpty()) return;
        bitmap = null;
        coverId = "";
        invalidate();
    }

    /** Corner radius in {@code dp}; applied immediately. */
    void setCornerRadiusDp(float dp) {
        int value = DesignTokens.dp(dp);
        if (value == cornerRadius) return;
        cornerRadius = value;
        invalidateOutline();
    }

    /** Whether the view keeps the 1:1 proportion of cover art. */
    void setSquare(boolean value) {
        if (square == value) return;
        square = value;
        requestLayout();
    }

    @Override protected void onMeasure(int widthMeasureSpec, int heightMeasureSpec) {
        if (!square) {
            super.onMeasure(widthMeasureSpec, heightMeasureSpec);
            return;
        }
        int width = MeasureSpec.getSize(widthMeasureSpec);
        int height = MeasureSpec.getSize(heightMeasureSpec);
        // A square cover must never be measured against an unbounded height, which is
        // exactly what a wrap-content parent in a vertical stack hands down.
        int side = height <= 0 ? width : Math.min(width, height);
        if (side <= 0) side = width;
        int exact = MeasureSpec.makeMeasureSpec(side, MeasureSpec.EXACTLY);
        super.onMeasure(exact, exact);
    }

    @Override protected void onDraw(Canvas canvas) {
        int width = getWidth();
        int height = getHeight();
        if (width <= 0 || height <= 0) return;
        drawPlaceholder(canvas, width, height);
        Bitmap current = bitmap;
        if (current == null || current.isRecycled()) return;
        destination.set(0, 0, width, height);
        canvas.drawBitmap(current, centreCrop(current), destination, cover);
    }

    private void drawPlaceholder(Canvas canvas, int width, int height) {
        if (gradientForWidth != width || placeholder.getShader() == null) {
            placeholder.setShader(new LinearGradient(0f, 0f, width, height,
                    DesignTokens.Palette.SURFACE_ARTWORK,
                    DesignTokens.scaleAlpha(DesignTokens.Palette.SURFACE_ARTWORK_EDGE,
                            DesignTokens.Palette.ALPHA_ARTWORK_EDGE),
                    Shader.TileMode.CLAMP));
            gradientForWidth = width;
        }
        canvas.drawRect(0f, 0f, width, height, placeholder);
    }

    /** The centre square of {@code value}: cover art is always square, so that is what survives. */
    private Rect centreCrop(Bitmap value) {
        int bitmapWidth = value.getWidth();
        int bitmapHeight = value.getHeight();
        if (bitmapWidth <= 0 || bitmapHeight <= 0) {
            source.set(0, 0, 1, 1);
            return source;
        }
        int side = Math.min(bitmapWidth, bitmapHeight);
        int left = (bitmapWidth - side) / 2;
        int top = (bitmapHeight - side) / 2;
        source.set(left, top, left + side, top + side);
        return source;
    }

    @Override protected void onSizeChanged(int width, int height, int oldWidth, int oldHeight) {
        super.onSizeChanged(width, height, oldWidth, oldHeight);
        invalidateOutline();
    }
}