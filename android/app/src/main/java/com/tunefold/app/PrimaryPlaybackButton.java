package com.tunefold.app;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Outline;
import android.graphics.Paint;
import android.view.View;
import android.view.ViewOutlineProvider;

/**
 * The one control the user is expected to press most: play or pause.
 *
 * <p>Adds exactly two things to {@link IconButton} — a filled accent disc and a larger
 * target — because that is the whole difference. It stays a single view: a
 * {@code FrameLayout} wrapping a background view and an icon view would double the
 * node count of the transport row and add a measure pass to every layout change for
 * no benefit, since a disc clips itself.
 *
 * <p>The disc is painted rather than being a rounded-rectangle drawable, so the same
 * code is correct at any target size and there is no corner radius to get wrong.
 *
 * <p>Pressing changes one colour and invalidates. Nothing here animates: the one
 * control that must never be busy is the one the user presses while audio is
 * starting.
 */
final class PrimaryPlaybackButton extends IconButton {

    private final Paint disc = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint working = new Paint(Paint.ANTI_ALIAS_FLAG);
    private boolean pressed;
    private boolean busy;

    PrimaryPlaybackButton(Context context) {
        super(context, DesignTokens.CONTROL_TARGET_PRIMARY, DesignTokens.ICON_SIZE_LARGE);
        disc.setColor(ArtworkTheme.accent());
        // The mask is a full square and the outline clip turns it into a circle, so
        // the highlight cannot spill onto the card behind it.
        setBackground(ripple(DesignTokens.rippleColor(true), true));
        setOutlineProvider(new ViewOutlineProvider() {
            @Override public void getOutline(View view, Outline outline) {
                outline.setOval(0, 0, view.getWidth(), view.getHeight());
            }
        });
        setClipToOutline(true);
        // El aro de trabajo se dibuja por fuera del disco, con el trazo fino: dice
        // "este control está ocupado" sin tapar el glifo ni parecer otro control.
        working.setStyle(Paint.Style.STROKE);
        working.setStrokeCap(Paint.Cap.ROUND);
        working.setStrokeWidth(DesignTokens.dp(DesignTokens.PROGRESS_LINE_THICKNESS) * 2f);
        working.setColor(DesignTokens.scaleAlpha(ArtworkTheme.onAccent(),
                DesignTokens.ALPHA_MUTED_SHAPE));
    }

    /**
     * Whether this control is waiting on something instead of being ready.
     *
     * <p>Un aro, sin giro: el estado se dibuja al ritmo del tick que ya existe, y
     * un spinner con su propio bucle sería trabajo de fondo para decir algo que el
     * estado ya dice con palabras. El aro es la misma forma que usa el punto de
     * estado, así que las dos señales se leen como una sola.
     */
    void setBusy(boolean value) {
        if (busy == value) return;
        busy = value;
        invalidate();
    }

    @Override public void setPressed(boolean value) {
        if (pressed == value) return;
        pressed = value;
        super.setPressed(value);
        disc.setColor(value ? DesignTokens.Palette.ACCENT_PRESSED : ArtworkTheme.accent());
        invalidate();
    }

    @Override boolean usesAccentSurface() { return true; }

    @Override int resolveGlyphColor() {
        return isEnabled() ? ArtworkTheme.onAccent() : DesignTokens.Palette.ON_SURFACE_FAINT;
    }

    @Override int rippleColor() { return DesignTokens.rippleColor(true); }

    @Override public void onAccentChanged(int accent) {
        if (!pressed) disc.setColor(accent);
        working.setColor(DesignTokens.scaleAlpha(DesignTokens.Palette.ON_ACCENT,
                DesignTokens.ALPHA_MUTED_SHAPE));
        super.onAccentChanged(accent);
    }

    @Override protected void onDraw(Canvas canvas) {
        float radius = Math.min(getWidth(), getHeight()) / 2f;
        if (isEnabled()) {
            canvas.drawCircle(getWidth() / 2f, getHeight() / 2f, radius, disc);
        }
        if (busy && isEnabled()) {
            canvas.drawCircle(getWidth() / 2f, getHeight() / 2f,
                    radius - working.getStrokeWidth() / 2f, working);
        }
        super.onDraw(canvas);
    }
}