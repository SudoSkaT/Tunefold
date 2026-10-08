package com.tunefold.app;

import android.content.Context;
import android.content.res.ColorStateList;
import android.graphics.drawable.RippleDrawable;
import android.text.TextUtils;
import android.view.View;
import android.widget.ImageView;

/**
 * An icon-only action: a fixed square target, a glyph, and a semantic description.
 *
 * <p>This is the replacement for the text buttons the surface used to be built from.
 * Three things are correct by construction rather than by review:
 *
 * <ul>
 *   <li>the touch target is never below {@link DesignTokens#TOUCH_TARGET_MIN}, because
 *       the height comes from the token, not from a caller;</li>
 *   <li>the {@code contentDescription} is set in the same call as the icon
 *       ({@link #setAction}), so an icon can never ship with a missing or stale
 *       description, and it can describe the <i>action</i> ("Pause playback") rather
 *       than the picture ("pause icon");</li>
 *   <li>pressed, disabled and selected colours all come from one table
 *       ({@link DesignTokens#controlColor}), so one control cannot end up with a
 *       disabled state the others forgot.</li>
 * </ul>
 *
 * <p>One view, no children and no listener indirection: the drawable is only
 * replaced when the icon actually changes, and the tint is recomputed only when the
 * drawable state actually changes, so a redraw is not paid for by a setText that
 * would have been a no-op anyway.
 */
class IconButton extends ImageView implements ArtworkTheme.AccentListener {

    private Icon currentIcon;
    private IconDrawable drawable;
    private CharSequence description = "";
    private int glyphColor = DesignTokens.Palette.ON_SURFACE;
    private int paintedColor = 0;
    private boolean toneIsAccent;

    IconButton(Context context) {
        this(context, DesignTokens.CONTROL_TARGET, DesignTokens.ICON_SIZE_MEDIUM);
    }

