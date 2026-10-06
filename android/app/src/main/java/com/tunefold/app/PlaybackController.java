package com.tunefold.app;

import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.atomic.AtomicLong;

/** Service-owned command and state boundary for the existing Rust/AudioTrack pipeline. */
final class PlaybackController implements AudioTrackOutput.Listener {
    static final int IDLE = 0, LOADING = 1, PLAYING = 2, PAUSED = 3,
            STOPPED = 4, ERROR = 5;

    interface Observer { void onPlaybackChanged(); }
    interface TrackCallback { void onTrack(MediaTrack track, String error); }
    /** Notified once when the current track reaches its end of media. */
    interface TrackFinishedListener { void onTrackFinished(); }
    /** Notified once the engine accepted the source and is starting output. */
    interface SourceStartedListener { void onSourceStarted(); }

    private final ExecutorService commands = Executors.newSingleThreadExecutor(r ->
            new Thread(r, "tunefold-playback-commands"));
    private final AudioTrackOutput output;
    private final Observer observer;
    private final AtomicLong operation = new AtomicLong();
    private volatile long engine;
    private volatile int state = IDLE;
    private volatile String error = "";
    private volatile String diagnostics = "";
    private volatile boolean released;
    private volatile TrackFinishedListener trackFinishedListener;
    private volatile SourceStartedListener sourceStartedListener;

    void setTrackFinishedListener(TrackFinishedListener value) {
        trackFinishedListener = value;
    }

    PlaybackController(Observer observer) {
        this.observer = observer;
        output = new AudioTrackOutput(this);
        commands.execute(() -> {
            try {
                engine = TunefoldBridge.createEngine();
                if (engine == 0) fail("Rust engine initialization failed");
            } catch (Throwable failure) {
                fail("JNI initialization failed: " + failure);
            }
            notifyChanged();
        });
    }

    void play(String url) { play(new PlayableSource(url, "[]")); }

    void play(PlayableSource source) { play(source, null); }

    void play(PlayableSource source, PlaybackTrace trace) {
        startPlaybackSource(source, trace, operation.incrementAndGet(), null);
    }

    /**
     * Starts a resolved source, reporting when the engine has accepted it.
     *
     * <p>{@code onStarted} fires once output has actually begun, which is what
     * lets the session distinguish "still buffering" from "playing" without
     * guessing from a timer.
     */
    void play(PlayableSource source, PlaybackTrace trace, Runnable onStarted) {
        startPlaybackSource(source, trace, operation.incrementAndGet(), onStarted);
    }

    private void startPlaybackSource(PlayableSource source, PlaybackTrace trace, long requestId,
                                     Runnable onStarted) {
        state = LOADING;
        startedCallback = onStarted;
        error = "";
        notifyChanged();
        commands.execute(() -> {
            if (operation.get() != requestId) return;
            long handle = engine;
            if (handle == 0 && !released) {
                try { handle = TunefoldBridge.createEngine(); }
                catch (Throwable failure) { fail("JNI initialization failed: " + failure); return; }
                engine = handle;
            }
            if (released || handle == 0) { fail("Rust engine is unavailable"); return; }
            output.requestStop();
            output.awaitStopped();
            TunefoldBridge.stopAudio(handle);
            if (operation.get() != requestId) return;
            currentSource = source;
            currentTrace = trace;
            if (trace != null) {
                trace.attachEngine(handle);
                trace.mark(PlaybackTrace.PLAYABLE_SOURCE_AVAILABLE,
                        source.url.startsWith("file:") ? "kind=local_media" : "kind=http");
            }
            // The engine anchors its own timeline to this offset so every Rust
            // event (HTTP, probe, decode) lands on the same Play-tap origin.
            long tapToDecoderUs = trace == null ? 0L : trace.elapsedUs();
            if (!TunefoldBridge.playStream(handle, source.url, source.headersJson, tapToDecoderUs)) {
                fail(readError(handle, "Could not start stream"));
                return;
            }
            output.start(handle, trace);
            refresh();
        });
    }

    /** Set for the play currently starting; consumed once output begins. */
    private volatile Runnable startedCallback;

    private void notifySourceStarted() {
        Runnable callback = startedCallback;
        startedCallback = null;
        if (callback != null) callback.run();
    }

    void playTrack(MediaTrack track, ProviderRegistry providers, PlaybackTrace trace,
                   TrackCallback callback) {
        long requestId = operation.incrementAndGet();
        if (trace != null) trace.mark(PlaybackTrace.CONTROLLER_RECEIVED_PLAY);
        state = LOADING;
        error = "";
        notifyChanged();
        providers.playableSource(track, trace, (source, failure) -> {
            if (operation.get() != requestId) return;
            if (failure != null) {
                fail(failure);
                if (callback != null) callback.onTrack(track, failure);
                return;
            }
            if (callback != null) callback.onTrack(track, null);
            startPlaybackSource(source, trace, requestId, null);
        });
    }

