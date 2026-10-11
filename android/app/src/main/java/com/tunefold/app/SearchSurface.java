package com.tunefold.app;

import android.content.ClipboardManager;
import android.content.Context;
import android.text.InputType;
import android.text.TextUtils;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.inputmethod.EditorInfo;
import android.widget.EditText;
import android.widget.LinearLayout;

/**
 * Search, as a destination of its own instead of a permanent box on Home.
 *
 * <p>A search field that lives on Home is a field that is always there, always in the
 * way, and belongs to the screen you are not on. Here it belongs to Search: the field,
 * the paste action, the results and the state between them.
 *
 * <p>The field takes both things people actually paste: words, and a YouTube URL. Which
 * one it is decides which command runs — {@link #isUrl} is that decision, and it is a
 * pure function because getting it wrong means typing a song title and trying to
 * resolve it as a stream.
 *
 * <p>Nothing here resolves anything. The surface reports a query and the screen sends
 * the one command the rest of the app already uses, so there is exactly one resolution
 * path in the product.
 */
@android.annotation.SuppressLint("ViewConstructor")
final class SearchSurface extends SectionSurface {

    /** What a query means, once it is typed. */
    enum Intent {
        /** Words to look for. */
        SEARCH,
        /** A stream to play. */
        URL
    }

    /** The screen's side of the contract. */
    interface SearchHost extends Host {
        /** Go back to wherever the user was before Search. */
        void onSearchBack();

        /** Run a search or a URL against the session. */
        void onSubmit(String query, Intent intent);

        /** A request that has finished, successfully or not. */
        void onSearchIdle();
    }

    private final EditText field;
    private final IconButton back;
    private final IconButton paste;
    private final IconButton submit;
    private final TrackGroup results;
    private final SearchHost searchHost;

    private boolean submitting;
    /** Whether this surface already has something to show; see {@link #showPrompt()}. */
    private boolean hasResults;

