package com.tunefold.app;

import android.app.Activity;
import android.content.ComponentName;
import android.content.Intent;
import android.content.ServiceConnection;
import android.content.pm.ActivityInfo;
import android.content.res.Configuration;
import android.graphics.Color;
import android.graphics.drawable.ColorDrawable;
import android.os.Build;
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

import java.util.Collections;
import java.util.List;

/**
 * Tunefold Android UI.
 *
 * <p>Organised around real playback states rather than an architecture:
 * search at the top, Now Playing as the primary surface, results/queue below,
 * and a diagnostics panel that is hidden unless explicitly opened.
 *
 * <p>Playback ownership stays with {@link ForegroundPlaybackService}: this
 * Activity only binds, issues commands and renders state. Rotation and Activity
 * recreation are safe because all durable state lives in the service.
 */
public final class MainActivity extends Activity {
    private final Handler mainHandler = new Handler(Looper.getMainLooper());

    private PlaybackController controller;
    private ProviderRegistry providers;
    private TrackDownloader downloader;
    private boolean bound;
    private boolean destroyed;
    private boolean landscape;

    private NowPlayingView nowPlaying;
    private PlaybackDebugPanel debugPanel;
    private EditText queryInput;
    private TextView resultsHeader;
    private LinearLayout results;
    private MediaTrack currentTrack;
    private PlaybackTrace currentTrace;
    private String activeArtworkId = "";
    /** Scripted validation hook: download the resolved track without tapping. */
    private boolean pendingDownload;

    /** Kept so the downloader can be attached once the registry is ready. */
    private ForegroundPlaybackService.LocalBinder serviceBinder;

    private final ServiceConnection connection = new ServiceConnection() {
        @Override public void onServiceConnected(ComponentName name, IBinder service) {
            ForegroundPlaybackService.LocalBinder binder =
                    (ForegroundPlaybackService.LocalBinder) service;
            serviceBinder = binder;
            controller = binder.controller();
            providers = binder.providers();
            // May still be null: the store is created asynchronously. The poll
            // loop picks it up as soon as the registry is ready.
            downloader = binder.downloader(binder.engineHandle());
            if (downloader != null) {
                downloader.setObserver((track, status) -> onDownloadStatus(track, status));
            }
            applyOrientation(getResources().getConfiguration());
            pendingDownload = getIntent().getBooleanExtra("runtime_download", false);
            if (getIntent().getBooleanExtra("runtime_smoke_test", false)) resolveAndPlay();
        }

        @Override public void onServiceDisconnected(ComponentName name) {
            controller = null;
            providers = null;
            serviceBinder = null;
        }
    };

    // ------------------------------------------------------------------ UI

