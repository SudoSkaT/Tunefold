package com.tunefold.app;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.media.MediaMetadata;
import android.media.session.MediaSession;
import android.os.Build;
import android.os.Bundle;

/**
 * System media controls for the foreground playback service (§17).
 *
 * <p>Publishes one {@link MediaSession} so the notification, the lock screen and
 * Bluetooth/headset controls all reflect the same playback. Every command those
 * surfaces send is forwarded to the one {@link PlaybackController} the UI also
 * uses, so there is no second player and no second source of truth — the system
 * simply drives the same object.
 */
final class PlaybackNotification {
    private static final String CHANNEL_ID = "tunefold-playback";
    private static final int NOTIFICATION_ID = 41;

    /** Command actions understood by the notification and system controls. */
    static final String ACTION_TOGGLE = "com.tunefold.app.TOGGLE";
    static final String ACTION_PLAY = "com.tunefold.app.PLAY";
    static final String ACTION_PAUSE = "com.tunefold.app.PAUSE";
    static final String ACTION_NEXT = "com.tunefold.app.NEXT";
    static final String ACTION_PREVIOUS = "com.tunefold.app.PREVIOUS";
    static final String ACTION_STOP = "com.tunefold.app.STOP";

    /** Typed as the concrete service so commands reach the session directly. */
    private final ForegroundPlaybackService service;
    private final MediaSession session;
    private final java.util.concurrent.ExecutorService artworkWorker =
            java.util.concurrent.Executors.newSingleThreadExecutor(r -> {
                Thread thread = new Thread(r, "tunefold-notification-artwork");
                thread.setDaemon(true);
                return thread;
            });
    private volatile Bitmap cachedArtwork;
    private volatile String cachedArtworkPath = "";
    private volatile boolean artworkPending;
    private final NotificationManager notifications;

    PlaybackNotification(ForegroundPlaybackService service) {
        this.service = service;
        this.notifications = service.getSystemService(NotificationManager.class);
        createChannel();
        this.session = new MediaSession(service, "TunefoldPlayback");
        session.setCallback(new MediaSession.Callback() {
            // Explicit states, not a toggle: the system knows which one it wants.
            @Override public void onPlay() { command(ACTION_PLAY); }
            @Override public void onPause() { command(ACTION_PAUSE); }
            @Override public void onSkipToNext() { command(ACTION_NEXT); }
            @Override public void onSkipToPrevious() { command(ACTION_PREVIOUS); }
            @Override public void onStop() { command(ACTION_STOP); }
        });
    }

    /**
     * Handles a transport command from the MediaSession callback.
     *
     * <p>Dispatched straight onto the owning service instead of through an
     * intent: a callback arrives while the app is in the background, where
     * {@code startService} is refused, and an implicit intent has no receiver
     * because the service declares no intent-filter. Both failures were silent,
     * so lock-screen and headset controls did nothing at all.
     */
    private void command(String action) {
        service.dispatchCommand(action);
    }

    /** True while audio is or soon will be playing, so the service stays foreground. */
    boolean shouldBeForeground(PlaybackState state) {
        return state == PlaybackState.PLAYING || state == PlaybackState.PAUSED
                || state == PlaybackState.BUFFERING;
    }

