package com.tunefold.app;

import android.util.Log;

import java.util.ArrayList;
import java.util.List;

/**
 * The single source of truth for what Tunefold is playing and why (§10, §18).
 *
 * <p>Owned by {@link ForegroundPlaybackService}, never by an Activity, so
 * playback, queue position, current track, error and autoplay survive Activity
 * recreation and process-boundary changes. A recreated Activity rebuilds its
 * whole view from {@link #snapshot()} instead of restoring its own copy.
 *
 * <p>Everything here runs on the main thread: commands come from the UI and from
 * system media controls, and every asynchronous provider/download callback is
 * delivered back on the main thread, so the state machine needs no locking and
 * stays easy to follow.
 */
final class PlaybackSession {
    private static final String TAG = "TunefoldPerf";
    /** Previous restarts the track after this many milliseconds into it. */
    private static final long PREVIOUS_RESTART_MS = 3_000L;

    interface Observer {
        /** The session changed in a way the UI must reflect. */
        void onSessionChanged();
    }

    /** Immutable view of everything the UI renders. */
    static final class Snapshot {
        final PlaybackState state;
        final MediaTrack currentTrack;
        final TrackKey currentKey;
        final PlaybackError error;
        final boolean liked;
        final boolean downloaded;
        final DownloadState download;
        final List<MediaTrack> queue;
        final int queueIndex;
        final boolean autoplayEnabled;
        final boolean canGoNext;
        final boolean canGoPrevious;

        Snapshot(PlaybackState state, MediaTrack currentTrack, TrackKey currentKey,
                 PlaybackError error, boolean liked, boolean downloaded,
                 DownloadState download, List<MediaTrack> queue, int queueIndex,
                 boolean autoplayEnabled, boolean canGoNext, boolean canGoPrevious) {
            this.state = state;
            this.currentTrack = currentTrack;
            this.currentKey = currentKey;
            this.error = error;
            this.liked = liked;
            this.downloaded = downloaded;
            this.download = download;
            this.queue = queue;
            this.queueIndex = queueIndex;
            this.autoplayEnabled = autoplayEnabled;
            this.canGoNext = canGoNext;
            this.canGoPrevious = canGoPrevious;
        }
    }

    private final PlaybackController controller;
    private final ProviderRegistry providers;
    private final PlaybackQueue queue = new PlaybackQueue();
    private final Library library;
    private final Recommendations recommendations;
    private final RecoveryPolicy recovery = new RecoveryPolicy();
    private final List<Observer> observers = new ArrayList<>();

    private TrackDownloader downloader;
    private PlaybackState state = PlaybackState.IDLE;
    private MediaTrack currentTrack;
    private PlaybackError error;
    private PlaybackTrace trace;
    /** Guards against two concurrent autoplay fetches for one EOF (§16). */
    private boolean autoplayInFlight;
    /** True once autoplay has been considered for the current EOF. */
    private boolean eofHandled;

    PlaybackSession(PlaybackController controller, ProviderRegistry providers, Library library) {
        this.controller = controller;
        this.providers = providers;
        this.library = library;
        this.recommendations = new Recommendations(
                (videoId, callback) -> providers.related(videoId,
                        (tracks, error) -> callback.onRecommendations(tracks, error)),
                library, queue,
                (event, detail) -> PlaybackTrace.markCurrent(event, detail));
        controller.setTrackFinishedListener(this::onTrackFinished);
    }

    /** Attaches the downloader once the LocalMediaStore exists. */
    void attachDownloader(TrackDownloader value) {
        this.downloader = value;
        if (downloader != null) {
            downloader.setObserver((key, state) -> main().post(this::notifyChanged));
            downloader.seedFromStore();
        }
    }

    void addObserver(Observer observer) {
        if (observer != null) observers.add(observer);
    }

    void removeObserver(Observer observer) { observers.remove(observer); }

    // ------------------------------------------------------------- snapshot

