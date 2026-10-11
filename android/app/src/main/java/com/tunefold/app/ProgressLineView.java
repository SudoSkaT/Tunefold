package com.tunefold.app;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.view.View;

/**
 * La posición de reproducción: una línea fina y su punto de juego.
 *
 * <p>Las barras de arriba son el sonido; esto es <em>dónde</em> vamos. Separar
 * las dos cosas es lo que hace que no estén compitiendo: antes una barra de
 * progreso y una forma de onda pretendían ser las dos el progreso, y quedaban dos
 * verdades sobre la misma posición.
 *
 * <p>Repintar sólo cuando la posición cambia de píxel, y la posición viene
 * siempre de la sesión. El punto del gesto (seek) vive aquí también, pero el
 * gesto sólo se acepta cuando la sesión puede cumplirlo (§8).
 */
final class ProgressLineView extends View implements ArtworkTheme.AccentListener {

    private final Paint played = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint rest = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint head = new Paint(Paint.ANTI_ALIAS_FLAG);

    private float fraction = ProgressScale.UNKNOWN;
    private float drawnFraction = -2f;
    private float drawnX = Integer.MIN_VALUE;
    private int headPx;

    ProgressLineView(Context context) {
        super(context);
        DesignTokens.init(context);
        // Los dos colores de la línea vienen de la portada, igual que las barras: la
        // posición y el sonido son la misma canción y se pintan con el mismo tono. El
        // punto de la cabeza sí es neutro, porque marca "ahora" sobre cualquier color.
        played.setColor(ArtworkTheme.barsPrimary());
        head.setColor(DesignTokens.Palette.ON_SURFACE);
        rest.setColor(DesignTokens.scaleAlpha(DesignTokens.Palette.ON_SURFACE_MUTED,
                DesignTokens.ALPHA_MUTED_SHAPE));
        setContentDescription("Playback position");
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
    }

    @Override protected void onAttachedToWindow() {
        super.onAttachedToWindow();
        ArtworkTheme.register(this);
    }

    @Override protected void onDetachedFromWindow() {
        ArtworkTheme.unregister(this);
        super.onDetachedFromWindow();
    }

    /**
     * Aplica la posición de la sesión.
     *
     * @param positionMs posición del active track, según la sesión
     * @param durationMs duración, o un valor no positivo si no se conoce
     */
    void setPosition(long positionMs, long durationMs) {
        fraction = ProgressScale.fraction(positionMs, durationMs);
        float x = fraction < 0f ? Integer.MIN_VALUE : Math.round(fraction * getWidth());
        if (fraction == drawnFraction && x == drawnX) return;
        drawnFraction = fraction;
        drawnX = x;
        invalidate();
    }

    @Override protected void onSizeChanged(int width, int height, int oldWidth, int oldHeight) {
        super.onSizeChanged(width, height, oldWidth, oldHeight);
        drawnX = Integer.MIN_VALUE;
        drawnFraction = -2f;
    }

    @Override protected void onDraw(Canvas canvas) {
        int width = getWidth();
        int height = getHeight();
        if (width <= 0 || height <= 0) return;
        int thickness = Math.max(1, DesignTokens.dp(DesignTokens.PROGRESS_LINE_THICKNESS));
        int middle = height / 2;
        float top = middle - thickness / 2f;
        // Resto de la pista: la línea entera, en silencio.
        canvas.drawRect(0f, top, width, top + thickness, rest);
        if (fraction < 0f) {
            // Sin duración conocida no se inventa progreso: sólo la posición, como
            // un punto, que es un hecho (§7).
            if (height > DesignTokens.dp(DesignTokens.SOUNDBARS_HEAD_SIZE) / 2) {
                canvas.drawCircle(0f, middle, radius(), head);
            }
            return;
        }
        float x = fraction * width;
        canvas.drawRect(0f, top, x, top + thickness, played);
        canvas.drawCircle(x, middle, radius(), head);
    }

    private float radius() {
        return DesignTokens.dp(DesignTokens.SOUNDBARS_HEAD_SIZE) / 2f;
    }

    @Override public void onAccentChanged(int accent) {
        invalidate();
    }

    @Override public void onPaletteChanged(int barsPrimary, int barsSecondary) {
        played.setColor(barsPrimary);
        invalidate();
    }
}