    IconButton(Context context, float targetDp, float iconDp) {
        super(context);
        DesignTokens.init(context);
        int target = DesignTokens.dp(targetDp);
        int inset = Math.max(0, (target - DesignTokens.dp(iconDp)) / 2);
        setPadding(inset, inset, inset, inset);
        setMinimumWidth(target);
        setMinimumHeight(target);
        setScaleType(ScaleType.FIT_CENTER);
        setClickable(true);
        setFocusable(true);
        // A button's own text was the accessible label before; with an icon it must be
        // supplied explicitly, and setAction is the only way to supply it.
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_YES);
        setBackground(ripple(DesignTokens.Palette.RIPPLE, false));
        // Deliberately NOT painted here. {@link #paint()} dispatches to
        // {@link #resolveGlyphColor()}, which a subclass may override in terms of its
        // own fields — and a superclass constructor runs before those initializers, so
        // painting from here would read them as null. The first paint happens on
        // attach, once construction is complete.
    }

    /** Circular ripple for a control that paints its own circular surface. */
    static RippleDrawable ripple(int rippleColor, boolean circular) {
        android.graphics.drawable.Drawable mask = null;
        if (circular) {
            android.graphics.drawable.Drawable circle =
                    new android.graphics.drawable.ColorDrawable(0xFFFFFFFF);
            circle.setBounds(0, 0, 100, 100);
            mask = circle;
        }
        return new RippleDrawable(ColorStateList.valueOf(rippleColor), null, mask);
    }

    /**
     * Sets the icon and the action's description together.
     *
     * <p>One call by design: an icon without a description is invisible to a screen
     * reader, and a description that outlives the icon is worse, because it advertises
     * the wrong action. Use this for actions whose meaning never changes; a control
     * whose description depends on its state sets them separately through
     * {@link #setIcon} and {@link #applyDescription}.
     *
     * <p>The drawable is rebuilt only when the icon actually differs, so a state render
     * that re-sets the same icon allocates nothing.
     */
    final void setAction(Icon icon, CharSequence description) {
        setIcon(icon);
        applyDescription(description);
    }

    /**
     * Replaces the glyph, leaving the description to the caller.
     *
     * <p>The drawable is rebuilt only when the icon actually differs, so a state render
     * that re-sets the same icon allocates nothing and does not touch the picture.
     */
    final void setIcon(Icon icon) {
        if (icon == null || currentIcon == icon) return;
        currentIcon = icon;
        drawable = new IconDrawable(icon);
        setImageDrawable(drawable);
        paint();
    }

    /**
     * Sets only the description, for controls whose icon has not changed.
     *
     * <p>Guarded: {@code setContentDescription} announces an accessibility event, so
     * re-announcing the same words on every tick would make a state change unusable
     * with a screen reader.
     */
    final void applyDescription(CharSequence description) {
        if (TextUtils.equals(getContentDescription(), description)) return;
        this.description = description;
        setContentDescription(description);
        if (description != null && description.length() > 0) {
            setTooltipText(description);
        } else {
            setTooltipText(null);
        }
    }

    /** Description last applied, for a caller composing a new one. */
    final CharSequence description() { return description; }

    /** The drawable, so a control can drive its own state feedback on the glyph. */
    final IconDrawable iconDrawable() { return drawable; }

    /** The icon currently shown, or {@code null} before the first {@link #setAction}. */
    final Icon icon() { return currentIcon; }

    /** Marks the control as representing an enabled setting; tints it with the accent. */
    final void setToggleState(boolean on) {
        if (isActivated() == on) return;
        setActivated(on);
        paint();
    }

    /** Marks the control's destination as the active one. */
    final void setActiveDestination(boolean active) {
        if (isSelected() == active) return;
        setSelected(active);
        paint();
    }

    @Override public void setEnabled(boolean enabled) {
        if (isEnabled() == enabled) return;
        super.setEnabled(enabled);
        paint();
    }

    @Override protected void drawableStateChanged() {
        super.drawableStateChanged();
        paint();
    }

    @Override protected void onAttachedToWindow() {
        super.onAttachedToWindow();
        // Registered on attach, not in the constructor, and released on detach. A
        // static listener list that a constructor writes to is a leak: a control built
        // and then thrown away — a row returned to a pool, a section rebuilt — would
        // stay reachable forever, holding its Activity through its context.
        ArtworkTheme.register(this);
        // First paint, once every subclass field is initialized. setIcon also paints, so
        // this only covers a control whose icon was set before it was ever shown.
        paint();
    }

    @Override protected void onDetachedFromWindow() {
        ArtworkTheme.unregister(this);
        super.onDetachedFromWindow();
    }

    @Override public void onAccentChanged(int accent) {
        paint();
    }

    /**
     * Repaints the glyph, and only if its colour really moved.
     *
     * <p>This runs on every drawable-state change, so the equality guard is what
     * keeps a state render from invalidating a view that already looks right.
     */
    final void paint() {
        int next = resolveGlyphColor();
        boolean accentSurface = usesAccentSurface();
        if (next == paintedColor && accentSurface == toneIsAccent) return;
        paintedColor = next;
        toneIsAccent = accentSurface;
        if (drawable != null) {
            drawable.setColor(next);
            drawable.setRingColor(accentSurface ? next : ArtworkTheme.accent());
        }
        applyRipple();
        invalidate();
    }

    /**
     * Drops this control's own touch target, for use inside a larger clickable parent.
     *
     * <p>A {@link NavigationItem} slot is the target and carries the caption; an inner
     * button that also reserved 56dp would claim the whole slot's height and squeeze
     * the caption out of existence. The target is not weakened — it moves to the parent.
     */
    final void asGlyph() {
        setMinimumWidth(0);
        setMinimumHeight(0);
        setPadding(0, 0, 0, 0);
    }

    /** Colour of the glyph in the current state. */
    int resolveGlyphColor() {
        return DesignTokens.controlColor(isEnabled(), isSelected(), isActivated(),
                ArtworkTheme.accent());
    }

    /** True when the control paints on the accent surface rather than over a card. */
    boolean usesAccentSurface() { return false; }

    /** Ripple colour for this control's surface. */
    int rippleColor() { return DesignTokens.Palette.RIPPLE; }

    private void applyRipple() {
        int color = rippleColor();
        if (getBackground() instanceof RippleDrawable) {
            ((RippleDrawable) getBackground()).setColor(ColorStateList.valueOf(color));
        }
    }
}