package com.tunefold.app;

import android.content.Context;
import android.content.res.Resources;

/**
 * The one place a Tunefold dimension, duration or colour may come from.
 *
 * <p>Every size in the UI is a token here. Nothing in a View may invent a number:
 * a value that appears twice must be a constant, because a constant is the only
 * thing that can be changed once and stay consistent. Tokens are declared in
 * {@code dp}/{@code sp} and converted once against the current display metrics.
 *
 * <p>The raw constants are deliberately free of Android types so the scale itself
 * is verifiable in a JVM unit test ({@code DesignTokensTest}); only
 * {@link #init(Context)} and the converters touch the framework.
 *
 * <h3>Why the chrome is olive, linen and gold</h3>
 * The product identity is ARTWORK &rarr; COLOUR &rarr; WAVEFORM &rarr; PLAYBACK, and
 * the surface the cover sits on is the first half of that statement: a cover
 * photographed against neutral chrome reads as a thumbnail in a file manager.
 * So the five brand materials carry the chrome themselves — a deep olive field, a
 * wood-toned place behind the cover, linen for anything that is read, and gold
 * reserved for what the user acts on. The field is dark so the cover still wins,
 * and it is olive rather than black so it never reads as "no background".
 *
 * <p>Every surface step and every ink is derived from those five and from nothing
 * else. No neutral grey, no pure white, no pure black: a ramp built by mixing the
 * brand hues keeps the relationship between the colours stable when the app is
 * dimmed, while a neutral ramp fights the cover for attention at every level.
 * {@link Palette} documents the derivation of each role, and
 * {@code BrandPaletteTest} refuses to let one of them drift out of contrast.
 *
 * <p>{@link ArtworkTheme} is the seam that replaces the fixed gold with one
 * derived from the current cover; the brand decides what the app is, the cover
 * decides what this song feels like, and neither is allowed to make the other
 * illegible.
 */
final class DesignTokens {

    // ------------------------------------------------------------- conversion

    /** Multiplier for {@code dp}; re-read on configuration change. */
    private static volatile float density = 1f;
    /** Multiplier for {@code sp}; follows the user's font scale. */
    private static volatile float scaledDensity = 1f;

    private DesignTokens() { }

    /**
     * Binds the scale to a display configuration.
     *
     * <p>Idempotent and cheap, so it is safe to call from {@code onCreate} and
     * again from {@code onConfigurationChanged}: font scale and density can both
     * change while the process is alive.
     */
    static void init(Context context) {
        if (context == null) return;
        Resources resources = context.getResources();
        if (resources == null) return;
        density = resources.getDisplayMetrics().density;
        scaledDensity = resources.getDisplayMetrics().scaledDensity;
    }

    /** Raw {@code dp} value as pixels. */
    static int dp(float value) { return Math.round(value * density); }

    /** Raw {@code sp} value as pixels. */
    static int sp(float value) { return Math.round(value * scaledDensity); }

    /**
     * Converts a dp length to pixels without a {@link Context}.
     *
     * <p>For static geometry only; interactive sizes must use {@link #dp(float)},
     * which needs no context but does need {@link #init(Context)} to have run.
     */
    static int px(float value) { return dp(value); }

    // ---------------------------------------------------------------- spacing

    /**
     * The spacing scale, in {@code dp}.
     *
     * <p>A 2dp base grid. Only these nine steps exist; anything else is a token
     * below built from them.
     */
    static final float SPACE_1 = 2f;
    static final float SPACE_2 = 4f;
    static final float SPACE_3 = 6f;
    static final float SPACE_4 = 8f;
    static final float SPACE_5 = 12f;
    static final float SPACE_6 = 16f;
    static final float SPACE_7 = 20f;
    static final float SPACE_8 = 24f;
    static final float SPACE_9 = 32f;

    /** Smallest gap between two controls in the same row. */
    static final float GAP_TIGHT = SPACE_3;
    /** Normal gap between siblings inside a block. */
    static final float GAP = SPACE_4;
    /** Gap between two blocks of the same surface. */
    static final float BLOCK_GAP = SPACE_5;
    /** Gap between major regions of a page. */
    static final float SECTION_GAP = SPACE_6;
    /** Gap between the top bar and the first content block. */
    static final float REGION_GAP = SPACE_7;

    // ------------------------------------------------------- content padding

