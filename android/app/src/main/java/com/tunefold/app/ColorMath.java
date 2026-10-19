package com.tunefold.app;

/**
 * Mezcla y medida de color, sin estado y sin Android.
 *
 * <p>Existe como clase aparte, y no como un par de métodos en
 * {@link DesignTokens}, por una razón muy concreta: {@link ThemePalette} deriva la
 * paleta de cada tema y necesita mezclar colores, y si vivieran dentro de
 * {@code DesignTokens} los tokens de color se inicializarían llamándose entre sí
 * —la paleta llama a mezclar, mezclar vive en la clase que se está
 * inicializando—. Al sacarlas a un sitio sin estado, la dirección de la
 * dependencia es de una sola manera y se puede comprobar.
 */
final class ColorMath {

    private ColorMath() { }

    /** Reemplaza el alfa de un color, conservando sus canales. */
    static int scaleAlpha(int color, float alpha) {
        int scaled = Math.round(Math.max(0f, Math.min(1f, alpha))
                * ((color >>> 24) & 0xFF));
        return (color & 0x00FFFFFF) | (scaled << 24);
    }

    /** Interpolación lineal entre dos colores opacos. */
    static int mix(int from, int to, float ratio) {
        float t = Math.max(0f, Math.min(1f, ratio));
        int a = Math.round(((from >>> 24) & 0xFF) * (1f - t) + ((to >>> 24) & 0xFF) * t);
        int r = Math.round(((from >> 16) & 0xFF) * (1f - t) + ((to >> 16) & 0xFF) * t);
        int g = Math.round(((from >> 8) & 0xFF) * (1f - t) + ((to >> 8) & 0xFF) * t);
        int b = Math.round((from & 0xFF) * (1f - t) + (to & 0xFF) * t);
        return (a << 24) | (r << 16) | (g << 8) | b;
    }

    /** El canal de {@code color} en 0..255, linealizado según WCAG. */
    private static double channel(int color, int shift) {
        double value = ((color >> shift) & 0xFF) / 255d;
        return value <= 0.03928d ? value / 12.92d : Math.pow((value + 0.055d) / 1.055d, 2.4d);
    }

    /** Luminancia relativa WCAG de un color. */
    static double luminance(int color) {
        return 0.2126d * channel(color, 16) + 0.7152d * channel(color, 8)
                + 0.0722d * channel(color, 0);
    }

    /** Contraste WCAG entre dos colores. */
    static double contrast(int left, int right) {
        double a = luminance(left);
        double b = luminance(right);
        return (Math.max(a, b) + 0.05d) / (Math.min(a, b) + 0.05d);
    }

    /** Matiz de un color en grados 0..360, con 0 en el rojo. */
    static float hue(int color) {
        float red = ((color >> 16) & 0xFF) / 255f;
        float green = ((color >> 8) & 0xFF) / 255f;
        float blue = (color & 0xFF) / 255f;
        float max = Math.max(red, Math.max(green, blue));
        float min = Math.min(red, Math.min(green, blue));
        float delta = max - min;
        float value;
        if (delta <= 0f) {
            value = 0f;
        } else if (max == red) {
            value = 60f * (((green - blue) / delta) % 6f);
        } else if (max == green) {
            value = 60f * (((blue - red) / delta) + 2f);
        } else {
            value = 60f * (((red - green) / delta) + 4f);
        }
        return value < 0f ? value + 360f : value;
    }

    /** Distancia más corta entre dos matices, en grados. */
    static float hueDistance(float left, float right) {
        float difference = Math.abs(left - right);
        return difference > 180f ? 360f - difference : difference;
    }

    /** Distancia entre el matiz de un color y un matiz de referencia. */
    static float hueDistance(int color, float reference) {
        return hueDistance(hue(color), reference);
    }

    /** Saturación de un color en 0..1. */
    static float saturation(int color) {
        float red = ((color >> 16) & 0xFF) / 255f;
        float green = ((color >> 8) & 0xFF) / 255f;
        float blue = (color & 0xFF) / 255f;
        float max = Math.max(red, Math.max(green, blue));
        float min = Math.min(red, Math.min(green, blue));
        return max <= 0f ? 0f : (max - min) / max;
    }

    /** {@code pixel} → HSV, con matiz en grados y saturación/valor en 0..1. */
    static void toHsv(int pixel, float[] out) {
        float red = ((pixel >> 16) & 0xFF) / 255f;
        float green = ((pixel >> 8) & 0xFF) / 255f;
        float blue = (pixel & 0xFF) / 255f;
        float max = Math.max(red, Math.max(green, blue));
        float min = Math.min(red, Math.min(green, blue));
        float delta = max - min;
        float value;
        if (delta <= 0f) {
            value = 0f;
        } else if (max == red) {
            value = 60f * (((green - blue) / delta) % 6f);
        } else if (max == green) {
            value = 60f * (((blue - red) / delta) + 2f);
        } else {
            value = 60f * (((red - green) / delta) + 4f);
        }
        out[0] = value < 0f ? value + 360f : value;
        out[1] = max <= 0f ? 0f : delta / max;
        out[2] = max;
    }

    /** HSV → `rgb` en 0..255, con matiz en grados. */
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