    /** Everything the UI needs, computed from live state. */
    Snapshot snapshot() {
        MediaTrack track = queue.current();
        TrackKey key = track == null ? null : track.key();
        DownloadState download = downloader == null || key == null
                ? DownloadState.idle(key)
                : downloader.stateOf(key);
        boolean canNext;
        boolean canPrevious;
        synchronized (queue.lock()) {
            canNext = queue.peekNext() != null;
            canPrevious = queue.peekPrevious(0, PREVIOUS_RESTART_MS) != null
                    || (controller.positionMs() > PREVIOUS_RESTART_MS && track != null);
        }
        return new Snapshot(state, track, key, error,
                library.isLiked(key), download.isDownloaded(), download,
                queue.snapshot(), queue.currentIndex(), queue.isAutoplayEnabled(),
                canNext, canPrevious);
    }

    // -------------------------------------------------------------- commands

    /** Plays a track from search, a playlist, recommendations or the queue. */
    void play(MediaTrack track) {
        if (track == null) return;
        if (track.key() == null) {
            setError(PlaybackError.classify(PlaybackError.Stage.SOURCE_RESOLUTION,
                    "Track has no identity"));
            return;
        }
        queue.playNow(track);
        startCurrent();
    }

    /** Appends to the queue without interrupting playback. */
    void enqueue(MediaTrack track) {
        if (track == null) return;
        if (queue.append(track) >= 0) notifyChanged();
    }

    /** Resolves a pasted URL and plays it. */
    void playUrl(String url) {
        if (url == null || url.trim().isEmpty()) return;
        beginTrace("url");
        state = PlaybackState.RESOLVING_METADATA;
        error = null;
        notifyChanged();
        PlaybackTrace.markCurrent(PlaybackTrace.SOURCE_RESOLUTION_START, "kind=url");
        providers.resolve(url, trace, (track, failure) -> {
            if (track == null) {
                setError(PlaybackError.classify(PlaybackError.Stage.METADATA,
                        failure == null ? "No track found" : failure));
                return;
            }
            if (track.key() == null) {
                setError(PlaybackError.classify(PlaybackError.Stage.METADATA,
                        "Track has no identity"));
                return;
            }
            queue.playNow(track);
            startCurrent();
        });
    }

    /** Runs a catalog search and reports the results to {@code callback}. */
    void search(String query, ProviderRegistry.Callback<List<MediaTrack>> callback) {
        state = PlaybackState.SEARCHING;
        error = null;
        notifyChanged();
        providers.search(query, (tracks, failure) -> {
            // Searching must not disturb what is playing.
            if (state == PlaybackState.SEARCHING) state = PlaybackState.IDLE;
            notifyChanged();
            if (callback != null) callback.complete(tracks, failure);
        });
    }

    void togglePause() {
        if (state == PlaybackState.PLAYING) {
            controller.pause();
        } else if (state == PlaybackState.PAUSED) {
            controller.resume();
        }
    }

    /** Skips to the next queued item, or stops when there is none. */
    void next() {
        MediaTrack target = queue.peekNext();
        if (target == null) {
            // No next: keep the product rule that Next never invents content.
            state = PlaybackState.STOPPED;
            notifyChanged();
            return;
        }
        PlaybackTrace.markCurrent(PlaybackTrace.QUEUE_NEXT,
                "to=" + target.key());
        queue.advance();
        startCurrent();
    }

    /**
     * Goes back: restart the current track when it has been playing long enough,
     * otherwise step to the previous one (§13).
     */
    void previous() {
        MediaTrack target = queue.peekPrevious(controller.positionMs(), PREVIOUS_RESTART_MS);
        if (target == null) {
            controller.restartCurrent();
            return;
        }
        if (target == queue.current()) {
            // Past the restart threshold: Previous restarts the current track
            // instead of skipping back, so the cursor must not move.
            PlaybackTrace.markCurrent(PlaybackTrace.QUEUE_PREVIOUS, "restart");
            controller.restartCurrent();
            return;
        }
        PlaybackTrace.markCurrent(PlaybackTrace.QUEUE_PREVIOUS, "to=" + target.key());
        queue.retreat();
        startCurrent();
    }

    void stop() {
        controller.stop();
        state = PlaybackState.STOPPED;
        error = null;
        endTrace();
        notifyChanged();
    }

    /** Retries the failed step, subject to the bounded policy (§20). */
    void retry() {
        PlaybackError current = error;
        if (current != null && !current.retryable) {
            // Terminal: offer Next instead of pretending Retry will help.
            notifyChanged();
            return;
        }
        if (!recovery.takePlaybackSourceRetry()) {
            setError(current == null
                    ? new PlaybackError(PlaybackError.Kind.RECOVERABLE,
                            PlaybackError.Stage.UNKNOWN, "Retry limit reached", false)
                    : current);
            return;
        }
        PlaybackTrace.markCurrent(PlaybackTrace.PLAYBACK_RETRY,
                "left=" + recovery.playbackSourceRetriesLeft());
        if (queue.current() != null) startCurrent();
    }