    /** Publishes the current track and state to the system. */
    void publish(PlaybackSession.Snapshot snapshot, String diagnostics) {
        MediaMetadata metadata = new MediaMetadata.Builder()
                .putString(MediaMetadata.METADATA_KEY_TITLE,
                        snapshot.currentTrack == null ? "Tunefold" : snapshot.currentTrack.title)
                .putString(MediaMetadata.METADATA_KEY_ARTIST,
                        snapshot.currentTrack == null
                                ? "" : attribution(snapshot.currentTrack))
                .putString(MediaMetadata.METADATA_KEY_ALBUM, "Tunefold")
                .putLong(MediaMetadata.METADATA_KEY_DURATION,
                        snapshot.currentTrack == null || snapshot.currentTrack.durationMs <= 0
                                ? 0L : snapshot.currentTrack.durationMs)
                .build();
        session.setMetadata(metadata);

        // Position is published so lock screen and system controls show the right
        // time; PlaybackState is rebuilt only when the status actually changes.
        long position = snapshot.currentTrack == null ? 0 : positionMs;
        android.media.session.PlaybackState platformState =
                new android.media.session.PlaybackState.Builder()
                        .setActions(platformActions())
                        .setState(toSystemStatus(snapshot.state), position,
                                snapshot.currentTrack == null ? 0f : 1f)
                        .build();
        session.setPlaybackState(platformState);
        session.setActive(true);

        if (notifications != null) notifications.notify(NOTIFICATION_ID, build(snapshot, diagnostics));
    }

    private long positionMs;
    private float playbackSpeed = 1f;

    /** Kept current by the service poll so published positions are real. */
    void updatePosition(long positionMs, float speed) {
        this.positionMs = Math.max(0, positionMs);
        this.playbackSpeed = speed <= 0 ? 1f : speed;
    }

    private static String attribution(MediaTrack track) {
        return track.artist.isEmpty() ? track.channel : track.artist;
    }

    private static int toSystemStatus(PlaybackState state) {
        switch (state) {
            case PLAYING: return android.media.session.PlaybackState.STATE_PLAYING;
            case PAUSED: return android.media.session.PlaybackState.STATE_PAUSED;
            case BUFFERING:
            case RESOLVING_SOURCE:
            case RESOLVING_METADATA:
                return android.media.session.PlaybackState.STATE_BUFFERING;
            case ERROR: return android.media.session.PlaybackState.STATE_ERROR;
            default: return android.media.session.PlaybackState.STATE_STOPPED;
        }
    }

    private static long platformActions() {
        return android.media.session.PlaybackState.ACTION_PLAY
                | android.media.session.PlaybackState.ACTION_PAUSE
                | android.media.session.PlaybackState.ACTION_PLAY_PAUSE
                | android.media.session.PlaybackState.ACTION_SKIP_TO_NEXT
                | android.media.session.PlaybackState.ACTION_SKIP_TO_PREVIOUS
                | android.media.session.PlaybackState.ACTION_STOP;
    }

    private Notification build(PlaybackSession.Snapshot snapshot, String diagnostics) {
        MediaTrack track = snapshot.currentTrack;
        String title = track == null ? "Tunefold" : track.title;
        String artist = track == null ? "" : attribution(track);
        String text = diagnostics == null ? snapshot.state.label() : diagnostics;

        Notification.Builder builder = Build.VERSION.SDK_INT >= 26
                ? new Notification.Builder(service, CHANNEL_ID)
                : new Notification.Builder(service);
        builder.setSmallIcon(android.R.drawable.ic_media_play)
                .setContentTitle(title)
                .setContentText(artist.isEmpty() ? text : artist + " · " + text)
                .setOngoing(snapshot.state == PlaybackState.PLAYING)
                .setOnlyAlertOnce(true)
                .setShowWhen(false);

        Bitmap icon = notificationArtwork(snapshot);
        if (icon != null) builder.setLargeIcon(icon);

        PendingIntent open = PendingIntent.getActivity(service, 0,
                new Intent(service, MainActivity.class)
                        .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
                pendingIntentFlags());
        builder.setContentIntent(open);

        // Previous and Play/Pause flanking Next matches the platform convention.
        builder.addAction(android.R.drawable.ic_media_previous,
                "Previous", actionIntent(ACTION_PREVIOUS, 1));
        if (snapshot.state == PlaybackState.PAUSED) {
            builder.addAction(android.R.drawable.ic_media_play, "Play",
                    actionIntent(ACTION_TOGGLE, 2));
        } else {
            builder.addAction(android.R.drawable.ic_media_pause, "Pause",
                    actionIntent(ACTION_TOGGLE, 2));
        }
        builder.addAction(android.R.drawable.ic_media_next, "Next",
                actionIntent(ACTION_NEXT, 3));
        builder.addAction(android.R.drawable.ic_menu_close_clear_cancel, "Stop",
                actionIntent(ACTION_STOP, 4));

        return builder.build();
    }