    /** Horizontal page padding. */
    static final float CONTENT_PADDING_HORIZONTAL = SPACE_7;
    /** Page padding above the first block. */
    static final float CONTENT_PADDING_TOP = SPACE_4;
    /** Page padding below the last block. */
    static final float CONTENT_PADDING_BOTTOM = SPACE_9;

    /** Inner padding of the now-playing card. */
    static final float CARD_PADDING = SPACE_6;

    /** Inner padding of a list row. */
    static final float ROW_PADDING_HORIZONTAL = SPACE_3;
    /** Gap between a row's artwork and its text. */
    static final float ROW_GAP = SPACE_4;

    // ---------------------------------------------------------- corner radius

    /** Radius for chips and small surfaces. */
    static final float RADIUS_SMALL = SPACE_4;
    /** Radius for cards and the search field. */
    static final float RADIUS_MEDIUM = 14f;
    /** Radius for the artwork and the primary control. */
    static final float RADIUS_LARGE = 22f;
    /** Radius for pills (chips, badges). */
    static final float RADIUS_PILL = 999f;
    /**
     * Radius of the small artwork in a list row.
     *
     * <p>Larger than a launcher icon's and much smaller than the player's: a row
     * cover is a printed thumbnail, and rounding it to 4dp made it read as an app
     * icon sitting next to the text rather than as the sleeve of a record.
     */
    static final float RADIUS_ARTWORK_THUMB = 10f;

    // ------------------------------------------------------------- ink rules

    /**
     * Thickness of a printed rule, in {@code dp}.
     *
     * <p>One value for every hairline that is meant to be seen: the masthead's
     * underline, the heading's rule, the selected tab's bar and the selected
     * navigation's marker. A rule thinner than this disappears against the cover
     * it often sits on; thicker and it stops being a rule and starts being an edge.
     */
    static final float RULE_THICKNESS = 2f;
    /** Width of a selected-destination marker in the navigation, in {@code dp}. */
    static final float NAV_INDICATOR_WIDTH = 18f;

    // --------------------------------------------------------------- elevation

    /** Card elevation, in {@code dp}. */
    static final float ELEVATION_CARD = 6f;
    /** Raised surface elevation (top bar, sheet), in {@code dp}. */
    static final float ELEVATION_RAISED = 10f;
    /** Elevation of a transient overlay, in {@code dp}. */
    static final float ELEVATION_OVERLAY = 18f;

    // ------------------------------------------------------------ iconography

    /** Authoring grid every icon is drawn on. Never rendered. */
    static final float ICON_GRID = 24f;
    /**
     * Stroke weight of the utility icon family, in grid units.
     *
     * <p>One value for the whole family: a mixed-weight icon set is what makes an
     * app look assembled rather than designed.
     */
    static final float ICON_STROKE = 2f;
    /** Smallest icon, for inline affordances. */
    static final float ICON_SIZE_EXTRA_SMALL = 16f;
    /** Icon inside a list row or a navigation slot. */
    static final float ICON_SIZE_SMALL = 20f;
    /** Default icon size. */
    static final float ICON_SIZE_MEDIUM = 24f;
    /** Icon of a primary control. */
    static final float ICON_SIZE_LARGE = 32f;
    /** Icon of an oversized control; the brand mark. */
    static final float ICON_SIZE_EXTRA_LARGE = 40f;

    // ------------------------------------------------------------ touch target

    /**
     * Smallest interactive height, in {@code dp}.
     *
     * <p>Never go below this. It is a hard accessibility floor, not a style
     * choice, and {@code DesignTokensTest} fails the build if any control
     * dimension drops under it.
     */
    static final float TOUCH_TARGET_MIN = 48f;
    /** Default square target for an icon-only action. */
    static final float CONTROL_TARGET = TOUCH_TARGET_MIN;
    /** Target of a secondary action in a row. */
    static final float CONTROL_TARGET_SECONDARY = TOUCH_TARGET_MIN;
    /** Target of the primary playback control. */
    static final float CONTROL_TARGET_PRIMARY = 64f;
    /** Height of the navigation bar. */
    static final float CONTROL_TARGET_NAVIGATION = 56f;
    /**
     * Height of one track row.
     *
     * <p>Taller than the 48&nbsp;dp floor because the row carries three lines of
     * content — title, attribution and a status word — and squeezing them into the
     * floor is what made the status unreadable at large font scales.
     */
    static final float ROW_HEIGHT = 68f;
    /** Height of the search field. */
    static final float SEARCH_FIELD_HEIGHT = 52f;
    /**
     * Height of the brand / diagnostics row.
     *
     * <p>Not below {@link #TOUCH_TARGET_MIN}: the row's only control is a 48dp target,
     * so a shorter bar would only clip it.
     */
    static final float TOP_BAR_HEIGHT = TOUCH_TARGET_MIN;