    SearchSurface(Context context, SearchHost host) {
        super(context, host);
        this.searchHost = host;

        LinearLayout row = new LinearLayout(context);
        row.setOrientation(LinearLayout.HORIZONTAL);
        row.setGravity(Gravity.CENTER_VERTICAL);

        back = new IconButton(context);
        back.setAction(Icon.BACK, "Back");
        back.setOnClickListener(view -> host.onSearchBack());
        row.addView(back, new LinearLayout.LayoutParams(
                DesignTokens.dp(DesignTokens.CONTROL_TARGET),
                DesignTokens.dp(DesignTokens.CONTROL_TARGET)));

        field = new EditText(context);
        field.setSingleLine(true);
        field.setHint("Search or paste a YouTube URL");
        field.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_BODY_SIZE);
        field.setTextColor(DesignTokens.Palette.ON_SURFACE);
        field.setHintTextColor(DesignTokens.Palette.ON_SURFACE_FAINT);
        field.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_URI);
        field.setImeOptions(EditorInfo.IME_ACTION_SEARCH);
        field.setBackground(MainActivity.roundDrawable(DesignTokens.Palette.SURFACE_RAISED,
                DesignTokens.RADIUS_MEDIUM));
        field.setOnEditorActionListener((view, actionId, event) -> {
            if (actionId == EditorInfo.IME_ACTION_SEARCH) {
                submit();
                return true;
            }
            return false;
        });
        int fieldHeight = DesignTokens.dp(DesignTokens.SEARCH_FIELD_HEIGHT);
        LinearLayout.LayoutParams fieldParams =
                new LinearLayout.LayoutParams(0, fieldHeight, 1f);
        fieldParams.leftMargin = DesignTokens.dp(DesignTokens.SPACE_2);
        fieldParams.rightMargin = DesignTokens.dp(DesignTokens.SPACE_2);
        row.addView(field, fieldParams);

        // Paste, because the URL a person wants to play is almost always already on
        // their clipboard from somewhere else, and a selection dialog to reach it is
        // three taps for the one thing this surface is for.
        paste = new IconButton(context);
        paste.setAction(Icon.CLIPBOARD, "Paste link");
        paste.setOnClickListener(view -> pasteClipboard());
        row.addView(paste, new LinearLayout.LayoutParams(
                DesignTokens.dp(DesignTokens.CONTROL_TARGET),
                DesignTokens.dp(DesignTokens.CONTROL_TARGET)));

        submit = new IconButton(context);
        submit.setAction(Icon.SEARCH, "Search");
        submit.setOnClickListener(view -> submit());
        row.addView(submit, new LinearLayout.LayoutParams(
                DesignTokens.dp(DesignTokens.CONTROL_TARGET),
                DesignTokens.dp(DesignTokens.CONTROL_TARGET)));
        addView(row, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT));

        results = new TrackGroup(context, "Results", host.rowPool(), host, host.scrollHost());
        results.setVisibleGroup(false);
        results.setRangeListener((first, count) -> { });
        addView(results, new LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT));
    }

    @Override void onEnter() {
        // The keyboard is not raised on arrival: arriving is not asking to type.
        field.clearFocus();
    }

    @Override void onShow() {
        field.clearFocus();
        submit.setEnabled(!TextUtils.isEmpty(trimmed()));
    }

    /** What is currently typed, trimmed; shared with the screen for scripted runs. */
    String query() { return trimmed(); }

    /** Puts text in the field without submitting it, for scripted runs and restores. */
    void setQuery(String value) {
        field.setText(value == null ? "" : value);
    }

    /** Runs the current query, exactly as the button would. */
    void submit() {
        String query = trimmed();
        if (query.isEmpty()) return;
        submit.setEnabled(false);
        submitting = true;
        Intent intent = Intent.SEARCH;
        if (isUrl(query)) {
            intent = Intent.URL;
            // A pasted URL is a thing to play, not to search for: say so before the
            // screen switches away from here.
            searchHost.onSubmit(query, Intent.URL);
            submitting = false;
            submit.setEnabled(true);
            return;
        }
        searchHost.onSubmit(query, intent);
    }

    /** The results, while they are loading. */
    void showSearching() {
        submitting = true;
        hasResults = true;
        results.setVisibleGroup(true);
        results.showLoading();
    }

    /** The results, once they exist. */
    void showResults(java.util.List<MediaTrack> tracks) {
        submitting = false;
        hasResults = true;
        results.setVisibleGroup(true);
        int count = tracks == null ? 0 : tracks.size();
        results.setTitle("Results", count);
        if (count == 0) {
            results.showEmpty("No results");
        } else {
            results.show(tracks, false);
        }
        submit.setEnabled(true);
    }

    /** A failure that belongs to this surface. */
    void showError(CharSequence text) {
        submitting = false;
        hasResults = true;
        results.setVisibleGroup(true);
        results.showError(text, () -> submit());
        submit.setEnabled(true);
    }

    /**
     * No query yet: the surface says what it is for instead of showing an empty list.
     *
     * <p>Only before the first query. Coming back from another destination with a
     * search already on screen must not wipe it: losing the results because you went
     * to look at your library is the same bug as losing them on a rotation, and both
     * are the UI deciding it knows better than the user.
     */
    void showPrompt() {
        if (submitting || hasResults) return;
        results.setVisibleGroup(false);
    }

    @Override void setActiveKey(TrackKey key) { results.setActiveTrack(key); }

    @Override void refresh() { results.refresh(); }

    private void pasteClipboard() {
        ClipboardManager clipboard =
                (ClipboardManager) getContext().getSystemService(Context.CLIPBOARD_SERVICE);
        if (clipboard == null) return;
        CharSequence pasted = clipboard.getPrimaryClip() == null ? null
                : clipboard.getPrimaryClip().getItemAt(0).coerceToText(getContext());
        if (pasted == null || pasted.length() == 0) return;
        field.setText(pasted.toString().trim());
        submit.setEnabled(true);
    }

    private String trimmed() {
        return field.getText().toString().trim();
    }

    /**
     * Whether this is something to play rather than something to look for.
     *
     * <p>Two conditions, both narrow on purpose. A pasted link starts with a scheme or
     * with {@code www.}, and it points at one of the shapes this app can resolve. The
     * first condition is what keeps "how do I post on youtube.com/watch" a search: a
     * sentence that happens to contain the URL of something is a sentence.
     */
    static boolean isUrl(String query) {
        if (query == null) return false;
        String value = query.trim().toLowerCase(java.util.Locale.ROOT);
        boolean shaped = value.startsWith("http://") || value.startsWith("https://")
                || value.startsWith("www.");
        if (!shaped) return false;
        return value.contains("youtube.com/watch") || value.contains("youtu.be/")
                || value.contains("youtube.com/shorts/");
    }
}