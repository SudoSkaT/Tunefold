package com.tunefold.app;

import android.app.Service;
import android.content.Intent;
import android.os.Binder;
import android.os.Build;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.util.Log;

/**
 * Owns the long-lived playback: the controller, the Rust engine, the AudioTrack
 * output, the queue, the library, downloads and the system media session.
 *
 * <p>Everything durable lives here, never in an Activity (§18). The Activity
 * binds, issues commands and renders; destroying and recreating it does not
 * touch playback, queue position, downloads or L1K3D membership.
 */
public final class ForegroundPlaybackService extends Service {
    private static final String TAG = "Tunefold";

    private final LocalBinder binder = new LocalBinder();
    private final Handler handler = new Handler(Looper.getMainLooper());
    /** Stable observer identity so add and remove pair up. */
    private final PlaybackSession.Observer sessionObserver = this::onSessionChanged;

    private PlaybackController controller;
    private ProviderRegistry providers;
    private Library library;
    private PlaybackSession session;
    private TrackDownloader downloader;
    private PlaybackNotification notification;
    private boolean foreground;

    public final class LocalBinder extends Binder {
        PlaybackController controller() { return controller; }
        ProviderRegistry providers() { return providers; }
        PlaybackSession session() { return session; }
        Library library() { return library; }
        TrackDownloader downloader() { return downloader; }
    }

    @Override public void onCreate() {
        super.onCreate();
        notification = new PlaybackNotification(this);
        controller = new PlaybackController(this::onControllerChanged);
        library = new Library(new java.io.File(getFilesDir(), "library"));
        library.load();
        providers = new ProviderRegistry();
        session = new PlaybackSession(controller, providers, library);
        session.addObserver(sessionObserver);

        providers.initialize(new java.io.File(getCacheDir(), "rustypipe").getAbsolutePath(),
                new java.io.File(getFilesDir(), "media").getAbsolutePath(),
                (ready, error) -> {
                    if (ready) attachDownloader();
                    else if (error != null) Log.i(TAG, error);
                });
        handler.post(poll);
    }

    /**
     * Creates the downloader once the LocalMediaStore exists.
     *
     * <p>Asynchronous because the store is created on the provider worker; the
     * downloader needs it because the store owns the download directory (§6).
     */
    private void attachDownloader() {
        if (downloader != null || providers == null) return;
        LocalMediaStore store = providers.localMediaStore();
        if (store == null) return;
        // The engine is supplied as a source, not as a value: it is created on the
        // controller's worker thread, so reading the handle here could capture a
        // startup-zero that no later refresh would ever replace.
        downloader = new TrackDownloader(providers, store, controller::engineHandle);
        downloader.cleanAbandonedPartsAsync();
        session.attachDownloader(downloader);
    }

    @Override public int onStartCommand(Intent intent, int flags, int startId) {
        // System media controls and the notification send their commands here, so
        // they land on exactly the same session the UI drives (§17).
        String action = intent == null ? null : intent.getAction();
        if (action != null) {
            String expected = intent.getStringExtra(PlaybackNotification.EXTRA_TRACK_KEY);
            dispatchCommand(action, expected);
            if (!foreground) startForegroundIfNeeded();
            return START_STICKY;
        }
        startForegroundIfNeeded();
        return START_STICKY;
    }

    /**
     * The single place a transport command turns into a session call.
     *
     * <p>Used by the notification intents, by the system (lock screen, headset,
     * Bluetooth) and by the MediaSession callback, so every external control
     * reaches the same {@link PlaybackController} with no second playback path.
     */
    void dispatchCommand(String action) {
        dispatchCommand(action, null);
    }

    /**
     * Dispatches {@code action}, acting always on the session's current track.
     *
     * @param expectedKey track identity the notification was built for, or
     *                    {@code null}; used only to detect (and log) a stale
     *                    notification acting after a track change. The action
     *                    itself never uses it: acting on a captured track is how
     *                    a late tap likes or downloads the wrong song.
     */
    void dispatchCommand(String action, String expectedKey) {
        if (session == null || action == null) return;
        if (expectedKey != null && !expectedKey.isEmpty()) {
            PlaybackSession.Snapshot current = session.snapshot();
            String actual = current.currentKey == null ? "" : current.currentKey.toString();
            if (!expectedKey.equals(actual)) {
                Log.i(TAG, "event=STALE_NOTIFICATION_ACTION action=" + action
                        + " expected=" + expectedKey + " actual=" + actual);
            }
        }
        switch (action) {
            case PlaybackNotification.ACTION_TOGGLE: session.togglePause(); break;
            case PlaybackNotification.ACTION_PLAY: session.setPlaying(true); break;
            case PlaybackNotification.ACTION_PAUSE: session.setPlaying(false); break;
            case PlaybackNotification.ACTION_NEXT: session.next(); break;
            case PlaybackNotification.ACTION_PREVIOUS: session.previous(); break;
            case PlaybackNotification.ACTION_STOP: session.stop(); break;
            case PlaybackNotification.ACTION_LIKE: session.toggleLike(); break;
            case PlaybackNotification.ACTION_DOWNLOAD: dispatchDownload(); break;
            default: break;
        }
    }

