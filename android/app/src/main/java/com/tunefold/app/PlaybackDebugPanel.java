package com.tunefold.app;

import android.graphics.Color;
import android.text.TextUtils;
import android.util.TypedValue;
import android.view.View;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * Technical diagnostics for a playback, kept OUT of the normal Now Playing
 * surface.
 *
 * <p>The main screen shows musical content and playback state; every Range
 * request, probe duration and TTFA belongs here, reachable only on demand.
 */
final class PlaybackDebugPanel {
    private final LinearLayout root;
    private final TextView body;
    private boolean visible;

    PlaybackDebugPanel(android.content.Context context) {
        root = new LinearLayout(context);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setVisibility(View.GONE);

        TextView header = new TextView(context);
        header.setText("Diagnostics");
        header.setTextSize(TypedValue.COMPLEX_UNIT_SP, 13);
        header.setTextColor(Color.parseColor("#5B6472"));
        root.addView(header, new LinearLayout.LayoutParams(-1, -2));

        body = new TextView(context);
        body.setTextSize(TypedValue.COMPLEX_UNIT_SP, 11);
        body.setTextColor(Color.parseColor("#2A3140"));
        body.setTextIsSelectable(true);
        // Long traces must wrap rather than push the layout sideways.
        body.setMaxLines(80);
        body.setEllipsize(TextUtils.TruncateAt.END);
        root.addView(body, new LinearLayout.LayoutParams(-1, 0, 1f));
    }

    View view() { return root; }

    boolean isVisible() { return visible; }

    void toggle() {
        visible = !visible;
        root.setVisibility(visible ? View.VISIBLE : View.GONE);
    }

    /** Replaces the diagnostics text; the trace keeps its own ordering. */
    void update(String diagnostics, PlaybackTrace trace) {
        StringBuilder text = new StringBuilder();
        if (diagnostics != null && !diagnostics.isEmpty()) text.append(diagnostics);
        if (trace != null) {
            String summary = trace.summary();
            if (!summary.isEmpty()) {
                if (text.length() > 0) text.append("\n\n");
                text.append("Timeline (since Play tap)\n");
                text.append(summary);
            }
        }
        body.setText(text.toString());
    }
}