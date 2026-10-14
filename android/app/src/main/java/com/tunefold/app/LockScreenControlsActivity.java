package com.tunefold.app;

import android.app.Activity;
import android.content.ComponentName;
import android.content.Intent;
import android.content.ServiceConnection;
import android.os.Bundle;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.util.DisplayMetrics;
import android.view.Gravity;
import android.view.View;
import android.view.WindowManager;
import android.widget.FrameLayout;
import android.widget.LinearLayout;

/**
 * Los controles de reproducción sobre la pantalla de bloqueo.
 *
 * <p>La notificación con {@code MediaStyle} ya pone el transporte del sistema en el
 * candado. Esto es la otra mitad: cuando el botón de la notificación o el gesto del
 * sistema abren los controles, lo que aparece es <em>el reproductor</em> —la misma
 * tarjeta, con la misma letra, la misma cola y los mismos botones que dentro de la
 * app— por encima del candado, sin desbloquear nada.
 *
 * <h3>Por qué una Activity y no una ventana propia</h3>
 * <p>Android no deja dibujar sobre la pantalla de bloqueo: lo que sí deja es una
 * Activity con {@code setShowWhenLocked}, que el sistema coloca encima del candado
 * y devuelve al bloquear de nuevo. Es el mismo camino que usa el reproductor del
 * sistema, y por eso sobrevive a que la pantalla se apague y vuelva a encenderse
 * sin que esta pantalla se entere.
 *
 * <h3>Por qué la misma tarjeta y no una copia</h3>
 * <p>Una segunda versión de estos controles se quedaría vieja el día que cambiase
 * uno de los dos: la letra nueva, la cola, el botón de descarga. Reutilizando
 * {@link NowPlayingView} los dos sitios enseñan exactamente lo mismo, y un fallo se
 * arregla una vez.
 *
 * <p>Todo lo durable sigue en el servicio: esta pantalla se ata, lee y manda
 * comandos. Destruirla —o que el sistema la mate con la pantalla apagada— no toca la
 * reproducción, ni la posición, ni la cola, ni las descargas (§18).
 */
public final class LockScreenControlsActivity extends Activity {

    private final Handler handler = new Handler(Looper.getMainLooper());
    private final PlaybackPosition position = new PlaybackPosition();
    /** Stable observer identity, so add and remove pair up across reconnects. */
    private final PlaybackSession.Observer observer = this::render;

    private NowPlayingView controls;
    private LiveAudioBars bars;
    private ForegroundPlaybackService.LocalBinder binder;
    private PlaybackSession session;
    private PlaybackController controller;
    private ProviderRegistry providers;
    private TrackDownloader downloader;
    private boolean bound;
    private boolean destroyed;
    private boolean portraitOnly;

    private final ServiceConnection connection = new ServiceConnection() {
        @Override public void onServiceConnected(ComponentName name, IBinder service) {
            binder = (ForegroundPlaybackService.LocalBinder) service;
            controller = binder.controller();
            providers = binder.providers();
            session = binder.session();
            downloader = binder.downloader();
            bars = new LiveAudioBars(controller);
            // One live-audio source per binding, same as the main screen: the bars
            // need the engine handle, and a second source would be a second buffer.
            controls.attachSound(bars);
            session.addObserver(observer);
            applyWindow(true);
            render();
        }

        @Override public void onServiceDisconnected(ComponentName name) {
            session = null;
            controller = null;
            providers = null;
            downloader = null;
            binder = null;
        }
    };

