package com.tunefold.app;

import android.content.Context;
import android.view.View;

import java.util.ArrayList;
import java.util.List;

/**
 * Home: what to play next, in the order a person reaches for it.
 *
 * <p><b>The order is the design.</b> There is exactly one thing the screen is for:
 * starting something, or continuing what is already started. So the screen is
 * <ol>
 *   <li><b>Continue listening</b>, when something is playing — the queue as a list,
 *       which the user can edit, not a picture of one;</li>
 *   <li><b>Start here</b>, when nothing is: what can be played right now, which is
 *       either what they saved or the one action that can produce something to save;</li>
 *   <li><b>L1K3D</b>, as a <em>preview</em> — a handful of the most recent, with the
 *       heading leading to all of them;</li>
 *   <li><b>Recommended</b>, asked for once per track.</li>
 * </ol>
 *
 * <p><b>Nothing is shown twice.</b> A block that previews something the user can
 * already open is not a second copy of it, it is a way in; and the count in the
 * heading is the true count, not the number of rows that survived the preview. Before
 * this, Home carried the whole of L1K3D and nothing anywhere led out of it.
 *
 * <h3>Reload policy</h3>
 * Recommendations are a provider round trip, so they are asked for once per track, not
 * once per render. The local blocks are local and re-derive when their input moves —
 * membership for L1K3D, the queue for "continue listening" — and not one time in
 * between. The session notifies its observers twice a second for every reason at
 * once, and a screen that rebuilds itself on each of those notifications is rebuilding
 * the same rows to show the same picture while the user reads it.
 *
 * <p>The player is deliberately <b>not</b> here: it is one tap away in the strip at
 * the bottom, always, and embedding it would make Home a screen whose main content is
 * a control the user already has.
 */
@android.annotation.SuppressLint("ViewConstructor")
final class HomeSurface extends SectionSurface {

    /** Where the blocks lead; the screen owns navigation, the surface owns content. */
    interface Navigation {
        void openSearch();
        void openLiked();
        void openDownloads();
    }

    /**
     * How much of L1K3D a preview shows.
     *
     * <p>Enough to recognise what it is and enough to start from; the rest is one tap
     * away on the heading. A preview that is the whole list is not a preview, and a
     * library of five hundred rows on the discovery screen is a list the user has to
     * scroll past to reach anything else.
     */
    private static final int PREVIEW_ROWS = 6;

    private final TrackGroup continueListening;
    private final TrackGroup startHere;
    private final TrackGroup liked;
    private final TrackGroup recommended;
    private final Navigation navigation;
    /** Time-of-day greeting, the first thing the screen says. */
    private final android.widget.TextView greeting;

    /** Identity the recommendations were asked for; {@code null} for "never". */
    private String recommendationsFor = null;
    private boolean built;

    /** Last inputs the local blocks were derived from; see the note above. */
    private TrackKey renderedActiveKey;
    private int renderedQueueSize = -1;
    private int renderedLikedRevision = -1;
    /** Download set as it was last derived; see the note at its use. */
    private int renderedDownloadsRevision = -1;

