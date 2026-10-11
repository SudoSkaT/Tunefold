package com.tunefold.app;

import android.content.Context;
import android.text.TextUtils;
import android.util.TypedValue;
import android.view.View;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * Technical diagnostics for a playback, kept OUT of the normal Now Playing surface.
 *
 * <p>The main screen shows musical content and playback state; every Range request,
 * probe duration and TTFA belongs here, reachable only on demand. It uses the same
 * tokens as everything else, so the panel looks like part of the product even though it
 * is a developer surface.
 */
final class PlaybackDebugPanel {

    /** Traces are capped so a long session cannot grow the panel without bound. */
    private static final int MAX_LINES = 80;

    /** Panel refresh cadence, in ms. */
    private static final long REFRESH_INTERVAL_MS = 1_000L;

    private final LinearLayout root;
    private final TextView body;
    private boolean visible;
    private CharSequence rendered = "";
    private long lastRefreshMillis;

    PlaybackDebugPanel(Context context) {
        DesignTokens.init(context);
        root = new LinearLayout(context);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setVisibility(View.GONE);

        TextView header = new TextView(context);
        header.setText("Diagnostics");
        header.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_LABEL_SIZE);
        header.setTextColor(DesignTokens.Palette.ON_SURFACE_MUTED);
        header.setLetterSpacing(DesignTokens.TYPE_LABEL_TRACKING);
        header.setAllCaps(true);
        root.addView(header, new LinearLayout.LayoutParams(-1, -2));

        body = new TextView(context);
        body.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_MONOSPACE_SIZE);
        body.setTextColor(DesignTokens.Palette.ON_SURFACE);
        body.setTextIsSelectable(true);
        // Long traces must wrap rather than push the layout sideways.
        body.setMaxLines(MAX_LINES);
        body.setEllipsize(TextUtils.TruncateAt.END);
        body.setTypeface(android.graphics.Typeface.MONOSPACE);
        // Wrap content rather than a weighted height: the panel is a request/response
        // surface, not a layout participant, and a weight made every section rebuild
        // pay for text nobody could see.
        root.addView(body, new LinearLayout.LayoutParams(-1, -2));
    }

    View view() { return root; }

    boolean isVisible() { return visible; }

    void toggle() {
        visible = !visible;
        root.setVisibility(visible ? View.VISIBLE : View.GONE);
        // Shown at once, not on the next tick: the panel is opened to read a moment.
        lastRefreshMillis = 0L;
    }

    /**
     * Replaces the diagnostics text; the trace keeps its own ordering.
     *
     * <p>Written only when it changed: this is refreshed from the playback poll, and a
     * {@code setText} on a selectable TextView re-lays-out and resets the selection.
     */
    /**
     * At most once a second.
     *
     * <p>The panel's text includes live counters, so it changes on every render; at
     * the session's own cadence that was a full re-measure and re-layout of a long
     * monospace TextView twice a second — and it kept the window permanently
     * non-idle, which is precisely when idle-based frame measurement wants to run.
     * One refresh a second loses nothing a developer can use.
     */
    void update(String diagnostics, PlaybackTrace trace) {
        long now = android.os.SystemClock.uptimeMillis();
        if (now - lastRefreshMillis < REFRESH_INTERVAL_MS) return;
        lastRefreshMillis = now;
        StringBuilder text = new StringBuilder(256);
        if (diagnostics != null && !diagnostics.isEmpty()) text.append(diagnostics);
        if (trace != null) {
            String summary = trace.summary();
            if (!summary.isEmpty()) {
                if (text.length() > 0) text.append("\n\n");
                text.append("Timeline (since Play tap)\n");
                text.append(summary);
            }
        }
        String next = text.toString();
        if (TextUtils.equals(rendered, next)) return;
        rendered = next;
        body.setText(next);
    }
}