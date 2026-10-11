package com.tunefold.app;

import android.content.Context;
import android.view.View;
import android.view.ViewGroup;
import android.view.ViewParent;

import java.util.ArrayList;
import java.util.List;

/**
 * A list of {@link TrackRow}s that only builds the rows that can be seen.
 *
 * <h3>Why this is not a LinearLayout of rows</h3>
 * The first version of this class put every track in the list into the tree, because
 * that is what a {@code LinearLayout} in a {@code ScrollView} does. A library of 500
 * tracks therefore meant 500 rows — each with a cover, three labels and two buttons —
 * built to show the eight that fit, on every render. The fix is not "recycle harder":
 * rows that were never built cannot be recycled, and the memory was the problem, not
 * the churn.
 *
 * <p>So the list keeps the <b>tracks</b> and owns a small window of <b>rows</b>. It
 * reports its full height from the track count, its host moves it, and it binds only
 * the rows inside the window plus {@link DesignTokens#LIST_PREFETCH_ROWS} above and
 * below it. A fling that outruns the binds still has rows ready to show.
 *
 * <h3>What makes it correct</h3>
 * Rows are laid out at their natural position ({@code index × row height}) rather than
 * at a window-relative one, so a row's place in the list is a property of the list and
 * not of the scroll position. The host clips whatever falls outside the window, and the
 * list's {@link #updateWindow} is what decides which rows exist at all.
 *
 * <p>The visible range is reported through {@link RangeListener}, which is what keeps
 * artwork loading honest: a cover is requested for rows that are on screen, not for
 * rows that are merely in the data.
 */
@android.annotation.SuppressLint("ViewConstructor")
final class TrackList extends ViewGroup {

    /** Where a row's like and download state come from. */
    interface StateSource {
        /** L1K3D membership for {@code track}. */
        boolean isLiked(MediaTrack track);

        /** Download state for {@code track}; never {@code null}. */
        DownloadState downloadOf(MediaTrack track);
    }

    /** Told which rows are on screen, so their covers can be asked for. */
    interface RangeListener {
        /**
         * @param first index of the first bound row
         * @param count how many rows are bound, so the last is {@code first + count - 1}
         */
        void onRangeChanged(int first, int count);
    }

    /**
     * A pool of {@link TrackRow}s, owned by one screen.
     *
     * <p>Deliberately <b>not</b> static. A pooled row is a {@code View}, so a static
     * pool would keep the whole Activity — window, decor, resources — alive after the
     * screen is gone. Owning the pool per screen means it dies with it, while still
     * letting three lists on the same screen share one set of rows: switching from
     * L1K3D to Descargadas re-points existing rows instead of inflating new ones.
     */
    static final class RowPool {

        private final ArrayList<TrackRow> free = new ArrayList<>();
        private final Context context;
        private final TrackRow.Callbacks callbacks;
        private TrackRow.ArtworkLoader loader;
        private TrackRow.OverflowHost overflowHost;
        private int built;

        RowPool(Context context, TrackRow.Callbacks callbacks) {
            this.context = context;
            this.callbacks = callbacks;
        }

        /** The one path covers take; set once, so every row asks through it. */
        void setArtworkLoader(TrackRow.ArtworkLoader value) { loader = value; }

        /**
         * Where a row's contextual actions are shown from; set once, like the loader.
         *
         * <p>On the pool rather than on each row, because a row built without one has
         * a button that reaches nothing: a control that cannot do its own action is
         * worse than no control, because it looks like it works.
         */
        void setOverflowHost(TrackRow.OverflowHost value) { overflowHost = value; }

        /** A row, either reused or newly built. */
        TrackRow obtain() {
            int last = free.size() - 1;
            if (last >= 0) {
                TrackRow row = free.remove(last);
                row.setArtworkLoader(loader);
                row.setOverflowHost(overflowHost);
                row.setVisibility(View.VISIBLE);
                return row;
            }
            TrackRow row = new TrackRow(context, callbacks);
            row.setArtworkLoader(loader);
            row.setOverflowHost(overflowHost);
            built++;
            return row;
        }

