package com.tunefold.app;

import android.app.Activity;
import android.content.ComponentName;
import android.content.Intent;
import android.content.ServiceConnection;
import android.content.res.Configuration;
import android.graphics.Color;
import android.graphics.drawable.ColorDrawable;
import android.os.Bundle;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.text.InputType;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.view.inputmethod.EditorInfo;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

import java.util.ArrayList;
import java.util.Collections;
import java.util.List;

/**
 * Tunefold Android UI.
 *
 * <p>Navigation over Home, Search, L1K3D and Descargadas, with Now Playing as
 * the primary surface (§24). Deliberately a single screen with a section switch
 * rather than a multi-screen architecture: the app does not yet need it, and one
 * surface keeps playback state visible everywhere.
 *
 * <p>This Activity holds <b>no playback state</b>. It binds to
 * {@link ForegroundPlaybackService}, renders {@link PlaybackSession} snapshots
 * and sends commands. Rotation and Activity recreation therefore rebuild the
 * whole screen from service-owned state and cannot desynchronise (§18).
 */
public final class MainActivity extends Activity {
    private final Handler mainHandler = new Handler(Looper.getMainLooper());

    private PlaybackController controller;
    private ProviderRegistry providers;
    private PlaybackSession session;
    private Library library;
    private TrackDownloader downloader;
    private ForegroundPlaybackService.LocalBinder serviceBinder;
    private boolean bound;
    private boolean destroyed;
    private boolean landscape;

    private NowPlayingView nowPlaying;
    private PlaybackDebugPanel debugPanel;
    private EditText queryInput;
    private TextView sectionHeader;
    private LinearLayout sectionContent;
    private LinearLayout results;
    private TextView statusLine;

    /** Which list the results area is showing. */
    private enum Section { HOME, SEARCH, LIKED, DOWNLOADED }
    private Section section = Section.HOME;

