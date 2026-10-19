package com.tunefold.app;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.Rect;
import android.text.TextUtils;
import android.view.View;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * The three shelves — Sigue, Recomendados, Radio — as a tab strip that marks its
 * selection with a <b>rule</b> and not only with a colour.
 *
 * <p>That is the whole reason this is a class. Colour alone is not an accessible
 * way to say "this is the one you are looking at": a user who cannot separate the
 * selected caption's hue from the unselected one is left with three equal-looking
 * buttons and no way to know what is on screen. A printed rule under the selected
 * segment says it in a second channel — position and shape — which is what a tab
 * strip on a sleeve-back or a mixing desk has always used.
 *
 * <p>It is one view with no children of its own: the tabs are still
 * {@link TextView}s the caller created and configured, because the player owns
 * their wording and their actions. The bar only paints, which is why the rule costs
 * one {@code drawRect} and no allocation: the rectangle is a field, the paint is a
 * field, and {@link #onDraw} decides nothing.
 *
 * <p>The rule follows the artwork-derived accent for the same reason the rest of
 * the accent does, and it is drawn even when the palette is the brand's own, so
 * selection looks the same whichever colour is in force.
 */
@android.annotation.SuppressLint("ViewConstructor")
final class ShelfTabBar extends LinearLayout implements ArtworkTheme.AccentListener {

    /** The rule under the selected shelf; it is a rule, so it stays thin. */
    private final Paint indicator = new Paint(Paint.ANTI_ALIAS_FLAG);
    /** Where the rule goes, reused so drawing never allocates. */
    private final Rect mark = new Rect();

    /** Index of the selected tab among this bar's children; -1 for none. */
    private int selected = -1;
    /** Slot width the captions were last fitted to; -1 before the first. */
    private int fittedSlot = -1;
    /** Air kept free inside a slot so a caption never touches its neighbours' edge. */
    private static final int SIDE_PADDING_DP = 8;
    /** Air between a tab's glyph and its caption. */
    private static final float GLYPH_GAP_DP = 3f;

    ShelfTabBar(Context context) {
        super(context);
        DesignTokens.init(context);
        setOrientation(HORIZONTAL);
        setGravity(android.view.Gravity.CENTER);
        // This bar draws. Without the flag the platform skips onDraw on a container
        // that has no background of its own, and the rule simply never appears —
        // a selection marker that does not render is worse than none, because
        // everything else about the selected tab is still saying "selected".
        setWillNotDraw(false);
        indicator.setColor(ArtworkTheme.accent());
    }

    /**
     * Fits each caption into its own third of the strip.
     *
     * <p>Three segments in a card that has already lost a page gutter and the card's
     * padding is about ninety-six dp a piece, and "Recomendados" at the label size
     * is a hundred and ten. It did not fit, and neither truncating nor letting the
     * platform shrink it helped: {@code setAutoSizeTextTypeUniform…} only shrinks
     * under an unbounded measurement, and a weighted row measures its children
     * exactly, so the label arrived one letter short and clipped.
     *
     * <p>So the rule is applied here, where the number is known: each caption takes
     * the largest size in its own range that fits its slot. A shorter word in the
     * same range — most of them — is left at full size, and a long one becomes a
     * smaller word rather than a broken one. Memoised on the slot width, because a
     * measure pass runs on every scroll-driven layout and the answer only moves
     * when the window does.
     */
    @Override protected void onMeasure(int widthMeasureSpec, int heightMeasureSpec) {
        int width = MeasureSpec.getSize(widthMeasureSpec);
        int count = getChildCount();
        if (count > 0 && width > 0) fitCaptions(width / count);
        super.onMeasure(widthMeasureSpec, heightMeasureSpec);
    }

    private void fitCaptions(int slotPx) {
        if (slotPx == fittedSlot || slotPx <= 0) return;
        fittedSlot = slotPx;
        int available = slotPx - DesignTokens.dp(SIDE_PADDING_DP);
        if (available <= 0) return;
        for (int index = 0; index < getChildCount(); index++) {
            TextView tab = captionOf(getChildAt(index));
            if (tab == null) continue;
            CharSequence text = tab.getText();
            if (text == null || text.length() == 0) continue;
            android.text.TextPaint paint = tab.getPaint();
            float size = DesignTokens.TYPE_LABEL_SIZE;
            if (!fits(paint, text, size, available)) {
                while (size > DesignTokens.SHELF_TAB_MIN_SIZE_SP) {
                    size -= 0.5f;
                    if (fits(paint, text, size, available)) break;
                }
                tab.setTextSize(android.util.TypedValue.COMPLEX_UNIT_SP, size);
            } else if (tab.getTextSize() != sp(size)) {
                // Back to full size after a window change; the caption is shorter
                // than the slot again and must not stay shrunk.
                tab.setTextSize(android.util.TypedValue.COMPLEX_UNIT_SP, size);
            }
        }
    }

    /**
     * The caption inside a tab, or {@code null} if this child is not a tab.
     *
     * <p>A tab stopped being a bare {@link TextView} when it grew a glyph, so both
     * the fitter and the bar's own description ask through here instead of each
     * casting and hoping.
     */
    private static TextView captionOf(View child) {
        if (child instanceof TextView) return (TextView) child;
        if (!(child instanceof LinearLayout)) return null;
        for (int index = 0; index < ((LinearLayout) child).getChildCount(); index++) {
            View grandchild = ((LinearLayout) child).getChildAt(index);
            if (grandchild instanceof TextView) return (TextView) grandchild;
        }
        return null;
    }

    /** Whether {@code text} at {@code sizeSp} fits {@code available} pixels. */
    private static boolean fits(android.text.TextPaint paint, CharSequence text,
            float sizeSp, int available) {
        paint.setTextSize(sp(sizeSp));
        return paint.measureText(text, 0, text.length()) <= available;
    }

    private static float sp(float value) { return DesignTokens.sp(value); }

    /**
     * Selects one of this bar's tabs.
     *
     * <p>Guarded: the label of the selected tab is written from the same call that
     * changes this index, and a session that notifies twice a second would
     * otherwise invalidate a strip whose words have not moved.
     */
    void setSelectedIndex(int index) {
        if (selected == index) return;
        selected = index;
        paintTabs();
        invalidate();
    }

    /**
     * Pinta cada pestaña según si es la seleccionada.
     *
     * <p>El glifo y la palabra cambian de tono a la vez. Con sólo la palabra, la
     * única señal de estado era la regla de arriba, y el glifo nuevo hacía
     * falta para eso: un icono que no dice si está activo se lee como
     * decoración, que es justo lo contrario de lo que se le pidió al añadirlo.
     */
    private void paintTabs() {
        for (int index = 0; index < getChildCount(); index++) {
            boolean active = index == selected;
            TextView caption = captionOf(getChildAt(index));
            if (caption != null) {
                caption.setTextColor(active ? DesignTokens.Palette.ACCENT_TEXT
                        : DesignTokens.Palette.ON_SURFACE_MUTED);
            }
            ImageView glyph = glyphOf(getChildAt(index));
            if (glyph != null && glyph.getDrawable() instanceof IconDrawable) {
                ((IconDrawable) glyph.getDrawable()).setColor(active
                        ? ArtworkTheme.accent() : DesignTokens.Palette.ON_SURFACE_FAINT);
            }
        }
    }

    private static ImageView glyphOf(View child) {
        if (!(child instanceof LinearLayout)) return null;
        for (int index = 0; index < ((LinearLayout) child).getChildCount(); index++) {
            View grandchild = ((LinearLayout) child).getChildAt(index);
            if (grandchild instanceof ImageView) return (ImageView) grandchild;
        }
        return null;
    }

    /**
     * Cambia la palabra de una pestaña sin tocar su glifo.
     *
     * <p>La radio cambia de nombre con el tiempo —«Radio» mientras la station se
     * arma, el nombre de la estación cuando ya suena— y ese cambio es del
     * reproductor, no de la barra. Se expone aquí y no como un {@code TextView}
     * público para que la barra siga siendo dueña de cómo se pinta una pestaña: dos
     * sitios pintando el mismo estado es exactamente como el estado se desincroniza
     * de lo que se ve.
     */
    void setCaption(int index, CharSequence text) {
        if (index < 0 || index >= getChildCount()) return;
        View child = getChildAt(index);
        TextView caption = captionOf(child);
        if (caption == null) return;
        if (!TextUtils.equals(caption.getText(), text)) caption.setText(text);
        if (!TextUtils.equals(child.getContentDescription(), text)) {
            child.setContentDescription(text);
        }
    }

    /** Which tab is selected, or {@code -1} when the strip is empty. */
    int selectedIndex() { return selected; }

    /** The caption of the selected tab, for the strip's own description. */
    CharSequence selectedLabel() {
        View child = selected >= 0 && selected < getChildCount()
                ? getChildAt(selected) : null;
        return child == null ? "" : child.getContentDescription();
    }

    @Override protected void onAttachedToWindow() {
        super.onAttachedToWindow();
        ArtworkTheme.register(this);
    }

    @Override protected void onDetachedFromWindow() {
        ArtworkTheme.unregister(this);
        super.onDetachedFromWindow();
    }

    @Override public void onAccentChanged(int accent) {
        indicator.setColor(accent);
        paintTabs();
        invalidate();
    }

    /**
     * The rule above the selected shelf.
     *
     * <p>Above, not under: the top edge of the strip is the only edge that is
     * unambiguously inside it — a rule drawn at a child's bottom lands exactly on
     * the strip's own bottom edge, where it is one pixel of clipping away from
     * not being drawn at all. It also puts both of the app's "you are here" marks
     * in the same place: the selected destination and the selected shelf are both
     * a gold rule at the top of their slot, so the idea is one idea.
     *
     * <p>Drawn from {@link #onDraw} rather than from a child view: a marker that
     * exists or does not exist costs a visibility change and a layout, and this is
     * one rectangle on a paint that is already there.
     */
    @Override protected void onDraw(Canvas canvas) {
        super.onDraw(canvas);
        View child = selected >= 0 && selected < getChildCount()
                ? getChildAt(selected) : null;
        if (child == null || child.getVisibility() != VISIBLE) return;
        int inset = DesignTokens.dp(SIDE_PADDING_DP);
        int left = child.getLeft() + inset;
        int right = child.getRight() - inset;
        int thickness = Math.max(1, DesignTokens.dp(DesignTokens.RULE_THICKNESS));
        if (right - left <= 0) return;
        mark.set(left, 0, right, thickness);
        canvas.drawRect(mark, indicator);
    }

    /**
     * One tab segment: a glyph, the word under it, and one node for both.
     *
     * <p>The glyph is what identifies the shelf; the word only names it. Three
     * words on their own is three things the eye has to read, and "Radio" next to
     * "Recomendados" next to "Siguiente" tells a first-time listener nothing about
     * what tapping any of them will show. A glyph is read before it is read.
     *
     * <p>Both halves live in the tab's own node, so a screen reader announces one
     * thing and a tap lands on both: a tab split across two nodes is two targets
     * for one action, and the ripple only covers half of what looks like the button.
     *
     * <p>The label is muted linen rather than gold. The strip's selected state is
     * carried by the rule and by the glyph's own accent, and gold on the caption
     * too would be the same fact spent three times.
     */
    LinearLayout tab(String text, Icon glyph) {
        LinearLayout tab = new LinearLayout(getContext());
        tab.setOrientation(VERTICAL);
        tab.setGravity(android.view.Gravity.CENTER);
        tab.setMinimumHeight(DesignTokens.dp(DesignTokens.CONTROL_TARGET));
        tab.setClickable(true);
        tab.setFocusable(true);
        tab.setContentDescription(text);
        tab.setBackground(IconButton.ripple(DesignTokens.Palette.RIPPLE, false));

        int glyphSize = DesignTokens.dp(DesignTokens.ICON_SIZE_SMALL);
        ImageView icon = new ImageView(getContext());
        icon.setImageDrawable(new IconDrawable(glyph));
        icon.setColorFilter(DesignTokens.Palette.ON_SURFACE_FAINT,
                android.graphics.PorterDuff.Mode.SRC_IN);
        icon.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
        tab.addView(icon, new LinearLayout.LayoutParams(glyphSize, glyphSize));

        TextView caption = new TextView(getContext());
        caption.setText(text);
        caption.setTextSize(android.util.TypedValue.COMPLEX_UNIT_SP,
                DesignTokens.TYPE_LABEL_SIZE);
        caption.setTextColor(DesignTokens.Palette.ON_SURFACE_MUTED);
        caption.setLetterSpacing(DesignTokens.TYPE_LABEL_TRACKING);
        caption.setAllCaps(true);
        caption.setGravity(android.view.Gravity.CENTER);
        caption.setMaxLines(1);
        caption.setSingleLine(true);
        caption.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
        LinearLayout.LayoutParams captionParams = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT,
                LinearLayout.LayoutParams.WRAP_CONTENT);
        captionParams.topMargin = DesignTokens.dp(GLYPH_GAP_DP);
        tab.addView(caption, captionParams);
        return tab;
    }

    /** One tab, a third of the strip wide. */
    LinearLayout.LayoutParams tabParams() {
        return new LinearLayout.LayoutParams(0, LayoutParams.WRAP_CONTENT, 1f);
    }
}