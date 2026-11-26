package com.tunefold.app;

import android.content.Context;
import android.view.View;
import android.widget.HorizontalScrollView;
import android.widget.ScrollView;

import java.util.ArrayList;
import java.util.HashSet;
import java.util.Set;
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
    private final HorizontalScrollView discoveryScroll;
    private final android.widget.LinearLayout discoveryRow;
    private final android.widget.TextView discoveryTitle;
    private final HorizontalScrollView artistsScroll;
    private final android.widget.LinearLayout artistsRow;
    private final android.widget.TextView artistsTitle;
    private final HorizontalScrollView albumsScroll;
    private final android.widget.LinearLayout albumsRow;
    private final android.widget.TextView albumsTitle;

    private final android.widget.TextView continueTitle;
    private final HorizontalScrollView continueScroll;
    private final android.widget.LinearLayout continueRow;
    private final android.widget.TextView continueState;
    private final android.widget.TextView likedTitle;
    private final HorizontalScrollView likedScroll;
    private final android.widget.LinearLayout likedRow;
    private final android.widget.TextView likedState;
    private final android.widget.TextView recommendedTitle;
    private final HorizontalScrollView recommendedScroll;
    private final android.widget.LinearLayout recommendedRow;
    private final android.widget.TextView recommendedState;
    private String renderedDiscoveryKey = null;
    private java.util.List<MediaTrack> recommendedTracks = new java.util.ArrayList<>();
    private String renderedArtistQuery;
    private String renderedAlbumQuery;
    /** The first thing on the screen: who Tunefold is and what the library holds. */
    private final MastheadView masthead;

    /** Identity the recommendations were asked for; {@code null} for "never". */
    private String recommendationsFor = null;
    private boolean built;

    /** Last inputs the local blocks were derived from; see the note above. */
    private TrackKey renderedActiveKey;
    private int renderedQueueSize = -1;
    private int renderedLikedRevision = -1;
    /** Download set as it was last derived; see the note at its use. */
    private int renderedDownloadsRevision = -1;
    /**
     * Revisions the masthead's counts were last read at.
     *
     * <p>Revisions, not counts: reading the counts is the expensive half, and a
     * revision is the cheap half that tells you whether the expensive half would
     * change anything.
     */
    private int renderedMastheadLikedRevision = -1;
    private int renderedMastheadDownloadsRevision = -1;
    /** Whether the masthead's last caption mentioned something playing. */
    private boolean renderedMastheadPlaying;

    HomeSurface(Context context, Host host, Navigation navigation) {
        super(context, host);
        this.navigation = navigation;
        // The order this screen is read in.
        masthead = new MastheadView(context);
        addView(masthead, new LayoutParams(LayoutParams.MATCH_PARENT,
                LayoutParams.WRAP_CONTENT));
        continueListening = group("Sigue escuchando");
        startHere = group("Empieza aquí");
        liked = group("L1K3D");
        recommended = group("Recomendado");
        recommended.state().setActionHost(() -> loadRecommendations(force = true));

        continueTitle = new android.widget.TextView(context);
        continueTitle.setText("Sigue escuchando");
        continueTitle.setTextSize(android.util.TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_TITLE_SIZE);
        continueTitle.setTextColor(DesignTokens.Palette.ON_SURFACE);
        addView(continueTitle, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        continueScroll = new HorizontalScrollView(context);
        continueScroll.setHorizontalScrollBarEnabled(false);
        continueRow = new android.widget.LinearLayout(context);
        continueRow.setOrientation(android.widget.LinearLayout.HORIZONTAL);
        continueScroll.addView(continueRow, new LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT));
        addView(continueScroll, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        continueState = new android.widget.TextView(context);
        continueState.setTextColor(DesignTokens.Palette.ON_SURFACE_MUTED);
        addView(continueState, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));

        likedTitle = new android.widget.TextView(context);
        likedTitle.setText("L1K3D");
        likedTitle.setTextSize(android.util.TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_TITLE_SIZE);
        likedTitle.setTextColor(DesignTokens.Palette.ON_SURFACE);
        addView(likedTitle, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        likedScroll = new HorizontalScrollView(context);
        likedScroll.setHorizontalScrollBarEnabled(false);
        likedRow = new android.widget.LinearLayout(context);
        likedRow.setOrientation(android.widget.LinearLayout.HORIZONTAL);
        likedScroll.addView(likedRow, new LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT));
        addView(likedScroll, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        likedState = new android.widget.TextView(context);
        likedState.setTextColor(DesignTokens.Palette.ON_SURFACE_MUTED);
        addView(likedState, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));

        recommendedTitle = new android.widget.TextView(context);
        recommendedTitle.setText("Recomendado");
        recommendedTitle.setTextSize(android.util.TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_TITLE_SIZE);
        recommendedTitle.setTextColor(DesignTokens.Palette.ON_SURFACE);
        addView(recommendedTitle, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        recommendedScroll = new HorizontalScrollView(context);
        recommendedScroll.setHorizontalScrollBarEnabled(false);
        recommendedRow = new android.widget.LinearLayout(context);
        recommendedRow.setOrientation(android.widget.LinearLayout.HORIZONTAL);
        recommendedScroll.addView(recommendedRow, new LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT));
        addView(recommendedScroll, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        recommendedState = new android.widget.TextView(context);
        recommendedState.setTextColor(DesignTokens.Palette.ON_SURFACE_MUTED);
        addView(recommendedState, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));

        discoveryTitle = new android.widget.TextView(context);
        discoveryTitle.setText("Descubre");
        discoveryTitle.setTextSize(android.util.TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_TITLE_SIZE);
        discoveryTitle.setTextColor(DesignTokens.Palette.ON_SURFACE);
        addView(discoveryTitle, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));

        discoveryScroll = new HorizontalScrollView(context);
        discoveryScroll.setHorizontalScrollBarEnabled(false);
        discoveryRow = new android.widget.LinearLayout(context);
        discoveryRow.setOrientation(android.widget.LinearLayout.HORIZONTAL);
        discoveryScroll.addView(discoveryRow, new LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT));
        addView(discoveryScroll, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));

        artistsTitle = new android.widget.TextView(context);
        artistsTitle.setText("Artistas para descubrir");
        artistsTitle.setTextSize(android.util.TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_TITLE_SIZE);
        artistsTitle.setTextColor(DesignTokens.Palette.ON_SURFACE);
        addView(artistsTitle, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        artistsScroll = new HorizontalScrollView(context);
        artistsScroll.setHorizontalScrollBarEnabled(false);
        artistsRow = new android.widget.LinearLayout(context);
        artistsRow.setOrientation(android.widget.LinearLayout.HORIZONTAL);
        artistsScroll.addView(artistsRow, new LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT));
        addView(artistsScroll, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));

        albumsTitle = new android.widget.TextView(context);
        albumsTitle.setText("Álbumes para descubrir");
        albumsTitle.setTextSize(android.util.TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_TITLE_SIZE);
        albumsTitle.setTextColor(DesignTokens.Palette.ON_SURFACE);
        addView(albumsTitle, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        albumsScroll = new HorizontalScrollView(context);
        albumsScroll.setHorizontalScrollBarEnabled(false);
        albumsRow = new android.widget.LinearLayout(context);
        albumsRow.setOrientation(android.widget.LinearLayout.HORIZONTAL);
        albumsScroll.addView(albumsRow, new LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT));
        addView(albumsScroll, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));

        // The previews lead to the whole list; the queue does not, because the rest of
        // it is the player and the player is one tap away in the strip.
        liked.setAction("Ver todo", () -> navigation.openLiked());

        for (TrackGroup candidate : new TrackGroup[] {continueListening, startHere, liked,
                recommended}) {
            candidate.setRangeListener((first, count) -> requestArtwork(candidate));
        }
    }

    /**
     * Puts the one control this screen's header may carry in the masthead row.
     *
     * <p>The diagnostics toggle is a development tool and belongs to the surface
     * the developer is looking at, not to the Activity's chrome: parked in the
     * masthead it costs 48&nbsp;dp of a row that is already there instead of a
     * full-width strip of its own above the identity.
     */
    void setAccessory(View accessory) { masthead.setAccessory(accessory); }

    private TrackGroup group(String title) {
        // State machine only: the visible representation is the mosaic rail below.
        return new TrackGroup(getContext(), title, host.rowPool(), host, host.scrollHost());
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
        PlaybackSession.Snapshot snapshot = session.snapshot();
        TrackKey active = snapshot.currentKey;
        int likedRevision = session.libraryRevision();
        applyMasthead(session, snapshot, force);

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
                continueTitle.setText("Sigue escuchando");
                java.util.List<MediaTrack> cont = continuing();
                populateSongRail(continueRow, cont, cont);
                continueState.setText(cont == null || cont.isEmpty() ? "Todavía no hay nada que reproducir." : "");
                continueScroll.setVisibility(cont == null || cont.isEmpty() ? View.GONE : View.VISIBLE);
                continueListening.setTitle("Sigue escuchando", snapshot.queue.size());
                continueListening.show(cont, false);
            } else {
                continueTitle.setText("Empieza aquí");
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
            liked.setAction(keys.size() > PREVIEW_ROWS ? "Ver todo" : null,
                    () -> navigation.openLiked());
            java.util.List<MediaTrack> likedPreview = mine.isEmpty() ? new java.util.ArrayList<>()
                    : preview(mine, PREVIEW_ROWS);
            if (mine.isEmpty()) {
                liked.showEmpty("Todavía no hay nada en L1K3D.");
                likedState.setText("Todavía no hay nada en L1K3D.");
                likedScroll.setVisibility(View.GONE);
            } else {
                liked.show(likedPreview, false);
                populateSongRail(likedRow, likedPreview, likedPreview);
                likedState.setText("");
                likedScroll.setVisibility(View.VISIBLE);
            }
        }

        renderDiscovery(session, snapshot, force);

        MediaTrack current = snapshot.currentTrack;
        if (current == null) {
            if (recommended.isVisibleGroup()) recommended.setVisibleGroup(false);
            recommendedTitle.setVisibility(View.GONE);
            recommendedScroll.setVisibility(View.GONE);
            recommendedState.setText("");
            recommendationsFor = null;
            return;
        }
        recommendedTitle.setVisibility(View.VISIBLE);
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
            startHere.showAction("Todavía no hay nada que reproducir. Busca algo y "
                    + "empieza por ahí.", "Abre la búsqueda", Icon.SEARCH,
                    () -> navigation.openSearch());
            continueState.setText("Todavía no hay nada que reproducir. Busca algo y empieza por ahí.");
            continueScroll.setVisibility(View.GONE);
            return;
        }
        startHere.setTitle("Empieza aquí", saved.size());
        startHere.setAction(saved.size() > PREVIEW_ROWS ? "Ver descargas" : null,
                () -> navigation.openDownloads());
        List<MediaTrack> previewSaved = preview(saved, PREVIEW_ROWS);
        startHere.show(previewSaved, false);
        populateSongRail(continueRow, previewSaved, previewSaved);
        continueState.setText("");
        continueScroll.setVisibility(View.VISIBLE);
    }

    /**
     * The masthead: who Tunefold is, and what the library holds.
     *
     * <p><b>Cheap on purpose, and the cheapness is the whole point.</b> The session
     * notifies twice a second for every reason at once, and reading the two counts
     * is not free: L1K3D hands over a copied list and a downloads count taken from
     * identities that each have to be walked. So the counts are asked for only when a
     * <em>revision</em> moved, which is a field load — the same rule the rest of this
     * screen follows for the same reason. The greeting is compared inside the
     * masthead, because the hour moves three times a day and the label is written
     * once for each of them.
     *
     * <p>Nothing here is owned by this surface: L1K3D is membership and downloads
     * are a derived view of the store, and this only says what they currently hold.
     */
    private void applyMasthead(PlaybackSession session, PlaybackSession.Snapshot snapshot,
            boolean force) {
        boolean playing = snapshot.currentTrack != null;
        int likedRevision = session.libraryRevision();
        int downloadsRevision = session.downloadsRevision();
        String greeting = greetingForHour();
        if (force || playing != renderedMastheadPlaying
                || likedRevision != renderedMastheadLikedRevision
                || downloadsRevision != renderedMastheadDownloadsRevision
                || !greeting.equals(renderedGreeting)) {
            renderedMastheadLikedRevision = likedRevision;
            renderedMastheadDownloadsRevision = downloadsRevision;
            renderedMastheadPlaying = playing;
            // El saludo se compara aquí y no dentro del masthead porque depende de
            // dos cosas —la hora y el nombre— y sólo una de ellas la resuelve la
            // vista; si el nombre cambia, la frase cambia aunque la hora no.
            renderedGreeting = greeting;
            masthead.apply(greeting, session.likedKeys().size(),
                    session.downloadedKeys().size(), playing);
        }
    }

    /** La última frase de saludo escrita, para no repetirla. */
    private String renderedGreeting = "";

    /**
     * Part of day and name, resolved once per render.
     *
     * <p>El nombre se lee de las preferencias del propio contexto, que es donde vive
     * y no en esta pantalla: los ajustes lo escriben, esta pantalla sólo lo dice.
     */
    private String greetingForHour() {
        return MastheadView.greetingForHour(
                java.util.Calendar.getInstance().get(java.util.Calendar.HOUR_OF_DAY),
                UiTheme.displayName(getContext()));
    }

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
    static java.util.List<MediaTrack> distinctArtists(java.util.List<MediaTrack> tracks) {
        return artistsFrom(tracks);
    }

    static java.util.List<MediaTrack> artistsFrom(java.util.List<MediaTrack> tracks) {
        java.util.List<MediaTrack> out = new java.util.ArrayList<>();
        java.util.Set<String> seen = new java.util.HashSet<>();
        if (tracks == null) return out;
        for (MediaTrack track : tracks) {
            if (track == null) continue;
            String name = track.artist == null || track.artist.trim().isEmpty()
                    ? (track.channel == null ? "" : track.channel.trim())
                    : track.artist.trim();
            if (name.isEmpty() || !seen.add(name)) continue;
            out.add(new MediaTrack(track.provider, name, name, name, track.channel, "",
                    "artist", -1, track.thumbnail, "", track.rawJson));
        }
        return out;
    }

    static java.util.List<MediaTrack> artistTiles(java.util.List<MediaTrack> tracks) {
        return artistsFrom(tracks);
    }

    static java.util.List<MediaTrack> distinctAlbums(java.util.List<MediaTrack> tracks) {
        return albumsFrom(tracks);
    }

    static java.util.List<MediaTrack> albumsFrom(java.util.List<MediaTrack> tracks) {
        java.util.List<MediaTrack> out = new java.util.ArrayList<>();
        java.util.Set<String> seen = new java.util.HashSet<>();
        if (tracks == null) return out;
        for (MediaTrack track : tracks) {
            if (track == null) continue;
            String name = track.album == null ? "" : track.album.trim();
            if (name.isEmpty()) name = track.title == null ? "" : track.title.trim();
            if (name.isEmpty() || !seen.add(name)) continue;
            out.add(new MediaTrack("Album", name, name, track.artist == null ? "" : track.artist,
                    "", name, "artist", -1, track.thumbnail, "", track.rawJson));
        }
        return out;
    }

    static java.util.List<MediaTrack> albumTiles(java.util.List<MediaTrack> tracks) {
        return albumsFrom(tracks);
    }

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

    private void renderDiscovery(PlaybackSession session, PlaybackSession.Snapshot snapshot, boolean force) {
        MediaTrack current = snapshot.currentTrack;
        String key = current == null ? "none" : identityOf(current);
        if (!force && key.equals(renderedDiscoveryKey)) return;
        renderedDiscoveryKey = key;
        discoveryRow.removeAllViews();
        artistsRow.removeAllViews();
        albumsRow.removeAllViews();

        if (current == null) {
            discoveryTitle.setVisibility(View.GONE);
            discoveryScroll.setVisibility(View.GONE);
            artistsTitle.setVisibility(View.GONE);
            artistsScroll.setVisibility(View.GONE);
            albumsTitle.setVisibility(View.GONE);
            albumsScroll.setVisibility(View.GONE);
            renderedArtistQuery = null;
            renderedAlbumQuery = null;
            return;
        }
        discoveryTitle.setVisibility(View.VISIBLE);
        discoveryScroll.setVisibility(View.VISIBLE);

        DiscoveryTile radio = new DiscoveryTile(getContext());
        radio.setVariant(DiscoveryTile.Variant.FEATURED);
        radio.setTitle(session.radioOn() ? "Detener radio" : "Radio");
        radio.setSubtitle(current.title);
        radio.setCategory("Estación");
        if (!current.thumbnail.isEmpty()) radio.artwork().expectCover(current.thumbnail);
        ((MainActivity) getContext()).loadTileArtwork(current, radio.artwork());
        radio.setOnClickListener(v -> {
            if (session.radioOn()) session.setRadioEnabled(false);
            else session.setRadioEnabled(true);
        });
        discoveryRow.addView(radio, new android.widget.LinearLayout.LayoutParams(DesignTokens.dp(160),
                android.widget.LinearLayout.LayoutParams.WRAP_CONTENT));

        // Artistas reales.
        String artistQuery = current.artist == null ? "" : current.artist.trim();
        if (artistQuery.isEmpty()) artistQuery = current.channel == null ? "" : current.channel.trim();
        if (!artistQuery.equals(renderedArtistQuery)) {
            renderedArtistQuery = artistQuery;
            artistsRow.removeAllViews();
            if (artistQuery.isEmpty()) {
                artistsTitle.setVisibility(View.GONE);
                artistsScroll.setVisibility(View.GONE);
            } else {
                artistsTitle.setVisibility(View.VISIBLE);
                artistsScroll.setVisibility(View.VISIBLE);
                ((MainActivity) getContext()).loadHomeArtists(artistQuery, tracks -> {
                    artistsRow.removeAllViews();
                    if (tracks == null || tracks.isEmpty()) {
                        artistsTitle.setVisibility(View.GONE);
                        artistsScroll.setVisibility(View.GONE);
                        return;
                    }
                    for (MediaTrack a : tracks) {
                        if (a == null) continue;
                        DiscoveryTile tile = new DiscoveryTile(getContext());
                        tile.setTitle(a.title);
                        tile.setSubtitle("Artista");
                        tile.setCategory("Artista");
                        if (!a.thumbnail.isEmpty()) tile.artwork().expectCover(a.thumbnail);
                        ((MainActivity) getContext()).loadTileArtwork(a, tile.artwork());
                        tile.setOnClickListener(v -> ((MainActivity) getContext()).openArtistDetail(a));
                        artistsRow.addView(tile, new android.widget.LinearLayout.LayoutParams(DesignTokens.dp(120),
                                android.widget.LinearLayout.LayoutParams.WRAP_CONTENT));
                    }
                });
            }
        }

        // Álbumes reales.
        String albumQuery = current.album == null ? "" : current.album.trim();
        if (albumQuery.isEmpty()) albumQuery = current.title == null ? "" : current.title.trim();
        if (!albumQuery.equals(renderedAlbumQuery)) {
            renderedAlbumQuery = albumQuery;
            albumsRow.removeAllViews();
            if (albumQuery.isEmpty()) {
                albumsTitle.setVisibility(View.GONE);
                albumsScroll.setVisibility(View.GONE);
            } else {
                albumsTitle.setVisibility(View.VISIBLE);
                albumsScroll.setVisibility(View.VISIBLE);
                ((MainActivity) getContext()).loadHomeAlbums(albumQuery, tracks -> {
                    albumsRow.removeAllViews();
                    if (tracks == null || tracks.isEmpty()) {
                        albumsTitle.setVisibility(View.GONE);
                        albumsScroll.setVisibility(View.GONE);
                        return;
                    }
                    for (MediaTrack a : tracks) {
                        if (a == null) continue;
                        DiscoveryTile tile = new DiscoveryTile(getContext());
                        tile.setTitle(a.title);
                        tile.setSubtitle(a.artist == null ? "" : a.artist);
                        tile.setCategory("Álbum");
                        if (!a.thumbnail.isEmpty()) tile.artwork().expectCover(a.thumbnail);
                        ((MainActivity) getContext()).loadTileArtwork(a, tile.artwork());
                        tile.setOnClickListener(v -> ((MainActivity) getContext()).openAlbumDetail(a));
                        albumsRow.addView(tile, new android.widget.LinearLayout.LayoutParams(DesignTokens.dp(120),
                                android.widget.LinearLayout.LayoutParams.WRAP_CONTENT));
                    }
                });
            }
        }
    }

    /**
     * Recommendations, at most one request per track.
     *
     * <p>A failure leaves the rest of Home usable and offers the one action that can
     * fix it, rather than an error screen over a surface that works.
     */
    private void populateSongRail(android.widget.LinearLayout row, java.util.List<MediaTrack> tracks, java.util.List<MediaTrack> playList) {
        row.removeAllViews();
        if (tracks == null || tracks.isEmpty()) return;
        for (MediaTrack track : tracks) {
            if (track == null) continue;
            DiscoveryTile tile = new DiscoveryTile(getContext());
            tile.setTitle(track.title);
            tile.setSubtitle(track.artist == null || track.artist.isEmpty() ? track.channel : track.artist);
            tile.setCategory("Canción");
            if (track.thumbnail != null && !track.thumbnail.isEmpty()) tile.artwork().expectCover(track.thumbnail);
            ((MainActivity) getContext()).loadTileArtwork(track, tile.artwork());
            tile.setOnClickListener(v -> {
                PlaybackSession s = session();
                if (s != null && playList != null) s.playList(playList, track);
            });
            row.addView(tile, new android.widget.LinearLayout.LayoutParams(DesignTokens.dp(120),
                    android.widget.LinearLayout.LayoutParams.WRAP_CONTENT));
        }
    }

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
        recommendedState.setText("Cargando…");
        recommendedScroll.setVisibility(View.GONE);
        recommendedRow.removeAllViews();
        session.recommendationsForHome(current, (candidates, error) -> {
            if (getVisibility() != View.VISIBLE) return;
            if (error != null) {
                recommended.showError(error, () -> loadRecommendations(true));
                recommendedState.setText(error);
                recommendedScroll.setVisibility(View.GONE);
                return;
            }
            List<MediaTrack> tracks = candidates == null ? new ArrayList<>() : candidates;
            recommendedTracks = new ArrayList<>(tracks);
            recommended.setTitle("Recomendado", tracks.size());
            recommended.show(tracks, false);
            populateSongRail(recommendedRow, tracks, tracks);
            recommendedState.setText(tracks.isEmpty() ? "Sin recomendaciones todavía." : "");
            recommendedScroll.setVisibility(tracks.isEmpty() ? View.GONE : View.VISIBLE);
            PlaybackSession s = session();
            if (s != null) renderDiscovery(s, s.snapshot(), true);
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