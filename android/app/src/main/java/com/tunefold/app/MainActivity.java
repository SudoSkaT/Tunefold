package com.tunefold.app;

import android.app.Activity;
import android.content.ComponentName;
import android.content.Intent;
import android.content.ServiceConnection;
import android.content.res.Configuration;
import android.graphics.drawable.ColorDrawable;
import android.graphics.drawable.GradientDrawable;
import android.os.Bundle;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.text.InputType;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.view.inputmethod.EditorInfo;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

import java.util.ArrayList;
import java.util.List;

/**
 * Tunefold Android UI.
 *
 * <p>Navigation over Home, Search, L1K3D and Descargadas, with Now Playing as the
 * primary surface (§24). Deliberately a single screen with a section switch rather than
 * a multi-screen architecture: the app does not yet need it, and one surface keeps
 * playback state visible everywhere.
 *
 * <p>This Activity holds <b>no playback state</b>. It binds to
 * {@link ForegroundPlaybackService}, renders {@link PlaybackSession} snapshots and
 * sends commands. Rotation and Activity recreation therefore rebuild the whole screen
 * from service-owned state and cannot desynchronise (§18).
 *
 * <h3>What this class is responsible for</h3>
 * Composition and commands only. Every dimension comes from {@link DesignTokens},
 * every glyph from {@link Icon}, and every control from the shared components. The
 * Activity builds its tree once in {@link #buildInterface} and then only mutates it:
 * nothing here re-creates a view after construction, and nothing here re-reads a value
 * it could have cached.
 */
public final class MainActivity extends Activity implements TrackList.StateSource {

    private final Handler mainHandler = new Handler(Looper.getMainLooper());
    /** Stable observer identity, so add/remove pair up across reconnects. */
    private final PlaybackSession.Observer sessionObserver = this::onSessionChanged;
    /** Reused across every tick, so polling the position allocates nothing. */
    private final PlaybackPosition position = new PlaybackPosition();
    /** Row handlers, held once so recycling never re-allocates a listener. */
    private final TrackRow.Callbacks rowCallbacks = new TrackRow.Callbacks() {
        @Override public void onPlay(TrackRow row) { playFromRow(row.track()); }
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
    private boolean landscape;

    private TrackList.RowPool rowPool;
    private NowPlayingView nowPlaying;
    private PlaybackDebugPanel debugPanel;
    private EditText queryInput;
    private IconButton searchAction;
    private LinearLayout sectionContent;
    private TextView statusLine;
    private final NavigationItem[] navItems = new NavigationItem[Section.values().length];

    /** Which list the content area is showing. */
    private enum Section { HOME, SEARCH, LIKED, DOWNLOADED }
    private Section section = Section.HOME;

    /** Identity Home was last rendered for, so it only reloads on a real change. */
    private String homeTrackKey = "";
    /**
     * {@code Descargadas} revision the visible list was built from.
     *
     * <p>The list is re-derived, not merely re-bound, when this changes: a newly
     * downloaded track has no row to rebind. Without it the view stayed permanently
     * empty after a restart, because it was built before the store scan reported what
     * was already on disk.
     */
    private int downloadsRenderedRevision = -1;
    private boolean searchInFlight;

    /** Request code for the runtime POST_NOTIFICATIONS prompt. */
    private static final int NOTIFICATION_PERMISSION_REQUEST = 4201;

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
            // A stable field, so adding is idempotent and removing actually
            // matches: a method reference created twice is a different object,
            // which let a reconnect stack duplicate observers.
            session.addObserver(sessionObserver);
            applyOrientation(getResources().getConfiguration());
            loadSection(Section.HOME);
            if (getIntent().getBooleanExtra("runtime_smoke_test", false)) resolveAndPlay();
            applyRuntimeDownloadHook();
        }

