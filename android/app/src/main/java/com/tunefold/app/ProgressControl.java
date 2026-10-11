package com.tunefold.app;

import android.content.Context;
import android.util.TypedValue;
import android.view.Gravity;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * La superficie de progreso: barras reactivas, línea de posición y los dos tiempos.
 *
 * <p>Son tres Cosas y una sola idea —dónde estamos y qué suena ahí—, por eso
 * viven juntas. Antes esta clase tenía una barra de progreso y ahora tiene las
 * barras del sonido: meter las dos cosas que significan «posición» en el mismo
 * sitio era tener dos verdades sobre el mismo dato, y discrepaban un fotograma
 * cada vez.
 *
 * <h3>El tick</h3>
 * Dos cadencias distintas y deliberadas. Las barras leen el audio a 20 Hz
 * ({@link #refreshSound}) y sólo redibujan si hay audio nuevo; la línea y las
 * etiquetas van a 5 Hz ({@link #setPosition}) y sólo cambian cuando el segundo
 * cambia o el punto cruza un píxel. Una superficie que dibuja a 20 Hz cuando el
 * audio está parado es una superficie que no está en reposo.
 */
final class ProgressControl extends LinearLayout {

    private final SoundBarsView bars;
    private final ProgressLineView line;
    private final TextView elapsed;
    private final TextView total;

    private final StringBuilder scratch = new StringBuilder(8);
    /** Segundo mostrado a la izquierda, o -1. */
    private long shownSecond = -1L;
    /** Identidad a la que pertenece el tiempo mostrado. */
    private TrackKey shownKey;
    /** Duración mostrada a la derecha, o -1. */
    private long shownDuration = -1L;
    /** Verdadero mientras la etiqueta izquierda muestra un destino, no la posición. */
    private boolean previewing;
    /** Color dado a la etiqueta izquierda, para no re-tintarla por fotograma. */
    private int shownLabelColor = DesignTokens.Palette.ON_SURFACE_MUTED;

    ProgressControl(Context context) {
        super(context);
        DesignTokens.init(context);
        setOrientation(VERTICAL);

        bars = new SoundBarsView(context);
        addView(bars, new LayoutParams(LayoutParams.MATCH_PARENT,
                DesignTokens.dp(DesignTokens.SOUNDBARS_HEIGHT)));

        line = new ProgressLineView(context);
        LayoutParams lineParams = new LayoutParams(LayoutParams.MATCH_PARENT,
                DesignTokens.dp(DesignTokens.PROGRESS_LINE_THICKNESS)
                        + DesignTokens.dp(DesignTokens.SOUNDBARS_HEAD_SIZE));
        lineParams.topMargin = DesignTokens.dp(DesignTokens.SOUNDBARS_LANE_GAP);
        addView(line, lineParams);

        LinearLayout times = new LinearLayout(context);
        times.setOrientation(HORIZONTAL);
        times.setGravity(Gravity.CENTER_VERTICAL);

        elapsed = timeLabel(context);
        elapsed.setGravity(Gravity.START);
        total = timeLabel(context);
        total.setGravity(Gravity.END);

        int labelWidth = DesignTokens.dp(DesignTokens.TIME_LABEL_WIDTH);
        times.addView(elapsed, new LayoutParams(labelWidth, LayoutParams.WRAP_CONTENT));
        times.addView(total, new LayoutParams(0, LayoutParams.WRAP_CONTENT, 1f));
        LayoutParams timesParams = new LayoutParams(LayoutParams.MATCH_PARENT,
                LayoutParams.WRAP_CONTENT);
        timesParams.topMargin = DesignTokens.dp(DesignTokens.GAP_TIGHT);
        addView(times, timesParams);
    }

    private static TextView timeLabel(Context context) {
        TextView view = new TextView(context);
        view.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_CAPTION_SIZE);
        view.setTextColor(DesignTokens.Palette.ON_SURFACE_MUTED);
        // A numeric format, not prose: not translatable and not a string resource.
        view.setText("0:00");
        view.setIncludeFontPadding(false);
        return view;
    }

    /** Las barras, para enlazar la fuente de audio. */
    SoundBarsView bars() { return bars; }

    /**
     * Compacta o devuelve la altura completa de las barras.
     *
     * <p>Una ventana corta —un teléfono en horizontal— no tiene sitio para las dos
     * cosas: las barras y el transporte. Las barras son las que ceden, porque son las
     * que siguen telling the truth at a glance; el transporte no se toca nunca.
     *
     * <p>Solo se escribe cuando el número cambia, porque cada {@code setLayoutParams}
     * pide un layout de la tarjeta entera.
     */
    void setCompact(boolean value) {
        int wanted = DesignTokens.dp(value
                ? DesignTokens.SOUNDBARS_HEIGHT_COMPACT : DesignTokens.SOUNDBARS_HEIGHT);
        LayoutParams params = (LayoutParams) bars.getLayoutParams();
        if (params.height == wanted) return;
        params.height = wanted;
        bars.setLayoutParams(params);
    }

    /**
     * Un tick de posición (5 Hz).
     *
     * @param key        identidad a la que pertenece la posición; un tick de otra
     *                   pista se descarta, así que una actualización tardía no
     *                   puede pintar el tiempo de la canción anterior
     * @param positionMs tiempo transcurrido de esa identidad
     * @param durationMs duración, o un valor no positivo si no se conoce
     */
    void setPosition(TrackKey key, long positionMs, long durationMs) {
        if (key == null) return;
        if (!key.equals(shownKey)) {
            // Una identidad nueva reinicia cada caché: nada de la pista anterior
            // sobrevive en la nueva, ni siquiera la duración.
            shownKey = key;
            shownSecond = -1L;
            shownDuration = -1L;
            previewing = false;
        }
        if (durationMs > 0L && durationMs != shownDuration) {
            shownDuration = durationMs;
            TimeFormat.time(durationMs, scratch);
            total.setText(scratch.toString());
        }
        if (!previewing) {
            long second = ProgressScale.displayedSecond(positionMs);
            if (second != shownSecond) {
                shownSecond = second;
                TimeFormat.time(positionMs, scratch);
                elapsed.setText(scratch.toString());
            }
        }
        line.setPosition(positionMs, durationMs);
    }

    /** Vacía la superficie; se usa cuando no suena nada. */
    void clear() {
        shownKey = null;
        shownSecond = -1L;
        shownDuration = -1L;
        previewing = false;
    }

    /** El tick rápido de las barras: lee el audio en vivo. */
    void refreshSound(boolean playable) { bars.refresh(playable); }

    /** Enlaza la fuente de audio en vivo. */
    void attach(LiveAudioBars source) { bars.attach(source); }

    /** Contadores para el panel de diagnóstico. */
    String stats() { return bars.stats(); }
}