    @Override protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        DesignTokens.init(this);
        // Sobre el candado y sin apagar la pantalla al entrar: esto se abre desde un
        // botón de la notificación, o sea desde una decisión ya tomada.
        setShowWhenLocked(true);
        setTurnScreenOn(false);
        // La pantalla se queda encendida mientras se manejan los controles, y la
        // ventana es opaca: translúcida se transparentaba el papel del candado detrás
        // de la letra y no se leía de nada.
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);

        portraitOnly = getIntent().getBooleanExtra(EXTRA_LOCKED, true);

        FrameLayout host = new FrameLayout(this);
        host.setBackgroundColor(DesignTokens.Palette.SURFACE_BASE);

        controls = new NowPlayingView(this, new NowPlayingView.Actions() {
            @Override public void onPlayPause() { if (session != null) session.togglePause(); }
            @Override public void onNext() { if (session != null) session.next(); }
            @Override public void onPrevious() { if (session != null) session.previous(); }
            @Override public void onLike() {
                if (session != null) { session.toggleLike(); render(); }
            }
            @Override public void onDownload() { advanceDownload(); }
            @Override public void onCancelDownload() {
                MediaTrack track = currentTrack();
                if (track != null && downloader != null) downloader.cancel(track);
            }
            @Override public void onRetryDownload() {
                MediaTrack track = currentTrack();
                if (track != null && downloader != null) downloader.retry(track);
            }
            @Override public void onRetryPlayback() { if (session != null) session.retry(); }
            @Override public void onSkipToNext() { if (session != null) session.skipToNext(); }
            @Override public void onToggleAutoplay() {
                if (session != null) session.setAutoplayEnabled(!session.autoplayEnabled());
            }
        });
        controls.setQueuedTrackChosen(track -> {
            if (session != null && track != null && track.key() != null) {
                session.playQueued(track.key());
            }
        });

        FrameLayout.LayoutParams card = new FrameLayout.LayoutParams(-1, -1);
        card.gravity = portraitOnly ? Gravity.CENTER : Gravity.CENTER_VERTICAL;
        host.addView(controls.view(), card);
        setContentView(host);
    }

    @Override protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
    }

    @Override protected void onStart() {
        super.onStart();
        Intent service = new Intent(this, ForegroundPlaybackService.class);
        bound = bindService(service, connection, BIND_AUTO_CREATE);
    }

    @Override protected void onStop() {
        super.onStop();
        handler.removeCallbacks(tick);
        if (bound) {
            unbindService(connection);
            bound = false;
        }
        if (deviceInteractive() && !isFinishing()) finish();
        session = null;
        controller = null;
        providers = null;
        downloader = null;
        binder = null;
    }

    @Override protected void onDestroy() {
        destroyed = true;
        handler.removeCallbacks(tick);
        if (session != null) session.removeObserver(observer);
        controls.release();
        controls.attachSound(null);
        super.onDestroy();
    }

    /**
     * The same read the main screen does, at the same cadence.
     *
     * <p>Identity and time come from one instant, so a tick can never pair this
     * track's title with the previous track's position.
     */
    private final Runnable tick = new Runnable() {
        @Override public void run() {
            if (destroyed || session == null || !bound) return;
            PlaybackPosition now = session.readPosition(position);
            controls.setPosition(now.key, now.positionMs, now.durationMs);
            controls.tick();
            controls.refreshSound();
            handler.postDelayed(this, DesignTokens.POSITION_INTERVAL_MS);
        }
    };

    private MediaTrack currentTrack() {
        return session == null ? null : session.snapshot().currentTrack;
    }

    /**
     * Download, cancel, retry or remove, decided by the state and not by the label.
     *
     * <p>The same dispatch the main screen uses: the control offers one verb and the
     * state says which one it is, so the button cannot mean "remove" on a track that
     * was never downloaded.
     */
    private void advanceDownload() {
        MediaTrack track = currentTrack();
        if (track == null || downloader == null) return;
        if (downloader.stateOf(track).isActive()) downloader.cancel(track);
        else if (downloader.stateOf(track).isRetryable()) downloader.retry(track);
        else if (downloader.stateOf(track).isDownloaded()) downloader.remove(track);
        else downloader.start(track);
    }

    private void render() {
        if (destroyed || session == null) return;
        PlaybackSession.Snapshot snapshot = session.snapshot();
        controls.applyTrack(snapshot);
        controls.applyPlaybackState(snapshot);
        controls.applyLibraryState(snapshot);
        controls.applyDownloadState(snapshot);
        controls.applyError(snapshot);
        controls.setUpNextFrom(snapshot.queue, snapshot.currentKey);
        loadArtwork(snapshot.currentTrack);
        requestLyrics(snapshot);
    }

    /**
     * La portada de lo que suena, decodificada al tamaño que la tarjeta está usando.
     *
     * <p>El mismo camino que la pantalla principal: una vez por pista, desde la caché,
     * y el resultado se entrega con la identidad a la que pertenece para que una
     * respuesta que llega tarde no pinte la portada de la canción anterior.
     */
    private void loadArtwork(MediaTrack track) {
        if (providers == null || track == null || track.thumbnail.isEmpty()) return;
        String identity = identityOf(track);
        if (identity.isEmpty() || identity.equals(coverIdentity)) return;
        coverIdentity = identity;
        controls.expectArtwork(identity);
        providers.loadArtwork(track, controls.artworkSidePx(), (bitmap, error) -> {
            if (destroyed || bitmap == null) return;
            controls.setArtwork(identity, bitmap);
        });
    }

    /** La portada que hay en pantalla, para no decodificarla dos veces. */
    private String coverIdentity = "";

    private static String identityOf(MediaTrack track) {
        TrackKey key = track.key();
        return key == null ? "" : key.toString();
    }

    /** One lyric asked per track, exactly as the main screen does. */
    private void requestLyrics(PlaybackSession.Snapshot snapshot) {
        TrackKey key = snapshot.currentKey;
        if (key == null || key.toString().equals(lyricsRequestedFor)) return;
        lyricsRequestedFor = key.toString();
        controls.setLyrics(null);
        session.lyrics(snapshot.currentTrack, (lyric, error) -> {
            if (destroyed || session == null || key.equals(session.snapshot().currentKey)) {
                controls.setLyrics(lyric);
            }
        });
    }

    private String lyricsRequestedFor;

    /**
     * Se cierra cuando deja de estar delante, y no antes.
     *
     * <p>La regla es "parada <b>y</b> pantalla encendida". Parada sola no vale: al
     * apagar la pantalla el sistema la esconde detrás del candado y esta actividad se
     * detiene; cerrarse ahí dejaba al usuario sin controles al encender el móvil otra
     * vez, cuando es justo entonces cuando los quiere. Y encendida sola tampoco: abrir
     * la sombra de notificaciones quita el foco sin pararla, y cerrarse en ese momento
     * era cerrar la pantalla del reproductor por mirar la hora.
     *
     * <p>Encendida y parada a la vez es una cosa sola: el usuario ha desbloqueado y
     * ha seguido con su vida. Ahí esta pantalla ya hizo su trabajo.
     */
    private boolean deviceInteractive() {
        android.os.PowerManager power = getSystemService(android.os.PowerManager.class);
        return power != null && power.isInteractive();
    }

    @Override public void onBackPressed() {
        // Back returns to the lock screen, not to the app: the whole point of being
        // here is not having to unlock.
        finish();
    }

    /**
     * One line per configuration change, never per frame.
     *
     * <p>The same derivation as the main screen, on the same window: two hosts of one
     * composition have to agree on when it splits into two regions.
     */
    private void applyWindow(boolean force) {
        DisplayMetrics metrics = getResources().getDisplayMetrics();
        float density = Math.max(0.01f, metrics.density);
        int widthPx = Math.max(1, metrics.widthPixels);
        int heightPx = Math.max(1, metrics.heightPixels);
        int smallestPx = Math.min(metrics.widthPixels, metrics.heightPixels);
        LayoutRules.Profile profile = LayoutRules.of(Math.round(widthPx / density),
                Math.round(heightPx / density), Math.round(smallestPx / density),
                widthPx > heightPx);
        if (!force && LayoutRules.sameLayout(profile, window)) return;
        window = profile;
        int cardWidthPx = profile.contentMaxDp() > 0
                ? DesignTokens.dp(profile.contentMaxDp()) : widthPx;
        int inner = Math.max(0, cardWidthPx - DesignTokens.dp(DesignTokens.CARD_PADDING) * 2);
        int innerHeight = Math.max(0, heightPx - DesignTokens.dp(DesignTokens.CARD_PADDING) * 2);
        controls.setWindow(profile, Math.round(inner / density),
                Math.round(innerHeight / density));
    }

    private LayoutRules.Profile window;

    @Override public void onConfigurationChanged(android.content.res.Configuration newConfig) {
        super.onConfigurationChanged(newConfig);
        applyWindow(false);
        // The card is not rebuilt, but the padding it derived from the window is.
        controls.view().requestLayout();
    }

    @Override protected void onResume() {
        super.onResume();
        handler.post(tick);
    }

    /** Extra that says "the device is locked": these controls stay in portrait. */
    static final String EXTRA_LOCKED = "locked";
}