    /** Artwork requests in flight, keyed by identity so a late load is ignored. */
    private String artworkRequestId = "";
    /** Identity whose artwork is already decoded into the view. */
    private String artworkShownKey = "";
    private boolean searchInFlight;

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
            session.addObserver(MainActivity.this::onSessionChanged);
            applyOrientation(getResources().getConfiguration());
            loadSection(Section.HOME, true);
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
        buildInterface();
        Intent service = new Intent(this, ForegroundPlaybackService.class);
        bound = bindService(service, connection, BIND_AUTO_CREATE);
        mainHandler.post(poll);
    }

    // ------------------------------------------------------------------ UI

    private void buildInterface() {
        int pad = NowPlayingView.dp(this, 16);
        // A ScrollView keeps every control reachable when the window is short.
        ScrollView scroller = new ScrollView(this);
        scroller.setFillViewport(true);
        LinearLayout page = new LinearLayout(this);
        page.setOrientation(LinearLayout.VERTICAL);
        page.setBackgroundColor(Color.parseColor("#FAFBFD"));
        page.setPadding(pad, NowPlayingView.dp(this, 8), pad, pad);
        // Stop the search field from grabbing focus at launch, which raised the
        // keyboard over the Now Playing panel before the user asked for it.
        page.setFocusableInTouchMode(true);
        page.requestFocus();
        scroller.addView(page, new ScrollView.LayoutParams(-1, -2));

        // ---- Brand + diagnostics ----
        LinearLayout brand = new LinearLayout(this);
        brand.setOrientation(LinearLayout.HORIZONTAL);
        brand.setGravity(Gravity.CENTER_VERTICAL);
        TextView brandName = new TextView(this);
        brandName.setText("Tunefold");
        brandName.setTextSize(TypedValue.COMPLEX_UNIT_SP, 22);
        brandName.setTextColor(Color.parseColor("#101317"));
        brand.addView(brandName, new LinearLayout.LayoutParams(0, -2, 1f));
        Button diagnostics = plainButton("Diagnostics");
        diagnostics.setOnClickListener(view -> {
            debugPanel.toggle();
            renderDiagnostics();
        });
        brand.addView(diagnostics, new LinearLayout.LayoutParams(-2, -2));
        page.addView(brand, new LinearLayout.LayoutParams(-1, -2));

        // ---- Navigation ----
        LinearLayout nav = new LinearLayout(this);
        nav.setOrientation(LinearLayout.HORIZONTAL);
        int navGap = NowPlayingView.dp(this, 6);
        nav.addView(navButton("Home", () -> loadSection(Section.HOME, true)),
                navItemParams(navGap, 0));
        nav.addView(navButton("Search", () -> loadSection(Section.SEARCH, true)),
                navItemParams(navGap, 0));
        nav.addView(navButton("L1K3D", () -> loadSection(Section.LIKED, true)),
                navItemParams(navGap, 0));
        nav.addView(navButton("Descargadas", () -> loadSection(Section.DOWNLOADED, true)),
                navItemParams(navGap, 0));
        LinearLayout.LayoutParams navParams = new LinearLayout.LayoutParams(-1, -2);
        navParams.topMargin = NowPlayingView.dp(this, 8);
        page.addView(nav, navParams);

        // ---- Search field ----
        queryInput = new EditText(this);
        queryInput.setSingleLine(true);
        queryInput.setHint("Search or paste a YouTube URL");
        queryInput.setTextSize(TypedValue.COMPLEX_UNIT_SP, 15);
        queryInput.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_URI);
        queryInput.setImeOptions(EditorInfo.IME_ACTION_SEARCH);
        queryInput.setBackground(new ColorDrawable(Color.parseColor("#EEF1F5")));
        queryInput.setOnEditorActionListener((view, actionId, event) -> {
            if (actionId == EditorInfo.IME_ACTION_SEARCH) {
                submitQuery();
                return true;
            }
            return false;
        });
        LinearLayout.LayoutParams inputParams = new LinearLayout.LayoutParams(-1, -2);
        inputParams.topMargin = NowPlayingView.dp(this, 10);
        page.addView(queryInput, inputParams);

        Button search = plainButton("Search");
        search.setOnClickListener(view -> submitQuery());
        LinearLayout.LayoutParams searchParams =
                new LinearLayout.LayoutParams(-1, NowPlayingView.dp(this, 48));
        searchParams.topMargin = NowPlayingView.dp(this, 8);
        page.addView(search, searchParams);

        // ---- Now Playing (primary surface) ----
        nowPlaying = new NowPlayingView(this, new NowPlayingView.Actions() {
            @Override public void onPlayPause() { session.togglePause(); }
            @Override public void onNext() { session.next(); }
            @Override public void onPrevious() { session.previous(); }
            @Override public void onStop() { session.stop(); }

            @Override public void onLike() {
                session.toggleLike();
                // L1K3D is visible in this same view, so refresh immediately.
                if (section == Section.LIKED) loadSection(Section.LIKED, false);
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
        nowPlaying.bindErrorAction(() -> session.retry(), () -> session.skipToNext());
        nowPlaying.bindDownloadAction(
                () -> startDownload(),
                () -> cancelDownload(),
                () -> retryDownload());
        LinearLayout.LayoutParams nowPlayingParams = new LinearLayout.LayoutParams(-1, -2);
        nowPlayingParams.topMargin = NowPlayingView.dp(this, 12);
        page.addView(nowPlaying.view(), nowPlayingParams);

        debugPanel = new PlaybackDebugPanel(this);
        LinearLayout.LayoutParams debugParams = new LinearLayout.LayoutParams(-1, -2);
        debugParams.topMargin = NowPlayingView.dp(this, 8);
        page.addView(debugPanel.view(), debugParams);

        // ---- Section content ----
        sectionHeader = new TextView(this);
        sectionHeader.setTextSize(TypedValue.COMPLEX_UNIT_SP, 13);
        sectionHeader.setTextColor(Color.parseColor("#5B6472"));
        page.addView(sectionHeader, new LinearLayout.LayoutParams(-1, -2));

        sectionContent = new LinearLayout(this);
        sectionContent.setOrientation(LinearLayout.VERTICAL);
        page.addView(sectionContent, new LinearLayout.LayoutParams(-1, -2));

        statusLine = new TextView(this);
        statusLine.setTextSize(TypedValue.COMPLEX_UNIT_SP, 12);
        statusLine.setTextColor(Color.parseColor("#5B6472"));
        page.addView(statusLine, new LinearLayout.LayoutParams(-1, -2));

        // Legacy results container kept for the search section.
        results = new LinearLayout(this);
        results.setOrientation(LinearLayout.VERTICAL);

        setContentView(scroller);
        applyOrientation(getResources().getConfiguration());
    }

    private Button navButton(String label, Runnable action) {
        Button view = plainButton(label);
        view.setMaxLines(1);
        view.setEllipsize(android.text.TextUtils.TruncateAt.END);
        view.setTextSize(TypedValue.COMPLEX_UNIT_SP, 11);
        view.setPadding(NowPlayingView.dp(this, 4), 0, NowPlayingView.dp(this, 4), 0);
        view.setOnClickListener(ignored -> action.run());
        return view;
    }

    /** Equal-width navigation slots with a gap, so labels never resize the row. */
    private LinearLayout.LayoutParams navItemParams(int gap, int leftMargin) {
        LinearLayout.LayoutParams params =
                new LinearLayout.LayoutParams(0, NowPlayingView.dp(this, 40), 1f);
        params.leftMargin = leftMargin;
        return params;
    }

    private Button plainButton(String label) {
        Button view = new Button(this);
        view.setText(label);
        view.setAllCaps(false);
        view.setTextSize(TypedValue.COMPLEX_UNIT_SP, 12);
        return view;
    }

    // ------------------------------------------------------------ behaviour

    private void submitQuery() {
        if (session == null) return;
        String query = queryInput.getText().toString().trim();
        if (query.isEmpty()) return;
        if (query.contains("youtube.com/") || query.contains("youtu.be/")) {
            session.playUrl(query);
            section = Section.SEARCH;
            showStatus("Playing URL");
            return;
        }
        section = Section.SEARCH;
        searchInFlight = true;
        sectionHeader.setText("Searching…");
        sectionContent.removeAllViews();
        session.search(query, (tracks, error) -> {
            searchInFlight = false;
            if (error != null) {
                showStatus(error);
                return;
            }
            showSearchResults(tracks);
        });
    }

    private void resolveAndPlay() {
        String url = getIntent().getStringExtra("runtime_stream_url");
        if (url == null || session == null) return;
        section = Section.SEARCH;
        session.playUrl(url);
    }

    /**
     * Deterministic hook so download flows can be exercised without screen taps.
     *
     * <p>It drives the same public commands the buttons call — no second code
     * path — and only acts when the extra is present, so normal launches are
     * unaffected.
     */
    private void applyRuntimeDownloadHook() {
        if (!getIntent().getBooleanExtra("runtime_download", false)) return;
        section = Section.SEARCH;
        String action = getIntent().getStringExtra("runtime_download_action");
        String label = getIntent().getStringExtra("runtime_track_label");
        MediaTrack track = label == null ? null : new MediaTrack(
                "YouTube", label, label, "", "", "", "artist", -1, "", "", "{}");
        if (track == null) {
            showStatus("runtime_download needs runtime_track_label");
            return;
        }
        if ("start".equals(action)) {
            session.play(track);
            startDownload();
        } else if ("cancel".equals(action)) {
            cancelDownload();
        } else if ("retry".equals(action)) {
            retryDownload();
        } else if ("like".equals(action)) {
            library.toggleLiked(track);
        } else if ("remove".equals(action)) {
            downloader.remove(track);
        }
        render();
    }

    private void startDownload() {
        MediaTrack track = session == null ? null : session.snapshot().currentTrack;
        if (track == null || downloader == null) return;
        // §6: the downloader decides whether this is a start, a no-op because
        // the file exists, or a refusal because one is already running.
        downloader.start(track);
        if (section == Section.DOWNLOADED) loadSection(Section.DOWNLOADED, false);
    }

    private void retryDownload() {
        MediaTrack track = session == null ? null : session.snapshot().currentTrack;
        if (track != null && downloader != null) downloader.retry(track);
    }

    /**
     * Cancels the in-flight download of the current track (§8). This must reach
     * the downloader: a button that only repaints would look functional and do
     * nothing (§26).
     */
    private void cancelDownload() {
        MediaTrack track = session == null ? null : session.snapshot().currentTrack;
        if (track != null && downloader != null) downloader.cancel(track);
        if (section == Section.DOWNLOADED) loadSection(Section.DOWNLOADED, false);
        render();
    }

    /** Loads the content of a navigation section. */
    private void loadSection(Section target, boolean refreshContent) {
        section = target;
        if (session == null) return;
        switch (target) {
            case HOME: renderHome(true); break;
            case SEARCH: showStatus("Search for a track above"); break;
            case LIKED: renderLiked(); break;
            case DOWNLOADED: renderDownloaded(); break;
            default: break;
        }
        render();
    }

    /** Identity Home was last rendered for, so it only reloads on a real change. */
    private String homeTrackKey = "";

    /**
     * Home: continue listening, L1K3D and recommendations, each independently.
     *
     * <p>Reloads when the current track changes (or the user asks), never on
     * every state tick: a recommendation request is a provider round trip and must
     * not be repeated on each render.
     */
    private void renderHome(boolean force) {
        PlaybackSession.Snapshot snapshot = session.snapshot();
        String currentId = snapshot.currentKey == null ? "" : snapshot.currentKey.toString();
        if (!force && currentId.equals(homeTrackKey)) return;
        homeTrackKey = currentId;
        sectionHeader.setText("Home");
        sectionContent.removeAllViews();

        if (snapshot.currentTrack != null) {
            sectionContent.addView(sectionLabel("Continue listening"));
            sectionContent.addView(trackRow(snapshot.currentTrack, true));
        }

        List<TrackKey> liked = session.likedKeys();
        if (!liked.isEmpty()) {
            sectionContent.addView(sectionLabel("L1K3D (" + liked.size() + ")"));
            for (TrackKey key : liked) {
                MediaTrack track = session.findKnown(key);
                if (track != null) sectionContent.addView(trackRow(track, false));
            }
        }

        sectionContent.addView(sectionLabel("Recommended"));
        if (snapshot.currentTrack == null) {
            sectionContent.addView(sectionLabel("Play something first to get recommendations"));
            return;
        }
        // Asynchronous and non-fatal: a failure leaves Home usable (§25).
        TextView pending = sectionLabel("Loading…");
        sectionContent.addView(pending);
        session.recommendationsForHome(snapshot.currentTrack, (candidates, error) -> {
            if (destroyed || section != Section.HOME) return;
            int mark = sectionContent.indexOfChild(pending);
            if (mark >= 0) sectionContent.removeViewAt(mark);
            if (candidates == null || candidates.isEmpty()) {
                sectionContent.addView(sectionLabel("No recommendations available"));
                return;
            }
            for (MediaTrack track : candidates) sectionContent.addView(trackRow(track, false));
        });
    }

    /** L1K3D: membership only. Downloads are a separate, derived playlist. */
    private void renderLiked() {
        List<TrackKey> keys = session.likedKeys();
        sectionHeader.setText("L1K3D (" + keys.size() + ")");
        sectionContent.removeAllViews();
        if (keys.isEmpty()) {
            sectionContent.addView(sectionLabel("Nothing here yet. Use Like on any track."));
            return;
        }
        for (TrackKey key : keys) {
            MediaTrack track = session.findKnown(key);
            if (track != null) {
                sectionContent.addView(trackRow(track, false));
            } else {
                // Membership is durable, but we may not hold the metadata yet.
                // Fetch it lazily rather than pretending the track is gone.
                sectionContent.addView(sectionLabel("Resolving " + key));
            }
        }
    }

    /**
     * `Descargadas`: a derived view of the LocalMediaStore (§5).
     *
     * <p>No membership list is stored. A deleted file disappears from here on the
     * next enumeration, and a downloaded track appears without ever being added.
     */
    private void renderDownloaded() {
        sectionHeader.setText("Descargadas");
        sectionContent.removeAllViews();
        List<TrackKey> keys = session.downloadedKeys();
        if (keys.isEmpty()) {
            sectionContent.addView(sectionLabel("No downloads yet. Use Download on any track."));
            return;
        }
        for (TrackKey key : keys) {
            MediaTrack track = session.findKnown(key);
            sectionContent.addView(trackRow(track != null ? track : placeholderFor(key), false));
        }
    }

    private MediaTrack placeholderFor(TrackKey key) {
        return new MediaTrack(key.provider(), key.providerTrackId(), key.providerTrackId(),
                "", "", "", "artist", -1, "", "", "{}");
    }

    private TextView sectionLabel(String text) {
        TextView view = new TextView(this);
        view.setText(text);
        view.setTextSize(TypedValue.COMPLEX_UNIT_SP, 12);
        view.setTextColor(Color.parseColor("#8A929E"));
        return view;
    }

    /**
     * One row per track, showing identity-independent state: liked, downloaded,
     * and the download action for this specific track (§8).
     */
    private View trackRow(MediaTrack track, boolean playing) {
        LinearLayout row = new LinearLayout(this);
        row.setOrientation(LinearLayout.HORIZONTAL);
        row.setGravity(Gravity.CENTER_VERTICAL);

        Button main = plainButton((playing ? "▶ " : "") + track.displayText());
        main.setGravity(Gravity.START | Gravity.CENTER_VERTICAL);
        main.setTextSize(TypedValue.COMPLEX_UNIT_SP, 14);
        main.setOnClickListener(view -> {
            if (session != null) session.play(track);
            loadArtwork(track);
        });
        row.addView(main, new LinearLayout.LayoutParams(0, NowPlayingView.dp(this, 48), 1f));

        DownloadState download = downloader == null
                ? DownloadState.idle(track.key()) : downloader.stateOf(track);
        Button downloadButton = plainButton(shortDownloadLabel(download));
        // Stays enabled while active so it can act as Cancel: an action the user
        // cannot reach would make cancellation look broken (§8, §26).
        downloadButton.setEnabled(!download.isDownloaded());
        downloadButton.setOnClickListener(view -> {
            if (downloader == null || track == null) return;
            DownloadState state = downloader.stateOf(track);
            if (state.isActive()) downloader.cancel(track);
            else if (state.isRetryable()) downloader.retry(track);
            else if (state.isDownloaded()) downloader.remove(track);
            else downloader.start(track);
            render();
        });
        LinearLayout.LayoutParams downloadParams =
                new LinearLayout.LayoutParams(-2, NowPlayingView.dp(this, 48));
        downloadParams.leftMargin = NowPlayingView.dp(this, 6);
        row.addView(downloadButton, downloadParams);

        Button likeButton = plainButton(library != null && library.isLiked(track) ? "Liked" : "Like");
        likeButton.setOnClickListener(view -> {
            if (library != null) {
                library.toggleLiked(track);
                loadArtwork(track);
                render();
                if (section == Section.LIKED) loadSection(Section.LIKED, false);
            }
        });
        LinearLayout.LayoutParams likeParams =
                new LinearLayout.LayoutParams(-2, NowPlayingView.dp(this, 48));
        likeParams.leftMargin = NowPlayingView.dp(this, 6);
        row.addView(likeButton, likeParams);
        return row;
    }

    /** Compact download label for a list row (§8). */
    private String shortDownloadLabel(DownloadState state) {
        switch (state.phase) {
            case RESOLVING_SOURCE: return "…";
            case DOWNLOADING: {
                int percent = state.progressPercent();
                return percent >= 0 ? percent + "%" : "…";
            }
            case COMPLETED:
            case ALREADY_DOWNLOADED: return "Downloaded";
            case FAILED: return "Retry";
            case CANCELLED: return "Retry";
            default: return "Download";
        }
    }

    private void showSearchResults(List<MediaTrack> tracks) {
        sectionHeader.setText(tracks.size() + (tracks.size() == 1 ? " result" : " results"));
        sectionContent.removeAllViews();
        if (tracks == null || tracks.isEmpty()) {
            sectionContent.addView(sectionLabel("No results"));
            return;
        }
        for (MediaTrack track : tracks) sectionContent.addView(trackRow(track, false));
    }

    /**
     * Decodes the artwork for {@code track} into the Now Playing view, once.
     *
     * <p>The UI refreshes on a timer, so requesting on every render meant
     * re-reading and re-decoding the same file twice a second for the whole
     * session. Guarding by identity keeps the view correct while making the cost
     * proportional to tracks, not to ticks (§27).
     */
    private void loadArtwork(MediaTrack track) {
        if (providers == null || track == null || track.thumbnail.isEmpty()) return;
        String expected = track.providerId;
        if (expected.equals(artworkShownKey)) return;
        artworkShownKey = expected;
        artworkRequestId = expected;
        providers.loadArtwork(track, (bitmap, error) -> {
            if (destroyed || bitmap == null) return;
            // A late load must never overwrite a newer track's artwork.
            if (!expected.equals(artworkRequestId)) return;
            nowPlaying.setArtwork(track, bitmap);
        });
    }

    private void showStatus(String text) { statusLine.setText(text); }

    // -------------------------------------------------------------- render

    /** Observer entry point: the session can change for reasons the UI did not cause. */
    private void onSessionChanged() {
        if (destroyed || session == null) return;
        render();
    }

    private void render() {
        if (destroyed || session == null) return;
        PlaybackSession.Snapshot snapshot = session.snapshot();
        nowPlaying.render(snapshot, session.positionMs());
        if (snapshot.currentTrack != null) {
            loadArtwork(snapshot.currentTrack);
            showStatus("");
        }
        // Keep Home honest: it used to say "play something first" while a track
        // was already playing, because it only rendered once at bind time.
        if (section == Section.HOME) renderHome(false);
        if (debugPanel.isVisible()) renderDiagnostics();
    }

    private void renderDiagnostics() {
        if (session == null || !debugPanel.isVisible()) return;
        StringBuilder text = new StringBuilder();
        text.append("state=").append(session.snapshot().state).append('\n');
        text.append("queue=").append(session.snapshot().queue.size())
                .append(" index=").append(session.snapshot().queueIndex).append('\n');
        text.append("autoplay=").append(session.autoplayEnabled()).append('\n');
        if (downloader != null) {
            text.append("recovery=").append(downloader.recoverySummary()).append('\n');
        }
        if (session.diagnostics() != null) text.append(session.diagnostics()).append('\n');
        debugPanel.update(text.toString(), session.trace());
    }

    /** Position advances faster than state changes, so it is polled. */
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
                PlaybackSession.Snapshot snapshot = session.snapshot();
                nowPlaying.setPosition(session.positionMs(),
                        snapshot.currentTrack == null ? 0 : snapshot.currentTrack.durationMs);
            }
            mainHandler.postDelayed(this, 500);
        }
    };

    // ------------------------------------------------------------ lifecycle

    @Override public void onConfigurationChanged(Configuration newConfig) {
        super.onConfigurationChanged(newConfig);
        applyOrientation(newConfig);
    }

    /**
     * Adapts to the window: orientation plus an artwork side derived from the
     * window, so the cover is never tiny on a large screen nor oversized on a
     * small one, and the controls keep a usable height on a short window.
     */
    private void applyOrientation(Configuration configuration) {
        landscape = configuration.orientation == Configuration.ORIENTATION_LANDSCAPE;
        if (nowPlaying == null) return;
        nowPlaying.setLandscape(landscape);
        android.util.DisplayMetrics metrics = getResources().getDisplayMetrics();
        int smaller = Math.min(metrics.widthPixels, metrics.heightPixels);
        int padding = NowPlayingView.dp(this, 32) + NowPlayingView.dp(this, 16);
        int side = landscape
                ? Math.min((int) (metrics.heightPixels * 0.72),
                        (int) (metrics.widthPixels * 0.42))
                : smaller - padding;
        nowPlaying.setArtworkSidePx(side);
    }

    @Override protected void onDestroy() {
        destroyed = true;
        mainHandler.removeCallbacks(poll);
        if (session != null) session.removeObserver(this::render);
        if (session != null) session.removeObserver(this::onSessionChanged);
        if (bound) unbindService(connection);
        session = null;
        controller = null;
        providers = null;
        downloader = null;
        serviceBinder = null;
        super.onDestroy();
    }
}