    /** Next, then Previous: the escape hatch offered next to a failed track. */
    void skipToNext() { next(); }

    // ----------------------------------------------------------------- like

    boolean toggleLike() {
        MediaTrack track = queue.current();
        if (track == null) return false;
        boolean liked = library.toggleLiked(track);
        PlaybackTrace.markCurrent(PlaybackTrace.LIKE_CHANGED,
                "id=" + track.key() + " liked=" + liked);
        notifyChanged();
        return liked;
    }

    boolean isLiked(MediaTrack track) { return library.isLiked(track); }

    void setAutoplayEnabled(boolean enabled) {
        queue.setAutoplayEnabled(enabled);
        notifyChanged();
    }

    boolean autoplayEnabled() { return queue.isAutoplayEnabled(); }

    // -------------------------------------------------------------- library

    /** L1K3D contents, for the playlist screen. */
    List<TrackKey> likedKeys() { return library.likedKeys(); }

    /** `Descargadas`: exactly the store's valid contents (§5). */
    List<MediaTrack> downloadedTracks() {
        // Membership is not stored here: the playlist is derived from the store.
        List<MediaTrack> result = new ArrayList<>();
        if (downloader == null) return result;
        for (TrackKey key : downloadedKeys()) result.add(new MediaTrack(
                key.provider(), key.providerTrackId(), trackTitle(key), "", "", "", "artist",
                -1, "", "", "{}"));
        return result;
    }

    /**
     * Downloaded identities, from already-known state.
     *
     * <p>Never walks the filesystem: the store is enumerated once at attach on a
     * worker thread and seeded into the registry, so rendering this view stays
     * free of disk I/O (§27).
     */
    List<TrackKey> downloadedKeys() {
        if (downloader == null) return new ArrayList<>();
        return downloader.registry().storedKeys();
    }

    private String trackTitle(TrackKey key) {
        MediaTrack track = findKnown(key);
        return track == null ? key.providerTrackId() : track.title;
    }

    /** Finds a track we already know about, by identity. */
    MediaTrack findKnown(TrackKey key) {
        if (key == null) return null;
        MediaTrack current = queue.current();
        if (current != null && key.equals(current.key())) return current;
        for (MediaTrack track : queue.snapshot()) {
            if (key.equals(track.key())) return track;
        }
        return null;
    }

    // ------------------------------------------------------------- download

    TrackDownloader downloader() { return downloader; }

    // ------------------------------------------------------- playback flow

    /** Resolves the current queue item and hands it to the decoder. */
    private void startCurrent() {
        MediaTrack track = queue.current();
        if (track == null) return;
        currentTrack = track;
        eofHandled = false;
        recovery.resetForNextTrack();
        library.markPlayed(track.key());
        beginTrace(track.key().toString());
        state = PlaybackState.RESOLVING_SOURCE;
        error = null;
        notifyChanged();

        TrackKey key = track.key();
        PlaybackTrace active = trace;
        providers.playableSource(track, active, (source, failure) -> {
            if (source == null) {
                setError(PlaybackError.classify(PlaybackError.Stage.SOURCE_RESOLUTION,
                        failure == null ? "Source resolution failed" : failure));
                return;
            }
            boolean local = source.url.startsWith("file:");
            PlaybackTrace.markCurrent(local
                    ? PlaybackTrace.LOCAL_TRACK_FOUND
                    : PlaybackTrace.LOCAL_TRACK_MISSING, "id=" + key);
            state = PlaybackState.BUFFERING;
            notifyChanged();
            // The controller owns the engine; the session only supplies the
            // source and the state it should show while it starts.
            controller.play(source, active, () -> {
                if (state == PlaybackState.BUFFERING || state == PlaybackState.RESOLVING_SOURCE) {
                    state = PlaybackState.PLAYING;
                    notifyChanged();
                }
            });
        });
    }

