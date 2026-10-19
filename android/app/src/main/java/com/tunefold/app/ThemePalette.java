package com.tunefold.app;

import java.util.ArrayList;
import java.util.List;

/**
 * De cinco colores a una interfaz entera.
 *
 * <p>Un tema llega como cinco colores sin nombre. Aquí se les decide un papel y de
 * ahí sale todo lo demás: las superficies, la tinta, el acento, los estados
 * funcionales. La ventaja es que no hay una tabla de roles por tema que mantener y
 * que se pueda equivocar: un tema nuevo son cinco líneas.
 *
 * <h3>Cómo se decide qué color es cuál</h3>
 * Por cómo se ve, no por su posición en la lista. Un tema puede venir con cuatro
 * colores claros y uno oscuro ({@link ThemeSpec#CHOIR_PASTE}) o con dos oscuros y
 * tres claros ({@link ThemeSpec#STAGE_SPOTLIGHT}), y los dos tienen que funcionar.
 *
 * <ol>
 *   <li><b>¿Campo claro u oscuro?</b> Por la segunda tinta más oscura: si incluso
 *       el segundo color más oscuro del tema es claro, el tema es de campo claro.
 *       Con cinco colores, un tema oscuro siempre tiene al menos dos.</li>
 *   <li><b>Campo</b> e <b>ink</b> son los dos extremos por luminosidad, en el orden
 *       que corresponde al campo.</li>
 *   <li><b>Acento</b>: entre los que quedan, el más vivo cuyo matiz no se separe
 *       del ink por menos de treinta grados. El acento tiene que poder separarse
 *       del texto, porque con el texto es con quien más se confunde.</li>
 *   <li><b>Tinta secundaria</b>: la tinta más tenue que todavía llega a 4,5:1
 *       sobre el campo. Si ninguna llega, se mezcla hacia el ink.</li>
 *   <li><b>Profundidad</b>: el color que sobra, que es el que va detrás de las
 *       portadas antes de que existan.</li>
 * </ol>
 *
 * <h3>Lo que sale de ahí</h3>
 * Las superficies se mezclan siempre hacia el ink, que es el otro extremo del
 * tema. Mezclar hacia un gris introduciría un color que nadie eligió, que es
 * justo lo que este trabajo se propone evitar.
 *
 * <h3>Estados funcionales</h3>
 * Error, correcto y aviso se toman por matiz —el color más rojo, el más verde y el
 * más ámbar que el tema ofrezca— y después se reparan contra el campo hasta que se
 * leen. La razón de no fijarlos es que fijarlos los volvería el único rincón de
 * la interfaz que no cambia de tema, y ese es exactamente el rincón donde un
 * estado mal reconocido acaba pareciendo «el tema es raro».
 *
 * <p>Todo aquí es aritmética pura sobre enteros: {@link #resolve} no toca Android
 * y la comprobación de los seis temas se hace en la JVM sin dispositivo.
 */
final class ThemePalette {

    /** Por debajo de esta luminosidad, el tema es de campo oscuro. */
    static final double DARK_FIELD_CEILING = 0.30d;

    /** Un acento que se parezca tanto al campo no es un acento: es otra superficie. */
    private static final double ACCENT_MIN_CONTRAST = 2.0d;

    /** Croma mínimo de un acento, para que oscurecerlo no lo convierta en gris. */
    private static final float ACCENT_MIN_CHROMA = 0.32f;

    /**
     * Dos colores están en el mismo barrio de luz cuando su luminosidad se
     * diferencia en menos de esto.
     *
     * <p>Por debajo de esta diferencia los dos se leen como el mismo tipo de cosa
     * sobre la misma superficie, y es sólo ahí donde se pueden confundir entre sí.
     */
    private static final double NEIGHBOURHOOD = 0.25d;

    /** Vueltas de reparación antes de darse por vencida; ver {@link #onAll}. */
    private static final int REPAIR_PASSES = 6;

    /**
     * Franja de luminosidad dentro de la cual dos colores se consideran empatados.
     *
     * <p>Del orden de magnitud del propio reloj de 8 bits: por debajo de esto dos
     * colores son indistinguibles en pantalla y elegir uno u otro por su redondeo
     * es arbitrario.
     */
    private static final double EXTREME_BAND = 0.02d;