    /** Width reserved for each timestamp beside the progress bar, in {@code dp}. */
    static final float TIME_LABEL_WIDTH = 52f;
    /**
     * Width reserved for a track's duration at the end of its row, in {@code dp}.
     *
     * <p>A fixed box, so a row's actions do not move when one track's duration is a
     * different number of characters than another's.
     */
    static final float ROW_DURATION_WIDTH = 44f;
    /** Target of the row's overflow button, in {@code dp}. */
    static final float CONTROL_TARGET_OVERFLOW = 40f;
    /**
     * Height of a row's status line and duration box, in {@code dp}.
     *
     * <p>Below the caption size so a row with a status and a row without one are the
     * same height: rows are recycled, and a list that changes its own row height while
     * scrolling is a list that scrolls wrongly.
     */
    static final float ROW_STATUS_HEIGHT = 20f;

    // ------------------------------------------------------- bottom navigation

    /**
     * Height of the bottom navigation bar, in {@code dp}.
     *
     * <p>The accessibility floor ({@link #TOUCH_TARGET_MIN}) plus the caption, and no
     * more: it is permanently on screen, so every dp here is a dp the content does
     * not get. The bar sits at the bottom because the thumb is there and because a
     * navigation strip that has to be scrolled to is not navigation.
     */
    static final float BOTTOM_NAV_HEIGHT = 60f;
    /** Thickness of the rule that separates the bar from the content, in {@code dp}. */
    static final float BOTTOM_NAV_RULE = SPACE_1;
    /** Background behind the bar, so content scrolling under it cannot show through. */
    /**
     * The surface under the navigation bar, read at the moment it is asked for.
     *
     * <p>It used to be a {@code static final} bound to {@link Palette#SURFACE_BASE},
     * which looks equivalent and is not: a constant freezes whatever the palette
     * held when the class was initialised, and the palette is installed at runtime
     * from the chosen theme. The bottom bar therefore kept the first theme's field
     * under all six, so a pale theme got a pale page sitting on a black bar with
     * the nav labels unreadable against it.
     */
    static int bottomNavSurface() {
        return Palette.SURFACE_BASE;
    }
    /** Caption size under each destination glyph, in {@code sp}. */
    static final float NAV_CAPTION_SIZE = 11f;
    /**
     * Smallest a destination caption may shrink to, in {@code sp}.
     *
     * <p>Four destinations across 360dp is ninety each, and the longest word in the
     * set does not fit at ten. Rather than truncating a navigation label — which reads
     * as a broken screen rather than as a short word — the caption takes the largest
     * size in this range that fits its slot, on any font scale and in any locale.
     */
    static final float NAV_CAPTION_MIN_SIZE_SP = 8f;
    /**
     * Smallest a shelf caption may shrink to, in {@code sp}.
     *
     * <p>Same rule as the navigation bar's, for the same reason: three segments
     * across a card that has already lost a gutter and a card padding, and one of
     * the three words is thirteen letters long. A truncated caption reads as a
     * broken screen; a smaller one reads as a long word.
     */
    static final float SHELF_TAB_MIN_SIZE_SP = 8f;

    // ---------------------------------------------------------- mini player

    /**
     * Height of the persistent mini player, in {@code dp}.
     *
     * <p>The whole point of this surface is that it costs almost nothing: 60dp leaves
     * room for a 44dp cover and a 48dp target, and no more. Every dp it takes is a dp
     * of content that stays hidden behind it.
     */
    static final float MINI_PLAYER_HEIGHT = 60f;
    /** Cover side inside the mini player, in {@code dp}. */
    static final float MINI_PLAYER_ARTWORK = 44f;
    /** Gap between the mini player's cover and its text, in {@code dp}. */
    static final float MINI_PLAYER_GAP = SPACE_3;

    // --------------------------------------------------------- windowed list