    @Override protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        buildInterface();
        Intent service = new Intent(this, ForegroundPlaybackService.class);
        bound = bindService(service, connection, BIND_AUTO_CREATE);
        mainHandler.post(poll);
    }

    private void buildInterface() {
        int pad = NowPlayingView.dp(this, 16);
        // A ScrollView keeps every control reachable when the window is short
        // (landscape, split screen) instead of letting the panel collapse.
        ScrollView scroller = new ScrollView(this);
        scroller.setFillViewport(true);
        LinearLayout page = new LinearLayout(this);
        page.setOrientation(LinearLayout.VERTICAL);
        page.setBackgroundColor(Color.parseColor("#FAFBFD"));
        page.setPadding(pad, NowPlayingView.dp(this, 8), pad, pad);
        scroller.addView(page, new ScrollView.LayoutParams(-1, -2));

        // ---- Search / paste URL ----
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

        queryInput = new EditText(this);
        queryInput.setSingleLine(true);
        queryInput.setHint("Search or paste a YouTube URL");
        queryInput.setTextSize(TypedValue.COMPLEX_UNIT_SP, 15);
        queryInput.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_URI);
        queryInput.setImeOptions(EditorInfo.IME_ACTION_SEARCH);
        queryInput.setBackground(new ColorDrawable(Color.parseColor("#EEF1F5")));
        queryInput.setText("https://www.youtube.com/watch?v=");
        String runtimeUrl = getIntent().getStringExtra("runtime_stream_url");
        if (runtimeUrl != null) queryInput.setText(runtimeUrl);
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
        LinearLayout.LayoutParams searchParams = new LinearLayout.LayoutParams(-1,
                NowPlayingView.dp(this, 48));
        searchParams.topMargin = NowPlayingView.dp(this, 8);
        page.addView(search, searchParams);

        // ---- Now Playing (primary surface) ----
        nowPlaying = new NowPlayingView(this, new NowPlayingView.Actions() {
            @Override public void onPlayPause() {
                PlaybackController active = controller;
                if (active != null) active.togglePause();
            }

            @Override public void onStop() {
                PlaybackController active = controller;
                if (active != null) active.stop();
            }

            @Override public void onDownload() { onDownloadAction(); }

            @Override public void onRemoveDownload() {
                MediaTrack track = currentTrack;
                if (track != null && downloader != null) downloader.remove(track);
            }

            @Override public void onCancelDownload() {
                MediaTrack track = currentTrack;
                if (track != null && downloader != null) downloader.cancel(track);
            }
        });
        LinearLayout.LayoutParams nowPlayingParams = new LinearLayout.LayoutParams(-1, -2);
        nowPlayingParams.topMargin = NowPlayingView.dp(this, 12);
        page.addView(nowPlaying.view(), nowPlayingParams);

        debugPanel = new PlaybackDebugPanel(this);
        LinearLayout.LayoutParams debugParams = new LinearLayout.LayoutParams(-1, -2);
        debugParams.topMargin = NowPlayingView.dp(this, 8);
        page.addView(debugPanel.view(), debugParams);

        // ---- Results / queue ----
        resultsHeader = new TextView(this);
        resultsHeader.setTextSize(TypedValue.COMPLEX_UNIT_SP, 13);
        resultsHeader.setTextColor(Color.parseColor("#5B6472"));
        resultsHeader.setText("Search results");
        page.addView(resultsHeader, new LinearLayout.LayoutParams(-1, -2));

        results = new LinearLayout(this);
        results.setOrientation(LinearLayout.VERTICAL);
        page.addView(results, new LinearLayout.LayoutParams(-1, -2));

        setContentView(scroller);
        applyOrientation(getResources().getConfiguration());
    }

    private Button plainButton(String label) {
        Button view = new Button(this);
        view.setText(label);
        view.setAllCaps(false);
        view.setTextSize(TypedValue.COMPLEX_UNIT_SP, 13);
        return view;
    }

    // ------------------------------------------------------------ behaviour

    private void submitQuery() {
        String query = queryInput.getText().toString().trim();
        if (query.isEmpty() || providers == null) return;
        if (query.contains("youtube.com/") || query.contains("youtu.be/")) {
            resolveUrl(query);
            return;
        }
        if (query.startsWith("https://") || query.startsWith("http://")) {
            playDirectUrl(query);
            return;
        }
        showResultsHeader("Searching…");
        providers.search(query, (tracks, error) -> {
            if (error != null) {
                showResultsHeader(error);
                return;
            }
            showTracks(tracks);
        });
    }

    private void resolveAndPlay() {
        String url = getIntent().getStringExtra("runtime_stream_url");
        if (url == null) return;
        if (url.contains("youtube.com/") || url.contains("youtu.be/")) resolveUrl(url);
        else playDirectUrl(url);
    }

    private void resolveUrl(String url) {
        if (controller == null || providers == null) return;
        PlaybackTrace trace = new PlaybackTrace("url");
        currentTrace = trace;
        startPlaybackService();
        controller.playUrl(url, providers, trace, (track, error) -> {
            if (error != null) {
                showResultsHeader(error);
                return;
            }
            onTrackReady(track);
        });
    }

    private void playDirectUrl(String url) {
        if (controller == null) return;
        startPlaybackService();
        PlaybackTrace trace = new PlaybackTrace("direct-http");
        trace.mark(PlaybackTrace.CONTROLLER_RECEIVED_PLAY);
        currentTrace = trace;
        controller.play(new PlayableSource(url, "[]"), trace);
    }

    private void playTrack(MediaTrack track) {
        if (controller == null || providers == null) return;
        PlaybackTrace trace = new PlaybackTrace(track.providerId);
        currentTrace = trace;
        onTrackReady(track);
        startPlaybackService();
        controller.playTrack(track, providers, trace, (resolved, error) -> {
            if (error != null) showResultsHeader(error);
        });
    }

    private void onTrackReady(MediaTrack track) {
        currentTrack = track;
        activeArtworkId = track.providerId;
        nowPlaying.setTrack(track);
        loadArtwork(track);
        showTracks(Collections.singletonList(track));
        if (pendingDownload && downloader != null) {
            pendingDownload = false;
            downloader.start(track);
        }
    }

    private void loadArtwork(MediaTrack track) {
        if (providers == null || track.thumbnail.isEmpty()) return;
        String expected = track.providerId;
        providers.loadArtwork(track, (bitmap, error) -> {
            if (destroyed || bitmap == null) return;
            if (!expected.equals(activeArtworkId)) return;
            nowPlaying.setArtwork(track, bitmap);
        });
    }

    private void onDownloadAction() {
        MediaTrack track = currentTrack;
        if (track == null || downloader == null) return;
        TrackDownloader.Status status = downloader.statusOf(track);
        switch (status.state) {
            case RUNNING: downloader.cancel(track); break;
            case COMPLETED: downloader.remove(track); break;
            default: downloader.start(track);
        }
    }

    private void onDownloadStatus(MediaTrack track, TrackDownloader.Status status) {
        if (destroyed || !track.providerId.equals(activeArtworkId)) return;
        nowPlaying.setDownloadStatus(status);
        PlaybackController active = controller;
        if (active != null) {
            nowPlaying.setPlaybackState(active.state(), active.error(), status);
        }
    }

    private void showTracks(List<MediaTrack> tracks) {
        results.removeAllViews();
        showResultsHeader(tracks.size() + (tracks.size() == 1 ? " result" : " results"));
        for (MediaTrack track : tracks) {
            Button row = plainButton(track.displayText());
            row.setGravity(Gravity.START | Gravity.CENTER_VERTICAL);
            row.setOnClickListener(view -> playTrack(track));
            results.addView(row, new LinearLayout.LayoutParams(-1, NowPlayingView.dp(this, 48)));
        }
    }

    private void showResultsHeader(String text) {
        resultsHeader.setText(text);
    }

    private void startPlaybackService() {
        Intent service = new Intent(this, ForegroundPlaybackService.class);
        if (Build.VERSION.SDK_INT >= 26) startForegroundService(service);
        else startService(service);
    }

    // -------------------------------------------------------------- polling

    private final Runnable poll = new Runnable() {
        @Override public void run() {
            if (destroyed) return;
            if (downloader == null && providers != null) {
                // The registry finished initializing; bind the downloader now.
                ForegroundPlaybackService.LocalBinder binder = serviceBinder;
                if (binder != null) {
                    TrackDownloader ready = binder.downloader(
                            controller == null ? 0L : controller.engineHandle());
                    if (ready != null) {
                        downloader = ready;
                        downloader.setObserver((track, status) -> onDownloadStatus(track, status));
                    }
                }
            }
            PlaybackController active = controller;
            if (active != null) {
                active.refresh();
                TrackDownloader.Status status =
                        currentTrack == null || downloader == null
                                ? null : downloader.statusOf(currentTrack);
                nowPlaying.setPlaybackState(active.state(), active.error(), status);
                nowPlaying.setPosition(active.positionMs(), durationOf(currentTrack));
                if (debugPanel.isVisible()) renderDiagnostics();
            }
            mainHandler.postDelayed(this, 400);
        }
    };

    private static long durationOf(MediaTrack track) {
        return track == null || track.durationMs <= 0 ? 0 : track.durationMs;
    }

    private void renderDiagnostics() {
        if (controller == null || !debugPanel.isVisible()) return;
        debugPanel.update(controller.diagnostics(), currentTrace);
    }

    // ------------------------------------------------------------ lifecycle

    @Override public void onConfigurationChanged(Configuration newConfig) {
        super.onConfigurationChanged(newConfig);
        applyOrientation(newConfig);
    }

    /**
     * Adapts to the current window: orientation plus an artwork side derived
     * from the smaller window dimension, so the cover is never tiny on a large
     * screen nor oversized on a small one.
     */
    private void applyOrientation(Configuration configuration) {
        landscape = configuration.orientation == Configuration.ORIENTATION_LANDSCAPE;
        if (nowPlaying == null) return;
        nowPlaying.setLandscape(landscape);
        android.util.DisplayMetrics metrics = getResources().getDisplayMetrics();
        int smaller = Math.min(metrics.widthPixels, metrics.heightPixels);
        int padding = NowPlayingView.dp(this, 32) + NowPlayingView.dp(this, 16);
        // The cover follows the shorter window edge so it stays square and never
        // eats the column the title and controls need.
        int side = landscape
                ? Math.min((int) (metrics.heightPixels * 0.72), (int) (metrics.widthPixels * 0.42))
                : smaller - padding;
        nowPlaying.setArtworkSidePx(side);
    }

    @Override protected void onDestroy() {
        destroyed = true;
        mainHandler.removeCallbacks(poll);
        if (bound) unbindService(connection);
        controller = null;
        providers = null;
        super.onDestroy();
    }
}