    /** La tabla completa de roles de un tema. */
    static final class Resolved {
        int surfaceBase;
        int surfaceRaised;
        int surfaceCard;
        int surfaceCardActive;
        int surfaceOverlay;
        int surfaceArtwork;
        int surfaceArtworkEdge;
        int onSurface;
        int onSurfaceMuted;
        int onSurfaceFaint;
        int accent;
        int accentText;
        int onAccent;
        int accentPressed;
        int error;
        int positive;
        int warning;
        int stateWorking;
        int hairline;
        int ripple;
        /** Si el campo es oscuro; decide en qué dirección se mezclan las superficies. */
        boolean dark;
    }

    private ThemePalette() { }

    /**
     * Resuelve un tema en su tabla de roles.
     *
     * @param theme el tema; {@code null} devuelve el tema por defecto
     */
    static Resolved resolve(ThemeSpec theme) {
        ThemeSpec source = theme == null ? ThemeSpec.DEFAULT : theme;
        Resolved out = new Resolved();

        List<Integer> ordered = byLuminance(source);
        boolean dark = isDarkOf(source);
        out.dark = dark;
        // El campo es el extremo que coincide con el tema, y la tinta el otro:
        // en un tema oscuro el campo es el más oscuro, en uno claro el más claro.
        int field = extreme(source, dark, !dark);
        int ink = extreme(source, dark, dark);

        List<Integer> rest = without(ordered, ink, field);
        int accent = pickAccent(rest, ink, field);
        List<Integer> left = without(rest, accent);
        int muted = pickMuted(left, field, ink);

        // ---- superficies primero -------------------------------------------
        //
        // El orden importa y no es un detalle de estilo: las tintas se reparan
        // contra las superficies donde se van a poner, así que las superficies
        // tienen que existir ya. Repararlas contra un campo que todavía valía cero
        // —transparente— no es una reparación: es no reparar nada.
        out.surfaceBase = field;
        out.surfaceRaised = ColorMath.mix(field, ink, dark ? 0.06f : 0.05f);
        // En un tema oscuro la tarjeta es un pozo y lo pulsado sale del pozo hacia
        // la luz; en un tema claro la tarjeta es una placa y lo pulsado se hunde,
        // pero sin llegar a ser un pozo: una placa casi negra con la tinta del
        // tema encima deja a las dos en el mismo negro, que es un 1:1 exacto.
        out.surfaceCard = dark ? well(field, 0.62f) : ColorMath.mix(field, ink, 0.08f);
        out.surfaceCardActive = dark ? well(field, 0.48f)
                : ColorMath.mix(field, ink, 0.16f);
        out.surfaceOverlay = ColorMath.mix(field, ink, dark ? 0.18f : 0.16f);
        // El sitio detrás de una portada sale del campo y de la tinta, no del
        // color que haya sobrado: un rojo o un naranja sueltos acabarían detrás de
        // cada carátula y el texto de encima dejaría de leerse.
        out.surfaceArtwork = ColorMath.mix(field, ink, dark ? 0.10f : 0.05f);
        out.surfaceArtworkEdge = ColorMath.mix(field, ink, dark ? 0.03f : 0.02f);

        // ---- tintas -----------------------------------------------------------
        out.onSurface = ink;
        // El acento se repara dos veces: una para poder ser una forma y otra para
        // poder llevar palabras. Y se repara contra todas las superficies donde
        // puede aparecer, no sólo contra el campo: un acento que se lee sobre la
        // página y se pierde sobre la tarjeta del reproductor no es un acento, es
        // dos acentos.
        out.accent = onAll(ColorMath.resaturate(accent, ACCENT_MIN_CHROMA), 3.0d,
                field, out.surfaceRaised, out.surfaceCard, out.surfaceCardActive,
                out.surfaceOverlay, out.surfaceArtwork);
        out.accentText = onAll(ColorMath.resaturate(accent, ACCENT_MIN_CHROMA), 4.5d,
                field, out.surfaceRaised, out.surfaceCard, out.surfaceCardActive,
                out.surfaceOverlay);
        out.onAccent = readable(ColorMath.mix(field, ink, dark ? 0.10f : 0.90f),
                out.accent, 4.5d);
        out.accentPressed = ColorMath.mix(out.accent, ink, 0.25f);
        out.onSurfaceMuted = onAll(muted, 4.5d, field, out.surfaceRaised,
                out.surfaceCard, out.surfaceCardActive, out.surfaceOverlay);
        out.onSurfaceFaint = ColorMath.mix(out.onSurfaceMuted, field, 0.35f);

        // ---- estados ----------------------------------------------------------
        //
        // Son texto además de forma: el mensaje de error vive en la tarjeta del
        // reproductor, así que se reparan contra el campo y contra el pozo, no
        // sólo contra el fondo de la página.
        int[] pool = statePool(source, ink, field, accent);
        out.error = apart(onAll(state(pool, accent, 0f), 4.5d, field, out.surfaceRaised,
                out.surfaceCard, out.surfaceOverlay), accent, 0f);
        out.positive = apart(onAll(state(pool, accent, 120f), 4.5d, field,
                out.surfaceRaised, out.surfaceCard, out.surfaceOverlay), accent, 120f);
        out.warning = apart(onAll(state(pool, accent, 55f), 3.0d, field,
                out.surfaceRaised, out.surfaceCard, out.surfaceOverlay), accent, 55f);
        // Trabajando no es el acento: el acento significa «esto es sobre lo que se
        // actúa» y esperar no es actuar. Tampoco es la tinta en reposo, que es lo
        // que lleva «en pausa»: si los dos estados compartieran color, la única
        // diferencia entre «está buscando» y «está parado» sería la forma de un
        // punto de catorce dp, y eso no es una diferencia que se pueda leer.
        out.stateWorking = ColorMath.mix(out.onSurfaceMuted, out.accent, 0.4f);

        out.hairline = ColorMath.scaleAlpha(ink, 0.18f);
        out.ripple = ColorMath.scaleAlpha(ink, dark ? 0.12f : 0.14f);
        return out;
    }

