package com.tunefold.app;

import android.app.Activity;
import android.content.ComponentName;
import android.content.Intent;
import android.content.ServiceConnection;
import android.content.res.Configuration;
import android.graphics.drawable.ColorDrawable;
import android.os.Bundle;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.view.Gravity;
import android.view.View;
import android.widget.FrameLayout;
import android.widget.LinearLayout;
import android.widget.PopupMenu;
import android.widget.ScrollView;

import java.util.ArrayList;
import java.util.List;
import java.util.Objects;

/**
 * Tunefold Android UI.
 *
 * <p><b>One screen, four destinations, one player.</b> Home, Search, L1K3D and
 * Descargadas are four {@link SectionSurface}s behind a bottom navigation bar; the
 * player is a view that can cover them. Navigating is a visibility change, so no
 * navigation ever recreates the Activity, and nothing the user did in a destination is
 * lost by leaving it.
 *
 * <p><b>This Activity holds no playback state.</b> It binds to
 * {@link ForegroundPlaybackService}, renders {@link PlaybackSession} snapshots and
 * sends commands. The strip at the bottom is a second view onto the same snapshot —
 * there is no second playback model anywhere in the UI, which is the only way two
 * surfaces can be trusted to agree.
 *
 * <h3>What this class is responsible for</h3>
 * Composition and commands only. Every dimension comes from {@link DesignTokens},
 * every glyph from {@link Icon}, and every control from the shared components. The
 * Activity builds its tree once in {@link #buildInterface} and then only mutates it.
 *
 * <h3>Playback is the centre</h3>
 * When the player is open, the navigation is not on screen at all: the thing the user
 * came for gets the whole display, and one back gesture returns them exactly where
 * they were.
 */
