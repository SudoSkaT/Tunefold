package com.tunefold.app;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.Path;
import android.os.SystemClock;
import android.view.View;

/**
 * Barras de sonido reactivas: lo que se está oyendo, en los colores de la portada.
 *
 * <p>Dos carriles —izquierda arriba, derecha abajo— porque la señal es estéreo y
 * comparar los canales es justo lo que una visualización estéreo permite. Cada
 * barra dibuja el valle y el pico del tramo, con extremos redondeados, y toma su
 * color de la portada que se está mostrando.
 *
 * <h3>Reaccionar sin repintar de más</h3>
 * La fuente ({@link LiveAudioBars}) devuelve la generación de la ventana de audio.
 * Si no ha cambiado —la pista en pausa, un buffering largo, la pantalla quieta—
 * esta vista no se invalida <b>una sola vez</b>. En reposo el coste es cero, no
 * "casi cero".
 *
 * <h3>Sin animación propia</h3>
 * No hay bucle, ni interpolación, ni suavizado con objetos: cada barra es la
 * medida de la ventana que la originó. Un visualizador que persigue al valor con
 * un temporizador propio sigue moviéndose con la pista en pausa, que es
 * exactamente el defecto que hace que una pantalla parezca viva cuando no lo está.
 *
 * <h3>Sin trabajo en el dibujo</h3>
 * {@link #onDraw} son dos {@code drawPath} sobre trazados que ya existen. La
 * geometría se construye cuando llega audio nuevo, en {@link #refresh()}, y la
 * <em>medida</em> de la geometría se hace una sola vez, cuando cambia el tamaño,
 * en {@link #onSizeChanged}: dibujar nunca reserva memoria ni decide nada, y
 * medir veinte veces por segundo para obtener el mismo número tampoco.
 *
 * <h3>Cambiar de tamaño no es volver a leer</h3>
 * Al rotar, el ancho decide cuántas barras caben y la figura tiene que cambiar;
 * el audio no. Así que un cambio de tamaño con el mismo número de barras recoloca
 * la geometría sobre los valores que ya están en memoria y no hace ni una llamada
 * al motor. Sólo cuando caben más barras se vuelve a leer, porque sólo entonces
 * hay algo nuevo que leer.
 */
final class SoundBarsView extends View implements ArtworkTheme.AccentListener {

    /**
     * Cuántos carriles se dibujan: <b>uno</b>.
     *
     * <p>Dos, que es lo que había: un canal arriba y otro abajo. El argumento para
     * los dos era que una visualización estéreo con un solo color se lee como el mismo
     * dibujo repetido, y eso sigue siendo cierto — por eso los dos carriles usan dos
     * tintes derivados de la portada. El argumento en contra pesó más: dos filas en
     * posiciones distintas parecen dos elementos descolocados en lugar de una figura,
     * y la mitad de la altura que ocupaban era más valiosa mostrando la letra.
     *
     * <p>Un carril no es una barra menos: es la envolvente de <em>ambos</em> canales,
     * así que sigue moviéndose con todo lo que se oye.
     */
    private static final int LANES = 1;

    /** Un trazo vertical por barra; los extremos redondeados salen del trazo. */
    private final Paint barPaint = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Path bars = new Path();

    private LiveAudioBars source;
    private int paintedBars;
    private int laneHeight;
    private int barPitch;
    private int barWidth;
    /** Barras que caben en el ancho actual; decide si hay que releer. */
    private int wantedBars;

    // ---- contadores: la evidencia del informe, no decoración ----
    private int repaints;
    private long lastDrawNanos;
    /** Mayor amplitud pintada, 0..1: si esto no se mueve, el audio tampoco. */
    private float peak;

    SoundBarsView(Context context) {
        super(context);
        DesignTokens.init(context);
        barPaint.setStyle(Paint.Style.STROKE);
        barPaint.setStrokeCap(Paint.Cap.ROUND);
        setContentDescription("Live stereo level");
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
        paintColors(ArtworkTheme.barsPrimary(), ArtworkTheme.barsSecondary());
    }

    @Override protected void onAttachedToWindow() {
        super.onAttachedToWindow();
        ArtworkTheme.register(this);
    }