    /** Si este tema tiene el campo oscuro. */
    static boolean isDark(ThemeSpec theme) {
        return isDarkOf(theme == null ? ThemeSpec.DEFAULT : theme);
    }

    // ------------------------------------------------------------- internals

    private static boolean isDarkOf(ThemeSpec source) {
        int dark = 0;
        for (int color : source.colors) {
            if (ColorMath.luminance(color) < DARK_FIELD_CEILING) dark++;
        }
        // Dos o más: un tema oscuro siempre trae al menos dos colores oscuros —
        // el campo y algo más. Un tema claro como Choir Paste trae cuatro claros y
        // uno solo oscuro, que es la tinta, no el campo.
        return dark >= 2;
    }

    /**
     * El extremo del tema: el más claro o el más oscuro.
     *
     * <p>Y cuando hay empate —varios colores con la misma luminosidad dentro de
     * una franja— gana <b>el que el usuario puso primero</b>. Sin esta regla el
     * campo de Choir Paste lo decidía una diferencia de una centésima entre el rosa
     * y el menta, y el tema se veía con un campo verde que su autor nunca eligió.
     * Un empate de luminosidad se resuelve con el orden, que es la única
     * información que el usuario dio aparte del color.
     */
    private static int extreme(ThemeSpec source, boolean dark, boolean wantLight) {
        double best = wantLight ? -1d : 2d;
        for (int color : source.colors) {
            double value = ColorMath.luminance(color);
            if (wantLight ? value > best : value < best) best = value;
        }
        for (int color : source.colors) {
            if (Math.abs(ColorMath.luminance(color) - best) <= EXTREME_BAND) return color;
        }
        return source.color(0);
    }

