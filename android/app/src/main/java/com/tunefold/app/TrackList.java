package com.tunefold.app;

import android.content.Context;
import android.view.View;
import android.widget.LinearLayout;

import java.util.ArrayList;
import java.util.List;

/**
 * A list of {@link TrackRow}s that reuses its rows.
 *
 * <p>The lists on this screen used to be built by inflating a fresh row per track,
 * every time the section was rendered — and because the previous implementation
 * re-rendered a section whenever any download state changed, that meant allocating a
 * {@code LinearLayout}, two {@code Button}s and a {@code TextView} per track several
 * times a second, then throwing all of it away. It is the most expensive thing the UI
 * used to do, and it is invisible: the user sees exactly the same list.
 *
 * <p>A {@link RowPool} turns a refresh into a rebind, so a long L1K3D costs its rows
 * once and then never again.
 *
 * <p>{@link #refresh()} is the tick-safe entry point: it re-applies like and download
 * state to every visible row, and because {@link TrackRow#applyLike} and
 * {@link TrackRow#applyDownload} are equality-guarded, a row whose visible state has
 * not moved does nothing at all.
 */
@android.annotation.SuppressLint("ViewConstructor")
final class TrackList extends LinearLayout {

    /** Where a row's like and download state come from. */
    interface StateSource {
        /** L1K3D membership for {@code track}. */
        boolean isLiked(MediaTrack track);

        /** Download state for {@code track}; never {@code null}. */
        DownloadState downloadOf(MediaTrack track);
    }

    /**
     * A pool of {@link TrackRow}s, owned by one screen.
     *
     * <p>Deliberately <b>not</b> static. A pooled row is a {@code View}, so a static pool
     * would keep the whole Activity — window, decor, resources — alive after the screen is
     * gone. Owning the pool per screen means it dies with it, while still letting three
     * lists on the same screen share one set of rows: switching from L1K3D to Descargadas
     * re-points existing rows instead of inflating new ones.
     */
    static final class RowPool {

        private final ArrayList<TrackRow> free = new ArrayList<>();
        private final Context context;
        private final TrackRow.Callbacks callbacks;

        RowPool(Context context, TrackRow.Callbacks callbacks) {
            this.context = context;
            this.callbacks = callbacks;
        }

        /** A row, either reused or newly built. */
        TrackRow obtain() {
            int last = free.size() - 1;
            if (last < 0) return new TrackRow(context, callbacks);
            TrackRow row = free.remove(last);
            row.setVisibility(View.VISIBLE);
            return row;
        }

        /** Returns a row to the pool after detaching it from its list. */
        void release(TrackRow row) {
            row.setVisibility(View.GONE);
            row.setArtwork(null);
            free.add(row);
        }

        /** Drops every pooled row; called when the screen is destroyed. */
        void clear() { free.clear(); }

        /** How many rows are parked, for the recycling assertions. */
        int parkedCount() { return free.size(); }
    }

    private final ArrayList<TrackRow> rows = new ArrayList<>();
    private final RowPool pool;
    private final StateSource state;
    private TrackKey activeKey;

    TrackList(Context context, RowPool pool, StateSource state) {
        super(context);
        DesignTokens.init(context);
        this.pool = pool;
        this.state = state;
        setOrientation(VERTICAL);
    }

    /**
     * Shows exactly {@code tracks}, reusing rows where possible.
     *
     * <p>Replaces the list contents. The caller may reuse or discard {@code tracks}
     * freely afterwards; each row keeps its own reference to the track it shows.
     */
    void submit(List<MediaTrack> tracks) {
        int count = tracks == null ? 0 : tracks.size();
        releaseFrom(count);
        for (int i = 0; i < count; i++) {
            TrackRow row = acquire(i);
            MediaTrack track = tracks.get(i);
            row.bind(track, isLiked(track), downloadOf(track));
            row.setActiveTrack(activeKey != null && activeKey.equals(track.key()));
        }
    }

    /**
     * Re-applies like and download state to every visible row.
     *
     * <p>Called on the download tick: O(visible rows) with no allocation and, thanks to
     * the guards inside the row, no View mutation for rows that did not change.
     */
    void refresh() {
        for (int i = 0; i < rows.size(); i++) {
            TrackRow row = rows.get(i);
            MediaTrack track = row.track();
            if (track == null) continue;
            row.applyLike(isLiked(track));
            row.applyDownload(downloadOf(track));
        }
    }

    /** Marks which track is currently playing, without rebinding anything else. */
    void setActiveTrack(TrackKey key) {
        boolean same = activeKey == null ? key == null : activeKey.equals(key);
        activeKey = key;
        if (same) return;
        for (int i = 0; i < rows.size(); i++) {
            TrackRow row = rows.get(i);
            row.setActiveTrack(key != null && row.key() != null && key.equals(row.key()));
        }
    }

    /** Number of rows currently attached. */
    int boundRowCount() { return rows.size(); }

    /** Empties the list and returns every row to the pool. */
    void detachAll() { releaseFrom(0); }

    private boolean isLiked(MediaTrack track) {
        return state == null || state.isLiked(track);
    }

    private DownloadState downloadOf(MediaTrack track) {
        return state == null ? DownloadState.idle(track.key()) : state.downloadOf(track);
    }

    private TrackRow acquire(int index) {
        while (rows.size() <= index) {
            TrackRow row = pool.obtain();
            // Reuse the row's own params: a pooled row keeps them across removeView,
            // so re-attaching costs no allocation.
            LayoutParams params = (LayoutParams) row.getLayoutParams();
            if (params == null) {
                params = new LayoutParams(LayoutParams.MATCH_PARENT,
                        LayoutParams.WRAP_CONTENT);
            }
            addView(row, params);
            rows.add(row);
        }
        return rows.get(index);
    }

    /** Detaches and pools every row from {@code keep} onwards. */
    private void releaseFrom(int keep) {
        while (rows.size() > keep) {
            TrackRow row = rows.remove(rows.size() - 1);
            removeView(row);
            pool.release(row);
        }
    }
}