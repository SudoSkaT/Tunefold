package com.tunefold.app;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.Service;
import android.content.Intent;
import android.os.Binder;
import android.os.Build;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;

/** Owns the long-lived playback controller, Rust engine, and AudioTrack output. */
public final class ForegroundPlaybackService extends Service {
    private static final String CHANNEL_ID = "tunefold-playback";
    private static final int NOTIFICATION_ID = 41;
    private final LocalBinder binder = new LocalBinder();
    private final Handler handler = new Handler(Looper.getMainLooper());
    private PlaybackController controller;
    private ProviderRegistry providers;
    private boolean foreground;

    public final class LocalBinder extends Binder {
        PlaybackController controller() { return controller; }
        ProviderRegistry providers() { return providers; }
    }

    @Override public void onCreate() {
        super.onCreate();
        createNotificationChannel();
        controller = new PlaybackController(this::playbackChanged);
        providers = new ProviderRegistry();
        providers.initialize(new java.io.File(getCacheDir(), "rustypipe").getAbsolutePath(),
                (ready, error) -> { if (!ready && error != null) android.util.Log.i("Tunefold", error); });
        handler.post(poll);
    }

    @Override public int onStartCommand(Intent intent, int flags, int startId) {
        startForegroundIfNeeded();
        return START_STICKY;
    }

    @Override public IBinder onBind(Intent intent) { return binder; }

    private final Runnable poll = new Runnable() {
        @Override public void run() {
            if (controller == null) return;
            controller.refresh();
            int state = controller.state();
            if (state == PlaybackController.STOPPED || state == PlaybackController.ERROR) {
                stopForegroundCompat();
                stopSelf();
            }
            handler.postDelayed(this, 500);
        }
    };

    private void playbackChanged() {
        if (controller == null) return;
        int state = controller.state();
        if (state == PlaybackController.LOADING || state == PlaybackController.PLAYING
                || state == PlaybackController.PAUSED) {
            startForegroundIfNeeded();
            NotificationManager manager = getSystemService(NotificationManager.class);
            if (manager != null) manager.notify(NOTIFICATION_ID, buildNotification(state));
        } else if (state == PlaybackController.STOPPED || state == PlaybackController.ERROR) {
            handler.post(() -> {
                stopForegroundCompat();
                stopSelf();
            });
        }
    }

    private void startForegroundIfNeeded() {
        if (foreground) return;
        if (Build.VERSION.SDK_INT >= 29) {
            startForeground(NOTIFICATION_ID, buildNotification(PlaybackController.LOADING),
                    android.content.pm.ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK);
        } else {
            startForeground(NOTIFICATION_ID, buildNotification(PlaybackController.LOADING));
        }
        foreground = true;
    }

    private void stopForegroundCompat() {
        if (!foreground) return;
        if (Build.VERSION.SDK_INT >= 24) stopForeground(STOP_FOREGROUND_REMOVE);
        else stopForeground(true);
        foreground = false;
    }

    private Notification buildNotification(int state) {
        String text = state == PlaybackController.PAUSED ? "Paused"
                : state == PlaybackController.PLAYING ? "Playing" : "Loading";
        if (Build.VERSION.SDK_INT >= 26) {
            return new Notification.Builder(this, CHANNEL_ID)
                    .setSmallIcon(android.R.drawable.ic_media_play)
                    .setContentTitle("Tunefold")
                    .setContentText(text)
                    .setOngoing(state != PlaybackController.PAUSED)
                    .build();
        }
        return new Notification.Builder(this)
                .setSmallIcon(android.R.drawable.ic_media_play)
                .setContentTitle("Tunefold")
                .setContentText(text)
                .setOngoing(state != PlaybackController.PAUSED)
                .build();
    }

    private void createNotificationChannel() {
        if (Build.VERSION.SDK_INT < 26) return;
        NotificationChannel channel = new NotificationChannel(CHANNEL_ID,
                "Playback", NotificationManager.IMPORTANCE_LOW);
        channel.setDescription("Tunefold audio playback");
        NotificationManager manager = getSystemService(NotificationManager.class);
        if (manager != null) manager.createNotificationChannel(channel);
    }

    @Override public void onDestroy() {
        handler.removeCallbacks(poll);
        if (controller != null) controller.release();
        if (providers != null) providers.close();
        stopForegroundCompat();
        super.onDestroy();
    }
}