    /**
     * El pozo donde se dibujan las barras y la línea de posición.
     *
     * <p>Es el único papel que no sale de mezclar campo y tinta, y hay dos motivos.
     * El primero es el que se ve: necesita ser claramente distinto del campo para
     * que la tarjeta del reproductor se lea como una placa y no como el fondo. El
     * segundo es el que no se ve: tiene que estar lo bastante oscuro para que el
     * color de una funda llegue a 3:1 contra él, y un campo claro o elevado no lo
     * da ni de lejos.
     *
     * <p>Y aun así no puede llegar a negro puro, porque negro puro es el único
     * color que este trabajo no se permite: de ahí el seis por ciento de vuelta
     * hacia el campo. Un campo que ya es casi negro se queda a un pelo de negro,
     * que es negro con una opinión propia.
     */
    /**
     * El pozo, sólo para temas de campo oscuro.
     *
     * <p>La versión clara es <b>la tarjeta levantada</b>, no un pozo: sobre un
     * campo pálido una placa casi negra con la tinta del tema encima deja a la
     * tinta y a la tarjeta en el mismo negro, que es un 1:1 exacto. Y no hace
     * falta un pozo claro para las barras: contra una superficie clara un azul
     * profundo o un carmesí saturados llegan a 3:1 sin tocarlos, que es lo
     * contrario de lo que pasaba con el campo oscuro y por la misma razón.
     */
    private static int well(int field, float towardsBlack) {
        return ColorMath.mix(ColorMath.mix(field, 0xFF000000, towardsBlack), field, 0.06f);
    }

    private static List<Integer> byLuminance(ThemeSpec theme) {
        List<Integer> out = new ArrayList<>(5);
        for (int color : theme.colors) out.add(color);
        out.sort((left, right) -> Double.compare(
                ColorMath.luminance(left), ColorMath.luminance(right)));
        return out;
    }

    private static int secondDarkest(List<Integer> ordered) {
        return ordered.size() > 1 ? ordered.get(1) : ordered.get(0);
    }

    private static List<Integer> without(List<Integer> source, int... removed) {
        List<Integer> out = new ArrayList<>(source);
        for (int color : removed) out.remove(Integer.valueOf(color));
        return out;
    }

    /**
     * El acento: el más vivo, con dos castigos.
     *
     * <p>Uno por parecido con el campo: un color que apenas se distingue del fondo
     * es otra superficie, no un acento, por muy saturado que sea. Sin ese castigo
     * Indie Dusk elegía de acento su violeta de fondo, que es justo el color que
     * ya hace ese papel.
     *
     * <p>Y otro por parecido con la tinta, pero <b>sólo si están en el mismo
     * barrio de luz</b>. Un acento a menos de treinta grados del ink se confunde
     * con el texto cuando los dos son claros sobre un campo oscuro, o los dos
     * oscuros sobre un campo claro. En Choir Paste la tinta es casi negra y un
     * azul pálido no se parece a nada de lo que hay en pantalla, así que penalizarlo
     * por su tono sería empujar el acento hacia el único color del tema con más
     * parecido, que resultaba ser el verde.
     *
     * <p>Y si aun así no queda ninguno —un tema de cuatro pasteles casi iguales,
     * como Choir Paste, donde ningún color se separa del campo por contraste—, se
     * elige el más vivo y luego {@link #readable} lo lleva hasta donde se vea. Es
     * un acento de campo claro: sólido sobre un fondo pálido.
     */
    private static int pickAccent(List<Integer> candidates, int ink, int field) {
        int best = candidates.isEmpty() ? ink : candidates.get(0);
        float bestScore = -Float.MAX_VALUE;
        double inkLight = ColorMath.luminance(ink);
        for (int color : candidates) {
            boolean sameNeighbourhood =
                    Math.abs(ColorMath.luminance(color) - inkLight) < NEIGHBOURHOOD;
            boolean nearInk = sameNeighbourhood
                    && ColorMath.hueDistance(color, ColorMath.hue(ink)) < 30f;
            boolean nearField = ColorMath.contrast(color, field) < ACCENT_MIN_CONTRAST;
            float score = ColorMath.saturation(color)
                    - (nearInk ? 0.4f : 0f)
                    - (nearField ? 0.8f : 0f);
            if (score > bestScore) {
                bestScore = score;
                best = color;
            }
        }
        return best;
    }

    /**
     * La tinta secundaria: la más viva que ya se lee sobre el campo sin reparación.
     *
     * <p>Preferirla a una mezcla es lo que hace que un tema use los colores que
     * le dieron: elStage Spotlight trae un amarillo pálido y ese amarillo es su
     * texto secundario, no un tono inventado al 75% entre el fondo y el blanco.
     * Si ninguno de los que sobran llega a 4,5:1 —temas de madera y de granate, en
     * los que los colores claros son sólo el acento— se interpola, que es donde
     * cualquier tema tiene un valor legible.
     */
    private static int pickMuted(List<Integer> candidates, int field, int ink) {
        int best = ink;
        float bestSaturation = Float.MAX_VALUE;
        for (int color : candidates) {
            if (ColorMath.contrast(color, field) < 4.5d) continue;
            float saturation = ColorMath.saturation(color);
            if (saturation < bestSaturation) {
                bestSaturation = saturation;
                best = color;
            }
        }
        return bestSaturation == Float.MAX_VALUE
                ? ColorMath.mix(field, ink, 0.75f) : best;
    }

