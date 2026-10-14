package com.tunefold.app;

import android.content.Context;
import android.view.View;
import android.widget.LinearLayout;

import java.util.ArrayList;
import java.util.List;

/**
 * A titled group of tracks: a heading, a count and a {@link TrackList}.
 *
 * <p>Everything a list needs that is not the data: the header's wording, the state
 * machine around it (loading, empty, error) and the range listener that turns visible
 * rows into cover requests. Three destinations are built out of these — Continue
 * listening, L1K3D, Recommended, the search results and Descargadas — so the state
 * behaviour is written once and cannot drift between them.
 *
 * <p>A group that is hidden collapses to nothing: a section with three empty groups
 * would otherwise leave three headings and three holes where lists should be.
 */
@android.annotation.SuppressLint("ViewConstructor")
final class TrackGroup extends LinearLayout {

    /** How a group asks for its data. */
    interface Source {
        /**
         * Fills {@code list} with what it has, right now.
         *
         * <p>Synchronous on purpose for everything the session already knows, and
         * asynchronous only where a provider round trip is involved: a group that
         * cannot answer yet says {@link SectionState.Phase#LOADING} and answers when
         * the answer arrives.
         */
        void load(TrackGroup group);
    }

    private final SectionHeader header;
    private final TrackList list;
    private final SectionState state;

    private CharSequence title = "";
    private int count = SectionHeader.NO_COUNT;
    private boolean visible = true;

    TrackGroup(Context context, String title, TrackList.RowPool pool,
            TrackList.StateSource stateSource, ScrollHost host) {
        super(context);
        DesignTokens.init(context);
        setOrientation(VERTICAL);

        header = new SectionHeader(context);
        LayoutParams headerParams = new LayoutParams(LayoutParams.MATCH_PARENT,
                LayoutParams.WRAP_CONTENT);
        headerParams.topMargin = DesignTokens.dp(DesignTokens.SECTION_GAP);
        headerParams.bottomMargin = DesignTokens.dp(DesignTokens.GAP_TIGHT);
        addView(header, headerParams);

        list = new TrackList(context, pool, stateSource, host);
        addView(list, new LayoutParams(LayoutParams.MATCH_PARENT,
                LayoutParams.WRAP_CONTENT));

        state = new SectionState(context, list);
        addView(state, new LayoutParams(LayoutParams.MATCH_PARENT,
                LayoutParams.WRAP_CONTENT));
        setTitle(title, SectionHeader.NO_COUNT);
    }

    /** The heading and its count; a repeated call with the same words costs nothing. */
    void setTitle(CharSequence value, int items) {
        title = value;
        count = items;
        header.setTitle(title, count);
    }

    /**
     * Where this block leads.
     *
     * <p>{@code null} takes the way out away: a block whose whole content fits on
     * screen has nothing to lead to, and a control that leads nowhere is a lie about
     * what the app can do.
     */
    void setAction(CharSequence label, Runnable target) {
        header.setAction(label, target);
    }

    /** Whether this group leads anywhere; the surface decides the wording. */
    boolean hasAction() { return header.hasAction(); }

    /** The list, for a caller that fills it. */
    TrackList list() { return list; }

    /** The state machine around the list. */
    SectionState state() { return state; }

    /** The range listener the list reports visible rows through. */
    void setRangeListener(TrackList.RangeListener listener) {
        list.setRangeListener(listener);
    }

/**
 * Whether this group takes part in the section at all.
 *
 * <p>Hiding rather than emptying is deliberate: an empty group is a heading with
 * nothing under it, and Home has three of those by default.
 */
void setVisibleGroup(boolean value) {
        if (visible == value) return;
        visible = value;
        setVisibility(value ? View.VISIBLE : View.GONE);
    }

    /** Whether the group is currently taking part in the section. */
    boolean isVisibleGroup() { return visible; }

    /**
     * Fills the list and settles the state in one call.
     *
     * <p>An empty list is <b>not</b> an empty group here: it is a group with nothing
     * in it, which is a heading floating over a hole. A caller that wants the empty
     * state to be visible asks for it with {@link #showEmpty}.
     */
    void show(List<MediaTrack> tracks, boolean loading) {
        List<MediaTrack> value = tracks == null ? new ArrayList<>() : tracks;
        list.submit(value);
        if (loading) {
            showLoading();
            return;
        }
        if (value.isEmpty()) {
            setVisibleGroup(false);
            return;
        }
        state.show(SectionState.Phase.CONTENT, "", false);
        setVisibleGroup(true);
    }

    /** Nothing here, and the line that says so. */
    void showEmpty(CharSequence text) {
        list.submit(null);
        state.show(SectionState.Phase.EMPTY, text, false);
        setVisibleGroup(true);
    }

    /** The loading state for a request that has not answered yet. */
    void showLoading() {
        list.submit(null);
        state.show(SectionState.Phase.LOADING, "Loading…", false);
        setVisibleGroup(true);
    }

    /** A failure, with the one action that can move it forward. */
    void showError(CharSequence text, SectionState.ActionHost action) {
        state.setActionHost(action);
        state.show(SectionState.Phase.ERROR, text, action != null);
        setVisibleGroup(true);
    }

    /**
     * Nothing here, and the one action that can change that.
     *
     * <p>An empty block with no way forward is a dead end, and the block that offers
     * the search when there is nothing saved yet is the difference between a screen
     * that waits for the user and one that asks them.
     */
    void showAction(CharSequence text, CharSequence description, Icon icon, Runnable onRun) {
        state.setActionHost(onRun::run);
        state.setAction(icon, description);
        state.show(SectionState.Phase.EMPTY, text, true);
        setVisibleGroup(true);
    }

/** Content, with its state settled. */
    void showContent() {
        state.show(SectionState.Phase.CONTENT, "", false);
        setVisibleGroup(true);
    }

    /** Re-applies like and download state to the rows that are bound. */
    void refresh() { list.refresh(); }

    /** Marks the row of whatever is playing. */
    void setActiveTrack(TrackKey key) { list.setActiveTrack(key); }
}