        /** Returns a row to the pool after detaching it from its list. */
        void release(TrackRow row) {
            row.setVisibility(View.GONE);
            // A pooled row must not keep a bitmap alive: a library scrolled past would
            // otherwise hold one decoded cover per row it ever built.
            row.setArtwork(null);
            free.add(row);
        }

        /** Drops every pooled row; called when the screen is destroyed. */
        void clear() { free.clear(); }

        /** How many rows exist, for the recycling assertions. */
        int pooledCount() { return free.size(); }

        /** How many rows were ever built, which is the memory this list is about. */
        int builtCount() { return built; }
    }

    private final ArrayList<MediaTrack> tracks = new ArrayList<>();
    /** Rows for {@code tracks[first .. first + bound.size())}. */
    private final ArrayList<TrackRow> bound = new ArrayList<>();
    private final RowPool pool;
    private final StateSource state;
    private final ScrollHost host;

    private int rowHeight;
    private int first = Integer.MIN_VALUE;
    private int last = -1;
    private TrackKey activeKey;
    private RangeListener rangeListener;
    /** The window this list last asked for, so a no-op scroll costs two comparisons. */
    private int windowTop = Integer.MIN_VALUE;
    private int windowHeight = Integer.MIN_VALUE;

    TrackList(Context context, RowPool pool, StateSource state, ScrollHost host) {
        super(context);
        DesignTokens.init(context);
        this.pool = pool;
        this.state = state;
        this.host = host;
        rowHeight = DesignTokens.dp(DesignTokens.ROW_HEIGHT);
    }

    @Override protected void onAttachedToWindow() {
        super.onAttachedToWindow();
        // Registered on attach, not on construction: a surface is built before the
        // content is handed to the host, and a registration made before that was the
        // thing the host drops when it takes its content child.
        if (host != null) host.registerWindow(this);
    }

    @Override protected void onDetachedFromWindow() {
        if (host != null) host.unregisterWindow(this);
        super.onDetachedFromWindow();
    }

    /** Told which rows are on screen. */
    void setRangeListener(RangeListener value) { rangeListener = value; }

    /** Declares which track is currently playing, without rebinding anything else. */
    void setActiveTrack(TrackKey key) {
        boolean same = activeKey == null ? key == null : activeKey.equals(key);
        activeKey = key;
        if (same) return;
        for (int i = 0; i < bound.size(); i++) {
            bound.get(i).setActiveTrack(key != null && key.equals(bound.get(i).key()));
        }
    }

    /**
     * Shows exactly {@code value}, without building a single row.
     *
     * <p>The caller may reuse or discard the list afterwards; rows keep their own
     * reference to the track they show. The window is re-evaluated immediately if the
     * list already knows where the viewport is, so a switch to a destination with a
     * different length shows the right rows on the first frame.
     *
     * <p><b>Identical content is a no-op.</b> This list is asked to show its data on
     * every session notification — twice a second, for every reason at once — and the
     * data is almost always the same data. Rebuilding it meant dropping every bound
     * row, re-binding every row and asking the host for a layout pass to end up with
     * the picture that was already on screen. So the contents are compared first, by
     * identity, which is the only thing a row actually renders.
     */
    void submit(List<MediaTrack> value) {
        if (sameTracks(tracks, value)) {
            // Same content, but the viewport may have moved since the last window:
            // the host still gets to say what is visible.
            if (host != null) host.refreshWindow(this);
            return;
        }
        tracks.clear();
        if (value != null) tracks.addAll(value);
        releaseAll();
        // The window is about to be a different window: the tracks behind it changed,
        // so every memo of the old one has to go — the viewport, the range and the
        // bound rows. Left unfixed, a list fills its whole height and then shows
        // nothing at all, because the "the viewport did not move" shortcut skips the
        // rebind against data that no longer exists.
        first = Integer.MIN_VALUE;
        last = -1;
        windowTop = Integer.MIN_VALUE;
        windowHeight = Integer.MIN_VALUE;
        requestLayout();
        if (host != null) host.refreshWindow(this);
    }