    /**
     * Rows laid out above and below the visible window, in {@code units of a row}.
     *
     * <p>Enough to cover a fling before the first frame of it has to wait for a bind,
     * and no more: rows outside the window are the only rows a long library pays for,
     * and this is what bounds that cost.
     */
    static final int LIST_PREFETCH_ROWS = 3;
    /** Thickness of the playback progress bar, in {@code dp}. */
    static final float PROGRESS_BAR_HEIGHT = SPACE_3;

    // ------------------------------------------------------ live sound bars

    /**
     * Alto de las barras reactivas, en {@code dp}.
     *
     * <p>Más bajo que la antigua forma de onda: ya no tiene que contar la historia
     * de la pista (eso lo cuenta la posición), sólo mostrar el sonido de ahora, y
     * el sitio que se le devuelve lo gana el aire y los controles.
     *
     * <p>Una sola fila de barras, con la envolvente de los dos canales. Se ha
     * probado a dibujar un carril por canal y el resultado es peor, no mejor: dos
     * filas en el hueco donde antes había una se leen como dos elementos
     * descolocados en lugar de como una figura, y la mitad de la altura que
     * ocupaban era más valiosa mostrando la letra. La envolvente sigue moviéndose
     * con todo lo que se oye.
     */
    static final float SOUNDBARS_HEIGHT = 36f;
    /**
     * Alto de las barras en una ventana corta, en {@code dp}.
     *
     * <p>Un teléfono en horizontal tiene que repartir la altura entre las barras y el
     * transporte, y el transporte no se toca nunca: bajan las barras. No es una escala
     * de todo, es el bloque que puede ceder su alto sin dejar de decir lo que dice.
     */
    static final float SOUNDBARS_HEIGHT_COMPACT = 26f;
    /** Paso de una barra a la siguiente, en {@code dp}. */
    static final float SOUNDBARS_BAR_PITCH = 4f;
    /** Hueco entre barras, en {@code dp}. */
    static final float SOUNDBARS_BAR_GAP = SPACE_1;
    /** Diámetro del punto de la línea de posición, en {@code dp}. */
    static final float SOUNDBARS_HEAD_SIZE = 10f;
    /** Grosor de la línea de posición, en {@code dp}. */
    static final float PROGRESS_LINE_THICKNESS = SPACE_1;

    /**
     * Cadencia de las barras, en {@code ms}.
     *
     * <p>20 Hz: suficiente para que el sonido se sienta vivo, muy por debajo de
     * lo que costaría perseguir el valor a la tasa de refresco. Las barras sólo
     * redibujan cuando hay audio nuevo, así que en pausa esta cadencia no dibuja
     * nada en absoluto.
     */
    static final long VISUAL_INTERVAL_MS = 50L;

    // --------------------------------------------------------------- artwork

    /** Cover is never smaller than this, in {@code dp}. */
    static final float ARTWORK_MIN = 160f;
    /** Cover is never larger than this, in {@code dp}. */
    static final float ARTWORK_MAX = 420f;
    /**
     * Small cover inside a list row.
     *
     * <p>Fifty-six, not forty-eight: a row cover is the only picture a list has, and
     * at 48dp it carried too little of the composition to be recognisable. It is
     * still smaller than the row is tall, so the text keeps the row.
     */
    static final float ARTWORK_THUMB = 56f;
    /** Gap between cover and details in landscape. */
    static final float ARTWORK_GUTTER_LANDSCAPE = SPACE_6;
    /** Gap between cover and details in portrait. */
    static final float ARTWORK_GUTTER_PORTRAIT = SPACE_4;
    /**
     * Cover width as a share of the width available to it, portrait.
     *
     * <p>The cover is the loudest thing on this surface, so it takes the great
     * majority of the available width (78–88%); everything else is set by what
     * is left over.
     */
    static final float ARTWORK_PORTRAIT_WIDTH_RATIO = 0.84f;
    /**
     * Cover height budget as a share of the window height, portrait.
     *
     * <p>The other half of the ratio's guard: a cover sized by width alone on a
     * very tall phone pushes every control under the fold, which is the opposite
     * of what Now Playing is for.
     */
    static final float ARTWORK_PORTRAIT_HEIGHT_RATIO = 0.42f;
    /** Cover width share in landscape, where the details need the rest. */
    static final float ARTWORK_LANDSCAPE_WIDTH_RATIO = 0.42f;
    /** Cover height share in landscape: it competes for the shorter dimension. */
    static final float ARTWORK_LANDSCAPE_HEIGHT_RATIO = 0.88f;
    /**
     * Longest side of a decoded cover, in pixels.
     *
     * <p>Decoding more than this wastes memory (a cover is the largest bitmap in
     * the app), and decoding less means upscaling a visibly soft cover. 640 px
     * covers a ~300 dp Now Playing square on a 2x–3x screen without scaling it
     * hard.
     */
    static final int ARTWORK_DECODE_MAX_PX = 640;
    /** Edge length of the tiny cover sampled for the palette, in pixels. */
    static final int ARTWORK_PALETTE_SAMPLE_PX = 32;

