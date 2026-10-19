package com.tunefold.app;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.RectF;
import android.view.View;

/**
 * Las cinco muestras de un tema, en una fila.
 *
 * <p>Es la única forma honesta de mostrar cinco colores sin nombres: el usuario
 * elige con los ojos, no leyendo una etiqueta. Y son los cinco que ha elegido, en
 * su orden y con su valor exacto — no una muestra «representativa» del tema, que
 * sería otra mentira pequeña más.
 *
 * <p>El dibujo es el coste: cinco rectángulos redondeados, un {@link Paint} y un
 * {@link RectF} que se reutilizan, y nada se reserva. Se pinta una vez al
 * construirse y una vez más si el tema cambia.
 */
final class ThemeSwatchView extends View {

    private static final int MAX_CHIPS = 5;

    private final ThemeSpec theme;
    private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final RectF chip = new RectF();
    /** Ancho del hueco entre muestras, en {@code dp}. */
    private final int gap;

    ThemeSwatchView(Context context, ThemeSpec theme) {
        super(context);
        DesignTokens.init(context);
        this.theme = theme == null ? ThemeSpec.DEFAULT : theme;
        gap = DesignTokens.dp(DesignTokens.SPACE_1);
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
        setFocusable(false);
        setContentDescription(this.theme.name);
    }

    /** El tema cuyas muestras se están dibujando. */
    ThemeSpec theme() { return theme; }

    @Override protected void onMeasure(int widthMeasureSpec, int heightMeasureSpec) {
        int width = MeasureSpec.getSize(widthMeasureSpec);
        int height = Math.max(1, Math.round(DesignTokens.dp(DesignTokens.ICON_SIZE_EXTRA_SMALL)));
        // La altura pedida manda: la fila es la que decide cuánto alto es este
        // control, y una muestra que se saliera de su fila rompería la línea base
        // con todo lo demás.
        setMeasuredDimension(width, MeasureSpec.getSize(heightMeasureSpec) > 0
                ? MeasureSpec.getSize(heightMeasureSpec) : height);
    }

    @Override protected void onDraw(Canvas canvas) {
        int width = getWidth();
        int height = getHeight();
        if (width <= 0 || height <= 0) return;
        int count = Math.min(MAX_CHIPS, theme.colors.length);
        float radius = height * 0.22f;
        // La última muestra se queda con el ancho que sobra, para que la fila
        // llegue justo al final aunque los colores no dividan.
        float unit = (width - gap * (count - 1)) / (float) count;
        for (int index = 0; index < count; index++) {
            float left = index * (unit + gap);
            chip.set(left, 0f, left + unit, height);
            paint.setColor(theme.color(index));
            canvas.drawRoundRect(chip, radius, radius, paint);
        }
    }
}