    /**
     * Whether two lists would draw the same rows.
     *
     * <p>A pure function over identity, because this is the decision that decides
     * whether the screen redraws itself, and it is exactly the kind of rule that has
     * to be checked without a device. Same size, same order, same identities: the
     * rows are interchangeable and there is nothing to rebind.
     */
    static boolean sameTracks(List<MediaTrack> left, List<MediaTrack> right) {
        int size = left == null ? 0 : left.size();
        if (right == null) return size == 0;
        if (size != right.size()) return false;
        for (int index = 0; index < size; index++) {
            TrackKey one = left.get(index).key();
            TrackKey two = right.get(index).key();
            if (one == null ? two != null : !one.equals(two)) return false;
        }
        return true;
    }

    /** How many tracks this list holds, whether or not they are built. */
    int trackCount() { return tracks.size(); }

    /** How many rows exist right now: the window, never the library. */
    int boundRowCount() { return bound.size(); }

    /** Rows parked in the pool; the recycling evidence. */
    int pooledRowCount() { return pool.pooledCount(); }

    /**
     * Recomputes the window for a viewport.
     *
     * @param scrollTop where the viewport starts inside this list, in pixels
     * @param viewport  how tall the viewport is, in pixels
     * @return {@code true} when the window moved and rows were bound or released
     */
    boolean updateWindow(int scrollTop, int viewport) {
        int height = Math.max(rowHeight, viewport);
        if (scrollTop == windowTop && height == windowHeight) return false;
        windowTop = scrollTop;
        windowHeight = height;
        int prefetch = DesignTokens.LIST_PREFETCH_ROWS * rowHeight;
        int start = Math.max(0, (scrollTop - prefetch) / rowHeight);
        int end = Math.min(tracks.size(), (scrollTop + height + prefetch) / rowHeight + 1);
        if (start == first && end == last) return false;
        first = start;
        last = end;
        rebindWindow();
        return true;
    }

    /**
     * Re-applies like and download state to every bound row.
     *
     * <p>Called on the download tick: O(window) with no allocation and, thanks to the
     * guards inside the row, no View mutation for rows that did not change.
     */
    void refresh() {
        for (int i = 0; i < bound.size(); i++) {
            TrackRow row = bound.get(i);
            MediaTrack track = row.track();
            if (track == null) continue;
            row.applyLike(isLiked(track));
            row.applyDownload(downloadOf(track));
        }
    }

    /**
     * Hands a decoded cover to whichever bound row is waiting for it.
     *
     * <p>Matching by identity rather than by position is what makes a slow decode
     * harmless: a row that has already been rebound to another track simply is not
     * the one being painted.
     */
    void onArtwork(String identity, android.graphics.Bitmap bitmap) {
        for (int i = 0; i < bound.size(); i++) {
            TrackRow row = bound.get(i);
            if (identity.equals(row.artworkIdentity())) row.setArtwork(bitmap);
        }
    }

    /** Returns every row to the pool and forgets the window. */
    void detachAll() {
        releaseAll();
        first = Integer.MIN_VALUE;
        last = -1;
        windowTop = Integer.MIN_VALUE;
        windowHeight = Integer.MIN_VALUE;
    }

    // --------------------------------------------------------------- windowing