    void playUrl(String url, ProviderRegistry providers, PlaybackTrace trace,
                 TrackCallback callback) {
        long requestId = operation.incrementAndGet();
        if (trace != null) trace.mark(PlaybackTrace.CONTROLLER_RECEIVED_PLAY);
        state = LOADING;
        error = "";
        notifyChanged();
        providers.resolve(url, trace, (track, failure) -> {
            if (operation.get() != requestId) return;
            if (failure != null) {
                fail(failure);
                if (callback != null) callback.onTrack(null, failure);
                return;
            }
            if (callback != null) callback.onTrack(track, null);
            providers.playableSource(track, trace, (source, sourceFailure) -> {
                if (operation.get() != requestId) return;
                if (sourceFailure != null) {
                    fail(sourceFailure);
                    return;
                }
                startPlaybackSource(source, trace, requestId, null);
            });
        });
    }

    void pause() {
        commands.execute(() -> {
            long handle = engine;
            if (handle == 0 || state != PLAYING && state != LOADING) return;
            TunefoldBridge.pauseAudio(handle);
            output.pause();
            state = PAUSED;
            notifyChanged();
        });
    }

    void resume() {
        commands.execute(() -> {
            long handle = engine;
            if (handle == 0 || state != PAUSED) return;
            TunefoldBridge.resumeAudio(handle);
            state = LOADING;
            notifyChanged();
        });
    }

    void togglePause() { if (state == PAUSED) resume(); else pause(); }

    void stop() {
        operation.incrementAndGet();
        commands.execute(() -> {
            long handle = engine;
            output.requestStop();
            output.awaitStopped();
            if (handle != 0) TunefoldBridge.stopAudio(handle);
            destroyEngine(handle);
            state = STOPPED;
            notifyChanged();
        });
    }

    /**
     * Restarts the current track from the beginning.
     *
     * <p>Used by Previous when the track has been playing long enough that the
     * user means "start again" rather than "go back".
     */
    void restartCurrent() {
        // There is no seek in the Android pipeline: the honest implementation is
        // to replay the same source, which is what a fresh play of the track is.
        if (currentSource == null) return;
        startPlaybackSource(currentSource, currentTrace,
                operation.incrementAndGet(), null);
    }

    private volatile PlayableSource currentSource;

    /** Trace of the play in progress, reused when Previous restarts it. */
    private volatile PlaybackTrace currentTrace;

    void refresh() {
        commands.execute(this::refreshOnCommands);
    }

    private void refreshOnCommands() {
        long handle = engine;
        if (handle == 0 || released) return;
        diagnostics = TunefoldBridge.getRuntimeDiagnostics(handle);

        // End-of-track is a distinct signal from a user stop; it is consumed
        // here so the queue/autoplay policy sees each EOF exactly once.
        if (TunefoldBridge.takeTrackFinished(handle)) {
            TrackFinishedListener listener = trackFinishedListener;
            // The listener owns the transition: it either advances the queue or
            // stops. Adopting the engine's STOPPED afterwards would clobber
            // whatever it just decided and briefly show "Stopped" mid-playback.
            if (listener != null) {
                listener.onTrackFinished();
                return;
            }
        }

        int nativeState = TunefoldBridge.getPlaybackState(handle);
        if (nativeState == ERROR) {
            error = readError(handle, "Playback failed");
            output.requestStop();
            output.awaitStopped();
            TunefoldBridge.stopAudio(handle);
            destroyEngine(handle);
            state = ERROR;
            notifyChanged();
        } else if (nativeState == STOPPED && state != STOPPED) {
            output.requestStop();
            output.awaitStopped();
            TunefoldBridge.stopAudio(handle);
            destroyEngine(handle);
            state = STOPPED;
            notifyChanged();
        } else {
            state = nativeState;
            diagnostics = TunefoldBridge.getRuntimeDiagnostics(handle);
            notifyChanged();
        }
    }

    int state() { return state; }

    /** Native engine handle, or 0 when the engine is not initialized. */
    long engineHandle() { return engine; }

    /** Playback position in milliseconds as tracked by the decoder. */
    long positionMs() {
        long handle = engine;
        return handle == 0 ? 0L : TunefoldBridge.getPositionMs(handle);
    }
    String error() { return error; }
    String diagnostics() { return diagnostics; }

    void release() {
        if (released) return;
        released = true;
        operation.incrementAndGet();
        commands.execute(() -> {
            long handle = engine;
            output.requestStop();
            output.awaitStopped();
            if (handle != 0) {
                TunefoldBridge.stopAudio(handle);
                destroyEngine(handle);
            }
            commands.shutdown();
        });
    }

    @Override public void onOutputMessage(String message) {
        if (message != null && (message.contains("failed") || message.contains("Exception"))) {
            error = message;
            state = ERROR;
        }
        notifyChanged();
    }

    private void fail(String message) { error = message; state = ERROR; notifyChanged(); }
    private void destroyEngine(long handle) {
        if (handle == 0) return;
        TunefoldBridge.destroyEngine(handle);
        if (engine == handle) engine = 0;
    }
    private void notifyChanged() { if (observer != null) observer.onPlaybackChanged(); }
    private static String readError(long handle, String fallback) {
        String value = TunefoldBridge.getLastError(handle);
        return value == null || value.isEmpty() ? fallback : value;
    }
}
