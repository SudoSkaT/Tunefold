package com.tunefold.app;

import android.content.Context;
import android.text.TextUtils;
import android.util.TypedValue;
import android.view.Gravity;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * A block title inside the page: "Continue listening", "L1K3D (12)", and the one
 * control that takes the block where it is complete.
 *
 * <p>The count is part of the title rather than a separate view, because a count
 * changes far more often than the heading around it and would otherwise force a
 * second measurement on every like toggle.
 *
 * <p>It also carries an <b>action</b>, and that is the point. A block that shows a
 * handful of something and gives no way to reach the rest of it is a sample of a list
 * the user has to go and find by hand: the heading is the cheapest control on the
 * screen to make the way out of the block, and it is already in the reading order of
 * the thing it summarises. So the heading says where it goes and goes there.
 *
 * <p>Both texts are written only when the composed string actually differs, so
 * refreshing a section on every tick costs one comparison and no allocation.
 */
final class SectionHeader extends LinearLayout {

    /** No count is shown. */
    static final int NO_COUNT = -1;

    private final TextView title;
    private final TextView action;

    /**
     * Per-header composition buffer.
     *
     * <p>Reused and cleared, so the common case — the same header refreshed
     * repeatedly — produces no garbage. The result is copied out as an immutable
     * {@code String}, which is what the TextView keeps, so nothing downstream can see
     * it mutate.
     */
    private final StringBuilder scratch = new StringBuilder(48);
    private CharSequence renderedTitle = "";
    private CharSequence renderedAction = "";
    private Runnable actionTarget;

    SectionHeader(Context context) {
        super(context);
        DesignTokens.init(context);
        setOrientation(HORIZONTAL);
        setGravity(Gravity.CENTER_VERTICAL);

        title = label(context, DesignTokens.TYPE_LABEL_SIZE,
                DesignTokens.Palette.ON_SURFACE_MUTED);
        addView(title, new LayoutParams(0, LayoutParams.WRAP_CONTENT, 1f));

        // The action is a control and says so: a 48dp tall target, all-caps like the
        // heading it belongs to, and a description for anyone not reading the glyph.
        action = label(context, DesignTokens.TYPE_LABEL_SIZE,
                DesignTokens.Palette.ACCENT);
        action.setVisibility(GONE);
        action.setGravity(Gravity.CENTER);
        LayoutParams actionParams = new LayoutParams(LayoutParams.WRAP_CONTENT,
                DesignTokens.dp(DesignTokens.CONTROL_TARGET));
        addView(action, actionParams);
        setOnClickListener(view -> {
            Runnable target = actionTarget;
            if (target != null) target.run();
        });
    }

    /**
     * Shows {@code title}, optionally followed by {@code count} in brackets.
     *
     * @param count a non-negative count, or {@link #NO_COUNT} to omit it
     */
    void setTitle(CharSequence value, int count) {
        scratch.setLength(0);
        scratch.append(value);
        if (count != NO_COUNT) {
            scratch.append(" (").append(count).append(')');
        }
        CharSequence composed = scratch.toString();
        if (!TextUtils.equals(composed, renderedTitle)) {
            renderedTitle = composed;
            title.setText(composed);
        }
    }

    /**
     * Where this block leads, or {@code null} for a block that goes nowhere.
     *
     * <p>The whole header becomes the target, not only the words on the right: a
     * heading is the thing the finger was aimed at anyway.
     *
     * @param label what the action offers; {@code null} removes it
     */
    void setAction(CharSequence label, Runnable target) {
        actionTarget = target;
        if (TextUtils.equals(label, renderedAction)) return;
        renderedAction = label == null ? "" : label;
        if (label == null) {
            action.setText("");
            action.setVisibility(GONE);
            setClickable(false);
            setContentDescription(null);
            return;
        }
        action.setText(label);
        action.setVisibility(VISIBLE);
        setClickable(true);
        setContentDescription(label);
    }

    /** Whether this header currently leads anywhere. */
    boolean hasAction() { return actionTarget != null; }

    private static TextView label(Context context, float sizeSp, int color) {
        TextView view = new TextView(context);
        view.setTextSize(TypedValue.COMPLEX_UNIT_SP, sizeSp);
        view.setTextColor(color);
        view.setAllCaps(true);
        view.setMaxLines(1);
        view.setEllipsize(TextUtils.TruncateAt.END);
        view.setIncludeFontPadding(false);
        return view;
    }
}