    /** Brings the bound rows in line with {@code [first, last)}. */
    private void rebindWindow() {
        int wanted = Math.max(0, last - first);
        // Shrink first: a row leaving the window is released before a new one is
        // built, so the peak number of rows is the window's, not window + delta.
        while (bound.size() > wanted) {
            int index = bound.size() - 1;
            TrackRow row = bound.remove(index);
            removeView(row);
            pool.release(row);
        }
        while (bound.size() < wanted) {
            int index = first + bound.size();
            TrackRow row = pool.obtain();
            LayoutParams params = (LayoutParams) row.getLayoutParams();
            addView(row, params != null ? params
                    : new LayoutParams(LayoutParams.MATCH_PARENT, rowHeight));
            bound.add(row);
            row.bind(tracks.get(index), isLiked(tracks.get(index)),
                    downloadOf(tracks.get(index)));
            row.setActiveTrack(activeKey != null && activeKey.equals(row.key()));
        }
        // Same size, different tracks: the window moved by a row or two.
        for (int i = 0; i < bound.size(); i++) {
            TrackRow row = bound.get(i);
            MediaTrack track = tracks.get(first + i);
            if (row.key() != null && row.key().equals(track.key())) continue;
            row.bind(track, isLiked(track), downloadOf(track));
            row.setActiveTrack(activeKey != null && activeKey.equals(row.key()));
        }
        if (rangeListener != null) rangeListener.onRangeChanged(first, bound.size());
        requestLayout();
    }

    private void releaseAll() {
        for (int i = bound.size() - 1; i >= 0; i--) {
            TrackRow row = bound.get(i);
            removeView(row);
            pool.release(row);
        }
        bound.clear();
    }

    /** Where this list starts inside its host, in pixels. */
    int contentOffset(ScrollHost target) {
        int top = 0;
        View view = this;
        while (view != null && view != target) {
            top += view.getTop();
            ViewParent parent = view.getParent();
            view = parent instanceof View ? (View) parent : null;
        }
        return top;
    }

    // ------------------------------------------------------------------ layout

    @Override protected void onMeasure(int widthSpec, int heightSpec) {
        int width = MeasureSpec.getSize(widthSpec);
        // The whole point: height comes from the DATA, not from the rows.
        int height = tracks.isEmpty() ? 0 : tracks.size() * rowHeight;
        int childWidthSpec = MeasureSpec.makeMeasureSpec(width, MeasureSpec.EXACTLY);
        int childHeightSpec = MeasureSpec.makeMeasureSpec(rowHeight, MeasureSpec.EXACTLY);
        for (int i = 0; i < bound.size(); i++) {
            bound.get(i).measure(childWidthSpec, childHeightSpec);
        }
        setMeasuredDimension(resolveSize(Math.max(width, getSuggestedMinimumWidth()), widthSpec),
                resolveSize(Math.max(height, getSuggestedMinimumHeight()), heightSpec));
    }

    @Override protected void onLayout(boolean changed, int left, int top, int right, int bottom) {
        // Natural position, not window-relative: a row's y is its index, so the host
        // can move the list without the list knowing, and a row that scrolls out is
        // clipped rather than teleported.
        for (int i = 0; i < bound.size(); i++) {
            int childTop = (first + i) * rowHeight;
            bound.get(i).layout(0, childTop, right - left, childTop + rowHeight);
        }
    }

    /** The row height this list lays out with, for the surfaces that stack around it. */
    int rowHeight() { return rowHeight; }

    private boolean isLiked(MediaTrack track) {
        return state == null || state.isLiked(track);
    }

    private DownloadState downloadOf(MediaTrack track) {
        return state == null ? DownloadState.idle(track.key()) : state.downloadOf(track);
    }

    // ------------------------------------------------------------------- tests

    /** The window rule, as a pure function so it can be tested without a device. */
    static int[] windowFor(int trackCount, int rowHeight, int scrollTop, int viewport,
            int prefetchRows) {
        int count = Math.max(0, trackCount);
        int height = Math.max(1, rowHeight);
        int prefetch = Math.max(0, prefetchRows) * height;
        int first = Math.max(0, (scrollTop - prefetch) / height);
        int last = Math.min(count, (scrollTop + Math.max(height, viewport) + prefetch)
                / height + 1);
        if (last < first) last = first;
        return new int[] {first, Math.max(0, last - first)};
    }

    /** The window as this list would compute it, for the tests that assert on it. */
    int[] currentWindow() { return new int[] {first, bound.size()}; }

    ScrollHost host() { return host; }
}