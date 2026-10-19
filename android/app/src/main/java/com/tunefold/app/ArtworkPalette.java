package com.tunefold.app;

/**
 * Los colores que salen de la portada, y la regla de cuándo se usan.
 *
 * <p>La cadena del producto es ARTWORK → COLOUR → WAVEFORM → PLAYBACK, y este
 * es el eslabón que la cierra. La portada es el objeto más ruidoso de la
 * pantalla, así que el acento se toma de ella en lugar de ser una constante que
 * casualmente convive al lado.
 *
 * <h3>Por qué dos colores y no uno</h3>
 * Las barras son <b>estéreo</b>: canal izquierdo arriba, canal derecho abajo. Con
 * un solo color los dos canales se leen como el mismo dibujo repetido, que es
 * justo lo que una visualization estéreo no puede ser. Así que se derivan dos
 * tonos distintos de la portada —el que más manda y el segundo más cercano en
 * matiz— y si la portada sólo tiene un tono, se usa ese para los dos.
 *
 * <h3>Por qué no se mezcla con el color de marca</h3>
 * Una primera versión teñía el color derivado un 55% hacia el verde de marca
 * para "no salir de la familia". En un dispositivo real eso convirtió tres
 * portadas distintas en tres rosas grises casi iguales: la extracción funcionaba
 * y no se notaba. Aquí el color de la portada se usa tal cual, con la saturación
 * reforzada después del promedio (promediar colores del mismo tono apaga el
 * croma) y un mínimo de luminosidad para que las barras se lean sobre la tarjeta.
 *
 * <p>Lógica pura de enteros y flotantes: qué píxeles pueden ser color, cuál gana
 * y con qué garantías es un test, no una discusión sobre una captura.
 */
final class ArtworkPalette {

    /** Dos colores derivados y la marca de si de verdad salieron de la portada. */
    static final class Palette {
        /** Tono dominante, saturado y con luminosidad legible. */
        final int primary;
        /** Segundo tono: distinto matiz si la portada lo tiene, si no el primario. */
        final int secondary;
        /** {@code false} cuando la portada no ofreció nada y manda la marca. */
        final boolean derived;
        /**
         * Si la portada ofreció de verdad <em>dos</em> tonos distintos.
         *
         * <p>No es lo mismo que "derivada": una portada monocroma sí da color, y con
         * ese color se tiñen las barras, pero los dos canales son el mismo dibujo y
         * una composición que se apoya en el contraste entre dos zonas no tiene nada
         * que enseñar. La decisión de componer en dos regiones se toma sobre esto.
         */
        final boolean duoTone;

        Palette(int primary, int secondary, boolean derived) {
            this(primary, secondary, derived, primary != secondary);
        }

        Palette(int primary, int secondary, boolean derived, boolean duoTone) {
            this.primary = primary;
            this.secondary = secondary;
            this.derived = derived;
            this.duoTone = duoTone;
        }

        /** La paleta de marca: lo que se ve cuando la portada no tiene color. */
        static Palette brand(int brand) { return new Palette(brand, brand, false, false); }

        /** Si esta portada ofrece más de un color. */
        boolean offersMoreThanOneColour() { return derived && duoTone; }
    }

    /** Resolución del matiz en el recuento de la portada. */
    private static final int HUE_BUCKETS = 12;
    /** Por debajo de esta saturación un píxel no tiene opinión sobre el color. */
    private static final float MIN_SATURATION = 0.40f;
    /** Por debajo de esta luminosidad no se le puede poner contenido encima. */
    private static final float MIN_VALUE = 0.28f;
    /**
     * El techo de luminosidad: 1.0, o sea ninguno.
     *
     * <p>Antes era 0.96 y parecía una regla de sentido común —"por encima de esto
     * el píxel es un brillo, no un color"—, pero lo que hace que un píxel sea un
     * brillo es que sea <b>poco saturado</b>, no que sea claro: el blanco y el gris
     * claro ya se caen solos por {@link #MIN_SATURATION}. Con el techo en 0.96 una
     * naranja o un amarillo puros (valor 1.0) se descartaban píxel a píxel, la
     * extracción se quedaba sin nada que promediar y devolvía la marca: tres
     * fundas de color devolvían el mismo oro. El brillo de verdad —el blanco, el
     * halo de una foto— no se cuela por aquí, sino por la saturación.
     */
    private static final float MAX_VALUE = 1.0f;
    /** Saturación mínima del color final: el promedio la apaga y hay que devolverla. */
    private static final float MIN_RESULT_SATURATION = 0.45f;
    /**
     * Contraste mínimo contra la tarjeta donde se dibujan las barras.
     *
     * <p>Una sola garantía de luminosidad no basta para los dos usos: un morado
     * profundo vive justo por encima del suelo del botón y aun así se perdía sobre
     * la tarjeta. Las barras son formas grandes y delgadas sobre un fondo claro, así
     * que se les pide el contraste de un texto grande, no el de una etiqueta mínima.
     */
    private static final double MIN_BAR_CONTRAST = 3.0d;
    /**
     * The accent of the theme in force, read at the moment it is asked for.
     *
     * <p>It used to be a {@code static final} captured at class-initialisation time,
     * which quietly made every theme fall back to the first one's accent whenever a
     * cover could not be made legible. A theme that cannot supply its own fallback
     * is not really a theme.
     */
    private static int brand() {
        return DesignTokens.Palette.ACCENT;
    }

