package com.tunefold.app;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.ColorFilter;
import android.graphics.Paint;
import android.graphics.PixelFormat;
import android.graphics.Rect;
import android.graphics.drawable.Drawable;

/**
 * The rule that separates a surface from the page it is laid over.
 *
 * <p>Two places need it and neither can afford a child view for it: the mini player
 * is priced by every dp it takes, and the navigation strip is permanently on screen.
 * A {@code foreground} costs one {@code drawLine} over a couple of pixels and
 * allocates nothing per frame.
 *
 * <p>It is also the reason the strip's contents stopped looking cut off: a list
 * scrolling under the bar used to simply stop at its edge, which reads as a
 * rendering fault rather than as "there is more below".
 */
final class TopRule extends Drawable {

    private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);

    TopRule(Context context) {
        paint.setColor(DesignTokens.Palette.HAIRLINE);
        paint.setStrokeWidth(Math.max(1f, DesignTokens.dp(DesignTokens.RULE_THICKNESS)));
    }

    @Override public int getOpacity() { return PixelFormat.TRANSLUCENT; }

    @Override public void setAlpha(int alpha) { paint.setAlpha(alpha); }

    @Override public void setColorFilter(ColorFilter filter) { paint.setColorFilter(filter); }

    @Override public void draw(Canvas canvas) {
        Rect bounds = getBounds();
        if (bounds.isEmpty()) return;
        float y = paint.getStrokeWidth() / 2f;
        canvas.drawLine(bounds.left, y, bounds.right, y, paint);
    }
}