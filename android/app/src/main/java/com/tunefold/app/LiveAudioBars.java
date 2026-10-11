package com.tunefold.app;

/**
 * Lee la envolvente estéreo <em>en vivo</em>: lo que se está oyendo ahora mismo.
 *
 * <p>No es la forma de onda de la pista (eso la dibuja el propio audio), es la
 * última ventana medida por el análisis: 46&nbsp;ms de cada canal reducidos a un
 * puñado de barras. Por eso reacciona al sonido y por eso no necesita memoria de
 * la canción.
 *
 * <h3>Por qué devuelve una generación</h3>
 * El motor publica una ventana nueva ~86 veces por segundo. El consumidor va a
 * 20&nbsp;Hz, así que casi siempre lee una ventana nueva, pero cuando la pista
 * está en pausa o buffering <b>no</b> hay ventana nueva: sin el número de
 * generación, el consumidor repintaría veinte veces por segundo un dibujo idéntico.
 * Con el número, la vista se queda quieta y el coste en reposo es cero.
 *
 * <h3>Coste de una lectura</h3>
 * Una llamada JNI y una copia en un búfer que ya existe: nunca se reasigna, nunca
 * crece. Un arreglo por lectura es exactamente el tipo debage que hace que una
 * pantalla que se repinta a 20 Hz aparece en el perfil de GC.
 */
final class LiveAudioBars {

    /** Floats por barra: mínimo y máximo de cada canal. */
    static final int BAR_VALUES = 4;
    /** Tope de barras: más que esto no cabe en una pantalla y sólo cuesta CPU. */
    static final int MAX_BARS = 96;

    /**
     * La lectura cruda, como función.
     *
     * <p>Es la costura que hace testeable la decisión que importa: si esta ventana
     * trae audio que la anterior no traía. En producción es la llamada JNI; en las
     * pruebas es una lambda, y ninguna de las dos sabe qué hay al otro lado.
     */
    interface Reader {
        /**
         * @param bars cuántas barras caben; el lector nunca escribe más
         * @return la generación de la ventana leída
         */
        long read(float[] out, int bars);
    }

    private final float[] values = new float[BAR_VALUES * MAX_BARS];
    private final Reader reader;

    private int bars;
    private long generation;
    private long previousGeneration = -1L;
    private int polls;
    private int updates;

    LiveAudioBars(PlaybackController controller) {
        this.reader = controller == null ? null : new Reader() {
            private final PlaybackController source = controller;
            @Override public long read(float[] out, int bars) {
                long handle = source.engineHandle();
                return handle == 0L ? 0L : TunefoldBridge.readLiveWaveform(handle, out, bars);
            }
        };
    }

    LiveAudioBars(Reader reader) {
        this.reader = reader;
    }

    /** Las barras, como se leyeron por última vez. El arreglo no se reasigna. */
    float[] values() { return values; }

    /** Cuántas barras hay en {@link #values()}. */
    int bars() { return bars; }

    /** La última ventana vista, como número del motor. */
    long generation() { return generation; }

    /** Lecturas hechas; la mayoría devuelven la misma ventana. */
    int pollCount() { return polls; }

    /** Lecturas que traían audio de verdad. */
    int updateCount() { return updates; }

    /** Vacía el búfer: la superficie no debe pintar audio que ya no existe. */
    void clear() {
        java.util.Arrays.fill(values, 0f);
        bars = 0;
        generation = 0L;
        previousGeneration = -1L;
    }

    /**
     * Lee la ventana en vivo si hay audio nuevo.
     *
     * @param wanted cuántas barras caben en el ancho disponible
     * @return {@code true} cuando la vista tiene algo nuevo que pintar; {@code false}
     *         significa <em>no cambies un solo píxel</em>
     */
    boolean poll(int wanted) {
        if (reader == null || wanted <= 0) return false;
        int count = Math.min(wanted, MAX_BARS);
        polls++;
        long now = reader.read(values, count);
        generation = now;
        bars = count;
        if (now == previousGeneration) return false;
        previousGeneration = now;
        updates++;
        return true;
    }

    /**
     * Cuántas barras para un ancho dado.
     *
     * <p>Un número entero de pasos, para que la geometría nunca dependa de un píxel
     * fraccionario y la misma anchura produzca siempre la misma figura.
     */
    static int barsFor(int widthPx, int pitchPx) {
        if (pitchPx <= 0) return 0;
        int count = widthPx / pitchPx;
        if (count < 8) return widthPx > 0 ? 8 : 0;
        return Math.min(count, MAX_BARS);
    }
}