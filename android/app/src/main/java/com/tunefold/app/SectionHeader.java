package com.tunefold.app;

import android.content.Context;
import android.text.TextUtils;
import android.util.TypedValue;
import android.widget.TextView;

/**
 * A block title inside the page: "Continue listening", "L1K3D (12)".
 *
 * <p>The count is part of the title rather than a separate view, because a count
 * changes far more often than the heading around it and would otherwise force a
 * second measurement on every like toggle.
 *
 * <p>{@link #setTitle} writes only when the composed string actually differs, so
 * refreshing a section on every tick costs one comparison and no allocation.
 */
final class SectionHeader extends TextView {

    /** No count is shown. */
    static final int NO_COUNT = -1;

    /**
     * Per-header composition buffer.
     *
     * <p>Reused and cleared, so the common case — the same header refreshed
     * repeatedly — produces no garbage. The result is copied out as an immutable
     * {@code String}, which is what the TextView keeps, so nothing downstream can see
     * it mutate.
     */
    private final StringBuilder scratch = new StringBuilder(48);

    SectionHeader(Context context) {
        super(context);
        DesignTokens.init(context);
        setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_LABEL_SIZE);
        setTextColor(DesignTokens.Palette.ON_SURFACE_MUTED);
        setLetterSpacing(DesignTokens.TYPE_LABEL_TRACKING);
        setAllCaps(true);
        setMaxLines(1);
        setEllipsize(TextUtils.TruncateAt.END);
        setIncludeFontPadding(false);
    }

    /**
     * Shows {@code title}, optionally followed by {@code count} in brackets.
     *
     * @param count a non-negative count, or {@link #NO_COUNT} to omit it
     */
    void setTitle(CharSequence title, int count) {
        scratch.setLength(0);
        scratch.append(title);
        if (count != NO_COUNT) {
            scratch.append(" (").append(count).append(')');
        }
        if (matches(scratch)) return;
        setText(scratch.toString());
    }

    private boolean matches(StringBuilder candidate) {
        return TextUtils.equals(getText(), candidate);
    }
}