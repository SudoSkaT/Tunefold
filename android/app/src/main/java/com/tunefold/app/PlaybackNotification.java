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
        boolean paused = snapshot.state == PlaybackState.PAUSED;

        Notification.Builder builder = Build.VERSION.SDK_INT >= 26
                ? new Notification.Builder(service, CHANNEL_ID)
                : new Notification.Builder(service);
        builder.setSmallIcon(smallIcon())
                .setContentTitle(title)
                // El estado va en su propia línea, no pegado al artista con un punto:
                // el sistema reserva ese renglón para "en pausa" y para lo que el
                // comando de voz o el relojongo tengan que decir, y mezclado con el
                // nombre del artista quedaba ilegible en la pantalla de bloqueo.
                .setContentText(artist)
                .setSubText(snapshot.state.label())
                .setCategory(Notification.CATEGORY_TRANSPORT)
                // Sin esto la notificación se oculta en la pantalla de bloqueo y el
                // sistema la expone como una más: los controles de medios disappears
                // con ella, que es justo donde se usan.
                .setVisibility(Notification.VISIBILITY_PUBLIC)
                .setOngoing(snapshot.state == PlaybackState.PLAYING)
                .setOnlyAlertOnce(true)
                .setShowWhen(false);

        Bitmap icon = notificationArtwork(snapshot);
        if (icon != null) builder.setLargeIcon(icon);
        // El acento en la barra de estado y en el LED: la notificación de una pista
        // con portada azul no puede ser el mismo gris de todas las demás.
        builder.setColor(accentColor);

        builder.setContentIntent(contentIntent());
        // Swipe-away on a paused notification stops the service: without a delete
        // intent the swipe is swallowed and the service stays foreground with
        // nothing playing.
        builder.setDeleteIntent(actionIntent(ACTION_STOP, 9));

        // Previous and Play/Pause flanking Next matches the platform convention.
        builder.addAction(action(notificationIcon(Icon.PREVIOUS), "Previous",
                actionIntent(ACTION_PREVIOUS, 1)));
        builder.addAction(action(notificationIcon(paused ? Icon.PLAY : Icon.PAUSE),
                paused ? "Play" : "Pause", actionIntent(ACTION_TOGGLE, 2)));
        builder.addAction(action(notificationIcon(Icon.NEXT), "Next",
                actionIntent(ACTION_NEXT, 3)));
        // Los controles grandes, para cuando el móvil está bloqueado: es la misma
        // pantalla del reproductor, puesta delante del candado.
        builder.addAction(action(notificationIcon(Icon.REMOTE), "Controls",
                PendingIntent.getActivity(service, 10,
                        new Intent(service, LockScreenControlsActivity.class),
                        pendingIntentFlags())));

        // MediaStyle es lo que convierte esto en una notificación de medios: la
        //趟 que dibuja el sistema en la pantalla de bloqueo, en el panel rápido, en
        // el coche y en los auriculares. Con addAction a secas era una notificación
        // normal con tres botones y no aparecía en ninguno de esos sitios.
        builder.setStyle(new Notification.MediaStyle()
                .setMediaSession(session.getSessionToken())
                // Expandida se ven los cuatro; en la versión corta caben tres y el
                // que se quita es el de los controles grandes, que en pantalla
                // bloqueada no se puede tocar sin desbloquear.
                .setShowActionsInCompactView(0, 1, 2));

        return builder.build();
    }

    /**
     * Qué abre la tarjeta de medios.
     *
     * <p>Con la pantalla encendida, la app: quien toca la notificación quiere el
     * reproductor y todo lo que hay alrededor. Con la pantalla apagada, los controles
     * sobre el candado: esa tarjeta es lo único que se puede tocar sin desbloquear, y
     * abrir la app entera obligaría a autenticarse para cambiar de canción — que es
     * justo lo que una pantalla de bloqueo no debería obligar a hacer.
     *
     * <p>Se decide al publicar, no al tocar: el sistema arma el intent cuando recibe
     * la notificación y no vuelve a preguntarle.
     */
    private PendingIntent contentIntent() {
        boolean interactive = service.getSystemService(android.os.PowerManager.class) != null
                && service.getSystemService(android.os.PowerManager.class).isInteractive();
        Intent target = interactive
                ? new Intent(service, MainActivity.class)
                        .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP)
                : new Intent(service, LockScreenControlsActivity.class);
        return PendingIntent.getActivity(service, interactive ? 0 : 1, target,
                pendingIntentFlags());
    }

    /**
     * Una acción con glifo propio.
     *
     * <p>El atajo {@code Notification.Builder.addAction(Icon, …)} no existe en el
     * {@code android.jar} con el que compila este proyecto, y el camino que sí existe
     * —un id de recurso— obligaría a crear la carpeta {@code res/} que la app no
     * tiene. El constructor de {@code Action} acepta el icono dibujado, que es
     * justo lo que hace falta para que el botón de la notificación tenga el mismo
     * glifo que el reproductor.
     */
    private static Notification.Action action(android.graphics.drawable.Icon icon,
            CharSequence title, PendingIntent intent) {
        return new Notification.Action.Builder(icon, title, intent).build();
    }

    /**
     * El icono pequeño, dibujado una vez.
     *
     * <p>El del framework era un triángulo del tema del sistema: al lado de una
     * portada en color parecía una notificación de otra aplicación. El sistema exige
     * que sea una silueta blanca, así que va el glifo de barras en blanco y plano,
     * con la misma traza que el reproductor.
     */
    private android.graphics.drawable.Icon smallIcon() {
        if (smallIcon == null) {
            IconDrawable drawable = new IconDrawable(Icon.DIAGNOSTICS);
            drawable.setColor(android.graphics.Color.WHITE);
            int side = DesignTokens.dp(DesignTokens.ICON_SIZE_SMALL);
            android.graphics.Bitmap bitmap = android.graphics.Bitmap.createBitmap(side, side,
                    android.graphics.Bitmap.Config.ARGB_8888);
            drawable.setBounds(0, 0, side, side);
            drawable.draw(new android.graphics.Canvas(bitmap));
            smallIcon = android.graphics.drawable.Icon.createWithBitmap(bitmap);
        }
        return smallIcon;
    }

    private android.graphics.drawable.Icon smallIcon;

    /**
     * El acento de la barra de estado.
     *
     * <p>El de la app y no el del sistema: el servicio no ve la portada —la Activity
     * es quien la decodifica— y cruzar ese dato para pintar un píxel del color de la
     * barra de estado no compensa una dependencia nueva entre las dos mitades.
     */
    private final int accentColor = DesignTokens.Palette.ACCENT;

    /** Caché por icono: el mismo glifo se pide en cada reconstrucción. */
    private final java.util.EnumMap<Icon, android.graphics.drawable.Icon> actionIcons =
            new java.util.EnumMap<>(Icon.class);

    /**
     * Los iconos de la notificación son los de la app.
     *
     * <p>Los del framework dibujaban triángulos amarillos de otra época al lado de
     * una portada en color, y en la pantalla de bloqueo esa mezcla se leía como dos
     * cosas distintas. El mismo glifo que en el reproductor, con el mismo trazo.
     *
     * <p>Se rasterizan una vez por icono y se recordan: la notificación se reconstruye
     * en cada tick del servicio, y dibujar un icono nuevo en cada una sería una
     * asignación por tick para siempre.
     */
    private android.graphics.drawable.Icon notificationIcon(Icon icon) {
        android.graphics.drawable.Icon cached = actionIcons.get(icon);
        if (cached != null) return cached;
        int side = DesignTokens.dp(DesignTokens.ICON_SIZE_MEDIUM);
        IconDrawable drawable = new IconDrawable(icon);
        drawable.setColor(android.graphics.Color.WHITE);
        android.graphics.Bitmap bitmap = android.graphics.Bitmap.createBitmap(side, side,
                android.graphics.Bitmap.Config.ARGB_8888);
        drawable.setBounds(0, 0, side, side);
        drawable.draw(new android.graphics.Canvas(bitmap));
        android.graphics.drawable.Icon made =
                android.graphics.drawable.Icon.createWithBitmap(bitmap);
        actionIcons.put(icon, made);
        return made;
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
        // Sin sonido ni vibración: el canal de reproducción no es un aviso, es la
        // línea de estado de lo que ya se está oyendo. Con sonido, cambiar de pista
        // —que pasa cada tres minutos— hacía sonar el móvil con el volumen al alto.
        channel.setSound(null, null);
        channel.enableVibration(false);
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