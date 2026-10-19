package com.tunefold.app;

import android.content.Context;
import android.text.TextUtils;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * The masthead: the first thing on Home, and the only place the product says its
 * own name.
 *
 * <p><b>Why this exists at all.</b> Home used to open with a time-of-day greeting
 * in the same size as a track title, under a row of identical list rows. Nothing
 * on the screen said what the app was, what state it was in, or how big the library
 * was — three facts the user has to be able to read without touching anything. The
 * masthead is where they live, and it is three lines with a strict order:
 *
 * <ol>
 *   <li><b>The wordmark.</b> Wide capitals at the display size, letterspaced, with
 *       a gold rule under it. This is the masthead of a record sleeve, not a
 *       system title bar, which is why the tracking opens up instead of closing.</li>
 *   <li><b>The greeting.</b> Warm, in the title size — the largest type in the app
 *       that is not the brand.</li>
 *   <li><b>The state of the library.</b> How much is saved and whether anything is
 *       playing, in the caption size.</li>
 * </ol>
 *
 * <p>Nothing here is invented. The line under the greeting is composed from the
 * session's own counts, and it says "nothing yet, go and find something" when both
 * counts are zero instead of showing an empty screen with no explanation.
 *
 * <p>The only control it carries is the one the caller hands it, so Home's
 * diagnostics toggle shares this row instead of occupying a full-width strip of its
 * own above the masthead.
 */
final class MastheadView extends LinearLayout {

    /** What the library currently holds, in the words the caption uses. */
    private final TextView state;
    private final TextView greeting;
    /** The row that carries the wordmark and the one optional control. */
    private final LinearLayout head;

    private CharSequence renderedGreeting = "";
    private CharSequence renderedState = "";
    private View accessory;

    MastheadView(Context context) {
        super(context);
        DesignTokens.init(context);
        setOrientation(VERTICAL);

        head = new LinearLayout(context);
        head.setOrientation(HORIZONTAL);
        head.setGravity(Gravity.CENTER_VERTICAL);
        addView(head, new LayoutParams(LayoutParams.MATCH_PARENT,
                LayoutParams.WRAP_CONTENT));

        TextView wordmark = new TextView(context);
        wordmark.setText("TUNEFOLD");
        wordmark.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_DISPLAY_SIZE);
        wordmark.setTextColor(DesignTokens.Palette.ON_SURFACE);
        wordmark.setLetterSpacing(DesignTokens.TYPE_DISPLAY_TRACKING);
        wordmark.setAllCaps(true);
        wordmark.setIncludeFontPadding(false);
        wordmark.setMaxLines(1);
        wordmark.setSingleLine(true);
        wordmark.setFocusable(false);
        // Decorative: the greeting underneath is the first readable line, and a
        // screen reader announcing "Tunefold" as a node nobody can act on is noise.
        wordmark.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
        LayoutParams wordmarkParams = new LayoutParams(0,
                LayoutParams.WRAP_CONTENT, 1f);
        head.addView(wordmark, wordmarkParams);

        // The rule is the identity: a printed one, under the word, gold on olive,
        // the same thickness as every other rule in the app.
        View rule = new View(context);
        rule.setBackgroundColor(DesignTokens.Palette.ACCENT);
        LayoutParams ruleParams = new LayoutParams(LayoutParams.MATCH_PARENT,
                DesignTokens.dp(DesignTokens.RULE_THICKNESS));
        ruleParams.topMargin = DesignTokens.dp(DesignTokens.SPACE_2);
        addView(rule, ruleParams);