    /**
     * The decoder reached the end of the media.
     *
     * <p>Runs at most once per track (§16): a second EOF for the same track is
     * ignored so a duplicated signal cannot start two new tracks.
     */
    private void onTrackFinished() {
        if (eofHandled) return;
        eofHandled = true;

        MediaTrack finished = queue.current();
        AutoplayPolicy.Outcome outcome = AutoplayPolicy.afterTrackFinished(
                queue, queue.isAutoplayEnabled(), null);

        if (outcome.decision == AutoplayPolicy.Decision.PLAY_NEXT) {
            queue.advance();
            startCurrent();
            return;
        }
        if (outcome.decision == AutoplayPolicy.Decision.RECOMMEND) {
            // A candidate was already resolved: play it without a round trip.
            queue.setCurrent(outcome.target);
            startCurrent();
            return;
        }
        if (outcome.decision == AutoplayPolicy.Decision.STOP) {
            state = PlaybackState.STOPPED;
            endTrace();
            notifyChanged();
            return;
        }

        // Autoplay: fetch a candidate. Bounded and single-flight.
        if (finished == null || autoplayInFlight) {
            state = PlaybackState.STOPPED;
            endTrace();
            notifyChanged();
            return;
        }
        if (!recovery.takeAutoplaySkip()) {
            Log.i(TAG, "event=AUTOPLAY_GAVE_UP reason=skip_budget");
            state = PlaybackState.STOPPED;
            endTrace();
            notifyChanged();
            return;
        }
        autoplayInFlight = true;
        PlaybackTrace.markCurrent(PlaybackTrace.AUTOPLAY_TRIGGER, "id=" + finished.key());
        recommendations.forTrack(finished, (candidates, failure) -> {
            autoplayInFlight = false;
            MediaTrack chosen = recommendations.select(candidates);
            if (chosen == null) {
                // §25: a recommendation failure stops playback, it never errors.
                Log.i(TAG, "event=AUTOPLAY_NO_CANDIDATE");
                state = PlaybackState.STOPPED;
                endTrace();
                notifyChanged();
                return;
            }
            recovery.onAutoplaySucceeded();
            queue.setCurrent(chosen);
            startCurrent();
        });
    }

    /** Recommendations for Home; failure yields an empty list, never an error. */
    void recommendationsForHome(MediaTrack origin, Recommendations.Callback callback) {
        if (origin == null) {
            if (callback != null) callback.onRecommendations(new ArrayList<>(), null);
            return;
        }
        recommendations.forTrack(origin, callback);
    }

    // -------------------------------------------------------------- plumbing

    /** Called by the controller when the engine reports a state change. */
    void onControllerState(int engineState, String message) {
        PlaybackState mapped = PlaybackState.fromEngine(engineState);
        if (mapped == PlaybackState.ERROR) {
            setError(PlaybackError.classify(PlaybackError.Stage.OUTPUT,
                    message == null ? "Playback failed" : message));
            return;
        }
        if (state != PlaybackState.SEARCHING
                && state != PlaybackState.RESOLVING_METADATA
                && state != PlaybackState.RESOLVING_SOURCE) {
            state = mapped;
        }
        notifyChanged();
    }

    private void setError(PlaybackError value) {
        error = value;
        state = PlaybackState.ERROR;
        PlaybackTrace.markCurrent(PlaybackTrace.PLAYBACK_ERROR,
                "kind=" + value.kind + " retryable=" + value.retryable);
        Log.i(TAG, "event=PLAYBACK_ERROR kind=" + value.kind + " stage=" + value.stage);
        endTrace();
        notifyChanged();
    }

    private void beginTrace(String id) {
        trace = new PlaybackTrace(id);
        PlaybackTrace.setCurrent(trace);
        trace.mark(PlaybackTrace.CONTROLLER_RECEIVED_PLAY);
    }

    private void endTrace() {
        PlaybackTrace.clearCurrent();
        trace = null;
    }

    PlaybackTrace trace() { return trace; }

    String diagnostics() { return controller.diagnostics(); }

    long positionMs() { return controller.positionMs(); }

    private void notifyChanged() {
        for (Observer observer : new ArrayList<>(observers)) {
            observer.onSessionChanged();
        }
    }

    private static android.os.Handler main() {
        return MAIN;
    }

    private static final android.os.Handler MAIN =
            new android.os.Handler(android.os.Looper.getMainLooper());
}