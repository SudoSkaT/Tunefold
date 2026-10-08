package com.tunefold.app;

import android.content.Context;
import android.util.TypedValue;
import android.view.Gravity;
import android.widget.LinearLayout;
import android.widget.ProgressBar;
import android.widget.TextView;

/**
 * Elapsed time, the scrubber-looking bar, and the total.
 *
 * <p>Built around the position tick, which arrives twice a second for as long as
 * audio plays. That makes this the hottest surface in the app by a wide margin, and
 * the reason {@link #setPosition} is careful:
 *
 * <ul>
 *   <li>the two labels are only re-set when the <em>second</em> they display
 *       changes, so a 500&nbsp;ms tick allocates nothing at all and calls
 *       {@code setText} half as often;</li>
 *   <li>the bar is only re-set once its value has moved by at least one pixel
 *       ({@link ProgressScale#pixelStep}), because a permille step smaller than a
 *       pixel cannot be seen and setting it would schedule a redraw for nothing;</li>
 *   <li>both caches are invalidated by identity, so a track change can never leave a
 *       previous song's time over the new one.</li>
 * </ul>
 *
 * <p>Every one of those is a guard that turns a redundant {@code setText} /
 * {@code setProgress} — each of which requests layout and then draw — into a couple of
 * integer comparisons.
 */
final class ProgressControl extends LinearLayout {

    private final ProgressBar bar;
    private final TextView elapsed;
    private final TextView total;

    private final StringBuilder scratch = new StringBuilder(8);
    /** Second currently shown on the left label, or -1. */
    private long shownSecond = -1L;
    /** Permille currently drawn on the bar. */
    private int shownPermille = -1;
    /** Identity the displayed time belongs to. */
    private TrackKey shownKey;
    /** Duration currently drawn on the right label, or -1. */
    private long shownDuration = -1L;

    ProgressControl(Context context) {
        super(context);
        DesignTokens.init(context);
        setOrientation(HORIZONTAL);
        setGravity(Gravity.CENTER_VERTICAL);

        elapsed = timeLabel(context);
        elapsed.setGravity(Gravity.START);
        total = timeLabel(context);
        total.setGravity(Gravity.END);

        bar = new ProgressBar(context, null, android.R.attr.progressBarStyleHorizontal);
        bar.setMax(ProgressScale.PERMILLE);
        bar.setProgress(0);
        bar.setContentDescription("Playback position");

        // Sized for the longest label the formatter can produce, "h:mm:ss", so a
        // long track never clips its own duration.
        int labelWidth = DesignTokens.dp(DesignTokens.TIME_LABEL_WIDTH);
        int barHeight = DesignTokens.dp(DesignTokens.PROGRESS_BAR_HEIGHT);
        addView(elapsed, new LayoutParams(labelWidth, LayoutParams.WRAP_CONTENT));
        addView(bar, new LayoutParams(0, barHeight, 1f));
        LayoutParams totalParams = new LayoutParams(labelWidth, LayoutParams.WRAP_CONTENT);
        totalParams.leftMargin = DesignTokens.dp(DesignTokens.GAP);
        addView(total, totalParams);
    }

    private static TextView timeLabel(Context context) {
        TextView view = new TextView(context);
        view.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_CAPTION_SIZE);
        view.setTextColor(DesignTokens.Palette.ON_SURFACE_MUTED);
        // A numeric format, not prose: not translatable and not a string resource.
        view.setText("0:00");
        view.setIncludeFontPadding(false);
        return view;
    }

    /**
     * Applies a position tick.
     *
     * @param key        identity the position belongs to; a tick for anything else is
     *                   refused, so a late update cannot repaint the previous song
     * @param positionMs elapsed time of that identity
     * @param durationMs total length, or {@code -1} when unknown
     */
    void setPosition(TrackKey key, long positionMs, long durationMs) {
        if (key == null) return;
        if (!key.equals(shownKey)) {
            // A new identity resets every cache: nothing from the previous track may
            // survive into the new one, not even the total.
            shownKey = key;
            shownSecond = -1L;
            shownPermille = -1;
            shownDuration = -1L;
        }
        if (durationMs > 0L && durationMs != shownDuration) {
            shownDuration = durationMs;
            TimeFormat.time(durationMs, scratch);
            total.setText(scratch.toString());
        }
        long second = ProgressScale.displayedSecond(positionMs);
        if (second != shownSecond) {
            shownSecond = second;
            TimeFormat.time(positionMs, scratch);
            elapsed.setText(scratch.toString());
            // The announced position should only change as often as the visible one.
            announce(second);
        }
        int permille = ProgressScale.permille(positionMs, durationMs);
        if (ProgressScale.shouldAdvanceBar(shownPermille, permille, pixelStep())) {
            shownPermille = permille;
            bar.setProgress(permille);
        }
    }

    /** Drops every cache; used when nothing is playing. */
    void clear() {
        shownKey = null;
        shownSecond = -1L;
        shownPermille = -1;
        shownDuration = -1L;
        if (bar.getProgress() != 0) bar.setProgress(0);
    }

    /**
     * Accessibility text for the elapsed second.
     *
     * <p>Recomposed only when the second changes, so a tick that cannot be seen is not
     * announced either.
     */
    private void announce(long second) {
        long minutes = second / 60L;
        long remainder = second % 60L;
        String text = minutes > 0L
                ? minutes + " minutes " + remainder + " seconds"
                : remainder + " seconds";
        bar.setContentDescription("Playback position, " + text);
    }

    private int cachedPixelWidth = -1;
    private int cachedStep = -1;

    /**
     * Permille gap that moves the bar by one pixel.
     *
     * <p>Recomputed only when the bar's width actually changes, which happens on
     * rotation and not on a tick.
     */
    private int pixelStep() {
        int width = bar.getWidth();
        if (width > 0 && width != cachedPixelWidth) {
            cachedPixelWidth = width;
            cachedStep = ProgressScale.pixelStep(width);
        }
        return cachedStep < 0 ? ProgressScale.PERMILLE : cachedStep;
    }

    @Override protected void onSizeChanged(int width, int height, int oldWidth, int oldHeight) {
        super.onSizeChanged(width, height, oldWidth, oldHeight);
        cachedPixelWidth = -1;
        cachedStep = -1;
    }
}