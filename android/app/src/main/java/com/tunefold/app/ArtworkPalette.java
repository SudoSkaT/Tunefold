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

        Palette(int primary, int secondary, boolean derived) {
            this.primary = primary;
            this.secondary = secondary;
            this.derived = derived;
        }

        /** La paleta de marca: lo que se ve cuando la portada no tiene color. */
        static Palette brand(int brand) { return new Palette(brand, brand, false); }
    }

    /** Resolución del matiz en el recuento de la portada. */
    private static final int HUE_BUCKETS = 12;
    /** Por debajo de esta saturación un píxel no tiene opinión sobre el color. */
    private static final float MIN_SATURATION = 0.40f;
    /** Por debajo de esta luminosidad no se le puede poner contenido encima. */
    private static final float MIN_VALUE = 0.28f;
    /** Por encima, el píxel es un brillo, no un color. */
    private static final float MAX_VALUE = 0.96f;
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
    private static final int BRAND = DesignTokens.Palette.ACCENT;

    private ArtworkPalette() { }

    /**
     * La paleta de una portada.
     *
     * @param argb     píxeles de la portada (típicamente una reducción 32×32);
     *                 {@code null} o vacío devuelve la paleta de marca
     * @return nunca nulo, y nunca un color ilegible
     */
    static Palette derive(int[] argb) {
        if (argb == null || argb.length == 0) return Palette.brand(BRAND);
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
        if (first < 0) return Palette.brand(BRAND);
        int second = best(counts, saturation, first);
        int primaryColor = averageOf(sums, counts, first);
        int secondaryColor = second >= 0 ? averageOf(sums, counts, second) : primaryColor;
        // Dos tonos casi idénticos no son dos canales: si los matices están a menos
        // de 25 grados, el segundo canal usa el mismo color y lo dice así.
        if (second >= 0 && hueDistance(hue[first] / counts[first], hue[second] / counts[second]) < 25f) {
            secondaryColor = primaryColor;
        }
        return new Palette(
                readableOnCard(vivid(primaryColor)),
                readableOnCard(vivid(secondaryColor)),
                true);
    }

    /**
     * Eleva la luminosidad de un color hasta que el contenido oscuro se lea encima.
     *
     * <p>Las barras usan el color tal cual (sólo muestran datos); el disco del
     * botón de reproducción lleva contenido oscuro encima y necesita esta garantía.
     */
    static int lift(int color) {
        if (luminance(color) >= MIN_ACCENT_LUMINANCE) return color;
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
        return best;
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
     * Sube el valor del color hasta que se lea sobre la tarjeta.
     *
     * <p>Se sube la luminosidad y no se mezcla con el blanco: mezclarse bajaría el
     * croma justo lo que {@link #vivid} acaba de recuperar, y el resultado sería
     * otra vez el rosa pálido que motivated todo esto.
     */
    static int readableOnCard(int color) {
        if (contrast(color, DesignTokens.Palette.SURFACE_CARD) >= MIN_BAR_CONTRAST) return color;
        float[] hsv = new float[3];
        float[] rgb = new float[3];
        toHsv(color, hsv);
        int alpha = DesignTokens.Palette.ACCENT & 0xFF000000;
        for (float value = hsv[2]; value <= 1f; value += 0.02f) {
            hsv[2] = value;
            fromHsv(hsv, rgb);
            int candidate = alpha | opaque(rgb[0]) << 16 | opaque(rgb[1]) << 8 | opaque(rgb[2]);
            if (contrast(candidate, DesignTokens.Palette.SURFACE_CARD) >= MIN_BAR_CONTRAST) {
                return candidate;
            }
        }
        // Ni el blanco llega al contraste pedido sobre una tarjeta clara: la marca
        // es el suelo conocido, y es un hecho que se puede defender.
        return DesignTokens.Palette.ACCENT;
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

    /** {@code pixel} → HSV, con matiz en grados y saturación/valor en 0..1. */
    static void toHsv(int pixel, float[] out) {
        float red = ((pixel >> 16) & 0xFF) / 255f;
        float green = ((pixel >> 8) & 0xFF) / 255f;
        float blue = (pixel & 0xFF) / 255f;
        float max = Math.max(red, Math.max(green, blue));
        float min = Math.min(red, Math.min(green, blue));
        float delta = max - min;
        float hue;
        if (delta <= 0f) {
            hue = 0f;
        } else if (max == red) {
            hue = 60f * (((green - blue) / delta) % 6f);
        } else if (max == green) {
            hue = 60f * (((blue - red) / delta) + 2f);
        } else {
            hue = 60f * (((red - green) / delta) + 4f);
        }
        out[0] = hue < 0f ? hue + 360f : hue;
        out[1] = max <= 0f ? 0f : delta / max;
        out[2] = max;
    }

    /** HSV → `rgb` en 0..255. */
    static void fromHsv(float[] hsv, float[] rgb) {
        float hue = ((hsv[0] % 360f) + 360f) % 360f;
        float saturation = Math.max(0f, Math.min(1f, hsv[1]));
        float value = Math.max(0f, Math.min(1f, hsv[2]));
        float chroma = value * saturation;
        float match = value - chroma;
        float sector = hue / 60f;
        float second = chroma * (1f - Math.abs(sector % 2f - 1f));
        switch ((int) sector) {
            case 0: rgb[0] = chroma + match; rgb[1] = second + match; rgb[2] = match; break;
            case 1: rgb[0] = second + match; rgb[1] = chroma + match; rgb[2] = match; break;
            case 2: rgb[0] = match; rgb[1] = chroma + match; rgb[2] = second + match; break;
            case 3: rgb[0] = match; rgb[1] = second + match; rgb[2] = chroma + match; break;
            case 4: rgb[0] = second + match; rgb[1] = match; rgb[2] = chroma + match; break;
            default: rgb[0] = chroma + match; rgb[1] = match; rgb[2] = second + match; break;
        }
        for (int channel = 0; channel < 3; channel++) {
            rgb[channel] = Math.max(0f, Math.min(255f, rgb[channel] * 255f));
        }
    }
}