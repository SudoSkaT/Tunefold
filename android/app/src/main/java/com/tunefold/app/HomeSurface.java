package com.tunefold.app;

import android.content.Context;
import android.view.View;

import java.util.ArrayList;
import java.util.List;

/**
 * Home: what to play next, not what is playing now.
 *
 * <p>Three groups, in the order a person uses them — keep going, then the music they
 * already said yes to, then something they did not choose — and a documented seam for
 * whatever comes after. The player is deliberately <b>not</b> here: it is one tap away
 * in the strip at the bottom, always, and embedding it would make Home a screen whose
 * main content is a control the user already has.
 *
 * <h3>Reload policy</h3>
 * Recommendations are a provider round trip, so they are asked for once per track, not
 * once per render. Continue listening is local (the session already knows the queue) and
 * is re-derived whenever the current track moves. L1K3D is local too and re-derives
 * when membership changes.
 *
 * <h3>Why there is a revision here</h3>
 * The session notifies its observers twice a second, for every reason at once. This
 * surface used to re-derive both local groups on every one of those notifications,
 * which meant building two lists, walking seventeen identities and asking two
 * {@link TrackList}s to drop and rebuild every row they had — twice a second, to
 * arrive at the picture already on screen. It is the classic cost of not asking
 * whether the answer would change: the screen was redrawing itself while the user
 * read it. The two guards below turn that into two integer comparisons.
 *
 * <h3>The fourth slot</h3>
 * There is room for one more group and no placeholder for it: a heading with nothing
 * under it is a promise the product has not made yet. Future content adds a
 * {@link TrackGroup} here and nothing else changes.
 */
@android.annotation.SuppressLint("ViewConstructor")
final class HomeSurface extends SectionSurface {

    private final TrackGroup continueListening;
    private final TrackGroup liked;
    private final TrackGroup recommended;

    /** Identity the recommendations were asked for; {@code null} for "never". */
    private String recommendationsFor = null;
    private boolean built;

    /** Last inputs the local groups were derived from; see the note above. */
    private TrackKey renderedActiveKey;
    private int renderedQueueSize = -1;
    private int renderedLikedRevision = -1;

    HomeSurface(Context context, Host host) {
        super(context, host);
        continueListening = group("Continue listening");
        liked = group("L1K3D");
        recommended = group("Recommended");
        recommended.state().setActionHost(() -> loadRecommendations(force = true));
        for (TrackGroup candidate : new TrackGroup[] {continueListening, liked, recommended}) {
            candidate.setRangeListener((first, count) -> requestArtwork(candidate));
        }
    }

    private TrackGroup group(String title) {
        TrackGroup group = new TrackGroup(getContext(), title, host.rowPool(), host,
                host.scrollHost());
        addView(group, new LayoutParams(LayoutParams.MATCH_PARENT,
                LayoutParams.WRAP_CONTENT));
        return group;
    }

    private boolean force;

    @Override void onEnter() { built = true; }

    @Override void onShow() {
        if (!built) onEnter();
        // Arriving is a moment where re-deriving is right: the session may have
        // learned things — a resolved title, a new membership — while this surface
        // was on the other side of the screen.
        render(true);
    }

    @Override void onSessionChanged() { render(false); }

    /**
     * Re-derives the local groups and asks for recommendations if they are stale.
     *
     * @param force re-derive even if no input moved; used on arrival
     */
    void render(boolean force) {
        PlaybackSession session = session();
        if (session == null) return;
        PlaybackSession.Snapshot snapshot = session.snapshot();
        TrackKey active = snapshot.currentKey;
        int likedRevision = session.libraryRevision();

        continueListening.setActiveTrack(active);
        liked.setActiveTrack(active);
        recommended.setActiveTrack(active);

        // "Keep going" is the queue from the current track onwards: it changes when
        // the track or the queue does, and not one single time in between.
        boolean queueMoved = active != renderedActiveKey
                ? active != null || renderedActiveKey != null
                : snapshot.queue.size() != renderedQueueSize;
        if (force || queueMoved) {
            renderedActiveKey = active;
            renderedQueueSize = snapshot.queue.size();
            continueListening.show(continuing(), false);
        }

        // L1K3D is a membership set, and a membership set has a revision.
        if (force || likedRevision != renderedLikedRevision) {
            renderedLikedRevision = likedRevision;
            List<MediaTrack> mine = new ArrayList<>();
            for (TrackKey key : session.likedKeys()) {
                MediaTrack track = session.describeLiked(key);
                if (track != null) mine.add(track);
            }
            liked.setTitle("L1K3D", mine.size());
            liked.show(mine, false);
        }

        MediaTrack current = snapshot.currentTrack;
        if (current == null) {
            if (recommended.isVisibleGroup()) recommended.setVisibleGroup(false);
            recommendationsFor = null;
            return;
        }
        loadRecommendations(force = false);
    }

    /** The queue from the current track onwards: what "keep going" means. */
    private List<MediaTrack> continuing() {
        PlaybackSession session = session();
        if (session == null) return null;
        PlaybackSession.Snapshot snapshot = session.snapshot();
        if (snapshot.currentTrack == null) return null;
        List<MediaTrack> tracks = new ArrayList<>();
        tracks.add(snapshot.currentTrack);
        int from = Math.max(0, snapshot.queueIndex + 1);
        for (int i = from; i < snapshot.queue.size() && tracks.size() < 12; i++) {
            MediaTrack track = snapshot.queue.at(i);
            if (track != null) tracks.add(track);
        }
        return tracks;
    }

    /**
     * Recommendations, at most one request per track.
     *
     * <p>A failure leaves the rest of Home usable and offers the one action that can
     * fix it, rather than an error screen over a surface that works.
     */
    private void loadRecommendations(boolean retry) {
        PlaybackSession session = session();
        if (session == null) return;
        MediaTrack current = session.snapshot().currentTrack;
        if (current == null) return;
        String identity = identityOf(current);
        if (!retry && identity.equals(recommendationsFor)) return;
        recommendationsFor = identity;
        recommended.setVisibleGroup(true);
        recommended.showLoading();
        session.recommendationsForHome(current, (candidates, error) -> {
            if (getVisibility() != View.VISIBLE) return;
            if (error != null) {
                recommended.showError(error, () -> loadRecommendations(true));
                return;
            }
            List<MediaTrack> tracks = candidates == null ? new ArrayList<>() : candidates;
            recommended.setTitle("Recommended", tracks.size());
            recommended.show(tracks, false);
        });
    }

    /** Asks for the covers of the rows that are actually on screen. */
    private void requestArtwork(TrackGroup group) {
        TrackKey active = session() == null ? null : session().snapshot().currentKey;
        group.setActiveTrack(active);
    }

    @Override void setActiveKey(TrackKey key) {
        continueListening.setActiveTrack(key);
        liked.setActiveTrack(key);
        recommended.setActiveTrack(key);
    }

    @Override void refresh() {
        continueListening.refresh();
        liked.refresh();
        recommended.refresh();
    }

    @Override MediaTrack activeTrack() {
        return session() == null ? null : session().snapshot().currentTrack;
    }

    private PlaybackSession session() {
        return ((MainActivity) getContext()).sessionForSurfaces();
    }

    private static String identityOf(MediaTrack track) {
        TrackKey key = track.key();
        return key == null ? "" : key.toString();
    }
}