    // -------------------------------------------------------------- typography

    /**
     * Product/brand line.
     *
     * <p>The largest type in the app and the only one that is never a track's name:
     * it exists for the masthead, so the first thing Home says is what this is
     * before it says what to play. It used to be declared here and set nowhere,
     * which is the same as having no display level at all — every screen opened at
     * the same body-sized text and the hierarchy had to be faked with weight.
     */
    static final float TYPE_DISPLAY_SIZE = 26f;
    /** Greeting under the masthead; the largest line that is not the brand. */
    static final float TYPE_TITLE_SIZE = 20f;
    /** Secondary body copy, artist names. */
    static final float TYPE_BODY_SIZE = 15f;
    /** Labels and secondary controls. */
    static final float TYPE_LABEL_SIZE = 13f;
    /** Timestamps and small state. */
    static final float TYPE_CAPTION_SIZE = 11f;

    /**
     * Lo que se ve donde la letra calla.
     *
     * <p>Un LRC real tiene líneas vacías entre frases y en los instrumentales. Dejar
     * ahí la última frase escrita es decir que se está cantando algo que ya terminó, y
     * vaciar el espacio en cada corte parpadea. Una nota apagada dice las dos cosas:
     * aquí no hay letra, pero la canción sigue sonando.
     */
    static final String LYRICS_SILENCE = "\u266a";
    /** Diagnostics body; fixed pitch so columns line up. */
    static final float TYPE_MONOSPACE_SIZE = 11f;

    /** Line height of the display type, in {@code dp}. */
    static final float TYPE_DISPLAY_LINE = 32f;
    /** Line height of the title type, in {@code dp}. */
    static final float TYPE_TITLE_LINE = 26f;
    /** Line height of the body type, in {@code dp}. */
    static final float TYPE_BODY_LINE = 20f;
    /** Line height of the label type, in {@code dp}. */
    static final float TYPE_LABEL_LINE = 16f;
    /** Line height of the caption type, in {@code dp}. */
    static final float TYPE_CAPTION_LINE = 14f;

    /**
     * Letterspacing of the display type, in {@code em}.
     *
     * <p>Not negative, and that is the editorial decision: the masthead is a word
     * set in wide capitals with a rule under it, the way a sleeve or a record
     * sleeve-back sets a title, so the tracking opens up instead of tightening. A
     * negative value here was the old default and made the wordmark read as a
     * system title bar.
     */
    static final float TYPE_DISPLAY_TRACKING = 0.16f;
    /** Letterspacing of the label type, in {@code em}. */
    static final float TYPE_LABEL_TRACKING = 0.06f;

    // ------------------------------------------------------------ state glyph

    /**
     * Size of the state indicator beside the status word, in {@code dp}.
     *
     * <p>Small on purpose: it has to read as a status, not as an icon competing
     * with the transport.
     */
    static final float STATE_DOT_SIZE = 14f;

    // ------------------------------------------------------------------ motion

    /** Press feedback duration, in {@code ms}. */
    static final long DURATION_FAST = 90L;
    /** Ordinary state change, in {@code ms}. */
    static final long DURATION_BASE = 150L;
    /** Larger surface transition, in {@code ms}. */
    static final long DURATION_SLOW = 260L;
    /**
     * Position refresh cadence, in {@code ms}.
     *
     * <p>The tick that feeds the elapsed label, the playhead and the waveform.
     * It is shorter than a second because the playhead has to move smoothly, and
     * the waveform shape arrives in bursts of ~128 ms of audio that would
     * otherwise be applied half a second at a time. It is still 5 Hz: the elapsed
     * label only changes once a second, so four ticks out of five cost a handful
     * of integer comparisons and nothing else.
     */
    static final long POSITION_INTERVAL_MS = 200L;