    /** Los colores que pueden dar un estado funcional: todo menos ink, campo y acento. */
    private static int[] statePool(ThemeSpec theme, int ink, int field, int accent) {
        List<Integer> pool = without(byLuminance(theme), ink, field, accent);
        int[] out = new int[pool.size()];
        for (int index = 0; index < out.length; index++) out[index] = pool.get(index);
        return out.length == 0 ? new int[] {accent} : out;
    }

    /**
     * Un estado funcional: el color del tema que más se acerca al matiz pedido.
     *
     * <p>Y si ninguno se acerca —Choir Paste no tiene ni un rojo ni un verde entre
     * sus cinco colores— se le pide al propio acento que mire para otro lado con
     * su misma saturación. Sigue siendo un color del tema, sólo que girado, y es
     * infinitamente preferible a devolver el acento y tener «descargado» y
     * «reproduciendo» pintados igual.
     */
    /**
     * Repara un color contra varias superficies a la vez.
     *
     * <p>Reparar en una sola pasada no vale, y no por falta de iteraciones sino
     * por el orden: subir el brillo para que se lea sobre una superficie clara
     * puede quitárselo de la oscura que venía después, y el resultado es un color
     * que cumple la última de la lista y falla la primera. Por eso seRepasa hasta
     * que ninguna superficie vuelva a pedir movimiento, con un tope de vueltas
     * para que un caso patológico no se quede girando.
     *
     * <p>La razón de que haga falta: una sola tinta tiene que servir sobre el
     * campo, sobre la tarjeta elevada, sobre el pozo del reproductor y sobre el
     * menú, y esos cuatro no se comparan igual.
     */
    private static int onAll(int color, double ratio, int... surfaces) {
        int out = color;
        for (int pass = 0; pass < REPAIR_PASSES; pass++) {
            boolean moved = false;
            for (int surface : surfaces) {
                int next = readable(out, surface, ratio);
                if (next != out) {
                    out = next;
                    moved = true;
                }
            }
            if (!moved) break;
        }
        return out;
    }

    /**
     * Un estado que no puede acabar siendo el acento.
     *
     * <p>La reparación por contraste puede llevar dos colores que empezaron
     * distintos al mismo sitio —dos Rayos sobre el mismo campo oscuro—, y entonces
     * «descargado» y «sonando» acaban pintados igual. Cuando eso ocurre se rehace
     * el color girándolo lejos del acento, que es lo que separa una señal de otra.
     */
    private static int apart(int color, int accent, float reference) {
        if (color != accent) return color;
        float moved = ColorMath.hueDistance(accent, reference) < 45f
                ? reference + 60f : reference;
        return ColorMath.withHue(accent, moved, 0.60f, 0.30f, 0.80f);
    }

    private static int state(int[] pool, int accent, float reference) {
        int best = accent;
        float bestDistance = 361f;
        for (int color : pool) {
            float distance = ColorMath.hueDistance(color, reference);
            if (distance < bestDistance) {
                bestDistance = distance;
                best = color;
            }
        }
        if (bestDistance > 60f) {
            best = ColorMath.withHue(accent, reference, 0.55f, 0.35f, 0.72f);
        }
        return best;
    }

    /**
     * Mueve un color hasta que llega a {@code ratio} contra {@code surface}.
     *
     * <p>Delegación pura: la misma reparación que usa la lectura de las barras,
     * y por el mismo motivo —subir el valor conserva el matiz y el croma, que es
     * lo que hace que la identidad de una funda sobreviva—. Dos copias de esta
     * función divergirían justo en el último decimal, que es donde se decide si un
     * texto se lee.
     */
    static int readable(int color, int surface, double ratio) {
        return ColorMath.readable(color, surface, ratio);
    }
}