    @Override protected void onDetachedFromWindow() {
        ArtworkTheme.unregister(this);
        super.onDetachedFromWindow();
    }

    /** Enlaza la fuente de audio; {@code null} la suelta y vacía la superficie. */
    void attach(LiveAudioBars value) {
        source = value;
        if (value != null) value.clear();
        bars.rewind();
        paintedBars = 0;
        invalidate();
    }

    /**
     * Drops whatever is painted without detaching the source.
     *
     * <p>For track changes and stops: the previous track's envelope must not
     * linger frozen while the next one resolves, and a stopped surface rests
     * empty rather than showing audio that is no longer there. Pauses keep
     * their frozen frame (see {@link #refresh}), which is why this is a
     * separate, explicit call and not part of it.
     */
    void clearPainted() {
        bars.rewind();
        paintedBars = 0;
        peak = 0f;
        invalidate();
    }

    /**
     * Lee la ventana en vivo y, sólo si hay audio nuevo, reconstruye y repinta.
     *
     * <p>Es el único punto donde estas barras hacen trabajo. Devuelve {@code false}
     * cuando no hay audio nuevo, y en ese caso no hay `invalidate`, no hay
     * asignación y no hay lectura de valores: la superficie sigue intacta.
     *
     * @param playable whether the engine is engaged with the current track
     * @return {@code true} si se repintó
     */
    boolean refresh(boolean playable) {
        LiveAudioBars current = source;
        if (current == null) return false;
        if (!playable) {
            // Sin pista en marcha no hay ventana que leer; lo que haya pintado se
            // queda, congelado, en lugar de parpadear a vacío.
            return false;
        }
        int wanted = barsThatFit();
        wantedBars = wanted;
        if (!current.poll(wanted)) return false;
        build(current);
        invalidate();
        return true;
    }

    @Override protected void onSizeChanged(int width, int height, int oldWidth, int oldHeight) {
        super.onSizeChanged(width, height, oldWidth, oldHeight);
        // La geometría es una medida por tamaño, no una por fotograma: se calcula
        // aquí y `build()` la usa tal cual.
        measureGeometry();
        int wanted = barsThatFit();
        if (wanted == wantedBars && paintedBars > 0) {
            // Mismo número de barras, otro ancho: la figura se recoloca sobre los
            // valores que ya están en memoria. Ni una lectura al motor, ni una
            // asignación, y la rotación no puede cortar el audio para recolocarlo.
            if (source != null) {
                build(source);
                invalidate();
            }
            return;
        }
        // El ancho decide cuántas barras caben: hay que releer.
        if (source != null) refresh(true);
    }

/**
 * Mide el alto de un carril y reparte el ancho en pasos de barra.
 *
 * <p>El paso no es el del token cuando el ancho no lo permite: ver
 * {@link LiveAudioBars#pitchFor}. Se calcula aquí, una vez por tamaño, porque es
 * geometría y la geometría no se recalcula veinte veces por segundo.
 */
    private void measureGeometry() {
        // Todo el alto es del carril. El hueco entre carriles dejó de tener sentido
        // cuando sólo hay uno, y descontarlo aquí robaba una cuarta parte de una banda
        // de 36dp: la barra crecía hacia dentro mientras el carril se quedaba corto.
        laneHeight = Math.max(1, getHeight() / LANES);
        barPitch = LiveAudioBars.pitchFor(getWidth(),
                DesignTokens.dp(DesignTokens.SOUNDBARS_BAR_PITCH));
        barWidth = Math.max(1, barPitch - DesignTokens.dp(DesignTokens.SOUNDBARS_BAR_GAP));
        barPaint.setStrokeWidth(barWidth);
    }

    /** Cuántas barras caben ahora mismo en el ancho actual. */
    private int barsThatFit() {
        return LiveAudioBars.barsFor(getWidth(), LiveAudioBars.pitchFor(getWidth(),
                DesignTokens.dp(DesignTokens.SOUNDBARS_BAR_PITCH)));
    }