    /**
     * Cuánto se retrasa la letra respecto a la posición del decodificador, en ms.
     *
     * <p>La posición que publica el motor cuenta fotogramas <em>entregados al
     * decodificador</em>, no fotogramas que ya han salido por el altavoz. leading
     * de esa cifra a la que se oye es el búfer de salida, y para una fuente en
     * streaming son varios cientos de ms. Consecuencia visible: la línea de letra
     * que se enciende es la <em>siguiente</em> a la que se está cantando, y en una
     * canción con frases de tres o cuatro segundos el desfase se lee como un verso
     * entero de adelanto.
     *
     * <p>Es una compensación, no una medición: no hay forma honesta de saber el
     * búfer real desde aquí, y un número inventado en milisegundos sería
     * igualmente inventado. Se aplica <b>solo a la letra</b> —el elapsed y el cursor
     * siguen siendo la posición real— y se deja en un token para que se pueda mover
     * sin buscarlo.
     *
     * <p>Son 400 ms y no menos porque 350 seguían dejando la línea un poco adelantada
     * al oído: en el hueco entre el búfer de salida y la atención del oyente hay
     * además el tiempo que tarda el propio cerebro en procesar lo que acaba de oír.
     * Ese segundo tramo no es un búfer y no se puede medir, así que se corrige a
     * oído, que es como se corrigen las cosas que el usuario nota antes que el
     * cronómetro.
     */
    static final long LYRIC_LEAD_MS = 475L;
    /**
     * Engine refresh cadence, in {@code ms}: the service poll that drives the
     * notification, the lock screen and the session's own state.
     *
     * <p>Deliberately slower than {@link #POSITION_INTERVAL_MS}. A refresh takes a
     * full snapshot of the session, and the session notifies every observer, which
     * re-renders the whole surface. Paying that 5 times a second to redraw a
     * notification twice a second would be work for nothing, and the surface's own
     * tick already reads the position directly.
     */
    static final long SESSION_REFRESH_INTERVAL_MS = 500L;
    /**
     * Interval of the working-state segments, in {@code ms}.
     *
     * <p>Driven by the same UI tick as everything else instead of a frame
     * callback: an indeterminate spinner redraws at 60–120 fps for as long as a
     * track takes to resolve, and it says nothing a 2.5 Hz three-segment
     * indicator does not.
     */
    static final long STATE_SEGMENT_INTERVAL_MS = 400L;

    // ----------------------------------------------------------------- colours

    /**
     * Los roles de color, resueltos para el tema que esté puesto.
     *
     * <p>Nombrados por papel y no por color, para que re-apuntar un papel —al color
     * que salga de la portada, o al de otro tema— no obligue a tocar ninguna vista.
     * Todos son {@code int} opacos empaquetados, así que leer uno es cargar un
     * campo y no una llamada a {@code Color.parseColor}.
     *
     * <h3>De constantes a valores</h3>
     * Antes eran {@code static final} y no había tema que cambiar. Ahora los escribe
     * {@link UiTheme}, y es el único que los escribe: «qué color hay ahora» tiene
     * una sola respuesta en toda la app. Los valores de arranque son los del tema
     * por defecto, resueltos por la misma aritmética que usa cualquier otro, de
     * modo que no hay dos caminos que puedan discrepar.
     *
     * <p>Un tema cambia y la ventana se reconstruye en lugar de repintar quince
     * vistas que cachean un color cada una; el razonamiento está en
     * {@link UiTheme}.
     *
     * <h3>La escalera de superficies</h3>
     * El pozo (la tarjeta del reproductor) es más oscuro que el campo incluso en un
     * tema claro, y con motivo: las barras en vivo y la línea de posición tienen
     * que alcanzar 3:1 contra él, y un campo claro no se lo da para los colores
     * saturados que salen de las portadas. Sin el pozo, una funda azul profunda
     * renunciaba a su color y la app pintaba el acento de marca en su lugar.
     *
     * <h3>Qué es el acento y qué no</h3>
     * El acento marca lo que el usuario actúa sobre. Dos garantías: ≥3:1 contra
     * cualquier superficie, que es lo que necesita un glifo, un anillo o un
     * control relleno; y <b>el acento no lleva palabras</b>, porque 13&nbsp;sp no
     * llegan a 4,5:1 sobre una tarjeta ni sobre una superposición. Para eso está
     * {@link #ACCENT_TEXT}, y sobre una tarjeta o un menú una etiqueta es
     * {@link #ON_SURFACE} con una regla de acento al lado.
     *
     * <p>{@code BrandPaletteTest} ejecuta estas mismas garantías para los seis
     * temas, no para uno.
     */
    static final class Palette {
        private Palette() { }