public final class MainActivity extends Activity
        implements SearchSurface.SearchHost, HomeSurface.Navigation {

    private final Handler mainHandler = new Handler(Looper.getMainLooper());
    /** La pista cuya letra ya se pidió, y la que ya llegó: una comparación por repintado. */
    private String lyricsRequestedFor;
    private String lyricsDeliveredFor;
    /** La pista para la que ya se pidieron recomendados; uno por pista. */
    private String recsRequestedFor;
    /** Stable observer identity, so add/remove pair up across reconnects. */
    private final PlaybackSession.Observer sessionObserver = this::onSessionChanged;
    /** Reused across every tick, so polling the position allocates nothing. */
    private final PlaybackPosition position = new PlaybackPosition();
    /** Row handlers, held once so recycling never re-allocates a listener. */
    private final TrackRow.Callbacks rowCallbacks = new TrackRow.Callbacks() {
        @Override public void onPlay(TrackRow row) { playFromRow(row); }
        @Override public void onToggleLike(TrackRow row) { toggleLike(row.track()); }
        @Override public void onAdvanceDownload(TrackRow row) {
            advanceDownload(row.track());
        }
    };
    private PlaybackController controller;
    private ProviderRegistry providers;
    private PlaybackSession session;
    private Library library;
    private TrackDownloader downloader;
    private ForegroundPlaybackService.LocalBinder serviceBinder;
    private boolean bound;
    private boolean destroyed;
    /** The window the interface was last adapted to; {@code null} before the first. */
    private LayoutRules.Profile window;
    /** Width of the content box in pixels; what a centred column is measured against. */
    private int windowWidthPx;

    // ---- the tree ----
    private FrameLayout root;
    private ScrollHost scroller;
    private LinearLayout content;
    private MiniPlayer mini;
    private LinearLayout bottomBar;
    private LinearLayout navBar;
    private FrameLayout player;
    /** The control that leaves the player, kept so it can follow the card's edge. */
    private IconButton playerClose;
    private FrameLayout.LayoutParams playerCloseParams;
    private PlaybackDebugPanel debugPanel;
    private LiveAudioBars bars;

    private TrackList.RowPool rowPool;
    private HomeSurface homeSurface;
    private SearchSurface searchSurface;
    private LibrarySurface likedSurface;
    private LibrarySurface downloadsSurface;
    private final SectionSurface[] surfaces = new SectionSurface[Section.values().length];
    private final NavigationItem[] navItems = new NavigationItem[Section.values().length];

    private NowPlayingView nowPlaying;

    /** Which destination the content area is showing. */
    private enum Section { HOME, SEARCH, LIKED, DOWNLOADED }
    private Section section = Section.HOME;
    /** Where Search returns to; {@code null} means "wherever you were". */
    private Section returnFromSearch = Section.HOME;
    /** Whether the player is covering the navigation. */
    private boolean playerOpen;

    private int downloadsRenderedRevision = -1;

    /** Request code for the runtime POST_NOTIFICATIONS prompt. */
    private static final int NOTIFICATION_PERMISSION_REQUEST = 4201;

    // Row menu items. Named, because a menu whose branches are the numbers 1..5 is
    // a menu where adding an item silently reroutes another one.
    private static final int MENU_LIKE = 1;
    private static final int MENU_DOWNLOAD = 2;
    private static final int MENU_QUEUE = 3;
    private static final int MENU_QUEUE_CLEAR = 4;

    /** Visible to the surfaces, which need the session and the screen's own pieces. */
    PlaybackSession sessionForSurfaces() { return session; }

    private final ServiceConnection connection = new ServiceConnection() {
        @Override public void onServiceConnected(ComponentName name, IBinder service) {
            ForegroundPlaybackService.LocalBinder binder =
                    (ForegroundPlaybackService.LocalBinder) service;
            serviceBinder = binder;
            controller = binder.controller();
            providers = binder.providers();
            session = binder.session();
            library = binder.library();
            downloader = binder.downloader();
            // One live-audio source per binding, owned by the screen: it holds a
            // single reusable buffer, so a second one would only mean a second
            // copy of the same bars.
            bars = new LiveAudioBars(controller);
            // A stable field, so adding is idempotent and removing actually
            // matches: a method reference created twice is a different object,
            // which let a reconnect stack duplicate observers.
            session.addObserver(sessionObserver);
            applyWindow(true);
            showSection(Section.HOME, true);
            render();
            if (getIntent().getBooleanExtra("runtime_smoke_test", false)) resolveAndPlay();
            applyRuntimeDownloadHook();
        }

        @Override public void onServiceDisconnected(ComponentName name) {
            session = null;
            controller = null;
            providers = null;
            downloader = null;
            serviceBinder = null;
            // The source reads the engine of a controller the service owned: with
            // the service gone, that handle must not be touched again.
            bars = null;
            if (nowPlaying != null) nowPlaying.attachSound(null);
        }
    };

    @Override protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        // Before anything measures: every dimension in the tree comes from a token.
        DesignTokens.init(this);
        getWindow().setBackgroundDrawable(
                new ColorDrawable(DesignTokens.Palette.SURFACE_BASE));
        buildInterface();
        requestNotificationPermissionIfNeeded();
        Intent service = new Intent(this, ForegroundPlaybackService.class);
        bound = bindService(service, connection, BIND_AUTO_CREATE);
        mainHandler.post(poll);
    }

    /**
     * Asks for {@code POST_NOTIFICATIONS} on the versions that require it.
     *
     * <p>Declaring the permission is not enough from Android 13 on: without the runtime
     * grant the media notification is never shown, so lock-screen and headset controls
     * would look broken. Asked once per install; the system remembers a refusal, and
     * playback never depends on the answer.
     */
    private void requestNotificationPermissionIfNeeded() {
        if (android.os.Build.VERSION.SDK_INT < 33) return;
        if (checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS)
                == android.content.pm.PackageManager.PERMISSION_GRANTED) return;
        requestPermissions(new String[] {android.Manifest.permission.POST_NOTIFICATIONS},
                NOTIFICATION_PERMISSION_REQUEST);
    }

    // ------------------------------------------------------------------ UI

    private void buildInterface() {
        rowPool = new TrackList.RowPool(this, rowCallbacks);
        rowPool.setArtworkLoader(this::loadRowArtwork);
        rowPool.setOverflowHost(this);

        root = new FrameLayout(this);
        root.setBackgroundColor(DesignTokens.Palette.SURFACE_BASE);

        scroller = new ScrollHost(this);
        int gutter = DesignTokens.dp(DesignTokens.CONTENT_PADDING_HORIZONTAL);
        content = new LinearLayout(this);
        content.setOrientation(LinearLayout.VERTICAL);
        content.setPadding(gutter, DesignTokens.dp(DesignTokens.CONTENT_PADDING_TOP),
                gutter, DesignTokens.dp(DesignTokens.CONTENT_PADDING_BOTTOM));
        // Stop the search field from grabbing focus at launch, which raised the
        // keyboard over the player before the user asked for it.
        content.setFocusableInTouchMode(true);
        content.requestFocus();
        scroller.addView(content);
        FrameLayout.LayoutParams scrollerParams = new FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT);
        root.addView(scroller, scrollerParams);

        buildSurfaces();
        buildPlayer();
        buildBottomBar();
        setContentView(root);
        applySystemBars();
        applyWindow(true);
    }

    /**
     * The diagnostics toggle, parked at the top of Home.
     *
     * <p>A development tool is a thing that must be reachable and a thing that must
     * not be in the way: one 48dp target at the top of the discovery screen, which
     * opens a panel that is hidden by default and costs nothing until it is asked
     * for. It was in the top bar until the bottom navigation replaced it.
     */
    private LinearLayout.LayoutParams diagnosticsParams() {
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(
                DesignTokens.dp(DesignTokens.CONTROL_TARGET),
                DesignTokens.dp(DesignTokens.CONTROL_TARGET));
        params.gravity = Gravity.END;
        return params;
    }

    /** The four destinations, built once and kept. */
    private void buildSurfaces() {
        debugPanel = new PlaybackDebugPanel(this);
        content.addView(debugPanel.view(), spaced(DesignTokens.SPACE_2));

        IconButton diagnostics = new IconButton(this, DesignTokens.CONTROL_TARGET,
                DesignTokens.ICON_SIZE_SMALL);
        diagnostics.setAction(Icon.DIAGNOSTICS, "Diagnostics");
        diagnostics.setOnClickListener(view -> {
            debugPanel.toggle();
            renderDiagnostics();
        });
        content.addView(diagnostics, diagnosticsParams());

        homeSurface = new HomeSurface(this, this, this);
        searchSurface = new SearchSurface(this, this);
        likedSurface = new LibrarySurface(this, this, "L1K3D");
        downloadsSurface = new LibrarySurface(this, this, "Descargadas");
        surfaces[Section.HOME.ordinal()] = homeSurface;
        surfaces[Section.SEARCH.ordinal()] = searchSurface;
        surfaces[Section.LIKED.ordinal()] = likedSurface;
        surfaces[Section.DOWNLOADED.ordinal()] = downloadsSurface;
        for (SectionSurface surface : surfaces) {
            content.addView(surface, spaced(0f));
        }
    }

    /**
     * The player, which covers the navigation when it is open.
     *
     * <p>A view in this Activity and not a second Activity on purpose: an Activity
     * switch would tear down the window, rebuild the strip, re-decode the cover and
     * hand the service a new binding, all to show a view that was already built.
     */
    private void buildPlayer() {
        player = new FrameLayout(this);
        player.setBackgroundColor(DesignTokens.Palette.SURFACE_BASE);
        player.setVisibility(View.GONE);

        nowPlaying = new NowPlayingView(this, new NowPlayingView.Actions() {
            @Override public void onPlayPause() { session.togglePause(); }
            @Override public void onNext() { session.next(); }
            @Override public void onPrevious() { session.previous(); }
            @Override public void onLike() {
                session.toggleLike();
                refreshLists();
            }

            @Override public void onDownload() {
                // El botón del reproductor llega aquí en los dos estados en que no hay
                // descarga corriendo: lo que no está descargado —donde quitar no
                // significa nada y el toque se iba sin hacer nada— y lo que ya está
                // descargado, donde sí tiene que quitar. Antes estas dos cosas eran la
                // misma llamada, `remove`, y descargar desde el reproductor era
                // imposible: se cancelaba lo que no existía.
                MediaTrack track = session.snapshot().currentTrack;
                if (track == null || downloader == null) return;
                if (downloader.isDownloaded(track)) downloader.remove(track);
                else downloader.start(track);
                refreshLists();
            }

            @Override public void onCancelDownload() {
                MediaTrack track = session.snapshot().currentTrack;
                if (track != null) downloader.cancel(track);
            }

            @Override public void onRetryDownload() {
                MediaTrack track = session.snapshot().currentTrack;
                if (track != null) downloader.retry(track);
            }

            @Override public void onRetryPlayback() { session.retry(); }

            @Override public void onSkipToNext() { session.skipToNext(); }

            @Override public void onToggleAutoplay() {
                session.setAutoplayEnabled(!session.autoplayEnabled());
            }

            @Override public void onEnqueueRecommended(MediaTrack track) {
                if (session == null || track == null || track.key() == null) return;
                session.enqueue(track);
                // What joined Sigue is not offered again: the row goes and the
                // rest stay exactly where they were.
                nowPlaying.removeRecommended(track.key());
            }

            @Override public void onCreateRadio() {
                if (session != null) session.setRadioEnabled(true);
            }

            @Override public void onCreateArtistRadio() {
                if (session == null) return;
                MediaTrack current = session.snapshot().currentTrack;
                String who = current == null ? ""
                        : (!current.artist.isEmpty() ? current.artist : current.channel);
                if (!who.isEmpty()) session.setArtistRadioEnabled(true, who);
            }

            @Override public void onStopRadio() {
                if (session != null) session.setRadioEnabled(false);
            }
        });
        nowPlaying.setQueuedTrackChosen(new NowPlayingView.QueuedTrackChosen() {
            @Override public void onQueuedTrackChosen(MediaTrack track) {
                playFromUpNext(track);
            }

            @Override public void onQueuedTrackRemoved(MediaTrack track) {
                removeFromQueue(track);
            }
        });

        ScrollView playerScroller = new ScrollView(this);
        playerScroller.setFillViewport(true);
        playerScroller.setBackgroundColor(DesignTokens.Palette.SURFACE_BASE);
        playerScroller.addView(nowPlaying.view(), new ScrollView.LayoutParams(-1, -2));
        FrameLayout.LayoutParams scrollerParams = new FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT);
        player.addView(playerScroller, scrollerParams);

        // One control, top left, that means exactly what the system gesture means.
        playerClose = new IconButton(this);
        playerClose.setAction(Icon.BACK, "Close player");
        playerClose.setOnClickListener(view -> closePlayer());
        playerCloseParams = new FrameLayout.LayoutParams(
                DesignTokens.dp(DesignTokens.CONTROL_TARGET),
                DesignTokens.dp(DesignTokens.CONTROL_TARGET));
        playerCloseParams.leftMargin = DesignTokens.dp(DesignTokens.CONTENT_PADDING_HORIZONTAL);
        playerCloseParams.topMargin = DesignTokens.dp(DesignTokens.CONTENT_PADDING_TOP);
        player.addView(playerClose, playerCloseParams);

        root.addView(player, new FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT,
                FrameLayout.LayoutParams.MATCH_PARENT));
    }

    /**
     * The bottom of the screen: the strip and the destinations.
     *
     * <p>Two views in one column because they are one thing: what is playing and where
     * you can go are both "the bottom of the screen", and the strip is anchored to the
     * destinations so they cannot drift apart when one of them hides.
     */
    private void buildBottomBar() {
        bottomBar = new LinearLayout(this);
        bottomBar.setOrientation(LinearLayout.VERTICAL);
        bottomBar.setBackgroundColor(DesignTokens.BOTTOM_NAV_SURFACE);

        mini = new MiniPlayer(this, new MiniPlayer.Actions() {
            @Override public void onPlayPause() {
                if (session != null) session.togglePause();
            }

            @Override public void onExpand() { openPlayer(); }
        });
        bottomBar.addView(mini, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                DesignTokens.dp(DesignTokens.MINI_PLAYER_HEIGHT)));

        navBar = buildNavigation();
        bottomBar.addView(navBar, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                DesignTokens.dp(DesignTokens.BOTTOM_NAV_HEIGHT)));

        LinearLayout.LayoutParams bottomParams = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT);
        bottomParams.gravity = Gravity.BOTTOM;
        root.addView(bottomBar, new FrameLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT, Gravity.BOTTOM));
    }

    /**
     * The four destinations, at the bottom, as glyphs with captions.
     *
     * <p>Built once. Switching destination only flips each slot's selected state, so
     * the bar never re-inflates and its width never depends on the current label.
     */
    private LinearLayout buildNavigation() {
        LinearLayout nav = new LinearLayout(this);
        nav.setOrientation(LinearLayout.HORIZONTAL);
        int height = DesignTokens.dp(DesignTokens.CONTROL_TARGET_NAVIGATION);
        Section[] order = {Section.HOME, Section.SEARCH, Section.LIKED, Section.DOWNLOADED};
        for (int index = 0; index < order.length; index++) {
            Section target = order[index];
            NavigationItem item = new NavigationItem(this, glyphFor(target), labelFor(target));
            item.setOnClickListener(view -> showSection(target, true));
            navItems[target.ordinal()] = item;
            LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(0, height, 1f);
            if (index > 0) params.leftMargin = DesignTokens.dp(DesignTokens.SPACE_1);
            nav.addView(item, params);
        }
        return nav;
    }

    /**
     * The system's own bars are the window's, not ours: we pad and never draw them.
     *
     * <p>Insets are remembered rather than applied twice: the window the layout rules
     * measure is the window <em>after</em> them, and a phone in landscape is 2388 px
     * shorter than the display once the navigation bar and the cutout have taken their
     * share. Handing them to {@link #applyWindow} is what keeps a phone on its side
     * from being laid out as if it were standing up.
     *
     * <p>One listener, installed once, for the life of the screen: a second one would
     * be a second set of writes on every inset change, and the two would fight.
     */
    @SuppressWarnings("deprecation")
    private void applySystemBars() {
        root.setOnApplyWindowInsetsListener((view, system) -> {
            int left = system.getSystemWindowInsetLeft();
            int top = system.getSystemWindowInsetTop();
            int right = system.getSystemWindowInsetRight();
            int bottom = system.getSystemWindowInsetBottom();
            boolean moved = insets.left != left || insets.top != top
                    || insets.right != right || insets.bottom != bottom;
            insets.set(left, top, right, bottom);
            bottomBar.setPadding(0, 0, 0, bottom);
            if (moved) applyWindow(true);
            return system;
        });
    }

    private static Icon glyphFor(Section target) {
        switch (target) {
            case HOME: return Icon.HOME;
            case SEARCH: return Icon.SEARCH;
            case LIKED: return Icon.HEART;
            default: return Icon.DOWNLOADS;
        }
    }

    private static CharSequence labelFor(Section target) {
        switch (target) {
            case HOME: return "Home";
            case SEARCH: return "Search";
            case LIKED: return "L1K3D";
            default: return "Descargadas";
        }
    }

    private LinearLayout.LayoutParams spaced(float gapDp) {
        LinearLayout.LayoutParams params =
                new LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT,
                        LinearLayout.LayoutParams.WRAP_CONTENT);
        if (gapDp > 0f) params.topMargin = DesignTokens.dp(gapDp);
        return params;
    }

    private LinearLayout.LayoutParams spaced(int gapDp) { return spaced((float) gapDp); }

    /** The app's rounded background, so a surface cannot invent another one. */
    static android.graphics.drawable.Drawable roundDrawable(int color, float radiusDp) {
        android.graphics.drawable.GradientDrawable shape =
                new android.graphics.drawable.GradientDrawable();
        shape.setColor(color);
        shape.setCornerRadius(DesignTokens.dp(radiusDp));
        return shape;
    }

    // ------------------------------------------------------------ navigation

    /**
     * Shows one destination.
     *
     * <p>The switch is a visibility change on views that already exist, so it costs no
     * inflation, no Activity recreation and no state loss: a destination keeps whatever
     * the user did to it until they ask for something else.
     */
    private void showSection(Section target, boolean userInitiated) {
        if (section != Section.SEARCH && target == Section.SEARCH && userInitiated) {
            returnFromSearch = section;
        }
        section = target;
        for (Section candidate : Section.values()) {
            boolean active = candidate == target;
            navItems[candidate.ordinal()].setActiveDestination(active);
            surfaces[candidate.ordinal()].setVisibility(active ? View.VISIBLE : View.GONE);
        }
        scroller.scrollToTop();
        if (session == null) return;
        refreshSection();
        surfaces[target.ordinal()].onShow();
    }

    /** Fills the visible destination from the session, once. */
    private void refreshSection() {
        switch (section) {
            // Home re-derives in its own onShow(): one place, one decision, and the
            // "arriving" semantics are the surface's rather than the screen's.
            case SEARCH:
                searchSurface.showPrompt();
                break;
            case LIKED:
                renderLiked();
                break;
            case DOWNLOADED:
                renderDownloaded();
                break;
            default:
                break;
        }
    }

    @Override public void onSearchBack() {
        showSection(returnFromSearch, false);
    }

    @Override public void onSubmit(String query, SearchSurface.Intent intent) {
        if (session == null) return;
        if (intent == SearchSurface.Intent.URL) {
            session.playUrl(query);
            return;
        }
        searchSurface.showSearching();
        session.search(query, (tracks, error) -> {
            if (destroyed || section != Section.SEARCH) return;
            if (error != null) {
                searchSurface.showError(error);
                return;
            }
            searchSurface.showResults(tracks == null ? new ArrayList<>() : tracks);
        });
    }

    @Override public void onSearchIdle() { }

    // --------------------------------------------------------------- the player

    /** Al tocar "sigue", esa pista suena ahora; la lista no es un adorno. */
    private void playFromUpNext(MediaTrack track) {
        if (track == null || track.key() == null) return;
        if (session.playQueued(track.key())) {
            PlaybackTrace.markCurrent(PlaybackTrace.QUEUE_NEXT, "upnext=" + track.key());
            render();
        }
    }

    /** Opens the full player, hiding the navigation behind it. */
    private void openPlayer() {
        if (playerOpen) return;
        playerOpen = true;
        player.setVisibility(View.VISIBLE);
        scroller.setVisibility(View.GONE);
        bottomBar.setVisibility(View.GONE);
        // The bars only run while somebody can see them.
        nowPlaying.attachSound(bars);
        render();
    }

    /** Closes the player and gives the screen back exactly as it was. */
    private void closePlayer() {
        if (!playerOpen) return;
        playerOpen = false;
        player.setVisibility(View.GONE);
        scroller.setVisibility(View.VISIBLE);
        bottomBar.setVisibility(View.VISIBLE);
        nowPlaying.attachSound(null);
    }

    /** One back gesture means one thing: leave the player, then leave Search. */
    @Override public void onBackPressed() {
        if (playerOpen) {
            closePlayer();
            return;
        }
        if (section != Section.SEARCH) {
            super.onBackPressed();
            return;
        }
        showSection(returnFromSearch, false);
    }

    // ------------------------------------------------------------ behaviour

    private void resolveAndPlay() {
        String url = getIntent().getStringExtra("runtime_stream_url");
        if (url == null || session == null) return;
        session.playUrl(url);
        openPlayer();
    }

    /**
     * Deterministic hook so download flows can be exercised without screen taps.
     *
     * <p>It drives the same public commands the controls call — no second code path — and
     * only acts when the extra is present, so normal launches are unaffected.
     */
    private void applyRuntimeDownloadHook() {
        if (!getIntent().getBooleanExtra("runtime_download", false)) return;
        String action = getIntent().getStringExtra("runtime_download_action");
        String label = getIntent().getStringExtra("runtime_track_label");
        MediaTrack track = label == null ? null : new MediaTrack(
                "YouTube", label, label, "", "", "", "artist", -1, "", "", "{}");
        if (track == null) {
            return;
        }
        showSection(Section.SEARCH, false);
        if ("start".equals(action)) {
            session.play(track);
            startDownload();
        } else if ("cancel".equals(action)) {
            cancelDownload();
        } else if ("retry".equals(action)) {
            retryDownload();
        } else if ("like".equals(action)) {
            toggleLike(track);
        } else if ("remove".equals(action)) {
            downloader.remove(track);
        }
        render();
    }

    private void startDownload() {
        MediaTrack track = session == null ? null : session.snapshot().currentTrack;
        if (track == null || downloader == null) return;
        // §6: the downloader decides whether this is a start, a no-op because the file
        // exists, or a refusal because one is already running.
        downloader.start(track);
        refreshLists();
    }

    private void retryDownload() {
        MediaTrack track = session == null ? null : session.snapshot().currentTrack;
        if (track != null && downloader != null) downloader.retry(track);
    }

    /**
     * Cancels the in-flight download of the current track (§8). This must reach the
     * downloader: a control that only repainted would look functional and do nothing
     * (§26).
     */
    private void cancelDownload() {
        MediaTrack track = session == null ? null : session.snapshot().currentTrack;
        if (track != null && downloader != null) downloader.cancel(track);
        refreshLists();
        render();
    }

    /** Plays a track chosen from a row and shows its cover. */
    private void playFromRow(TrackRow row) {
        if (session == null || row == null) return;
        MediaTrack track = row.track();
        if (track == null) return;
        // The whole list the row is in becomes the queue, starting at the tapped track.
        // Playing one track from a list used to queue exactly that track, so the song
        // ended and the list stopped: tapping the third song of L1K3D played the third
        // song of L1K3D and nothing else, with no way to hear the next one.
        session.playList(row.listTracks(), track);
    }

    private void toggleLike(MediaTrack track) {
        if (library == null || track == null) return;
        library.toggleLiked(track);
        if (section == Section.LIKED) renderLiked();
        else refreshLists();
    }

    /**
     * Advances a row's download by one step.
     *
     * <p>The row's own rendered phase decides which step that is, and the control stays
     * enabled while a download runs so it can act as Cancel: an action the user cannot
     * reach would make cancellation look broken (§8, §26).
     */
    private void advanceDownload(MediaTrack track) {
        if (downloader == null || track == null) return;
        DownloadState state = downloader.stateOf(track);
        if (state.isActive()) downloader.cancel(track);
        else if (state.isRetryable()) downloader.retry(track);
        else if (state.isDownloaded()) downloader.remove(track);
        else downloader.start(track);
        refreshLists();
    }

    // ------------------------------------------------------------ navigation

    /**
     * Where Home's blocks lead.
     *
     * <p>The surface owns what a block says and this owns where a block goes: a view
     * that navigated would have to know about destinations, and a destination switch is
     * a visibility change on views that already exist — which is the whole reason it
     * costs nothing.
     */
    @Override public void openSearch() { showSection(Section.SEARCH, true); }

    @Override public void openLiked() { showSection(Section.LIKED, true); }

    @Override public void openDownloads() { showSection(Section.DOWNLOADED, true); }

    // ------------------------------------------------------------ row actions

    @Override public TrackList.RowPool rowPool() { return rowPool; }

    @Override public ScrollHost scrollHost() { return scroller; }

    @Override public boolean isLiked(MediaTrack track) {
        return library != null && library.isLiked(track);
    }

    @Override public DownloadState downloadOf(MediaTrack track) {
        if (downloader == null || track == null) return DownloadState.idle(null);
        // Memory only: one call per visible row per bind, on the main thread
        // during scroll. The probing read would stat the disk per row.
        return downloader.stateOfCached(track);
    }

    /**
     * A row's contextual actions, from its button or its long press.
     *
     * <p>Built per row because the actions depend on the row: a liked track offers to
     * unlike it, a downloaded one offers to remove it, a running download offers to
     * cancel it, and a track that is in the queue offers to take it out of it. A menu
     * that offered the same items regardless would be lying about two of them.
     */
    @Override public void onOverflow(TrackRow row, View anchor) {
        MediaTrack track = row.track();
        if (track == null) return;
        PopupMenu menu = new PopupMenu(this, anchor);
        boolean liked = row.isLiked();
        menu.getMenu().add(0, MENU_LIKE, 0, liked ? "Quitar de L1K3D" : "Añadir a L1K3D");
        DownloadState.Phase phase = row.downloadPhase();
        switch (phase) {
            case RESOLVING_SOURCE:
            case DOWNLOADING:
                menu.getMenu().add(0, MENU_DOWNLOAD, 1, "Cancelar descarga");
                break;
            case COMPLETED:
            case ALREADY_DOWNLOADED:
                menu.getMenu().add(0, MENU_DOWNLOAD, 1, "Quitar descarga");
                break;
            case FAILED:
            case CANCELLED:
                menu.getMenu().add(0, MENU_DOWNLOAD, 1, "Reintentar descarga");
                break;
            default:
                menu.getMenu().add(0, MENU_DOWNLOAD, 1, "Descargar");
                break;
        }
        // The queue is a decision the user can change, not a picture of one. It is
        // only offered where there is one: a track that is not queued has nothing to
        // take out of, and an item that always appears and always fails is worse than
        // one that is absent.
        if (session != null && session.isQueued(track.key())) {
            menu.getMenu().add(0, MENU_QUEUE, 2, "Quitar de la cola");
            if (session.snapshot().queue.size() > 1) {
                menu.getMenu().add(0, MENU_QUEUE_CLEAR, 3, "Vaciar la cola");
            }
        }
        menu.setOnMenuItemClickListener(item -> {
            switch (item.getItemId()) {
                case MENU_LIKE: toggleLike(track); break;
                case MENU_DOWNLOAD: advanceDownload(track); break;
                case MENU_QUEUE: removeFromQueue(track); break;
                default: clearQueue(); break;
            }
            return true;
        });
        menu.show();
    }

    /**
     * Takes a track out of the queue and re-derives whatever shows it.
     *
     * <p>Home's "continue listening" is that queue, so a removal that did not refresh
     * it would leave the row on screen claiming to be next.
     */
    private void removeFromQueue(MediaTrack track) {
        if (session == null || track == null) return;
        if (!session.removeFromQueue(track.key())) return;
        refreshLists();
        if (section == Section.HOME) homeSurface.render(true);
    }

    /** Empties the queue; the song playing keeps playing. */
    private void clearQueue() {
        if (session == null) return;
        if (!session.clearQueue()) return;
        refreshLists();
        if (section == Section.HOME) homeSurface.render(true);
    }

    // ---------------------------------------------------------- list sources

    /** L1K3D: membership only. Downloads are a separate, derived playlist. */
    private void renderLiked() {
        List<TrackKey> keys = session.likedKeys();
        List<MediaTrack> tracks = new ArrayList<>(Math.min(keys.size(), 64));
        int limit = LibrarySurface.limit(keys.size());
        for (int index = 0; index < limit; index++) {
            TrackKey key = keys.get(index);
            MediaTrack track = session.describeRemembered(key);
            // Membership is durable, but we may not hold the metadata yet. Showing the
            // identity beats pretending the track is gone.
            tracks.add(track != null ? track : placeholderFor(key));
        }
        likedSurface.submit(tracks, false,
                keys.isEmpty() ? "Nothing here yet. Use Like on any track." : null, null);
    }

    /**
     * `Descargadas`: a derived view of the LocalMediaStore (§5).
     *
     * <p>No membership list is stored. A deleted file disappears from here on the next
     * enumeration, and a downloaded track appears without ever being added.
     */
    private void renderDownloaded() {
        downloadsRenderedRevision = session.downloadsRevision();
        // Described by the session, which is where the remembered cover and title live.
        // Reading identities here and describing them as plain likes — which is what
        // this used to do for downloads — threw the cover away, so every row of the
        // list the user saved their music into rendered as a grey square.
        List<MediaTrack> stored = session.downloadedTracks();
        int limit = LibrarySurface.limit(stored.size());
        List<MediaTrack> tracks = new ArrayList<>(Math.min(limit, 64));
        for (int index = 0; index < limit; index++) tracks.add(stored.get(index));
        downloadsSurface.submit(tracks, false,
                stored.isEmpty() ? "No downloads yet. Use Download on any track." : null, null);
    }

    private MediaTrack placeholderFor(TrackKey key) {
        return new MediaTrack(key.provider(), key.providerTrackId(), key.providerTrackId(),
                "", "", "", "artist", -1, "", "", "{}");
    }

    /**
     * Re-applies like and download state to whatever rows are on screen.
     *
     * <p>Rebinding is O(window) and allocation-free, and it does not touch the layout
     * when nothing changed — which is what a list of fifty rows used to do several
     * times a second, rebuilding the section and moving the scroll position out from
     * under the user.
     */
    private void refreshLists() {
        TrackKey active = session == null ? null : session.snapshot().currentKey;
        for (SectionSurface surface : surfaces) {
            if (surface == null) continue;
            surface.setActiveKey(active);
            surface.refresh();
        }
    }

    // ------------------------------------------------------------- artwork

    /**
     * The one path covers take, at whatever size they will be drawn.
     *
     * <p>Rows ask through here when they are bound, which means a row that was never
     * built never asks: the windowing and the artwork loading are the same decision
     * seen from two sides.
     */
    private void loadRowArtwork(MediaTrack track, String identity, int sidePx) {
        if (providers == null || track == null) return;
        if (track.thumbnail.isEmpty()) {
            // A remembered track saved before the app kept its cover: nobody local can
            // draw it, so ask the provider once for what it is. The answer is
            // remembered where it belongs, so this is a first run and not a per-launch
            // cost, and the row that asked is the row that gets painted.
            if (session == null) return;
            session.learnMetadata(track.key(), (learned, failure) -> {
                if (destroyed || learned == null || learned.thumbnail.isEmpty()) return;
                loadRowArtwork(learned, identity, sidePx);
            });
            return;
        }
        providers.loadArtwork(track, sidePx, (bitmap, error) -> {
            if (destroyed || bitmap == null) return;
            // The row refuses any identity it is not waiting for, so a decode that
            // finishes after a fast scroll cannot paint the wrong cover on a row that
            // has already been rebound to something else.
            lists.clear();
            collectLists(content, lists);
            for (int i = 0; i < lists.size(); i++) {
                lists.get(i).onArtwork(identity, bitmap);
            }
        });
    }

    /** Reused, so a decode answering does not allocate while it paints. */
    private final ArrayList<TrackList> lists = new ArrayList<>(8);

    /** Every list under {@code root}, in one pass over the tree. */
    private static void collectLists(View root, List<TrackList> into) {
        if (root instanceof TrackList) into.add((TrackList) root);
        if (!(root instanceof android.view.ViewGroup)) return;
        android.view.ViewGroup group = (android.view.ViewGroup) root;
        for (int i = 0; i < group.getChildCount(); i++) {
            collectLists(group.getChildAt(i), into);
        }
    }

    /**
     * The cover of whatever is playing, decoded once for the size the player shows.
     *
     * <p>The strip and the player share the bitmap: a cover is the largest object in
     * the app, and decoding it twice for two views of one thing is how a screen that
     * looks simple ends up costing megabytes.
     */
    private void loadPlayerArtwork(MediaTrack track) {
        if (providers == null || track == null || track.thumbnail.isEmpty()) return;
        String identity = identityOf(track);
        if (identity.isEmpty()) return;
        if (identity.equals(playerCoverIdentity)) return;
        playerCoverIdentity = identity;
        nowPlaying.expectArtwork(identity);
        mini.expectArtwork(identity);
        providers.loadArtwork(track, nowPlaying.artworkSidePx(), (bitmap, error) -> {
            if (destroyed || bitmap == null) return;
            nowPlaying.setArtwork(identity, bitmap);
            mini.setArtwork(bitmap);
        });
    }

    /** Identity of the Sigue window whose covers are already loading. */
    private String queueArtKey = "";

    /**
     * Covers of the Sigue window, once per window.
     *
     * <p>The view owns which tracks are shown; this owns loading their covers.
     * Asked once per window identity rather than once per render, so a
     * notification-heavy minute does not requeue the same four decodes.
     */
    private void loadQueueArtwork() {
        if (session == null || providers == null) return;
        java.util.List<MediaTrack> shown = nowPlaying.queuedShown();
        StringBuilder identity = new StringBuilder(shown.size());
        for (MediaTrack track : shown) {
            TrackKey key = track.key();
            identity.append(key == null ? "" : key.toString()).append('\u0001');
        }
        String stamp = identity.toString();
        if (stamp.equals(queueArtKey)) return;
        queueArtKey = stamp;
        for (MediaTrack track : shown) loadThumbArtwork(track, nowPlaying::setQueueArtwork);
    }

    /**
     * One row-size cover delivery.
     *
     * <p>Shared by the Sigue window and the recommendations: same size, same
     * identity guard, different painter.
     */
    private void loadThumbArtwork(MediaTrack track,
                                  ThumbArtworkDrop drop) {
        if (providers == null || track == null || track.thumbnail.isEmpty()) return;
        TrackKey key = track.key();
        if (key == null) return;
        String identity = key.toString();
        providers.loadArtwork(track, DesignTokens.dp(DesignTokens.ARTWORK_THUMB),
                (bitmap, error) -> {
                    if (destroyed || bitmap == null) return;
                    drop.set(identity, bitmap);
                });
    }

    /** Where a decoded row cover lands. */
    private interface ThumbArtworkDrop {
        void set(String identity, android.graphics.Bitmap bitmap);
    }

    /** Identity of the cover currently on screen, so it is decoded once per track. */
    private String playerCoverIdentity = "";

    private static String identityOf(MediaTrack track) {
        TrackKey key = track.key();
        return key == null ? "" : key.toString();
    }

    // -------------------------------------------------------------- render

    /** Observer entry point: the session can change for reasons the UI did not cause. */
    private void onSessionChanged() {
        if (destroyed || session == null) return;
        render();
        if (section == Section.DOWNLOADED
                && session.downloadsRevision() != downloadsRenderedRevision) {
            // A newly downloaded track has no row to rebind; the store scan that
            // finishes after binding does the same.
            renderDownloaded();
        } else {
            refreshLists();
        }
        if (section == Section.LIKED) renderLiked();
        if (section == Section.HOME) homeSurface.onSessionChanged();
    }

    /**
     * Applies one snapshot to every part of the surface.
     *
     * <p>Deliberately narrow calls rather than one render: each one compares against
     * what it last rendered, so a change that touches nothing visible costs a handful
     * of comparisons instead of a dozen {@code setText} calls and the layout passes
     * they schedule.
     */
    private void render() {
        if (destroyed || session == null) return;
        PlaybackSession.Snapshot snapshot = session.snapshot();
        mini.apply(snapshot);
        if (playerOpen) {
            nowPlaying.applyTrack(snapshot);
            nowPlaying.applyPlaybackState(snapshot);
            nowPlaying.applyLibraryState(snapshot);
            nowPlaying.applyDownloadState(snapshot);
            nowPlaying.applyError(snapshot);
            nowPlaying.setUpNextFrom(snapshot.queue, snapshot.currentKey);
            nowPlaying.applyRadio(session.radioOn(), session.radioLabel());
            loadQueueArtwork();
            requestLyrics(snapshot);
            requestRecommendations(snapshot);
        }
        // One cover for both surfaces, decoded once at the size the player shows: the
        // strip is 40dp of it and the cache already holds it, so the strip costs
        // nothing of its own and the player opens with the picture already there.
        if (snapshot.currentTrack != null) loadPlayerArtwork(snapshot.currentTrack);
        if (debugPanel.isVisible()) renderDiagnostics();
    }

    /**
     * Pide la letra de lo que suena, una vez por pista.
     *
     * <p>El fallo se guarda como respuesta vacía a propósito: una canción sin letra
     * sincronizada es el resultado normal, no un error, y reintentarla en cada
     * repintado convertiría una respuesta de "no hay" en tráfico de red constante.
     */
    private void requestLyrics(PlaybackSession.Snapshot snapshot) {
        MediaTrack track = snapshot.currentTrack;
        TrackKey key = snapshot.currentKey;
        if (key == null || Objects.equals(lyricsRequestedFor, key.toString())) return;
        lyricsRequestedFor = key.toString();
        nowPlaying.setLyrics(null);
        session.lyrics(track, (lyric, error) -> {
            if (destroyed || key.toString().equals(lyricsDeliveredFor)) return;
            if (key.equals(session.snapshot().currentKey)) {
                lyricsDeliveredFor = key.toString();
                nowPlaying.setLyrics(lyric);
            }
        });
    }

    /**
     * Pide los recomendados de lo que suena, una vez por pista.
     *
     * <p>Descubrimiento, no autoplay: la sesión ya quita lo que suena y lo
     * encolado. Lo encolado desde la repisa desaparece de la oferta en cuanto
     * entra en Sigue.
     */
    private void requestRecommendations(PlaybackSession.Snapshot snapshot) {
        TrackKey key = snapshot.currentKey;
        String identity = key == null ? "" : key.toString();
        if (identity.equals(recsRequestedFor)) return;
        recsRequestedFor = identity;
        nowPlaying.clearRecommendations();
        if (key == null || session == null) return;
        MediaTrack track = snapshot.currentTrack;
        session.recommendationsForDisplay(track, (candidates, error) -> {
            if (destroyed || !identity.equals(recsRequestedFor)) return;
            List<MediaTrack> offered = candidates == null
                    ? new ArrayList<>() : new ArrayList<>(candidates);
            nowPlaying.setRecommendations(identity, offered);
            for (MediaTrack candidate : offered) loadRecommendedArtwork(candidate);
        });
    }

    /**
     * The cover of one recommended row, at row size.
     *
     * <p>One request per row per list: the view refuses any identity it is not
     * waiting for, so a decode that finishes after the list moved on cannot
     * paint the wrong cover.
     */
    private void loadRecommendedArtwork(MediaTrack track) {
        loadThumbArtwork(track, nowPlaying::setRecommendedArtwork);
    }

    private void renderDiagnostics() {
        if (session == null || !debugPanel.isVisible()) return;
        StringBuilder text = new StringBuilder(256);
        PlaybackSession.Snapshot snapshot = session.snapshot();
        // First, because it is the one line a reviewer of this surface came for, and
        // the panel caps its height: anything appended below the decoder trace can be
        // scrolled out of sight.
        text.append(nowPlaying.progressStats()).append('\n');
        text.append("state=").append(snapshot.state).append('\n');
        text.append("queue=").append(snapshot.queue.size())
                .append(" index=").append(snapshot.queueIndex).append('\n');
        lists.clear();
        collectLists(content, lists);
        StringBuilder window = new StringBuilder(64);
        for (int i = 0; i < lists.size(); i++) {
            if (i > 0) window.append(' ');
            int[] range = lists.get(i).currentWindow();
            window.append(range[0]).append('+').append(range[1])
                    .append('/').append(lists.get(i).trackCount());
        }
        text.append("lists=").append(window)
                .append(" built=").append(rowPool.builtCount())
                .append(" pooled=").append(rowPool.pooledCount()).append('\n');
        text.append("accent=").append(String.format(java.util.Locale.ROOT, "#%08X",
                        ArtworkTheme.accent()))
                .append(" bars=").append(String.format(java.util.Locale.ROOT, "#%08X/#%08X",
                        ArtworkTheme.barsPrimary(), ArtworkTheme.barsSecondary()))
                .append('\n');
        text.append("autoplay=").append(snapshot.autoplayEnabled).append('\n');
        if (downloader != null) {
            text.append("recovery=").append(downloader.recoverySummary()).append('\n');
        }
        if (session.diagnostics() != null) text.append(session.diagnostics()).append('\n');
        debugPanel.update(text.toString(), session.trace());
    }

    /**
     * Position tick.
     *
     * <p>The only periodic work left on the UI thread, and it reads through the cheap
     * path: no snapshot, no queue copy, no allocation, and the control skips the
     * {@code setText}/{@code setProgress} calls unless the second or the bar actually
     * moved. When the player is closed there is nothing position-driven on screen,
     * so the tick runs at the service cadence and only checks the downloader
     * attach — waking five times a second to do nothing would be pure battery.
     */
    private final Runnable poll = new Runnable() {
        @Override public void run() {
            if (destroyed) return;
            if (serviceBinder != null && downloader == null) {
                // The store may have finished initializing after we bound.
                TrackDownloader ready = serviceBinder.downloader();
                if (ready != null) {
                    downloader = ready;
                    if (session != null) session.attachDownloader(ready);
                }
            }
            if (session != null && playerOpen) {
                // One consistent read per tick: identity and position always come from
                // the same instant, so a tick can never pair one track with another
                // track's time.
                PlaybackPosition now = session.readPosition(position);
                nowPlaying.setPosition(now.key, now.positionMs, now.durationMs);
                // The only thing the tick animates: the working indicator, and only
                // while something is actually being waited for.
                nowPlaying.tick();
                // The bars move at the pace of the sound, not at the pace of the
                // clock: 20 Hz against the same consistent read the position uses.
                nowPlaying.refreshSound();
                mainHandler.postDelayed(this, DesignTokens.POSITION_INTERVAL_MS);
            } else {
                mainHandler.postDelayed(this, DesignTokens.SESSION_REFRESH_INTERVAL_MS);
            }
        }
    };

    // ------------------------------------------------------------ lifecycle

    /**
     * Re-applies the validation hooks when the Activity is already running.
     *
     * <p>Needed to reproduce rapid track switches deterministically: delivering a second
     * intent while the first track is still resolving is the only way to exercise the
     * supersede path on a real device. Validation-only, and it calls exactly the same
     * commands a tap would.
     */
    @Override protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        if (destroyed || session == null) return;
        if (intent.getBooleanExtra("runtime_smoke_test", false)) resolveAndPlay();
        applyRuntimeDownloadHook();
    }

    /**
     * Adapts the whole interface to the window it is in.
     *
     * <p><b>Once per change.</b> The numbers come from {@link LayoutRules}, which is
     * pure arithmetic, so the window is read here, once, and handed to the surfaces
     * that care. Nothing downstream recomputes a dimension: the cover's side is
     * decided in one place and every listener reads the result, which is what stops a
     * rotation from being three layout passes over a card holding the largest bitmap
     * in the app.
     *
     * <p><b>Insets are part of the window.</b> The content box is what is left of
     * the display after the status bar, the navigation bar and any cutout, and the
     * rules are applied to that box rather than to the panel — otherwise a phone in
     * landscape is measured as taller than the screen it is on.
     *
     * @param force re-read the display even if the layout rules did not change
     */
    private void applyWindow(boolean force) {
        if (nowPlaying == null) return;
        android.util.DisplayMetrics metrics = getResources().getDisplayMetrics();
        int widthPx = Math.max(1, metrics.widthPixels - insets.left - insets.right);
        int heightPx = Math.max(1, metrics.heightPixels - insets.top - insets.bottom);
        // The screen's smaller side, in dp: what decides whether this is a phone or a
        // tablet, in every orientation.
        windowWidthPx = widthPx;
        int smallestPx = Math.min(metrics.widthPixels, metrics.heightPixels);
        float density = Math.max(0.01f, metrics.density);
        LayoutRules.Profile profile = LayoutRules.of(Math.round(widthPx / density),
                Math.round(heightPx / density), Math.round(smallestPx / density),
                widthPx > heightPx);
        if (!force && LayoutRules.sameLayout(profile, window)) return;
        window = profile;
        // One line per configuration change, never per frame. It is the trace that
        // turns "the layout looked wrong on that tablet" into a number: the window
        // the app read, the rules it derived from it and the decision it took.
        android.util.Log.i("Tunefold", "window " + widthPx + "x" + heightPx
                + " @" + density + " -> " + profile.key()
                + " gutter=" + profile.gutterDp() + "dp"
                + " column=" + (profile.contentMaxDp() > 0
                        ? profile.contentMaxDp() + "dp" : "full")
                + " cover=" + Math.round(widthPx / (float) density) + "dp space");

        // One column of content, centred, with a maximum width: a tablet should not
        // turn a card into a letterbox, and a phone should not pay for a limit it
        // will never reach.
        applyContentWidth(profile);

        // The card's own width, not the window's: a tablet caps the column, and a
        // cover sized against the window would be wider than the column it sits in.
        int cardWidthPx = profile.contentMaxDp() > 0
                ? DesignTokens.dp(profile.contentMaxDp()) : widthPx;
        int inner = Math.max(0, cardWidthPx - DesignTokens.dp(DesignTokens.CARD_PADDING) * 2);
        // Height is only known once the player is on screen and measured; until then
        // the width rule alone decides, which is the safer of the two errors.
        int innerHeight = player != null && player.getHeight() > 0
                ? Math.max(0, player.getHeight() - DesignTokens.dp(DesignTokens.CARD_PADDING) * 2)
                : 0;
        nowPlaying.setWindow(profile, Math.round(inner / density), Math.round(innerHeight / density));
    }

    /**
     * Caps and centres the content column.
     *
     * <p>{@code 0} for a phone, which has no width to spare and keeps its gutter as
     * padding. On anything larger the column stops at {@link LayoutRules#CONTENT_MAX_DP}
     * and the leftover becomes margin around it, which is the difference between a
     * composition that breathes and one stretched across the window. Both the width
     * and the padding are written only when they change: this runs on every insets
     * change, and a {@code setLayoutParams} here is a measure of the whole content.
     */
    private void applyContentWidth(LayoutRules.Profile profile) {
        int max = profile.contentMaxDp();
        int gutterDp = profile.gutterDp();
        FrameLayout.LayoutParams params =
                (FrameLayout.LayoutParams) content.getLayoutParams();
        if (max <= 0) {
            if (params.width != FrameLayout.LayoutParams.MATCH_PARENT
                    || params.gravity != Gravity.NO_GRAVITY) {
                params.width = FrameLayout.LayoutParams.MATCH_PARENT;
                params.gravity = Gravity.NO_GRAVITY;
                content.setLayoutParams(params);
            }
            setContentPadding(gutterDp);
            applyCardWidth(0);
            return;
        }
        int limited = DesignTokens.dp(max);
        if (params.width != limited || params.gravity != Gravity.CENTER_HORIZONTAL) {
            params.width = limited;
            params.gravity = Gravity.CENTER_HORIZONTAL;
            content.setLayoutParams(params);
        }
        setContentPadding(0);
        applyCardWidth(limited);
    }

    /**
     * Caps the player's card at the same column as the lists.
     *
     * <p>The player is a different view over the same window, and a maximum width that
     * only the lists obey is a maximum width the player quietly ignores: on a tablet
     * the card stretched the whole way across while the lists beside it stopped at
     * 560dp, which is the letterbox this rule exists to prevent.
     *
     * @param widthPx {@code 0} for "the whole width", which is what a phone wants
     */
    private void applyCardWidth(int widthPx) {
        if (nowPlaying == null) return;
        View card = nowPlaying.view();
        FrameLayout.LayoutParams cardParams = (FrameLayout.LayoutParams) card.getLayoutParams();
        int wanted = widthPx <= 0 ? FrameLayout.LayoutParams.MATCH_PARENT : widthPx;
        int gravity = widthPx <= 0 ? Gravity.NO_GRAVITY : Gravity.CENTER_HORIZONTAL;
        if (cardParams.width == wanted && cardParams.gravity == gravity) return;
        cardParams.width = wanted;
        cardParams.gravity = gravity;
        card.setLayoutParams(cardParams);
        // The close control belongs to the card, so it follows the card's left edge
        // instead of sitting in the far corner of a window the card no longer fills.
        if (playerClose != null && playerCloseParams != null) {
            int side = widthPx <= 0 ? 0 : Math.max(0, (windowWidthPx - widthPx) / 2);
            int margin = DesignTokens.dp(DesignTokens.CONTENT_PADDING_HORIZONTAL) + side;
            if (playerCloseParams.leftMargin != margin) {
                playerCloseParams.leftMargin = margin;
                playerClose.setLayoutParams(playerCloseParams);
            }
        }
    }

    /**
     * The page padding: the gutter on the sides, the status bar inset on top.
     *
     * <p>Written only when a number moved, because the inset listener calls this on
     * every inset change and padding is a measure of the whole content.
     *
     * @param horizontalDp side padding, already converted; {@code 0} when the column
     *                     carries its own width instead
     */
    private void setContentPadding(int horizontalDp) {
        int left = DesignTokens.dp(horizontalDp);
        int right = left;
        int top = DesignTokens.dp(DesignTokens.CONTENT_PADDING_TOP) + insets.top;
        int bottom = DesignTokens.dp(DesignTokens.CONTENT_PADDING_BOTTOM);
        if (content.getPaddingLeft() == left && content.getPaddingTop() == top
                && content.getPaddingRight() == right && content.getPaddingBottom() == bottom) {
            return;
        }
        content.setPadding(left, top, right, bottom);
    }

    /** Insets the system has handed us, in pixels; zero until it does. */
    private final android.graphics.Rect insets = new android.graphics.Rect();

    @Override public void onConfigurationChanged(Configuration newConfig) {
        super.onConfigurationChanged(newConfig);
        // The user's font scale can change with the configuration; tokens must follow.
        DesignTokens.init(this);
        // Forced: a configuration change can change the insets without changing any
        // dimension the rules look at, and the column has to follow the bar.
        applyWindow(true);
    }

    @Override protected void onDestroy() {
        destroyed = true;
        mainHandler.removeCallbacks(poll);
        if (session != null) session.removeObserver(sessionObserver);
        // The card listens to the palette to decide how to compose, and a palette
        // listener that outlives its Activity keeps the whole window alive.
        if (nowPlaying != null) {
            nowPlaying.attachSound(null);
            nowPlaying.release();
        }
        if (bound) unbindService(connection);
        // Pooled rows are Views: dropping the pool is what lets the Activity and its
        // window go, instead of being retained by rows nobody will ever show again.
        if (rowPool != null) rowPool.clear();
        session = null;
        controller = null;
        providers = null;
        library = null;
        downloader = null;
        serviceBinder = null;
        super.onDestroy();
    }
}