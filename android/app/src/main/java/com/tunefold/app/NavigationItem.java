package com.tunefold.app;

import android.content.Context;
import android.text.TextUtils;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.ViewOutlineProvider;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * One destination in the app's navigation bar.
 *
 * <p>An icon slot that also carries the destination's <i>name</i> as a caption,
 * because a glyph alone is not a navigation label: "L1K3D" is a product word the user
 * has to read, not a symbol they can guess. The caption costs one {@link TextView}
 * per slot, built once and never rebuilt, and the caption is what the accessibility
 * description is derived from.
 *
 * <p>Selection is state, not a second view: {@link #setActiveDestination} repaints
 * the glyph and the caption, so a section switch touches four captions and four
 * glyphs instead of rebuilding the bar.
 */
@android.annotation.SuppressLint("ViewConstructor")
final class NavigationItem extends LinearLayout implements ArtworkTheme.AccentListener {

    private final IconButton icon;
    private final TextView caption;
    private final CharSequence label;

    NavigationItem(Context context, Icon glyph, CharSequence label) {
        super(context);
        DesignTokens.init(context);
        this.label = label;
        setOrientation(VERTICAL);
        setGravity(Gravity.CENTER);
        // The whole slot is the target, not just the glyph, which is what makes a
        // 20dp glyph comfortably tappable.
        int target = DesignTokens.dp(DesignTokens.CONTROL_TARGET_NAVIGATION);
        setMinimumHeight(target);
        setClickable(true);
        setFocusable(true);
        setBackground(IconButton.ripple(DesignTokens.Palette.RIPPLE, false));
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_YES);

        icon = new IconButton(context, DesignTokens.CONTROL_TARGET_NAVIGATION,
                DesignTokens.ICON_SIZE_SMALL);
        // The slot owns the click and the description, so the inner button must not
        // also be focusable: two nodes describing one destination is worse for a
        // screen reader than one node describing it well.
        icon.setClickable(false);
        icon.setFocusable(false);
        icon.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
        icon.setBackground(null);
        // The slot is the target; the inner button is only a glyph. Without this the
        // button reserves a full 56dp height and pushes the caption out of the slot.
        icon.asGlyph();
        icon.setAction(glyph, label);
        int iconSide = DesignTokens.dp(DesignTokens.ICON_SIZE_SMALL);
        LayoutParams iconParams = new LayoutParams(iconSide, iconSide);
        addView(icon, iconParams);

        caption = new TextView(context);
        caption.setText(label);
        caption.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_LABEL_SIZE);
        caption.setGravity(Gravity.CENTER);
        caption.setLetterSpacing(DesignTokens.TYPE_LABEL_TRACKING);
        caption.setMaxLines(1);
        caption.setEllipsize(TextUtils.TruncateAt.END);
        caption.setIncludeFontPadding(false);
        caption.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
        LayoutParams captionParams = new LayoutParams(LayoutParams.WRAP_CONTENT,
                LayoutParams.WRAP_CONTENT);
        captionParams.topMargin = DesignTokens.dp(DesignTokens.SPACE_1);
        addView(caption, captionParams);

        applyState(false);
    }

    @Override protected void onAttachedToWindow() {
        super.onAttachedToWindow();
        // Registered on attach so a slot that is built and dropped never stays
        // reachable from the process-wide listener list.
        ArtworkTheme.register(this);
    }

    /** The glyph, so a caller can swap it without rebuilding the slot. */
    IconButton icon() { return icon; }

    /** This slot's name. */
    CharSequence label() { return label; }

    /** Selects this destination; the accent follows the glyph and the caption. */
    void setActiveDestination(boolean active) {
        if (isSelected() == active) return;
        setSelected(active);
        applyState(active);
    }

    @Override public void onAccentChanged(int accent) { applyState(isSelected()); }

    @Override protected void onDetachedFromWindow() {
        ArtworkTheme.unregister(this);
        super.onDetachedFromWindow();
    }

    private void applyState(boolean active) {
        int color = active ? ArtworkTheme.accent() : DesignTokens.Palette.ON_SURFACE_MUTED;
        icon.setActiveDestination(active);
        caption.setTextColor(color);
        // One node, one description, and it states where the tap goes.
        setContentDescription(active ? label + ", selected" : label);
    }

    /** Rounds the slot so its ripple follows the bar it sits in. */
    void roundRipple() {
        setOutlineProvider(new ViewOutlineProvider() {
            @Override public void getOutline(android.view.View view,
                                              android.graphics.Outline outline) {
                int radius = DesignTokens.dp(DesignTokens.RADIUS_MEDIUM);
                outline.setRoundRect(0, 0, view.getWidth(), view.getHeight(), radius);
            }
        });
        setClipToOutline(true);
    }
}