        // ---- surfaces ---------------------------------------------------------

        /** App background and page field. */
        static int SURFACE_BASE;
        /** Raised surface: mini player, search field. */
        static int SURFACE_RAISED;
        /** The well the live bars and the progress line are drawn on. */
        static int SURFACE_CARD;
        /** A control raised out of a well. */
        static int SURFACE_CARD_ACTIVE;
        /** Highest surface: menus, sheets, the diagnostics panel. */
        static int SURFACE_OVERLAY;
        /** Behind cover art before it has loaded. */
        static int SURFACE_ARTWORK;
        /** Behind cover art, second stop of the placeholder gradient. */
        static int SURFACE_ARTWORK_EDGE;

        // ---- ink --------------------------------------------------------------

        /** Primary text: the far end of the theme, the one that reads on the field. */
        static int ON_SURFACE;
        /** Secondary text: artists, durations, state. */
        static int ON_SURFACE_MUTED;
        /** Tertiary text and disabled glyphs; the one role below 4.5:1 anywhere. */
        static int ON_SURFACE_FAINT;

        // ---- accent -----------------------------------------------------------

        /** The accent. Reserved for the actions and the currently-selected surface. */
        static int ACCENT;
        /** The only accent allowed to carry words: the same hue, read for text. */
        static int ACCENT_TEXT;
        /** Accent under the finger: lifted, so the press reads as heat. */
        static int ACCENT_PRESSED;
        /** Content drawn on top of {@link #ACCENT}. */
        static int ON_ACCENT;
        /**
         * How far a press lifts <em>whatever</em> accent towards the ink.
         *
         * <p>Not a colour but a rule, because the accent moves with the cover: a
         * press state painted from a fixed brand colour snapped the control back
         * the moment the artwork derived a different accent, which is exactly the
         * kind of visual state that contradicts the real one.
         */
        static final float ACCENT_PRESS_LIFT = 0.25f;

        // ---- functional states ------------------------------------------------

        /** Recoverable failure; the theme's reddest hue, repaired to be readable. */
        static int ERROR;
        /** Error pressed. */
        static int ERROR_PRESSED;
        /** Completed action: downloaded, downloaded already. */
        static int POSITIVE;
        /** A warning hue: the theme's warmest surviving colour. */
        static int WARNING;
        /**
         * Work in progress (resolving, buffering).
         *
         * <p>Deliberately <em>not</em> the accent: the accent means "this is what
         * you act on", and work in progress is something the user is waiting for.
         * It is the muted ink, which also separates from the accent by lightness
         * rather than by a hue the theme does not have.
         */
        static int STATE_WORKING;

        // ---- lines and overlays ----------------------------------------------

        /** Hairline separator: the ink, quiet. */
        static int HAIRLINE;
        /** Ripple colour over a card surface. */
        static int RIPPLE;
        /** Ripple colour over the accent. */
        static int RIPPLE_ON_ACCENT;
        /** Scrim behind the cover when it is still decoding. */
        static int SCRIM;

        /**
         * Alpha for a disabled glyph, applied to its role colour.
         */
        static final float ALPHA_DISABLED = 0.38f;
        /** Alpha for a hairline or a decorative rule. */
        static final float ALPHA_HAIRLINE = 0.12f;
        /** Alpha of the placeholder artwork gradient. */
        static final float ALPHA_ARTWORK_EDGE = 0.55f;

        /**
         * Pone todos los roles a los de un tema resuelto.
         *
         * <p>La única puerta de entrada: nadie más escribe en estos campos, así que
         * la tabla no puede quedar a medias con un tema viejo y otro nuevo.
         */
        static void install(ThemePalette.Resolved table) {
            SURFACE_BASE = table.surfaceBase;
            SURFACE_RAISED = table.surfaceRaised;
            SURFACE_CARD = table.surfaceCard;
            SURFACE_CARD_ACTIVE = table.surfaceCardActive;
            SURFACE_OVERLAY = table.surfaceOverlay;
            SURFACE_ARTWORK = table.surfaceArtwork;
            SURFACE_ARTWORK_EDGE = table.surfaceArtworkEdge;
            ON_SURFACE = table.onSurface;
            ON_SURFACE_MUTED = table.onSurfaceMuted;
            ON_SURFACE_FAINT = table.onSurfaceFaint;
            ACCENT = table.accent;
            ACCENT_TEXT = table.accentText;
            ACCENT_PRESSED = table.accentPressed;
            ON_ACCENT = table.onAccent;
            ERROR = table.error;
            ERROR_PRESSED = mix(table.error, 0xFF000000, 0.25f);
            POSITIVE = table.positive;
            WARNING = table.warning;
            STATE_WORKING = table.stateWorking;
            HAIRLINE = table.hairline;
            RIPPLE = table.ripple;
            RIPPLE_ON_ACCENT = scaleAlpha(table.onAccent, 0.20f);
            SCRIM = scaleAlpha(table.surfaceBase, 0.66f);
        }