        @Override public void onServiceDisconnected(ComponentName name) {
            session = null;
            controller = null;
            providers = null;
            downloader = null;
            serviceBinder = null;
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
        int gutter = DesignTokens.dp(DesignTokens.CONTENT_PADDING_HORIZONTAL);

        // A ScrollView keeps every control reachable when the window is short.
        ScrollView scroller = new ScrollView(this);
        scroller.setFillViewport(true);
        LinearLayout page = new LinearLayout(this);
        page.setOrientation(LinearLayout.VERTICAL);
        page.setBackgroundColor(DesignTokens.Palette.SURFACE_BASE);
        page.setPadding(gutter, DesignTokens.dp(DesignTokens.CONTENT_PADDING_TOP),
                gutter, DesignTokens.dp(DesignTokens.CONTENT_PADDING_BOTTOM));
        // Stop the search field from grabbing focus at launch, which raised the
        // keyboard over the Now Playing panel before the user asked for it.
        page.setFocusableInTouchMode(true);
        page.requestFocus();
        scroller.addView(page, new ScrollView.LayoutParams(-1, -2));

        page.addView(buildTopBar(), spaced(-1, -2, 0f));

        LinearLayout nav = buildNavigation();
        page.addView(nav, spaced(-1, -2, DesignTokens.REGION_GAP));

        page.addView(buildSearchRow(), spaced(-1, -2, DesignTokens.REGION_GAP));

        // ---- Now Playing (primary surface) ----
        nowPlaying = new NowPlayingView(this, new NowPlayingView.Actions() {
            @Override public void onPlayPause() { session.togglePause(); }
            @Override public void onNext() { session.next(); }
            @Override public void onPrevious() { session.previous(); }
            @Override public void onStop() { session.stop(); }

            @Override public void onLike() {
                session.toggleLike();
                // L1K3D is visible in this same view, so refresh immediately.
                if (section == Section.LIKED) loadSection(Section.LIKED);
            }

            @Override public void onDownload() {
                MediaTrack track = session.snapshot().currentTrack;
                if (track != null) downloader.remove(track);
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
        });
        page.addView(nowPlaying.view(), spaced(-1, -2, DesignTokens.REGION_GAP));

        debugPanel = new PlaybackDebugPanel(this);
        page.addView(debugPanel.view(), spaced(-1, -2, DesignTokens.GAP));

        // ---- Section content ----
        sectionContent = new LinearLayout(this);
        sectionContent.setOrientation(LinearLayout.VERTICAL);
        page.addView(sectionContent, spaced(-1, -2, 0f));

        statusLine = new TextView(this);
        statusLine.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_CAPTION_SIZE);
        statusLine.setTextColor(DesignTokens.Palette.ON_SURFACE_MUTED);
        statusLine.setVisibility(View.GONE);
        page.addView(statusLine, spaced(-1, -2, DesignTokens.GAP));

        setContentView(scroller);
        applyOrientation(getResources().getConfiguration());
    }

    private View buildTopBar() {
        LinearLayout bar = new LinearLayout(this);
        bar.setOrientation(LinearLayout.HORIZONTAL);
        bar.setGravity(Gravity.CENTER_VERTICAL);
        bar.setMinimumHeight(DesignTokens.dp(DesignTokens.TOP_BAR_HEIGHT));

        TextView brandName = new TextView(this);
        brandName.setText("Tunefold");
        brandName.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_DISPLAY_SIZE);
        brandName.setLetterSpacing(DesignTokens.TYPE_DISPLAY_TRACKING);
        brandName.setTextColor(DesignTokens.Palette.ON_SURFACE);
        brandName.setIncludeFontPadding(false);
        bar.addView(brandName, new LinearLayout.LayoutParams(0, -2, 1f));

