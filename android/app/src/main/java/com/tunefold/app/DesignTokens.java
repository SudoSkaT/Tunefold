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
    /** Thickness of the playback progress bar, in {@code dp}. */
    static final float PROGRESS_BAR_HEIGHT = SPACE_3;

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

    // ------------------------------------------------------------------ motion

    /** Press feedback duration, in {@code ms}. */
    static final long DURATION_FAST = 90L;
    /** Ordinary state change, in {@code ms}. */
    static final long DURATION_BASE = 150L;
    /** Larger surface transition, in {@code ms}. */
    static final long DURATION_SLOW = 260L;
    /** Position refresh cadence, in {@code ms}. */
    static final long POSITION_INTERVAL_MS = 500L;

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

        /** Alpha for a disabled glyph, applied to its role colour. */
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