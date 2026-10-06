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
        session.addObserver(this::onSessionChanged);

        providers.initialize(new java.io.File(getCacheDir(), "rustypipe").getAbsolutePath(),
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
        downloader = new TrackDownloader(providers, store, controller.engineHandle());
        session.attachDownloader(downloader);
    }

    @Override public int onStartCommand(Intent intent, int flags, int startId) {
        // System media controls and the notification send their commands here, so
        // they land on exactly the same session the UI drives (§17).
        String action = intent == null ? null : intent.getAction();
        if (action != null && session != null) {
            switch (action) {
                case PlaybackNotification.ACTION_TOGGLE: session.togglePause(); break;
                case PlaybackNotification.ACTION_NEXT: session.next(); break;
                case PlaybackNotification.ACTION_PREVIOUS: session.previous(); break;
                case PlaybackNotification.ACTION_STOP: session.stop(); break;
                default: break;
            }
            if (!foreground) startForegroundIfNeeded();
            return START_STICKY;
        }
        startForegroundIfNeeded();
        return START_STICKY;
    }

    @Override public IBinder onBind(Intent intent) { return binder; }

    /** Polls the engine so state, position and the notification stay live. */
    private final Runnable poll = new Runnable() {
        @Override public void run() {
            if (controller == null) return;
            if (downloader == null) attachDownloader();
            controller.refresh();
            if (notification != null) {
                notification.updatePosition(controller.positionMs(), 1f);
            }
            handler.postDelayed(this, 500);
        }
    };

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
        if (session != null) session.removeObserver(this::onSessionChanged);
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