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
 * <h3>Why a dark, artwork-first palette</h3>
 * The product identity is ARTWORK &rarr; COLOUR &rarr; WAVEFORM &rarr; PLAYBACK.
 * A white or pure-black chrome would out-shout the cover art and flatten that
 * chain, so the surfaces are a deep, slightly blue ink that is dark enough to
 * recede and never fully black, and the accent is reserved for the few things
 * the user acts on. {@link ArtworkTheme} is the seam that later replaces the
 * fixed accent with one derived from the current cover.
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
    /** Radius of the small artwork in a list row. */
    static final float RADIUS_ARTWORK_THUMB = SPACE_2;

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
    /** Height of one track row. */
    static final float ROW_HEIGHT = 64f;
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
    static final int BOTTOM_NAV_SURFACE = Palette.SURFACE_BASE;
    /** Caption size under each destination glyph, in {@code sp}. */
    static final float NAV_CAPTION_SIZE = 10f;
    /**
     * Smallest a destination caption may shrink to, in {@code sp}.
     *
     * <p>Four destinations across 360dp is ninety each, and the longest word in the
     * set does not fit at ten. Rather than truncating a navigation label — which reads
     * as a broken screen rather than as a short word — the caption takes the largest
     * size in this range that fits its slot, on any font scale and in any locale.
     */
    static final float NAV_CAPTION_MIN_SIZE_SP = 8f;

    // ---------------------------------------------------------- mini player

    /**
     * Height of the persistent mini player, in {@code dp}.
     *
     * <p>The whole point of this surface is that it costs almost nothing: 56dp leaves
     * room for a 40dp cover and a 48dp target, and no more. Every dp it takes is a dp
     * of content that stays hidden behind it.
     */
    static final float MINI_PLAYER_HEIGHT = 56f;
    /** Cover side inside the mini player, in {@code dp}. */
    static final float MINI_PLAYER_ARTWORK = 40f;
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
     */
    /**
     * Alto de las barras en una ventana normal, en {@code dp}.
     *
     * <p>Medio carril, no dos: la figura es una sola fila y lo que se freed va a la
     * letra, que informa más que un segundo canal dibujado.
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
    /** Separación entre el carril izquierdo y el derecho, en {@code dp}. */
    static final float SOUNDBARS_LANE_GAP = SPACE_3;
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
    /** Small cover inside a list row. */
    static final float ARTWORK_THUMB = 48f;
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

    /** Product/brand line. */
    static final float TYPE_DISPLAY_SIZE = 22f;
    /** Current track title. */
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
    static final float TYPE_DISPLAY_LINE = 28f;
    /** Line height of the title type, in {@code dp}. */
    static final float TYPE_TITLE_LINE = 26f;
    /** Line height of the body type, in {@code dp}. */
    static final float TYPE_BODY_LINE = 20f;
    /** Line height of the label type, in {@code dp}. */
    static final float TYPE_LABEL_LINE = 16f;
    /** Line height of the caption type, in {@code dp}. */
    static final float TYPE_CAPTION_LINE = 14f;

    /** Letterspacing of the display type, in {@code em}. */
    static final float TYPE_DISPLAY_TRACKING = -0.02f;
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
     * Colour roles, not colours.
     *
     * <p>Named by role so a role can be re-pointed (to an artwork-derived colour,
     * or to a high-contrast variant) without touching a single View. Values are
     * opaque {@code ARGB} packed as {@code int}s resolved at build time, so
     * reading one is a field load and not a {@code Color.parseColor} call.
     */
    static final class Palette {
        private Palette() { }

        /** App background. A deep blue-black: dark, but never {@code #000000}. */
        static final int SURFACE_BASE = 0xFF0D1117;
        /** Slightly lifted surface: top bar, search field. */
        static final int SURFACE_RAISED = 0xFF141922;
        /** Card and list-row surface. */
        static final int SURFACE_CARD = 0xFF1A202B;
        /** Pressed or selected card. */
        static final int SURFACE_CARD_ACTIVE = 0xFF202836;
        /** Highest surface: menus, sheets, the diagnostics panel. */
        static final int SURFACE_OVERLAY = 0xFF232935;
        /** Behind cover art before it has loaded. */
        static final int SURFACE_ARTWORK = 0xFF2A3140;
        /** Behind cover art, second stop of the placeholder gradient. */
        static final int SURFACE_ARTWORK_EDGE = 0xFF1B2130;

        /** Primary text. Cool white, never pure white. */
        static final int ON_SURFACE = 0xFFE8EFF8;
        /** Secondary text: artists, durations, state. */
        static final int ON_SURFACE_MUTED = 0xFF97A3B5;
        /** Tertiary text and disabled glyphs. */
        static final int ON_SURFACE_FAINT = 0xFF67727F;

        /** Accent. Reserved for the actions and the currently-selected surface. */
        static final int ACCENT = 0xFF3DD6C0;
        /** Accent under the finger. */
        static final int ACCENT_PRESSED = 0xFF2AB9A5;
        /** Content drawn on top of {@link #ACCENT}. */
        static final int ON_ACCENT = 0xFF04231E;

        /** Recoverable failure. */
        static final int ERROR = 0xFFFF6B5E;
        /** Error pressed. */
        static final int ERROR_PRESSED = 0xFFE0544A;
        /** Completed action: downloaded, downloaded already. */
        static final int POSITIVE = 0xFF57E39C;
        /** Destructive confirmation. */
        static final int WARNING = 0xFFFFC46B;

        /** Hairline separator. */
        static final int HAIRLINE = 0x1FFFFFFF;
        /** Ripple colour over a card surface. */
        static final int RIPPLE = 0x1AFFFFFF;
        /** Ripple colour over the accent. */
        static final int RIPPLE_ON_ACCENT = 0x33000000;

        /** Scrim behind the cover when it is still decoding. */
        static final int SCRIM = 0x660D1117;

        /**
         * Colour of a state that is work in progress (resolving, buffering).
         *
         * <p>The warning hue, deliberately <em>not</em> the accent: the accent
         * means "this is what you act on", and work in progress is something the
         * user is waiting for, not something they chose.
         */
        static final int STATE_WORKING = WARNING;

        /**
         * Alpha for a disabled glyph, applied to its role colour.
         */
        static final float ALPHA_DISABLED = 0.38f;
        /** Alpha for a hairline or a decorative rule. */
        static final float ALPHA_HAIRLINE = 0.12f;
        /** Alpha of the placeholder artwork gradient. */
        static final float ALPHA_ARTWORK_EDGE = 0.55f;
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

    /** Replaces {@code color}'s alpha with {@code alpha}, keeping its RGB. */
    /**
     * Alpha of a shape that is context, not content: the rest of a progress line,
     * the still part of a bar field. Loud enough to belong to the composition,
     * quiet enough never to read as a value.
     */
    static final float ALPHA_MUTED_SHAPE = 0.45f;

    static int scaleAlpha(int color, float alpha) {
        int scaled = Math.round(Math.max(0f, Math.min(1f, alpha))
                * ((color >>> 24) & 0xFF));
        return (color & 0x00FFFFFF) | (scaled << 24);
    }

    /** Linear interpolation between two opaque colours, for press fades. */
    static int mix(int from, int to, float ratio) {
        float t = Math.max(0f, Math.min(1f, ratio));
        int a = Math.round(((from >>> 24) & 0xFF) * (1f - t) + ((to >>> 24) & 0xFF) * t);
        int r = Math.round(((from >> 16) & 0xFF) * (1f - t) + ((to >> 16) & 0xFF) * t);
        int g = Math.round(((from >> 8) & 0xFF) * (1f - t) + ((to >> 8) & 0xFF) * t);
        int b = Math.round((from & 0xFF) * (1f - t) + (to & 0xFF) * t);
        return (a << 24) | (r << 16) | (g << 8) | b;
    }
}