    private ArtworkPalette() { }

    /**
     * La paleta de una portada.
     *
     * @param argb     píxeles de la portada (típicamente una reducción 32×32);
     *                 {@code null} o vacío devuelve la paleta de marca
     * @return nunca nulo, y nunca un color ilegible
     */
    static Palette derive(int[] argb) {
        if (argb == null || argb.length == 0) return Palette.brand(brand());
        float[] sums = new float[HUE_BUCKETS * 3];
        int[] counts = new int[HUE_BUCKETS];
        float[] saturation = new float[HUE_BUCKETS];
        float[] hue = new float[HUE_BUCKETS];
        float[] hsv = new float[3];
        for (int pixel : argb) {
            if ((pixel >>> 24) < 0x40) continue;
            toHsv(pixel, hsv);
            if (hsv[1] < MIN_SATURATION || hsv[2] < MIN_VALUE || hsv[2] > MAX_VALUE) continue;
            int bucket = (int) (hsv[0] * HUE_BUCKETS / 60f) % HUE_BUCKETS;
            sums[bucket * 3] += (pixel >> 16) & 0xFF;
            sums[bucket * 3 + 1] += (pixel >> 8) & 0xFF;
            sums[bucket * 3 + 2] += pixel & 0xFF;
            counts[bucket]++;
            saturation[bucket] += hsv[1];
            hue[bucket] += hsv[0];
        }
        int first = best(counts, saturation, -1);
        if (first < 0) return Palette.brand(brand());
        int second = best(counts, saturation, first);
        int primaryColor = averageOf(sums, counts, first);
        int secondaryColor = second >= 0 ? averageOf(sums, counts, second) : primaryColor;
        // Dos tonos casi idénticos no son dos canales: si los matices están a menos
        // de 25 grados, el segundo canal usa el mismo color y lo dice así. Y si
        // la portada no tenía un segundo tono, tampoco lo hay.
        boolean duoTone = second >= 0;
        if (second >= 0 && hueDistance(hue[first] / counts[first], hue[second] / counts[second]) < 25f) {
            secondaryColor = primaryColor;
            duoTone = false;
        }
        return new Palette(
                readableOnCard(vivid(primaryColor)),
                readableOnCard(vivid(secondaryColor)),
                true,
                duoTone);
    }

    /**
     * Eleva la luminosidad de un color hasta que el contenido oscuro se lea encima.
     *
     * <p>Las barras usan el color tal cual (sólo muestran datos); el disco del
     * botón de reproducción lleva contenido oscuro encima y necesita esta garantía.
     */
    static int lift(int color) {
        int byFloor = color;
        if (luminance(byFloor) < MIN_ACCENT_LUMINANCE) {
            int low = 0;
            int high = 1_000;
            int best = 0xFFFFFFFF;
            while (low <= high) {
                int middle = (low + high) / 2;
                int candidate = DesignTokens.mix(color, 0xFFFFFFFF, middle / 1000f);
                if (luminance(candidate) >= MIN_ACCENT_LUMINANCE) {
                    best = candidate;
                    high = middle - 1;
                } else {
                    low = middle + 1;
                }
            }
            byFloor = best;
        }
        // El suelo de luminosidad no basta por sí solo: lo que tiene que leerse es
        // el glifo que va ENCIMA del disco, y ese depende del tema. Un disco que
        // cumple el suelo y no el contraste es un disco con el glifo invisible, que
        // es el peor estado posible en el control que más se pulsa.
        return ColorMath.readable(byFloor, DesignTokens.Palette.ON_ACCENT, 4.5d);
    }

    /**
     * Luminosidad mínima del acento para los controles.
     *
     * <p>La misma que fijaba la primera versión de la extracción, y por el mismo
     * motivo: con este suelo el color sigue sosteniendo 4.5:1 contra el contenido
     * del disco y 3:1 contra la tarjeta. Lo que cambió no fue la garantía, fue que
     * ya no se cumplía diluyendo el color hacia el verde de marca.
     */
    static final double MIN_ACCENT_LUMINANCE = 0.26d;

    /** Luminancia relativa WCAG de un color opaco. */
    static double luminance(int color) {
        return 0.2126d * channel(color, 16) + 0.7152d * channel(color, 8)
                + 0.0722d * channel(color, 0);
    }

