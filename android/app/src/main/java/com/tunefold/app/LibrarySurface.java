package com.tunefold.app;

import android.content.Context;

import java.util.ArrayList;
import java.util.List;

/**
 * One membership or one store, rendered the same way.
 *
 * <p>L1K3D and Descargadas differ in where the tracks come from and in nothing else:
 * both are "a list of things the user kept", and the moment they stop sharing this
 * class is the moment a fix to one of them stops being a fix to both. What each of
 * them knows is passed in by the screen through {@link #submit}.
 *
 * <p>Neither keeps its tracks. The session owns membership and the store owns files, so
 * this surface holds the rendered window and nothing else, which is what makes it safe
 * to keep four of them alive at once.
 */
@android.annotation.SuppressLint("ViewConstructor")
final class LibrarySurface extends SectionSurface {

    /** How many tracks are described before the list is truncated. */
    static final int MAX_TRACKS = 500;

    private final TrackGroup group;
    private final String heading;

    LibrarySurface(Context context, Host host, String heading) {
        super(context, host);
        this.heading = heading;
        group = new TrackGroup(context, heading, host.rowPool(), host, host.scrollHost());
        addView(group, new LayoutParams(LayoutParams.MATCH_PARENT,
                LayoutParams.WRAP_CONTENT));
    }

    /**
     * Shows what the caller knows.
     *
     * <p>Truncating is not a compromise: a list with ten thousand rows would build ten
     * thousand worth of data to show eight, and the first screenful is the only part
     * anybody reaches by scrolling.
     */
    void submit(List<MediaTrack> tracks, boolean loading, CharSequence empty,
            CharSequence error) {
        // `empty` is the caller's wording for "there is nothing here yet"; the group's
        // own state machine decides whether that is an empty section or content.
        if (tracks == null || tracks.isEmpty()) {
            if (error != null) {
                group.setTitle(heading, 0);
                group.showError(error, null);
                return;
            }
            if (loading) {
                group.setTitle(heading, 0);
                group.showLoading();
                return;
            }
        }
        List<MediaTrack> value = tracks == null ? new ArrayList<>() : tracks;
        group.setTitle(heading, value.size());
        group.setActiveTrack(activeKey());
        if (error != null) {
            group.showError(error, null);
            return;
        }
        group.show(value, loading);
    }

    @Override void onEnter() { }

    @Override void onShow() {
        group.setActiveTrack(activeKey());
    }

    @Override void setActiveKey(TrackKey key) { group.setActiveTrack(key); }

    private TrackKey activeKey() {
        MainActivity activity = (MainActivity) getContext();
        return activity.sessionForSurfaces() == null ? null
                : activity.sessionForSurfaces().snapshot().currentKey;
    }

    @Override void refresh() { group.refresh(); }

    /** The truncation rule, as a pure function so the bound is a test. */
    static int limit(int count) { return count < MAX_TRACKS ? count : MAX_TRACKS; }
}