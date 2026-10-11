package com.tunefold.app;

/**
 * The rules that turn a window into a layout, and nothing else.
 *
 * <p>Every adaptive decision in this app — how big the cover is, whether the player is
 * one column or two, how wide the content may get, how much air sits between blocks —
 * is one call into here. The alternative is a number per View, and a number per View
 * is a number that cannot be checked without a device.
 *
 * <h3>Why this class is pure arithmetic</h3>
 * It takes a window in {@code dp} and answers in {@code dp}. It does not read
 * {@code Resources}, does not know about insets and does not touch a {@code View}, so
 * every rule below is verifiable in a JVM unit test
 * ({@code LayoutRulesTest}) on a machine that has no phone attached. The caller
 * converts to pixels once, per configuration change, with {@link DesignTokens#dp}.
 *
 * <h3>What it refuses to do</h3>
 * A larger screen gets a bigger cover, more air and a wider column — and nothing else.
 * Touch targets, type sizes and control sizes are tokens and stay exactly as they are:
 * a tablet is a bigger room, not a bigger person, and scaling the controls is how an
 * interface turns into a diagram of itself. Growth is bounded by
 * {@link #ARTWORK_MAX_BY_FORM} and {@link #CONTENT_MAX_DP}, so the composition stops
 * spreading once it is comfortable and never becomes a letterbox.
 */
final class LayoutRules {

    // ------------------------------------------------------------ form factors

    /** A phone in either orientation. The default, and the one everything fits in. */
    static final int FORM_COMPACT = 0;
    /** A small tablet, or a phone in a very wide window: two columns start to pay. */
    static final int FORM_MEDIUM = 1;
    /** A tablet. Content gets a maximum width and the player keeps two regions. */
    static final int FORM_EXPANDED = 2;

    /** Smallest width, in {@code dp}, at which a window stops being a phone. */
    static final int MEDIUM_MIN_WIDTH_DP = 600;
    /** Smallest width at which the content is allowed to reach its maximum width. */
    static final int EXPANDED_MIN_WIDTH_DP = 840;

    /**
     * Height under which the window is treated as short, in {@code dp}.
     *
     * <p>A landscape phone is short, and that is the whole point: a surface that fits
     * a portrait phone will not fit one turned on its side, and the controls that go
     * missing are the transport.
     */
    static final int SHORT_WINDOW_DP = 480;

    // ----------------------------------------------------------------- bounds

    /**
     * Largest content column, in {@code dp}, before the composition stops growing.
     *
     * <p>A line of text longer than this is harder to read, not easier, and a card
     * stretched across a tablet is a card with a title in the middle of nowhere.
     */
    static final int CONTENT_MAX_DP = 560;
    /**
     * Largest artwork side per form factor, in {@code dp}.
     *
     * <p>Rising with the form factor and stopping well short of "fill the screen":
     * the cover is the loudest thing here, and a cover that grows without limit on a
     * large display pushes the controls that the user actually came for off the
     * bottom.
     */
    static final int ARTWORK_MAX_COMPACT_DP = 420;
    static final int ARTWORK_MAX_MEDIUM_DP = 480;
    static final int ARTWORK_MAX_EXPANDED_DP = 540;

    /** Extra air per step, as a fraction, on the screens that have room for it. */
    static final float SPACING_STEP = 0.25f;
    /** Air added between blocks on a medium or expanded window, in {@code dp}. */
    static final float SPACING_STEP_DP = 4f;
    /** How much the page gutter grows per step, in {@code dp}. */
    static final float GUTTER_STEP_DP = 8f;

    private LayoutRules() { }

    // ----------------------------------------------------------------- result

    /**
     * Everything the UI needs to know about one window, in {@code dp}.
     *
     * <p>Immutable and comparable through {@link #key()}, so a caller can hand the
     * whole thing to a View and let it decide whether anything moved. That is what
     * keeps a configuration change that did not change the layout free.
     */
    static final class Profile {

        private final int widthDp;
        private final int heightDp;
        private final int smallestWidthDp;
        private final boolean landscape;
        private final int form;
        private final boolean shortWindow;

        Profile(int widthDp, int heightDp, int smallestWidthDp, boolean landscape,
                int form, boolean shortWindow) {
            this.widthDp = widthDp;
            this.heightDp = heightDp;
            this.smallestWidthDp = smallestWidthDp;
            this.landscape = landscape;
            this.form = form;
            this.shortWindow = shortWindow;
        }

        /** Width of the window, in {@code dp}, insets already removed. */
        int widthDp() { return widthDp; }

        /** Height of the window, in {@code dp}, insets already removed. */
        int heightDp() { return heightDp; }

        /** Which of the three sizes this window is. */
        int form() { return form; }

        /** Whether the window is wider than it is tall. */
        boolean landscape() { return landscape; }

        /**
         * Whether the window is too short for the full composition.
         *
         * <p>A landscape phone always is, which is why the player's two regions and
         * the smaller sound bars are not a tablet feature: they are what makes a
         * phone on its side usable.
         */
        boolean shortWindow() { return shortWindow || (landscape && heightDp < 600); }

        /**
         * Whether the player is composed as two regions side by side.
         *
         * <p>Landscape, or a window wide enough that a cover beside its details
         * beats a cover above them. A short <em>portrait</em> window is not: the
         * cover is the first thing to give up, not the first thing to move.
         */
        boolean twoColumnPlayer() { return landscape || form() >= FORM_MEDIUM; }