    /** Contraste WCAG entre dos colores opacos. */
    static double contrast(int left, int right) {
        double a = luminance(left);
        double b = luminance(right);
        double lighter = Math.max(a, b);
        double darker = Math.min(a, b);
        return (lighter + 0.05d) / (darker + 0.05d);
    }

    private static double channel(int color, int shift) {
        double value = ((color >> shift) & 0xFF) / 255d;
        return value <= 0.03928d ? value / 12.92d : Math.pow((value + 0.055d) / 1.055d, 2.4d);
    }

    /**
     * Deja el color de una funda legible sobre la tarjeta donde se dibujan las
     * barras.
     *
     * <p>Delega en {@link ColorMath#readable}, que mueve el <b>valor</b> en vez de
     * mezclar con blanco: mezclar con blanco baja el croma justo lo que
     * {@link #vivid} acaba de recuperar, y el resultado sería otra vez el rosa
     * pálido que motivó todo esto.
     *
     * <p>Y hay un caso que el movimiento de valor sólo no resuelve. Un azul
     * profundo o un carmesí muy saturados no ganan luminosidad subiendo el valor —
     *al 100% de saturación el canal del medio sigue en el suelo—, así que no hay
     *ningún valor que los lleve a 3:1 contra la tarjeta. Antes de que existiera
     * este recorrido, esa combinación caía al final de la función y devolvía el
     * acento de marca: tres fundas azules, carmesí y púrpuras salían las tres
     * doradas, que es exactamente el defecto que estas pruebas existen para
     * cazar.
     *
     * <p>La salida es subir el croma del color original hasta
     * {@link #MIN_RESULT_SATURATION} y volver a intentarlo: el matiz se conserva
     * entero y el color deja de ser un gris. El suelo garantiza que siga siendo un
     * color y no una sombra.
     */
    static int readableOnCard(int color) {
        int repaired = ColorMath.readable(color, DesignTokens.Palette.SURFACE_CARD,
                MIN_BAR_CONTRAST);
        if (repaired == color) return color;
        float[] hsv = new float[3];
        toHsv(color, hsv);
        toHsv(repaired, hsv);
        // Alcanzó el contraste moviendo el valor, que conserva matiz y croma: la
        // funda sigue siendo la funda y no hay nada que arreglar.
        if (hsv[1] >= MIN_RESULT_SATURATION - 0.01f) return repaired;
        return ColorMath.readable(
                ColorMath.resaturate(color, MIN_RESULT_SATURATION),
                DesignTokens.Palette.SURFACE_CARD, MIN_BAR_CONTRAST);
    }

    /** Devuelve al croma que el promedio se llevó. */
    static int vivid(int color) {
        float[] hsv = new float[3];
        float[] rgb = new float[3];
        toHsv(color, hsv);
        hsv[1] = Math.max(MIN_RESULT_SATURATION, Math.min(0.92f, hsv[1] * 1.15f));
        hsv[2] = Math.max(hsv[2], 0.30f);
        fromHsv(hsv, rgb);
        return DesignTokens.Palette.ACCENT & 0xFF000000
                | opaque(rgb[0]) << 16 | opaque(rgb[1]) << 8 | opaque(rgb[2]);
    }

    private static int best(int[] counts, float[] saturation, int avoid) {
        int winner = -1;
        float best = 0f;
        for (int bucket = 0; bucket < HUE_BUCKETS; bucket++) {
            if (bucket == avoid || counts[bucket] == 0) continue;
            // Cuánto pesa el tono en la portada y cuán vivo es: dos portadas pueden
            // repartir sus píxeles por igual y merecer colores distintos.
            float score = counts[bucket] * (0.5f + saturation[bucket] / counts[bucket]);
            if (winner < 0 || score > best) {
                winner = bucket;
                best = score;
            }
        }
        return winner;
    }

    private static int averageOf(float[] sums, int[] counts, int bucket) {
        float count = counts[bucket];
        return opaque(sums[bucket * 3] / count) << 16
                | opaque(sums[bucket * 3 + 1] / count) << 8
                | opaque(sums[bucket * 3 + 2] / count) | 0xFF000000;
    }

    private static float hueDistance(float left, float right) {
        float difference = Math.abs(left - right);
        return difference > 180f ? 360f - difference : difference;
    }

    private static int opaque(float value) {
        int channel = Math.round(value);
        return channel < 0 ? 0 : channel > 255 ? 255 : channel;
    }

    /**
     * {@code pixel} → HSV, con matiz en grados y saturación/valor en 0..1.
     *
     * <p>Delega en {@link ColorMath} para que la conversión exista una sola vez: la
     * resolución de temas la necesita también, y dos copias de un HSV divergen
     * justo en el último decimal.
     */
    static void toHsv(int pixel, float[] out) {
        ColorMath.toHsv(pixel, out);
    }

    /** HSV → `rgb` en 0..255. */
    static void fromHsv(float[] hsv, float[] rgb) {
        ColorMath.fromHsv(hsv, rgb);
    }
}
