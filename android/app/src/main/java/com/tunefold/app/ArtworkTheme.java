package com.tunefold.app;

import java.util.concurrent.CopyOnWriteArrayList;

/**
 * El color que viene de la portada, y quién tiene que repintarse cuando cambia.
 *
 * <p>Existe para que el acento sea un <i>valor</i> y no una constante grabada en
 * cada control. La identidad del producto es ARTWORK → COLOUR → WAVEFORM →
 * PLAYBACK, y este es el punto donde el color deja de decidirlo la marca y lo
 * decide la portada. Nada en la UI fija un acento, así que un color derivado llega
 * a todos los sitios a la vez y no puede dejarse ninguno atrás.
 *
 * <p>Son dos colores porque la visualización es estéreo: el canal izquierdo y el
 * derecho llevan tonos distintos de la misma portada, y un control lleva el tono
 * principal elevado para que el contenido oscuro se lea encima. Los dos vienen de
 * la misma extracción ({@link ArtworkPalette}).
 *
 * <h3>Modelo de coste</h3>
 * El registro ocurre una vez, al construir cada control. Los repintados son
 * explícitos y dirigidos: un oyente recibe el color nuevo y se invalida, que son
 * unas cuantas vistas por cambio de pista, no un rebind del árbol entero. La
 * lista es copy-on-write porque el hilo interesante (el que decodifica la
 * portada) no es el que registra.
 */
final class ArtworkTheme {

    /** Una vista que se pinta con el acento y debe repintarse cuando se mueve. */
    interface AccentListener {
        /** El acento cambió a {@code accent}; repintar ahora, no reconstruir. */
        void onAccentChanged(int accent);

        /**
         * Los dos tonos de la portada han cambiado.
         *
         * <p>Por defecto no hace nada: a un botón de 48&nbsp;dp sólo le importa
         * el acento, y obligar a todos los controles a conocer la paleta de las
         * barras sería repartir un detalle de la visualización por toda la app.
         */
        default void onPaletteChanged(int barsPrimary, int barsSecondary) { }
    }

    private static final CopyOnWriteArrayList<AccentListener> LISTENERS =
            new CopyOnWriteArrayList<>();
    private static volatile int accent = DesignTokens.Palette.ACCENT;
    private static volatile int barsPrimary = DesignTokens.Palette.ACCENT;
    private static volatile int barsSecondary = DesignTokens.Palette.ACCENT;
    /** Si la portada actual ofrece de verdad dos tonos distintos. */
    private static volatile boolean duoTone;
    private static volatile boolean derived;

    /**
     * Si la portada en pantalla trajo color, o si manda el color de marca.
     *
     * <p>Lo lee la composición del reproductor: una portada gris, negra o blanca no
     * tiene con qué construir una composición partida, y fingir el contraste de dos
     * zonas con la misma tinta sólo hace que la pantalla parezca partida sin motivo.
     * En cuanto la portada lendió su color —aunque sean dos tonos muy próximos y las
     * barras compartan tinta— la partida se justifica.
     */
    static boolean paletteDerived() { return derived; }

    /**
     * Si la portada trajo dos tonos distintos entre sí.
     *
     * <p>No gobierna la composición: gobierna las barras, que son dos canales y
     * necesitan dos tintas para no leerse como el mismo dibujo repetido.
     */
    static boolean duoTone() { return duoTone; }

    private ArtworkTheme() { }

    /** El acento con el que se pintan los controles. */
    static int accent() { return accent; }

    /** El contenido que se dibuja sobre {@link #accent()}. */
    static int onAccent() { return DesignTokens.Palette.ON_ACCENT; }

    /** Tono del canal izquierdo de las barras, derivado de la portada. */
    static int barsPrimary() { return barsPrimary; }

    /** Tono del canal derecho de las barras, derivado de la portada. */
    static int barsSecondary() { return barsSecondary; }

    /**
     * Registra {@code listener} y lo pone al día con el color actual.
     *
     * <p>Se llama desde los constructores, así que un control no puede empezar
     * teñido con un color distinto del resto de la pantalla.
     */
    static void register(AccentListener listener) {
        if (listener == null) return;
        LISTENERS.addIfAbsent(listener);
        listener.onAccentChanged(accent);
        listener.onPaletteChanged(barsPrimary, barsSecondary);
    }

    /** Da de baja {@code listener}; siempre emparejado con {@link #register}. */
    static void unregister(AccentListener listener) {
        if (listener == null) return;
        LISTENERS.remove(listener);
    }

    /** Cuántos oyentes hay; para la guarda de fugas de las pruebas de componente. */
    static int listenerCount() { return LISTENERS.size(); }

    /**
     * Publica una paleta derivada de la portada y avisa a todos.
     *
     * <p>El acento de los controles es el tono principal <em>elevado</em>: el disco
     * del botón lleva contenido casi negro encima y ese contenido tiene que leerse
     * con cualquier portada. Las barras usan los tonos sin tocar, porque sólo
     * muestran datos sobre una tarjeta oscura.
     *
     * <p>Sin cambios no hay coste: una portada monocroma o muy apagada devuelve la
     * marca y no se repinta nada.
     */
    static void setPalette(ArtworkPalette.Palette palette) {
        if (palette == null) return;
        int control = palette.derived
                ? ArtworkPalette.lift(palette.primary) : DesignTokens.Palette.ACCENT;
        int primary = palette.derived ? palette.primary : DesignTokens.Palette.ACCENT;
        int secondary = palette.derived ? palette.secondary : DesignTokens.Palette.ACCENT;
        boolean two = palette.offersMoreThanOneColour();
        boolean lent = palette.derived;
        if (control == accent && primary == barsPrimary && secondary == barsSecondary
                && two == duoTone && lent == derived) return;
        boolean accentMoved = control != accent;
        accent = control;
        barsPrimary = primary;
        barsSecondary = secondary;
        duoTone = two;
        derived = lent;
        for (AccentListener listener : LISTENERS) {
            if (accentMoved) listener.onAccentChanged(control);
            listener.onPaletteChanged(primary, secondary);
        }
    }

    /** Vuelve al color de marca; lo usa el recreo de la actividad y las pruebas. */
    static void reset() {
        setPalette(ArtworkPalette.Palette.brand(DesignTokens.Palette.ACCENT));
    }
}