    /**
     * Advances the current track's download by one step, from live state.
     *
     * <p>The same dispatch the in-app controls use: active downloads cancel,
     * failed/cancelled ones retry, downloaded ones are respected (no second
     * transfer), and only idle ones start. Read at action time, so a queued
     * tap from before a track change cannot download the previous song.
     */
    private void dispatchDownload() {
        if (session == null) return;
        PlaybackSession.Snapshot snapshot = session.snapshot();
        MediaTrack track = snapshot.currentTrack;
        TrackDownloader downloads = downloader;
        if (track == null || downloads == null) return;
        DownloadState state = downloads.stateOf(track);
        if (state.isActive()) downloads.cancel(track);
        else if (state.isRetryable()) downloads.retry(track);
        else if (state.isDownloaded()) {
            Log.i(TAG, "event=DOWNLOAD_ALREADY_EXISTS id=" + track.key());
        } else downloads.start(track);
    }

    @Override public IBinder onBind(Intent intent) { return binder; }

    /** Polls the engine so state, position and the notification stay live. */
    private final Runnable poll = new Runnable() {
        @Override public void run() {
            if (controller == null) return;
            if (downloader == null) attachDownloader();
            // One snapshot per tick decides the cadence below. It is the same
            // cheap read the UI tick uses (no queue copy, memory-only download
            // state, one engine position read).
            PlaybackSession.Snapshot snapshot =
                    session == null ? null : session.snapshot();
            PlaybackState current = snapshot == null
                    ? PlaybackState.IDLE : snapshot.state;
            controller.refresh();
            if (notification != null && snapshot != null
                    && (current == PlaybackState.PLAYING
                        || current == PlaybackState.BUFFERING)) {
                // Position-only: moves the lock-screen playhead without
                // rebuilding or re-notifying the notification (see publish).
                notification.refreshPosition(snapshot.state, snapshot.positionMs);
            }
            handler.postDelayed(this, intervalFor(current));
        }
    };

    /**
     * Service cadence for a playback state.
     *
     * <p>500ms while audio can be advancing (position has 1s display precision
     * on the lock screen and buffering must flip to playing promptly);
     * 2000ms otherwise. Paused/stopped/idle positions are static and were
     * already published with the state change that made them so — polling
     * faster would only re-read an unchanging engine and churn the session.
     */
    static long intervalFor(PlaybackState state) {
        if (state == PlaybackState.PLAYING || state == PlaybackState.BUFFERING
                || state == PlaybackState.RESOLVING_SOURCE
                || state == PlaybackState.RESOLVING_METADATA) {
            return DesignTokens.SESSION_REFRESH_INTERVAL_MS;
        }
        return IDLE_POLL_MS;
    }

    /**
     * Cadence when nothing can be advancing: commands (play/pause/next) arrive
     * as intents and drive transitions synchronously, so the poll is only
     * liveness (downloader attach, engine EOF edge). 2s, not stopped entirely,
     * so a missed transition still surfaces without a manual kick.
     */
    static final long IDLE_POLL_MS = 2_000L;

    /**
     * The engine changed state; feed it into the session so the product state,
     * the notification and the UI all move together.
     */
    private void onControllerChanged() {
        handler.post(() -> {
            if (session != null && controller != null) {
                session.onControllerState(controller.state(), controller.error());
            }
            render();
        });
    }

    /** Observer entry point for changes that did not come from a UI command. */
    private void onSessionChanged() { render(); }

    /** Pushes the current snapshot to the UI and to the system surfaces. */
    private void render() {
        if (session == null || notification == null) return;
        PlaybackSession.Snapshot snapshot = session.snapshot();
        boolean active = notification.shouldBeForeground(snapshot.state);
        if (active) startForegroundIfNeeded();
        notification.publish(snapshot, null);
        if (!active && foreground && snapshot.state == PlaybackState.STOPPED) {
            // Stopped and paused-free: the service may go away. L1K3D and the
            // LocalMediaStore persist on disk regardless.
            notification.stopForeground();
            foreground = false;
        }
    }

    private void startForegroundIfNeeded() {
        if (foreground || session == null || notification == null) return;
        notification.startForeground(session.snapshot());
        foreground = true;
    }

    @Override public void onDestroy() {
        handler.removeCallbacks(poll);
        if (downloader != null) downloader.close();
        if (session != null) session.removeObserver(sessionObserver);
        if (controller != null) controller.release();
        if (providers != null) providers.close();
        if (notification != null) {
            notification.stopForeground();
            notification.release();
        }
        PlaybackTrace.clearCurrent();
        super.onDestroy();
    }
}