        static {
            install(ThemePalette.resolve(ThemeSpec.DEFAULT));
        }
    }

    /**
     * Colour of a control in each visual state, derived from its role.
     *
     * <p>One table for every icon button, so pressed and disabled can never be
     * forgotten on one control and present on another.
     *
     * @param enabled    whether the control is available
     * @param selected   whether the control's section is the active one
     * @param activated  whether the control represents an ON setting
     * @param accent     the accent to use, so the theme seam can override it
     * @return opaque {@code ARGB}
     */
    static int controlColor(boolean enabled, boolean selected, boolean activated, int accent) {
        if (!enabled) return scaleAlpha(Palette.ON_SURFACE_FAINT, Palette.ALPHA_DISABLED);
        if (selected || activated) return accent;
        return Palette.ON_SURFACE;
    }

    /** Colour of a control's ripple for the given surface. */
    static int rippleColor(boolean onAccent) {
        return onAccent ? Palette.RIPPLE_ON_ACCENT : Palette.RIPPLE;
    }

    /**
     * The colour a control takes while the finger is on it.
     *
     * <p>Derived from the accent it is actually using, never from the brand. That
     * distinction is the whole reason this exists: the accent moves with the cover,
     * and a press state read from a fixed brand colour snapped the control back to
     * gold the moment a different cover derived a different accent — a visual state
     * contradicting the real one on the one control the user is most likely to hold
     * down.
     *
     * <p>The direction is not fixed. It is decided by the glyph the disc has to keep
     * carrying, because the gesture has to move the disc <i>away</i> from that
     * glyph: a fixed "always lighter" rule read correctly on the dark themes and
     * buried the glyph on the light ones, where the ink is nearly black and
     * darkening the disc walks it straight into it.
     *
     * @param accent  the accent currently in force
     * @param onAccent the content the disc carries, which is what decides which way
     *                 the press has to move
     */
    static int pressedAccent(int accent, int onAccent) {
        int lighter = mix(accent, 0xFFFFFFFF, Palette.ACCENT_PRESS_LIFT);
        int darker = mix(accent, 0xFF000000, Palette.ACCENT_PRESS_LIFT);
        // El gesto tiene que apartar el disco del glifo que lleva encima, y el
        // glifo no es siempre oscuro: con un tema de campo claro la tinta es casi
        // negra y oscurecer el disco lo acerca al glifo en vez de alejarlo. Se
        // prueban las dos direcciones, cada una ya reparada contra el glifo, y se
        // queda con la que más separa las dos cosas.
        //
        // Que el glifo siga leyéndose pulsado no es un extra: es el único estado en
        // que el disco se ve de reojo, y una pulsación no puede tapar lo único que
        // el disco dice.
        int up = ColorMath.readable(lighter, onAccent, 4.5d);
        int down = ColorMath.readable(darker, onAccent, 4.5d);
        return ColorMath.contrast(down, onAccent) >= ColorMath.contrast(up, onAccent)
                ? down : up;
    }

    /**
     * Alpha of a shape that is context, not content: the rest of a progress line,
     * the still part of a bar field. Loud enough to belong to the composition,
     * quiet enough never to read as a value.
     */
    static final float ALPHA_MUTED_SHAPE = 0.45f;

    /** Replaces {@code color}'s alpha with {@code alpha}, keeping its RGB. */
    static int scaleAlpha(int color, float alpha) {
        return ColorMath.scaleAlpha(color, alpha);
    }

    /** Linear interpolation between two opaque colours, for press fades. */
    static int mix(int from, int to, float ratio) {
        return ColorMath.mix(from, to, ratio);
    }
}