    HomeSurface(Context context, Host host, Navigation navigation) {
        super(context, host);
        this.navigation = navigation;
        greeting = new android.widget.TextView(context);
        greeting.setTextSize(android.util.TypedValue.COMPLEX_UNIT_SP,
                DesignTokens.TYPE_TITLE_SIZE);
        greeting.setTextColor(DesignTokens.Palette.ON_SURFACE);
        greeting.setIncludeFontPadding(false);
        greeting.setFocusable(false);
        addView(greeting, new LayoutParams(LayoutParams.MATCH_PARENT,
                LayoutParams.WRAP_CONTENT));
        // The order this screen is read in.
        continueListening = group("Sigue escuchando");
        startHere = group("Empieza aquí");
        liked = group("L1K3D");
        recommended = group("Recomendado");
        recommended.state().setActionHost(() -> loadRecommendations(force = true));

        // The previews lead to the whole list; the queue does not, because the rest of
        // it is the player and the player is one tap away in the strip.
        liked.setAction("Ver todo", () -> navigation.openLiked());

        for (TrackGroup candidate : new TrackGroup[] {continueListening, startHere, liked,
                recommended}) {
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
     * Re-derives the local blocks and asks for recommendations if they are stale.
     *
     * @param force re-derive even if no input moved; used on arrival
     */
    void render(boolean force) {
        PlaybackSession session = session();
        if (session == null) return;
        applyGreeting();
        PlaybackSession.Snapshot snapshot = session.snapshot();
        TrackKey active = snapshot.currentKey;
        int likedRevision = session.libraryRevision();

        continueListening.setActiveTrack(active);
        liked.setActiveTrack(active);
        recommended.setActiveTrack(active);
        startHere.setActiveTrack(active);

        // Exactly one of the two top blocks is ever on screen: what is playing, or
        // what could be. Both at once would be the same information twice, which is
        // what this screen is being reorganised to stop doing.
        boolean playing = snapshot.currentTrack != null;
        continueListening.setVisibleGroup(playing);
        startHere.setVisibleGroup(!playing);

        // "Keep going" is the queue from the current track onwards: it changes when
        // the track or the queue does, and not one single time in between. The
        // downloads revision is in the guard because the store scan lands after the
        // first render — without it the block is drawn once, empty, and stays empty,
        // which is the same bug the downloads view had and the same fix.
        int downloadsRevision = session.downloadsRevision();
        boolean queueMoved = active != null ? active != renderedActiveKey
                : snapshot.queue.size() != renderedQueueSize;
        boolean downloadsMoved = downloadsRevision != renderedDownloadsRevision;
        renderedDownloadsRevision = downloadsRevision;
        if (force || playing != (renderedActiveKey != null) || queueMoved || downloadsMoved) {
            renderedActiveKey = active;
            renderedQueueSize = snapshot.queue.size();
            if (playing) {
                continueListening.setTitle("Sigue escuchando", snapshot.queue.size());
                continueListening.show(continuing(), false);
            } else {
                renderStartHere(session, force);
            }
        }

        // L1K3D is a membership set, and a membership set has a revision.
        if (force || likedRevision != renderedLikedRevision) {
            renderedLikedRevision = likedRevision;
            List<TrackKey> keys = session.likedKeys();
            List<MediaTrack> mine = new ArrayList<>();
            // The whole membership, described: a row may need the track's own
            // metadata to be playable, and the preview is applied after.
            for (TrackKey key : keys) {
                MediaTrack track = session.describeRemembered(key);
                if (track != null) mine.add(track);
            }
            liked.setTitle("L1K3D", keys.size());
            // The heading counts what exists, not what is previewed.
            // The way out exists whenever the preview is a preview. Comparing the
            // preview against what was described would hide it exactly when the list
            // is long enough to need it.
            liked.setAction(keys.size() > PREVIEW_ROWS ? "Ver todo" : null,
                    () -> navigation.openLiked());
            if (mine.isEmpty()) liked.showEmpty("Todavía no hay nada en L1K3D.");
            else liked.show(preview(mine, PREVIEW_ROWS), false);
        }

        MediaTrack current = snapshot.currentTrack;
        if (current == null) {
            if (recommended.isVisibleGroup()) recommended.setVisibleGroup(false);
            recommendationsFor = null;
            return;
        }
        loadRecommendations(force = false);
    }

    /**
     * What can be played right now, for someone who has not played anything yet.
     *
     * <p>One block, and one idea: <b>what they saved to disk</b>. Not the whole
     * library — L1K3D is right below and a discovery screen that shows the same tracks
     * twice is not a discovery screen, it is a list with a heading on it. Downloads are
     * the part that is genuinely different from a taste: they are playable with no
     * network, which is exactly what somebody opening the app for the first time
     * needs.
     *
     * <p>And when there is nothing downloaded, there is nothing to offer but the one
     * action that can produce something: a recommendation is a function of something
     * played, and nothing has been played, so inventing one would mean inventing
     * tracks the app has never been told about.
     */
    private void renderStartHere(PlaybackSession session, boolean force) {
        List<MediaTrack> saved = new ArrayList<>(session.downloadedTracks());
        if (saved.isEmpty()) {
            startHere.setTitle("Empieza aquí", SectionHeader.NO_COUNT);
            startHere.setAction(null, null);
            // No tracks, no tracks: the block is the way in, not a list.
            startHere.showAction("Todavía no hay nada que reproducir. Busca algo y "
                    + "empieza por ahí.", "Abre la búsqueda", Icon.SEARCH,
                    () -> navigation.openSearch());
            return;
        }
        startHere.setTitle("Empieza aquí", saved.size());
        startHere.setAction(saved.size() > PREVIEW_ROWS ? "Ver descargas" : null,
                () -> navigation.openDownloads());
        startHere.show(preview(saved, PREVIEW_ROWS), false);
    }

    /**
     * Time-of-day greeting, written only when the part of day moved.
     *
     * <p>Cheap on purpose: the hour is read on every render and the label only
     * moves three times a day, so arriving with the same word costs one
     * comparison instead of a layout pass.
     */
    private void applyGreeting() {
        int hour = java.util.Calendar.getInstance().get(java.util.Calendar.HOUR_OF_DAY);
        String word = hour < 6 ? "Buenas noches" : hour < 13 ? "Buenos días" : hour < 21
                ? "Buenas tardes" : "Buenas noches";
        if (!word.equals(renderedGreeting)) {
            renderedGreeting = word;
            greeting.setText(word);
        }
    }

    /** Part of day the greeting was last written for; "" before the first. */
    private String renderedGreeting = "";

    /**
     * The rows a preview shows: the most recent ones, in order.
     *
     * <p>The tail, not the head: L1K3D is membership in the order it was built, and
     * what somebody wants from a preview of their own library is what they added last —
     * which is also the only part of the list that is not already old enough to have
     * been heard on Home before.
     *
     * @param all   the whole list, in membership order
     * @param limit how many rows the preview may show
     */
    static java.util.List<MediaTrack> preview(java.util.List<MediaTrack> all, int limit) {
        java.util.List<MediaTrack> source = all == null ? java.util.Collections.emptyList()
                : all;
        // A block that shows nothing is empty; it is not a block that shows
        // everything because someone asked for zero rows.
        if (limit <= 0) return java.util.Collections.emptyList();
        if (source.size() <= limit) return source;
        return new ArrayList<>(source.subList(source.size() - limit, source.size()));
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
            recommended.setTitle("Recomendado", tracks.size());
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
        startHere.setActiveTrack(key);
        liked.setActiveTrack(key);
        recommended.setActiveTrack(key);
    }

    @Override void refresh() {
        continueListening.refresh();
        startHere.refresh();
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