    /**
     * Convierte las barras medidas en un trazado, ya colocado.
     *
     * <p>Un segmento vertical por barra, centrado en su paso. La escala convierte
     * amplitud normalizada a píxeles contra el alto del carril, y un silencio se dibuja
     * como una marca mínima en el eje en vez de desaparecer: que no suene nada es
     * información, no la ausencia de información.
     *
     * <p>Con un solo carril cada barra es la <b>envolvente de los dos canales</b>: el
     * mínimo más bajo y el máximo más alto de la izquierda y la derecha. Dibujar un
     * canal y tirar el otro sería media información; promediarlos sería media
     * dinámica.
     */
    private void build(LiveAudioBars current) {
        if (laneHeight <= 0) measureGeometry();
        bars.rewind();
        peak = 0f;
        int count = Math.min(current.bars(), LiveAudioBars.MAX_BARS);
        float[] data = current.values();
        // Half the lane per unit amplitude: full-scale (±1) reaches exactly the
        // lane edge. The previous scale (a whole lane per unit) drew peaks at
        // twice the available height, clipping everything loud. The stroke's
        // round caps extend half a bar width past the endpoints, so that is
        // reserved up front.
        float halfStroke = barPaint.getStrokeWidth() / 2f;
        float scale = Math.max(0f, laneHeight / 2f - halfStroke);
        float middle = laneHeight / 2f;
        for (int bar = 0; bar < count; bar++) {
            int index = LiveAudioBars.BAR_VALUES * bar;
            // Los dos canales se leen en la misma barra: mínimo y máximo de la
            // envolvente de izquierda y derecha.
            float low = Math.min(data[index], data[index + 2]);
            float high = Math.max(data[index + 1], data[index + 3]);
            float x = (bar + 0.5f) * barPitch;
            // Raíz: una pista tranquila es toda la música de quien la escucha, y
            // con escala lineal sus barras no se moverían. La raíz expande los
            // valores bajos sin tocar los altos, así que la dinámica se sigue
            // viendo y ninguna barra llega a salirse del carril.
            float top = middle - expand(high) * scale;
            float bottom = middle - expand(low) * scale;
            if (high > peak) peak = high;
            if (-low > peak) peak = -low;
            if (top - bottom < 1f) {
                float centre = (top + bottom) * 0.5f;
                top = centre - 0.5f;
                bottom = centre + 0.5f;
            }
            bars.moveTo(x, top);
            bars.lineTo(x, bottom);
        }
        paintedBars = count;
    }

    /** Amplitud normalizada a altura de carril; 0 se queda en 0 y 1 llega al borde. */
    private static float expand(float amplitude) {
        float magnitude = Math.abs(amplitude);
        return magnitude <= 0f ? 0f : (float) Math.sqrt(magnitude);
    }

    @Override protected void onDraw(Canvas canvas) {
        int width = getWidth();
        int height = getHeight();
        if (width <= 0 || height <= 0) return;
        repaints++;
        long started = SystemClock.elapsedRealtimeNanos();
        if (paintedBars > 0) {
            canvas.drawPath(bars, barPaint);
        }
        lastDrawNanos = SystemClock.elapsedRealtimeNanos() - started;
    }

    @Override public void onAccentChanged(int accent) {
        // El acento no pinta las barras; sólo cambia tras un cambio de portada, que
        // llega por onPaletteChanged. Se repinta por si acaso.
        invalidate();
    }

    @Override public void onPaletteChanged(int barsPrimary, int barsSecondary) {
        // La figura no cambia, el color sí: un invalidate y nada más. Con un solo
        // carril la tinta es la del canal dominante; la segunda se queda para cuando
        // las barras vuelvan a ser dos.
        barPaint.setColor(barsPrimary);
        invalidate();
    }

    private void paintColors(int barsPrimary, int barsSecondary) {
        barPaint.setColor(barsPrimary);
    }

    /** Una línea para el panel de diagnóstico. */
    String stats() {
        return "bars=" + paintedBars + "/" + LiveAudioBars.MAX_BARS
                + " repaints=" + repaints
                + " reads=" + (source == null ? 0 : source.pollCount())
                + " new_audio=" + (source == null ? 0 : source.updateCount())
                + " peak=" + String.format(java.util.Locale.ROOT, "%.2f", peak)
                + " last_draw_us=" + (lastDrawNanos / 1000L);
    }
}