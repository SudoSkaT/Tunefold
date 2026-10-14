package com.tunefold.app;

import android.content.Context;
import android.text.TextUtils;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * One section's worth of screen, with the four states a screen can be in.
 *
 * <p>A list that is empty and a list that is loading and a list that failed look the
 * same unless something says otherwise, and the usual result is a blank half of the
 * screen with no explanation. This owns that decision: {@link #show} takes the phase
 * and the words, and the same line of code cannot forget one of them.
 *
 * <p>The phase itself is {@link Phase}, a pure function of what is known, because
 * "which of the four am I" is the part that has to be right and is the part a reviewer
 * can check without a device.
 *
 * <p>The message sits inside the content flow rather than over it: a section in a
 * state other than CONTENT has nothing to show, so there is nothing to overlap.
 */
@android.annotation.SuppressLint("ViewConstructor")
final class SectionState extends LinearLayout {

    /** What a section is showing right now. */
    enum Phase {
        /** A request is in flight and nothing is known yet. */
        LOADING,
        /** There is something to show. */
        CONTENT,
        /** The request succeeded and there is nothing. */
        EMPTY,
        /** The request failed. */
        ERROR
    }

    /** Told when a state offers its one action, e.g. "retry". */
    interface ActionHost {
        /** The section's single recovery action; {@code null} hides it. */
        void onSectionAction();
    }

    private final TextView message;
    private final IconButton action;
    private final View content;
    private ActionHost actionHost;

    private Phase phase;
    private CharSequence renderedText = "";
    private CharSequence renderedAction = "";
    private boolean renderedActionVisible;
    /** What the one action offers; a retry is the default, not the only option. */
    private Icon actionIcon = Icon.RETRY;
    private CharSequence actionDescription = "Try again";

    SectionState(Context context, View content) {
        super(context);
        DesignTokens.init(context);
        this.content = content;
        setOrientation(VERTICAL);
        setGravity(Gravity.CENTER_HORIZONTAL);

        message = new TextView(context);
        message.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_BODY_SIZE);
        message.setTextColor(DesignTokens.Palette.ON_SURFACE_FAINT);
        message.setGravity(Gravity.CENTER);
        message.setVisibility(View.GONE);
        LayoutParams messageParams = new LayoutParams(LayoutParams.MATCH_PARENT,
                LayoutParams.WRAP_CONTENT);
        messageParams.topMargin = DesignTokens.dp(DesignTokens.REGION_GAP);
        addView(message, messageParams);

        action = new IconButton(context);
        // No glyph until one is asked for: `setAction` gives this button its face,
        // and a section that never offers its action never needs one.
        action.setVisibility(View.GONE);
        action.setOnClickListener(view -> {
            if (actionHost != null) actionHost.onSectionAction();
        });
        LayoutParams actionParams = new LayoutParams(
                DesignTokens.dp(DesignTokens.CONTROL_TARGET),
                DesignTokens.dp(DesignTokens.CONTROL_TARGET));
        actionParams.topMargin = DesignTokens.dp(DesignTokens.GAP);
        addView(action, actionParams);

        phase = Phase.EMPTY;
    }

    /** Where the section's recovery action goes. */
    void setActionHost(ActionHost value) { actionHost = value; }

    /** The phase last shown, for the tests and for the surfaces that ask. */
    Phase phase() { return phase; }

    /**
     * What the action button is, for a state whose action is not a retry.
     *
     * <p>"No tracks, and here is the one thing you can do about it" is the same
     * shape as "it failed, try again", so it is the same control wearing different
     * words — and a button labelled "try again" next to a block that has nothing to
     * retry is a control that lies about what it does.
     */
    void setAction(Icon icon, CharSequence description) {
        if (icon == null) icon = Icon.RETRY;
        if (description == null || description.length() == 0) description = "Try again";
        if (icon == actionIcon && TextUtils.equals(description, actionDescription)) return;
        actionIcon = icon;
        actionDescription = description;
        action.setAction(icon, description);
        action.setContentDescription(description);
    }

    /** The phase a section is in, as a pure function of what is known. */
    static Phase phaseOf(boolean loading, boolean failed, int count) {
        if (failed) return Phase.ERROR;
        if (count > 0) return Phase.CONTENT;
        return loading ? Phase.LOADING : Phase.EMPTY;
    }

    /**
     * Shows the content, or the line that replaces it.
     *
     * @param value    the phase; {@link Phase#CONTENT} shows {@link #content}
     * @param text     the words for the other three phases
     * @param retryable whether the phase offers its action
     */
    void show(Phase value, CharSequence text, boolean retryable) {
        Phase next = value == null ? Phase.EMPTY : value;
        phase = next;
        boolean showingContent = next == Phase.CONTENT;
        content.setVisibility(showingContent ? View.VISIBLE : View.GONE);
        message.setVisibility(showingContent ? View.GONE : View.VISIBLE);
        if (!showingContent && !TextUtils.equals(text, renderedText)) {
            renderedText = text == null ? "" : text;
            message.setText(renderedText);
        }
        if (renderedActionVisible != retryable) {
            renderedActionVisible = retryable;
            action.setVisibility(retryable ? View.VISIBLE : View.GONE);
        }
        CharSequence described = retryable ? actionDescription : "";
        if (!TextUtils.equals(described, renderedAction)) {
            renderedAction = described;
            action.setContentDescription(renderedAction);
        }
        // The content's own height is not visible while it is hidden, so the section
        // collapses to the message instead of leaving a hole where the list was.
        setGravity(showingContent ? Gravity.CENTER_HORIZONTAL : Gravity.CENTER);
    }
}