        /**
         * How many steps of extra air this window earns; 0 on a phone.
         *
         * <p>Two steps, not one, on a tablet, because the difference between a phone
         * and a tablet in the hand is larger than the difference between a small
         * phone and a large one.
         */
        int spacingStep() { return form() == FORM_EXPANDED ? 2 : form() == FORM_MEDIUM ? 1 : 0; }

        /** The page gutter: wider on a screen with room to breathe in. */
        int gutterDp() {
            return Math.round(DesignTokens.CONTENT_PADDING_HORIZONTAL
                    + spacingStep() * GUTTER_STEP_DP);
        }

        /** Air between two blocks of the same surface. */
        int blockGapDp() {
            return Math.round(DesignTokens.BLOCK_GAP + spacingStep() * SPACING_STEP_DP);
        }

        /** Air between two major regions. */
        int sectionGapDp() {
            return Math.round(DesignTokens.SECTION_GAP + spacingStep() * SPACING_STEP_DP);
        }

        /**
         * Largest width the content may occupy, in {@code dp}; {@code 0} for the
         * whole width, because a phone has nothing to spare.
         */
        int contentMaxDp() {
            if (form() == FORM_COMPACT) return 0;
            return Math.min(CONTENT_MAX_DP, Math.max(0, widthDp - 2 * gutterDp()));
        }

        /** Largest cover side for this window's form factor, in {@code dp}. */
        int artworkMaxDp() {
            switch (form()) {
                case FORM_MEDIUM: return ARTWORK_MAX_MEDIUM_DP;
                case FORM_EXPANDED: return ARTWORK_MAX_EXPANDED_DP;
                default: return ARTWORK_MAX_COMPACT_DP;
            }
        }

        /**
         * The cover's side, in {@code dp}, for a card of the given inner size.
         *
         * @param innerWidthDp  width the card may use, gutters and padding removed
         * @param innerHeightDp height the card may use, or {@code 0} when unknown
         *
         * <p>Two rules and two bounds. In one column the cover takes the great
         * majority of the width, because it is the loudest thing on the surface,
         * and the window height is the guard that stops a very tall phone from
         * pushing every control under the fold. In two columns it keeps the
         * dominant share of the width but yields enough of it for the metadata and
         * the transport to stay in the same window, bounded by the shorter side.
         * Either way the result never leaves {@link DesignTokens#ARTWORK_MIN}, so a
         * window too small for the composition is a window problem and not a
         * deformed cover.
         */
        int artworkSideDp(int innerWidthDp, int innerHeightDp) {
            int width = Math.max(0, innerWidthDp);
            float share = landscape
                    ? DesignTokens.ARTWORK_LANDSCAPE_WIDTH_RATIO
                    : DesignTokens.ARTWORK_PORTRAIT_WIDTH_RATIO;
            float heightShare = landscape
                    ? DesignTokens.ARTWORK_LANDSCAPE_HEIGHT_RATIO
                    : DesignTokens.ARTWORK_PORTRAIT_HEIGHT_RATIO;
            int side = Math.round(width * share);
            // A height budget only binds when the caller knows the height. A caller
            // that does not (the first layout, before anything has been measured)
            // gets the width rule alone, which is the safer of the two errors.
            if (innerHeightDp > 0) side = Math.min(side, Math.round(innerHeightDp * heightShare));
            return Math.max(Math.round(DesignTokens.ARTWORK_MIN),
                    Math.min(artworkMaxDp(), side));
        }

        /**
         * The identity of this layout, for the callers that memoize.
         *
         * <p>One string, compared instead of a struct: a rotation that lands on the
         * same window must cost one comparison and no layout.
         */
        String key() {
            return widthDp + "x" + heightDp + "/" + smallestWidthDp + "/"
                    + form + (landscape ? "L" : "P") + (shortWindow ? "S" : "");
        }
    }

    // ----------------------------------------------------------------- factory

    /**
     * Reads a window and answers with its rules.
     *
     * @param widthDp         width available to the app, insets removed
     * @param heightDp        height available to the app, insets removed
     * @param smallestWidthDp the screen's smaller side, {@code 0} when unknown
     * @param landscape       whether the window is wider than it is tall
     */
    static Profile of(int widthDp, int heightDp, int smallestWidthDp, boolean landscape) {
        int width = Math.max(1, widthDp);
        int height = Math.max(1, heightDp);
        int smallest = Math.max(0, smallestWidthDp);
        // The form follows the screen, not the current orientation: a tablet turned
        // on its side is still a tablet, and a phone turned on its side should not
        // grow the limits its portrait layout uses. Without a known smallest width
        // the current width decides, which is the honest answer on a phone.
        int basis = smallest > 0 ? Math.min(smallest, Math.max(width, height)) : width;
        int form = basis >= EXPANDED_MIN_WIDTH_DP ? FORM_EXPANDED
                : basis >= MEDIUM_MIN_WIDTH_DP ? FORM_MEDIUM : FORM_COMPACT;
        return new Profile(width, height, smallest, landscape, form, height < SHORT_WINDOW_DP);
    }

    /**
     * Whether two windows produce the same layout, for the callers that memoize.
     *
     * <p>A pure function so the rule is testable: two windows that differ only in
     * size the layout does not use must not cost a relayout.
     */
    static boolean sameLayout(Profile left, Profile right) {
        return left != null && right != null && left.key().equals(right.key());
    }
}