    /**
     * Large icon for the current track, decoded off the calling thread.
     *
     * <p>The notification is rebuilt from the service's polling loop, so decoding
     * here would put a file read and a bitmap allocation on the main thread every
     * few hundred milliseconds (§27). One bitmap is cached and reused; while a new
     * path is being decoded the previous one is shown instead of blocking.
     */
    private Bitmap notificationArtwork(PlaybackSession.Snapshot snapshot) {
        MediaTrack track = snapshot.currentTrack;
        String path = track == null ? "" : track.artworkCachePath;
        if (path.isEmpty()) {
            cachedArtworkPath = "";
            cachedArtwork = null;
            return null;
        }
        if (path.equals(cachedArtworkPath)) return cachedArtwork;
        if (!artworkPending) {
            artworkPending = true;
            artworkWorker.execute(() -> {
                Bitmap decoded = null;
                try {
                    decoded = decodeSampled(path);
                } catch (Throwable unavailable) {
                    decoded = null;
                }
                final Bitmap result = decoded;
                new android.os.Handler(android.os.Looper.getMainLooper()).post(() -> {
                    artworkPending = false;
                    cachedArtworkPath = path;
                    cachedArtwork = result;
                });
            });
        }
        return cachedArtwork;
    }

    /** Decodes to at most 256px on the long edge, which is all a status icon shows. */
    private static Bitmap decodeSampled(String path) {
        BitmapFactory.Options bounds = new BitmapFactory.Options();
        bounds.inJustDecodeBounds = true;
        BitmapFactory.decodeFile(path, bounds);
        int sample = 1;
        while (bounds.outWidth / sample > 256 || bounds.outHeight / sample > 256) sample *= 2;
        BitmapFactory.Options options = new BitmapFactory.Options();
        options.inSampleSize = sample;
        return BitmapFactory.decodeFile(path, options);
    }

    private PendingIntent actionIntent(String action, int requestCode) {
        // Explicit component: the service declares no intent-filter, so an implicit
        // intent could never be delivered to it.
        return PendingIntent.getService(service, requestCode,
                new Intent(service, ForegroundPlaybackService.class).setAction(action),
                pendingIntentFlags());
    }

    private static int pendingIntentFlags() {
        int flags = PendingIntent.FLAG_UPDATE_CURRENT;
        if (Build.VERSION.SDK_INT >= 23) flags |= PendingIntent.FLAG_IMMUTABLE;
        return flags;
    }

    private void createChannel() {
        if (Build.VERSION.SDK_INT < 26 || notifications == null) return;
        NotificationChannel channel = new NotificationChannel(CHANNEL_ID, "Playback",
                NotificationManager.IMPORTANCE_LOW);
        channel.setDescription("Tunefold audio playback");
        channel.setShowBadge(false);
        notifications.createNotificationChannel(channel);
    }

    /** Enters the foreground, which playback requires on modern Android. */
    void startForeground(PlaybackSession.Snapshot snapshot) {
        Notification notification = build(snapshot, null);
        if (Build.VERSION.SDK_INT >= 29) {
            service.startForeground(NOTIFICATION_ID, notification,
                    android.content.pm.ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK);
        } else {
            service.startForeground(NOTIFICATION_ID, notification);
        }
    }

    void stopForeground() {
        if (Build.VERSION.SDK_INT >= 24) {
            service.stopForeground(Service.STOP_FOREGROUND_REMOVE);
        } else {
            service.stopForeground(true);
        }
    }

    void release() {
        if (session != null) session.release();
    }
}