        IconButton diagnostics = new IconButton(this, DesignTokens.CONTROL_TARGET,
                DesignTokens.ICON_SIZE_SMALL);
        diagnostics.setAction(Icon.DIAGNOSTICS, "Diagnostics");
        diagnostics.setOnClickListener(view -> {
            debugPanel.toggle();
            renderDiagnostics();
        });
        bar.addView(diagnostics, new LinearLayout.LayoutParams(
                DesignTokens.dp(DesignTokens.CONTROL_TARGET),
                DesignTokens.dp(DesignTokens.CONTROL_TARGET)));
        return bar;
    }

    /**
     * The four destinations.
     *
     * <p>Built once. Switching section only flips each slot's selected state, so the bar
     * never re-inflates and its width never depends on the current label.
     */
    private LinearLayout buildNavigation() {
        LinearLayout nav = new LinearLayout(this);
        nav.setOrientation(LinearLayout.HORIZONTAL);
        int height = DesignTokens.dp(DesignTokens.CONTROL_TARGET_NAVIGATION);
        int gap = DesignTokens.dp(DesignTokens.GAP_TIGHT);
        Section[] order = {Section.HOME, Section.SEARCH, Section.LIKED, Section.DOWNLOADED};
        for (int index = 0; index < order.length; index++) {
            Section target = order[index];
            NavigationItem item = new NavigationItem(this, glyphFor(target), labelFor(target));
            item.setOnClickListener(view -> loadSection(target));
            navItems[target.ordinal()] = item;
            LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(0, height, 1f);
            if (index > 0) params.leftMargin = gap;
            nav.addView(item, params);
        }
        return nav;
    }

    private LinearLayout buildSearchRow() {
        LinearLayout row = new LinearLayout(this);
        row.setOrientation(LinearLayout.HORIZONTAL);
        row.setGravity(Gravity.CENTER_VERTICAL);

        queryInput = new EditText(this);
        queryInput.setSingleLine(true);
        queryInput.setHint("Search or paste a YouTube URL");
        queryInput.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_BODY_SIZE);
        queryInput.setTextColor(DesignTokens.Palette.ON_SURFACE);
        queryInput.setHintTextColor(DesignTokens.Palette.ON_SURFACE_FAINT);
        queryInput.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_URI);
        queryInput.setImeOptions(EditorInfo.IME_ACTION_SEARCH);
        queryInput.setBackground(roundDrawable(DesignTokens.Palette.SURFACE_RAISED,
                DesignTokens.RADIUS_MEDIUM));
        queryInput.setOnEditorActionListener((view, actionId, event) -> {
            if (actionId == EditorInfo.IME_ACTION_SEARCH) {
                submitQuery();
                return true;
            }
            return false;
        });
        int fieldHeight = DesignTokens.dp(DesignTokens.SEARCH_FIELD_HEIGHT);
        row.addView(queryInput, new LinearLayout.LayoutParams(0, fieldHeight, 1f));

        // The IME action is the usual way to search; this is the discoverable one, and
        // both reach the same command.
        searchAction = new IconButton(this);
        searchAction.setAction(Icon.SEARCH, "Search");
        searchAction.setOnClickListener(view -> submitQuery());
        LinearLayout.LayoutParams actionParams =
                new LinearLayout.LayoutParams(DesignTokens.dp(DesignTokens.CONTROL_TARGET),
                        DesignTokens.dp(DesignTokens.CONTROL_TARGET));
        actionParams.leftMargin = DesignTokens.dp(DesignTokens.GAP);
        row.addView(searchAction, actionParams);
        return row;
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

    private LinearLayout.LayoutParams spaced(int width, int height, float gapDp) {
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(width, height);
        if (gapDp > 0f) params.topMargin = DesignTokens.dp(gapDp);
        return params;
    }

    private static GradientDrawable roundDrawable(int color, float radiusDp) {
        GradientDrawable shape = new GradientDrawable();
        shape.setColor(color);
        shape.setCornerRadius(DesignTokens.dp(radiusDp));
        return shape;
    }

    // ------------------------------------------------------------ behaviour

    private void submitQuery() {
        if (session == null) return;
        String query = queryInput.getText().toString().trim();
        if (query.isEmpty()) return;
        if (query.contains("youtube.com/") || query.contains("youtu.be/")) {
            session.playUrl(query);
            selectSection(Section.SEARCH);
            showStatus("Playing URL");
            return;
        }
        selectSection(Section.SEARCH);
        searchInFlight = true;
        showSearchResults(null);
        session.search(query, (tracks, error) -> {
            searchInFlight = false;
            if (error != null) {
                showStatus(error);
                return;
            }
            if (section == Section.SEARCH) {
                showSearchResults(tracks);
            }
        });
    }

    private void resolveAndPlay() {
        String url = getIntent().getStringExtra("runtime_stream_url");
        if (url == null || session == null) return;
        // Switch the surface as well as the section, otherwise the previously rendered
        // list stays on screen under a search header.
        loadSection(Section.SEARCH);
        session.playUrl(url);
    }

    /**
     * Deterministic hook so download flows can be exercised without screen taps.
     *
     * <p>It drives the same public commands the controls call — no second code path — and
     * only acts when the extra is present, so normal launches are unaffected.
     */
    private void applyRuntimeDownloadHook() {
        if (!getIntent().getBooleanExtra("runtime_download", false)) return;
        selectSection(Section.SEARCH);
        String action = getIntent().getStringExtra("runtime_download_action");
        String label = getIntent().getStringExtra("runtime_track_label");
        MediaTrack track = label == null ? null : new MediaTrack(
                "YouTube", label, label, "", "", "", "artist", -1, "", "", "{}");
        if (track == null) {
            showStatus("runtime_download needs runtime_track_label");
            return;
        }
        loadSection(Section.SEARCH);
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
    private void playFromRow(MediaTrack track) {
        if (session == null || track == null) return;
        session.play(track);
    }

    private void toggleLike(MediaTrack track) {
        if (library == null || track == null) return;
        library.toggleLiked(track);
        if (section == Section.LIKED) loadSection(Section.LIKED);
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

    // ---------------------------------------------------------- list sources

    @Override public boolean isLiked(MediaTrack track) {
        return library != null && library.isLiked(track);
    }

    @Override public DownloadState downloadOf(MediaTrack track) {
        if (downloader == null || track == null) return DownloadState.idle(null);
        return downloader.stateOf(track);
    }

    // ------------------------------------------------------------- sections

    /** Loads the content of a navigation section. */
    private void loadSection(Section target) {
        selectSection(target);
        if (session == null) return;
        switch (target) {
            case HOME: renderHome(true); break;
            case SEARCH: renderSearchPrompt(); break;
            case LIKED: renderLiked(); break;
            default: renderDownloaded(); break;
        }
        render();
    }

    /** Highlights a destination without touching the content. */
    private void selectSection(Section target) {
        section = target;
        for (Section candidate : Section.values()) {
            navItems[candidate.ordinal()].setActiveDestination(candidate == target);
        }
    }

    /**
     * Home: continue listening, L1K3D and recommendations, each independently.
     *
     * <p>Reloads when the current track changes (or the user asks), never on every state
     * tick: a recommendation request is a provider round trip and must not be repeated on
     * each render.
     */
    private void renderHome(boolean force) {
        PlaybackSession.Snapshot snapshot = session.snapshot();
        String currentId = snapshot.currentKey == null ? "" : snapshot.currentKey.toString();
        if (!force && currentId.equals(homeTrackKey)) return;
        homeTrackKey = currentId;
        clearSection();

        if (snapshot.currentTrack != null) {
            List<MediaTrack> continuing = new ArrayList<>(1);
            continuing.add(snapshot.currentTrack);
            addGroup("Continue listening", SectionHeader.NO_COUNT).submit(continuing);
        }

        List<TrackKey> liked = session.likedKeys();
        if (!liked.isEmpty()) {
            List<MediaTrack> tracks = new ArrayList<>(liked.size());
            for (TrackKey key : liked) {
                MediaTrack track = session.describeLiked(key);
                if (track != null) tracks.add(track);
            }
            addGroup("L1K3D", liked.size()).submit(tracks);
        }

        if (snapshot.currentTrack == null) {
            addMessage("Play something first to get recommendations");
            return;
        }
        // Asynchronous and non-fatal: a failure leaves Home usable (§25).
        TrackList recommendations = addGroup("Recommended", SectionHeader.NO_COUNT);
        TextView pending = message("Loading…");
        recommendations.addView(pending);
        session.recommendationsForHome(snapshot.currentTrack, (candidates, error) -> {
            if (destroyed || section != Section.HOME) return;
            recommendations.removeView(pending);
            List<MediaTrack> tracks = candidates == null ? new ArrayList<>() : candidates;
            if (tracks.isEmpty()) {
                recommendations.addView(message("No recommendations available"));
                return;
            }
            recommendations.submit(tracks);
        });
    }

    /** L1K3D: membership only. Downloads are a separate, derived playlist. */
    private void renderLiked() {
        List<TrackKey> keys = session.likedKeys();
        clearSection();
        if (keys.isEmpty()) {
            addMessage("Nothing here yet. Use Like on any track.");
            return;
        }
        List<MediaTrack> tracks = new ArrayList<>(keys.size());
        int unresolved = 0;
        for (TrackKey key : keys) {
            MediaTrack track = session.describeLiked(key);
            if (track != null) {
                tracks.add(track);
            } else {
                // Membership is durable, but we may not hold the metadata yet. Showing
                // the identity beats pretending the track is gone.
                tracks.add(placeholderFor(key));
                unresolved++;
            }
        }
        addGroup("L1K3D", keys.size()).submit(tracks);
        if (unresolved > 0) {
            addMessage(unresolved + (unresolved == 1
                    ? " track is still resolving its metadata."
                    : " tracks are still resolving their metadata."));
        }
    }

    /**
     * `Descargadas`: a derived view of the LocalMediaStore (§5).
     *
     * <p>No membership list is stored. A deleted file disappears from here on the next
     * enumeration, and a downloaded track appears without ever being added.
     */
    private void renderDownloaded() {
        List<TrackKey> keys = session.downloadedKeys();
        downloadsRenderedRevision = session.downloadsRevision();
        clearSection();
        if (keys.isEmpty()) {
            addMessage("No downloads yet. Use Download on any track.");
            return;
        }
        List<MediaTrack> tracks = new ArrayList<>(keys.size());
        for (TrackKey key : keys) {
            MediaTrack track = session.describeLiked(key);
            tracks.add(track != null ? track : placeholderFor(key));
        }
        addGroup("Descargadas", keys.size()).submit(tracks);
    }

    private void renderSearchPrompt() {
        clearSection();
        addMessage("Search for a track above");
    }

    private void showSearchResults(List<MediaTrack> tracks) {
        clearSection();
        int count = tracks == null ? 0 : tracks.size();
        if (count == 0) {
            addMessage(searchInFlight ? "Searching…" : "No results");
            return;
        }
        addGroup(count + (count == 1 ? " result" : " results"),
                SectionHeader.NO_COUNT).submit(tracks);
    }

    private MediaTrack placeholderFor(TrackKey key) {
        return new MediaTrack(key.provider(), key.providerTrackId(), key.providerTrackId(),
                "", "", "", "artist", -1, "", "", "{}");
    }

    // ------------------------------------------------------- section content

    /**
     * Empties the content area, returning every list's rows to the shared pool.
     *
     * <p>Returning the rows rather than dropping them is what makes a section switch
     * cost no view inflation: the next section re-points them.
     */
    private void clearSection() {
        for (int i = 0; i < sectionContent.getChildCount(); i++) {
            View child = sectionContent.getChildAt(i);
            if (child instanceof TrackList) ((TrackList) child).detachAll();
        }
        sectionContent.removeAllViews();
    }

    private TrackList addGroup(String title, int count) {
        SectionHeader header = new SectionHeader(this);
        header.setTitle(title, count);
        sectionContent.addView(header, spaced(-1, -2, DesignTokens.SECTION_GAP));
        TrackList list = new TrackList(this, rowPool, this);
        sectionContent.addView(list, spaced(-1, -2, 0f));
        list.setActiveTrack(session == null ? null : session.snapshot().currentKey);
        return list;
    }

    private void addMessage(CharSequence text) {
        sectionContent.addView(message(text), spaced(-1, -2, DesignTokens.GAP));
    }

    private TextView message(CharSequence text) {
        TextView view = new TextView(this);
        view.setText(text);
        view.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_BODY_SIZE);
        view.setTextColor(DesignTokens.Palette.ON_SURFACE_FAINT);
        return view;
    }

    /**
     * Re-applies like and download state to every visible list.
     *
     * <p>This replaces the previous "rebuild the section when the download signature
     * changed" behaviour. Building a signature string meant allocating a StringBuilder
     * and a String over every listed row on every notification, and the rebuild it
     * triggered threw away and re-inflated the whole section, which moved the scroll
     * position out from under the user. Rebinding is O(rows), allocation-free, and does
     * not touch the layout when nothing changed.
     */
    private void refreshLists() {
        TrackKey active = session == null ? null : session.snapshot().currentKey;
        for (int i = 0; i < sectionContent.getChildCount(); i++) {
            View child = sectionContent.getChildAt(i);
            if (!(child instanceof TrackList)) continue;
            TrackList list = (TrackList) child;
            list.setActiveTrack(active);
            list.refresh();
        }
    }

    // ------------------------------------------------------------- artwork

    /**
     * Decodes the cover for {@code track} into the Now Playing view, once per track.
     *
     * <p>The UI refreshes on a timer, so requesting on every render meant re-reading and
     * re-decoding the same file twice a second for the whole session. Guarding by
     * identity keeps the view correct while making the cost proportional to tracks, not
     * to ticks (§27).
     */
    private void loadArtwork(MediaTrack track) {
        if (providers == null || track == null || track.thumbnail.isEmpty()) return;
        String identity = identityOf(track);
        if (identity.isEmpty() || identity.equals(nowPlaying.artwork().shownCoverId())) return;
        nowPlaying.expectArtwork(identity);
        providers.loadArtwork(track, (bitmap, error) -> {
            if (destroyed || bitmap == null) return;
            // The view refuses any identity it is not waiting for, so a decode that
            // finishes after a skip cannot repaint the previous cover.
            nowPlaying.setArtwork(identity, bitmap);
        });
    }

    private static String identityOf(MediaTrack track) {
        TrackKey key = track.key();
        return key == null ? "" : key.toString();
    }

    private void showStatus(CharSequence text) {
        statusLine.setText(text);
        statusLine.setVisibility(text == null || text.length() == 0 ? View.GONE : View.VISIBLE);
    }

    // -------------------------------------------------------------- render

    /** Observer entry point: the session can change for reasons the UI did not cause. */
    private void onSessionChanged() {
        if (destroyed || session == null) return;
        render();
        if (section == Section.DOWNLOADED) {
            // The store scan finishes after this screen binds, and a download commits
            // after that. Both change which tracks exist, which a rebind cannot express.
            if (session.downloadsRevision() != downloadsRenderedRevision) {
                renderDownloaded();
            } else {
                refreshLists();
            }
        } else {
            refreshLists();
        }
        if (section == Section.HOME) renderHome(false);
    }

    /**
     * Applies one snapshot to every part of the surface.
     *
     * <p>Deliberately six narrow calls rather than one {@code nowPlaying.render(...)}:
     * each one compares against what it last rendered, so a change that touches nothing
     * visible costs six comparisons instead of a dozen {@code setText} calls and the
     * layout passes they schedule.
     */
    private void render() {
        if (destroyed || session == null) return;
        PlaybackSession.Snapshot snapshot = session.snapshot();
        nowPlaying.applyTrack(snapshot);
        nowPlaying.applyPlaybackState(snapshot);
        nowPlaying.applyLibraryState(snapshot);
        nowPlaying.applyDownloadState(snapshot);
        nowPlaying.applyError(snapshot);
        if (snapshot.currentTrack != null) {
            loadArtwork(snapshot.currentTrack);
            clearStatus();
        }
        if (debugPanel.isVisible()) renderDiagnostics();
    }

    private void clearStatus() {
        if (statusLine.getVisibility() == View.GONE) return;
        statusLine.setText("");
        statusLine.setVisibility(View.GONE);
    }

    private void renderDiagnostics() {
        if (session == null || !debugPanel.isVisible()) return;
        StringBuilder text = new StringBuilder(256);
        PlaybackSession.Snapshot snapshot = session.snapshot();
        text.append("state=").append(snapshot.state).append('\n');
        text.append("queue=").append(snapshot.queue.size())
                .append(" index=").append(snapshot.queueIndex).append('\n');
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
     * moved.
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
            if (session != null) {
                // One consistent read per tick: identity and position always come from
                // the same instant, so a tick can never pair one track with another
                // track's time.
                PlaybackPosition now = session.readPosition(position);
                nowPlaying.setPosition(now.key, now.positionMs, now.durationMs);
            }
            mainHandler.postDelayed(this, DesignTokens.POSITION_INTERVAL_MS);
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
        loadSection(Section.SEARCH);
        if (intent.getBooleanExtra("runtime_smoke_test", false)) resolveAndPlay();
        applyRuntimeDownloadHook();
    }

    @Override public void onConfigurationChanged(Configuration newConfig) {
        super.onConfigurationChanged(newConfig);
        // The user's font scale can change with the configuration; tokens must follow.
        DesignTokens.init(this);
        applyOrientation(newConfig);
    }

    /**
     * Adapts to the window: orientation plus an artwork side derived from the window, so
     * the cover is never tiny on a large screen nor oversized on a small one, and the
     * controls keep a usable height on a short window.
     */
    private void applyOrientation(Configuration configuration) {
        landscape = configuration.orientation == Configuration.ORIENTATION_LANDSCAPE;
        if (nowPlaying == null) return;
        nowPlaying.setLandscape(landscape);
        android.util.DisplayMetrics metrics = getResources().getDisplayMetrics();
        int padding = DesignTokens.dp(DesignTokens.CONTENT_PADDING_HORIZONTAL)
                + DesignTokens.dp(DesignTokens.GAP);
        int side;
        if (landscape) {
            // Beside the details, so the cover competes with a column of controls for
            // the shorter dimension rather than filling it.
            side = Math.min((int) (metrics.heightPixels * 0.72f),
                    (int) (metrics.widthPixels * 0.42f));
        } else {
            // Above the details, so the cover is also bounded by the window's height.
            // Sizing it by width alone made a full-bleed square that pushed every
            // control below the fold on a tall phone, which is the opposite of what
            // Now Playing is for.
            int available = metrics.widthPixels - padding;
            int heightBudget = (int) (metrics.heightPixels * 0.38f);
            side = Math.min(available, heightBudget);
        }
        nowPlaying.setArtworkSidePx(side);
    }

    @Override protected void onDestroy() {
        destroyed = true;
        mainHandler.removeCallbacks(poll);
        if (session != null) session.removeObserver(sessionObserver);
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