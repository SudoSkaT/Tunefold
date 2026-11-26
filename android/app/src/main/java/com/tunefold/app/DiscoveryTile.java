package com.tunefold.app;

import android.content.Context;
import android.graphics.drawable.GradientDrawable;
import android.view.Gravity;
import android.view.View;
import android.widget.FrameLayout;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * A selectable tile for musical discovery: cover, title, optional subtitle and
 * category. Reuses the existing tokens and ArtworkView so covers are decoded at
 * the size they are drawn at.
 */
@android.annotation.SuppressLint("ViewConstructor")
final class DiscoveryTile extends LinearLayout {

    enum Variant { FEATURED, COMPACT }

    private final ArtworkView artwork;
    private final TextView title;
    private final TextView subtitle;
    private final TextView category;
    private Variant variant = Variant.COMPACT;

    DiscoveryTile(Context context) {
        super(context);
        DesignTokens.init(context);
        setOrientation(VERTICAL);
        setGravity(Gravity.START);
        setClickable(true);
        setFocusable(true);
        setMinimumWidth(DesignTokens.dp(48));
        setMinimumHeight(DesignTokens.dp(48));
        int padding = DesignTokens.dp(DesignTokens.SPACE_2);
        setPadding(padding, padding, padding, padding);

        artwork = new ArtworkView(context);
        artwork.setCornerRadiusDp(DesignTokens.RADIUS_MEDIUM);
        addView(artwork, new LayoutParams(LayoutParams.MATCH_PARENT, DesignTokens.dp(120)));

        title = new TextView(context);
        title.setTextSize(android.util.TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_BODY_SIZE);
        title.setTextColor(DesignTokens.Palette.ON_SURFACE);
        title.setMaxLines(2);
        title.setEllipsize(android.text.TextUtils.TruncateAt.END);
        LayoutParams titleParams = new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT);
        titleParams.topMargin = DesignTokens.dp(DesignTokens.SPACE_1);
        addView(title, titleParams);

        subtitle = new TextView(context);
        subtitle.setTextSize(android.util.TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_CAPTION_SIZE);
        subtitle.setTextColor(DesignTokens.Palette.ON_SURFACE_MUTED);
        subtitle.setMaxLines(1);
        subtitle.setEllipsize(android.text.TextUtils.TruncateAt.END);
        addView(subtitle, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));

        category = new TextView(context);
        category.setTextSize(android.util.TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_LABEL_SIZE);
        category.setTextColor(DesignTokens.Palette.ON_SURFACE_FAINT);
        category.setMaxLines(1);
        addView(category, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));

        applyVariant();
    }

    void setVariant(Variant value) {
        variant = value == null ? Variant.COMPACT : value;
        applyVariant();
    }

    Variant variant() { return variant; }

    void setTitle(CharSequence value) { title.setText(value == null ? "" : value); updateDescription(); }
    void setSubtitle(CharSequence value) { subtitle.setText(value == null ? "" : value); updateDescription(); }
    void setCategory(CharSequence value) { category.setText(value == null ? "" : value); updateDescription(); }

    ArtworkView artwork() { return artwork; }

    void setCover(String identity, android.graphics.Bitmap bitmap) {
        if (identity != null) artwork.expectCover(identity);
        if (bitmap != null) artwork.setCover(identity, bitmap);
    }

    private void updateDescription() {
        StringBuilder sb = new StringBuilder();
        if (title.getText().length() > 0) sb.append(title.getText());
        if (subtitle.getText().length() > 0) { if (sb.length() > 0) sb.append(", "); sb.append(subtitle.getText()); }
        if (category.getText().length() > 0) { if (sb.length() > 0) sb.append(", "); sb.append(category.getText()); }
        setContentDescription(sb.toString());
    }

    private void applyVariant() {
        int side = variant == Variant.FEATURED ? DesignTokens.dp(160) : DesignTokens.dp(120);
        LayoutParams params = (LayoutParams) artwork.getLayoutParams();
        params.height = side;
        artwork.setLayoutParams(params);
        title.setTextSize(android.util.TypedValue.COMPLEX_UNIT_SP,
                variant == Variant.FEATURED ? DesignTokens.TYPE_TITLE_SIZE : DesignTokens.TYPE_BODY_SIZE);
    }
}