    /**
     * El mismo color con otro matiz.
     *
     * <p>Para fabricar un estado que un tema no trae: se le pide al propio tema un
     * tono y sólo se le cambia por dónde mira. El color resultante sigue siendo del
     * tema, que es lo que evita que un estado sea la única cosa ajena a él.
     */
    static int withHue(int color, float targetHue, float saturationFloor,
            float valueFloor, float valueCeiling) {
        float[] hsv = new float[3];
        float[] rgb = new float[3];
        toHsv(color, hsv);
        hsv[0] = targetHue;
        hsv[1] = Math.max(saturationFloor, Math.min(0.85f, hsv[1] * 1.1f));
        // El techo importa: un croma al valor 1 es un foco de luz, no un color, y un
        // estado de error que parece un foco de luz no se lee como un error.
        hsv[2] = Math.max(valueFloor, Math.min(valueCeiling, hsv[2]));
        fromHsv(hsv, rgb);
        return 0xFF000000 | (rounded(rgb[0]) << 16) | (rounded(rgb[1]) << 8)
                | rounded(rgb[2]);
    }

    /**
     * Mueve un color hasta que llega a {@code ratio} contra {@code surface}.
     *
     * <p><b>Por el valor, no mezclando con blanco ni con negro.</b> Es la misma
     * razón por la que la extracción de la funda sube el valor en vez de
     * aclararlo: mezclar con blanco baja el croma y un acento que se pone gris al
     * oscurecerse deja de ser un acento y pasa a ser una sombra. Moviendo el valor
     * en HSV, el matiz y la saturación se quedan donde estaban.
     *
     * <p><b>Y hay dos caminos, y no siempre vale el evidente.</b> Un color apenas
     * más claro que la superficie tiene un contraste bajísimo con ella, y aclararlo
     * más lo empeora: para llegar a 3:1 hay que pasarlo al otro lado y oscurecerlo.
     * Elegir la dirección por «¿el color es más claro que la superficie?» acierta en
     * el caso fácil y falla en el difícil, y el caso difícil es justo donde está un
     * tema de campo claro con acento pálido. Se calculan las dos luminancias de
     * destino, se descartan las imposibles y se va por la más cercana.
     *
     * <p>Si ni subiendo ni bajando el valor se llega —un tema con un campo y un
     * acento casi idénticos— se cae a mezclar con los extremos, que sí llega a
     * cualquier parte aunque pierda el tono.
     */
    static int readable(int color, int surface, double ratio) {
        if (contrast(color, surface) >= ratio) return color;
        double field = luminance(surface) + 0.05d;
        double current = luminance(color);
        double onLightSide = ratio * field - 0.05d;
        double onDarkSide = field / ratio - 0.05d;
        boolean canLighten = onLightSide <= 1d;
        boolean canDarken = onDarkSide >= 0d;
        boolean lighten;
        if (canLighten && !canDarken) {
            lighten = true;
        } else if (!canLighten && canDarken) {
            lighten = false;
        } else if (canLighten) {
            lighten = Math.abs(onLightSide - current) <= Math.abs(onDarkSide - current);
        } else {
            // Ningún lado llega: se va hacia el que ya se aleja del campo.
            lighten = current < luminance(surface);
        }
        float[] hsv = new float[3];
        float[] rgb = new float[3];
        toHsv(color, hsv);
        float start = hsv[2];
        for (int step = 2; step <= 100; step += 2) {
            hsv[2] = lighten
                    ? start + (1f - start) * step / 100f
                    : start * (1f - step / 100f);
            fromHsv(hsv, rgb);
            int candidate = 0xFF000000 | (rounded(rgb[0]) << 16) | (rounded(rgb[1]) << 8)
                    | rounded(rgb[2]);
            if (contrast(candidate, surface) >= ratio) return candidate;
        }
        // El valor no bastaba: mezclar con los extremos sí llega, aunque el tono
        // se pierda. Un color legible sin tono es mejor que un color con el tono
        // que no se lee.
        int extreme = lighten ? 0xFFFFFFFF : 0xFF000000;
        int last = color;
        for (int step = 2; step <= 100; step += 2) {
            int candidate = mix(color, extreme, step / 100f);
            if (contrast(candidate, surface) >= ratio) return candidate;
            last = candidate;
        }
        return last;
    }

    /**
     * El mismo color, con al menos el croma pedido.
     *
     * <p>Para cuando un acento llega demasiado apagado para cumplir su parte. Un
     * tema de pasteles sobre fondo claro es el caso: el color elegido tiene un
     * croma del quince por ciento, y oscurecerlo para que llegue a 3:1 sobre un
     * fondo pálido lo convierte en un gris. Un acento que tiene que gritar un poco
     * para verse es exactamente el motivo por el que existe el acento.
     */
    static int resaturate(int color, float floor) {
        float[] hsv = new float[3];
        float[] rgb = new float[3];
        toHsv(color, hsv);
        if (hsv[1] >= floor) return color;
        hsv[1] = floor;
        fromHsv(hsv, rgb);
        return 0xFF000000 | (rounded(rgb[0]) << 16) | (rounded(rgb[1]) << 8)
                | rounded(rgb[2]);
    }

    private static int rounded(float value) {
        int channel = Math.round(value);
        return channel < 0 ? 0 : channel > 255 ? 255 : channel;
    }
}