        greeting = new TextView(context);
        greeting.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_TITLE_SIZE);
        greeting.setTextColor(DesignTokens.Palette.ON_SURFACE);
        greeting.setIncludeFontPadding(false);
        greeting.setMaxLines(1);
        greeting.setEllipsize(TextUtils.TruncateAt.END);
        LayoutParams greetingParams = new LayoutParams(LayoutParams.MATCH_PARENT,
                LayoutParams.WRAP_CONTENT);
        greetingParams.topMargin = DesignTokens.dp(DesignTokens.SPACE_4);
        addView(greeting, greetingParams);

        state = new TextView(context);
        state.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_CAPTION_SIZE);
        state.setTextColor(DesignTokens.Palette.ON_SURFACE_MUTED);
        state.setLetterSpacing(DesignTokens.TYPE_LABEL_TRACKING);
        state.setAllCaps(true);
        state.setIncludeFontPadding(false);
        state.setMaxLines(2);
        LayoutParams stateParams = new LayoutParams(LayoutParams.MATCH_PARENT,
                LayoutParams.WRAP_CONTENT);
        stateParams.topMargin = DesignTokens.dp(DesignTokens.SPACE_1);
        addView(state, stateParams);
    }

    /**
     * The one control this row may carry, or {@code null} to remove it.
     *
     * <p>Kept in the masthead row so a development toggle stops costing a full-width
     * strip of its own above the identity it belongs next to.
     */
    void setAccessory(View view) {
        if (accessory == view) return;
        if (accessory != null) head.removeView(accessory);
        accessory = view;
        if (view == null) return;
        LayoutParams params = new LayoutParams(LayoutParams.WRAP_CONTENT,
                LayoutParams.WRAP_CONTENT);
        params.leftMargin = DesignTokens.dp(DesignTokens.GAP);
        head.addView(view, params);
    }

    /**
     * The greeting, decided by the hour and nothing else.
     *
     * <p>Pure so the three-times-a-day rule is a test rather than an observation:
     * the word moves three times a day and the hour is read on every render, so the
     * caller's job is to compare this against what it last drew.
     */
    static String greetingForHour(int hour) { return greetingForHour(hour, ""); }

    /**
     * The greeting, with the name the user chose in Settings.
     *
     * <p>The name is an ending, not a new sentence: it is appended to whichever word
     * the hour produced, so changing the name can never change the time of day and
     * a blank one leaves the greeting exactly as it was. That is the only thing the
     * name does anywhere in the app, and pretending otherwise would be inventing a
     * feature that does not exist.
     */
    static String greetingForHour(int hour, String name) {
        String word;
        if (hour < 0 || hour > 23) {
            word = "Hola";
        } else if (hour < 6) {
            word = "Buenas noches";
        } else if (hour < 13) {
            word = "Buenos días";
        } else if (hour < 21) {
            word = "Buenas tardes";
        } else {
            word = "Buenas noches";
        }
        String who = name == null ? "" : name.trim();
        return who.isEmpty() ? word : word + ", " + who;
    }

    /**
     * What the library looks like right now, in one caption.
     *
     * <p>Only facts the session actually knows: how many tracks are saved, how many
     * of them are on disk, and whether something is playing. When both are zero it
     * says what to do about it, because an empty home screen with no sentence on it
     * is a screen that looks broken.
     *
     * @param liked     tracks in L1K3D
     * @param downloads tracks saved to disk
     * @param playing   whether a track is loaded
     */
    static String libraryLine(int liked, int downloads, boolean playing) {
        int savedLiked = Math.max(0, liked);
        int savedDownloads = Math.max(0, downloads);
        if (savedLiked == 0 && savedDownloads == 0) {
            return playing ? "Sonando · guarda algo para volver a él"
                    : "Nada guardado todavía · empieza por la búsqueda";
        }
        StringBuilder line = new StringBuilder(64);
        if (playing) line.append("Sonando");
        if (savedLiked > 0) {
            if (line.length() > 0) line.append(" · ");
            line.append(savedLiked).append(" en L1K3D");
        }
        if (savedDownloads > 0) {
            if (line.length() > 0) line.append(" · ");
            line.append(savedDownloads).append(savedDownloads == 1
                    ? " descarga" : " descargas");
        }
        return line.toString();
    }

    /**
     * Applies what the session knows.
     *
     * <p>Every setter compares before it writes: this is handed a snapshot twice a
     * second for every reason at all, and two {@code setText} calls that change
     * nothing are two measure passes on the first thing the user reads.
     */
    void apply(String word, int liked, int downloads, boolean playing) {
        if (!TextUtils.equals(word, renderedGreeting)) {
            renderedGreeting = word;
            greeting.setText(word);
        }
        CharSequence line = libraryLine(liked, downloads, playing);
        if (TextUtils.equals(line, renderedState)) return;
        renderedState = line;
        state.setText(line);
    }

    /** What the masthead last published, for the component tests. */
    CharSequence renderedStateLine() { return renderedState; }
}