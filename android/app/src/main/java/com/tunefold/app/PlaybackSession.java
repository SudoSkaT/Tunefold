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
        /**
         * Position of {@link #currentTrack}, in milliseconds.
         *
         * <p>Carried inside the snapshot on purpose: reading the position
         * separately and pairing it with a track from another call is what let a
         * new track be shown with the previous track's elapsed time.
         */
        final long positionMs;

        Snapshot(PlaybackState state, MediaTrack currentTrack, TrackKey currentKey,
                 PlaybackError error, boolean liked, boolean downloaded,
                 DownloadState download, List<MediaTrack> queue, int queueIndex,
                 boolean autoplayEnabled, boolean canGoNext, boolean canGoPrevious,
                 long positionMs) {
            this.positionMs = positionMs;
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
    /**
     * Observers, as a set so registering the same one twice cannot deliver every
     * notification twice (§18). Insertion order is kept so rendering stays
     * predictable.
     */
    private final java.util.Set<Observer> observers = new java.util.LinkedHashSet<>();

    private TrackDownloader downloader;
    private PlaybackState state = PlaybackState.IDLE;
    /**
     * Decides which track the reported position belongs to.
     *
     * <p>Single owner of the temporal state, shared by the app UI, the
     * notification and the lock screen so none of them can invent their own.
     */
    private final PlaybackClock clock = new PlaybackClock();
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
                    || (positionMs() > PREVIOUS_RESTART_MS && track != null);
        }
        return new Snapshot(state, track, key, error,
                library.isLiked(key), download.isDownloaded(), download,
                queue.snapshot(), queue.currentIndex(), queue.isAutoplayEnabled(),
                canNext, canPrevious, positionMs());
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
        setPlaying(state != PlaybackState.PLAYING);
    }

    /**
     * Moves to an explicit playing or paused state, ignoring the current one.
     *
     * <p>System controls ask for a state, not for the opposite of whatever they
     * believe is happening. Toggling on their behalf meant a lock-screen "play"
     * could leave the player paused, because the two could disagree.
     */
    void setPlaying(boolean playing) {
        if (playing) {
            // Nothing to resume when the engine was never started for this track.
            if (state == PlaybackState.PLAYING || state == PlaybackState.PAUSED) {
                controller.resume();
            }
            return;
        }
        if (state == PlaybackState.PLAYING) controller.pause();
    }

    /** Skips to the next queued item, or stops when there is none. */
    void next() {
        MediaTrack target = queue.peekNext();
        if (target == null) {
            // The button stays enabled with an empty queue when autoplay is on,
            // because it then routes to a recommendation instead of stopping
            // (§13). With autoplay off there is nothing Next can honestly do.
            if (queue.isAutoplayEnabled() && attemptAutoplay(queue.current())) return;
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
        // The session's own position, not the engine's: right after a track change
        // the engine still reports the previous stream, which would make Previous
        // think the new track is already past the restart threshold.
        MediaTrack target = queue.peekPrevious(positionMs(), PREVIOUS_RESTART_MS);
        if (target == null) {
            // Nothing before this track: the only honest action is to start it over.
            restartActiveTrack("no_previous");
            return;
        }
        if (target == queue.current()) {
            // Past the restart threshold: Previous restarts the current track
            // instead of skipping back, so the cursor must not move.
            restartActiveTrack("restart");
            return;
        }
        PlaybackTrace.markCurrent(PlaybackTrace.QUEUE_PREVIOUS, "to=" + target.key());
        queue.retreat();
        startCurrent();
    }

    /**
     * Replays the active track from the beginning, without moving the cursor.
     *
     * <p>The position is dropped for the duration of the replay: the engine is
     * about to be told to start the same source again, and until it does, its
     * clock still describes the playback that just ended.
     */
    private void restartActiveTrack(String reason) {
        PlaybackTrace.markCurrent(PlaybackTrace.QUEUE_PREVIOUS, reason);
        long generation = clock.generation();
        clock.detachEngine();
        controller.restartCurrent(() -> {
            if (clock.adoptEngine(generation)) notifyChanged();
        });
        // The restart opened a new attempt synchronously, so read it back rather
        // than assuming it is the previous one plus one.
        clock.expectEofFrom(controller.attempt());
        notifyChanged();
    }

    void stop() {
        traceEvent("STOP");
        controller.stop();
        state = PlaybackState.STOPPED;
        error = null;
        // No engine reading may be reported after a stop.
        clock.release();
        endTrace();
        notifyChanged();
    }

    /** Retries the failed step, subject to the bounded policy (§20). */
    void retry() {
        traceEvent("RETRY");
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
        if (track != null) return track.title;
        // After a restart the live track is gone but the like may carry a label,
        // which beats showing the user a bare provider id.
        String remembered = library.labelFor(key);
        return remembered == null || remembered.isEmpty()
                ? key.providerTrackId() : remembered;
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

    /**
     * A track for a liked identity, reconstructed from its remembered label.
     *
     * <p>L1K3D is only membership, so after a restart there is no live track to
     * find; without this the rows would read "Resolving &lt;provider/id&gt;".
     */
    MediaTrack describeLiked(TrackKey key) {
        MediaTrack known = findKnown(key);
        if (known != null) return known;
        if (key == null) return null;
        // providerId must stay the real identity: it is what the download registry
        // and the queue key on. Only the display fields are synthesised.
        return new MediaTrack(key.provider(), key.providerTrackId(), trackTitle(key),
                "", "", "", "", -1, "", "", "{}");
    }

    // ------------------------------------------------------------- download

    TrackDownloader downloader() { return downloader; }

    // ------------------------------------------------------- playback flow

    /** Resolves the current queue item and hands it to the decoder. */
    private void startCurrent() {
        MediaTrack track = queue.current();
        if (track == null) return;
        // Every track change opens a new generation. Callbacks and engine readings
        // that belong to an older one are refused, which is what stops the previous
        // track's position, artwork or error from landing on the new track.
        long generation = clock.beginTrack();
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
            // A newer track was selected while this one was still resolving.
            if (!clock.isCurrent(generation)) return;
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
            traceEvent("SOURCE_START");
            controller.play(source, active, () -> {
                // Output has begun for this exact track: only now may the engine's
                // position be reported as this track's position.
                if (!clock.adoptEngine(generation)) return;
                if (state == PlaybackState.BUFFERING || state == PlaybackState.RESOLVING_SOURCE) {
                    state = PlaybackState.PLAYING;
                }
                traceEvent("PLAYING");
                notifyChanged();
            });
            // Read the attempt back AFTER play(): it opens a new attempt
            // synchronously, and arming the end-of-track with the previous id
            // would make every genuine end of track look stale and silently stop
            // the queue from advancing.
            clock.expectEofFrom(controller.attempt());
        });
    }

    /**
     * The decoder reached the end of the media.
     *
     * <p>Runs at most once per track (§16): a second EOF for the same track is
     * ignored so a duplicated signal cannot start two new tracks.
     */
    private void onTrackFinished(long attempt) {
        // §14: an end-of-track from a superseded attempt must not move the queue.
        // Without this, a retry of A could be followed by A's older decoder
        // finishing, which advanced to B and made a retry look like a skip.
        if (!clock.acceptEof(attempt)) {
            PlaybackTrace.markCurrent(PlaybackTrace.STALE_EOF,
                    "attempt=" + attempt + " expected=" + clock.eofAttempt());
            Log.i(TAG, "event=STALE_EOF dropped attempt=" + attempt
                    + " expected=" + clock.eofAttempt());
            return;
        }
        if (eofHandled) return;
        eofHandled = true;
        traceEvent("EOF_ACCEPTED");

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
        if (!attemptAutoplay(finished)) {
            state = PlaybackState.STOPPED;
            endTrace();
            notifyChanged();
        }
    }

    /**
     * Starts one bounded autoplay fetch for {@code origin}.
     *
     * <p>Single-flight and budget-limited, so neither a repeated end-of-track
     * signal nor a user leaning on Next can spin the provider.
     *
     * @return {@code true} when the fetch was started.
     */
    private boolean attemptAutoplay(MediaTrack origin) {
        if (origin == null || autoplayInFlight) return false;
        if (!recovery.takeAutoplaySkip()) {
            Log.i(TAG, "event=AUTOPLAY_GAVE_UP reason=skip_budget");
            return false;
        }
        autoplayInFlight = true;
        PlaybackTrace.markCurrent(PlaybackTrace.AUTOPLAY_TRIGGER, "id=" + origin.key());
        recommendations.forTrack(origin, (candidates, failure) -> {
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
        return true;
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
            // While a new track is still resolving, the engine is still running
            // the attempt we replaced. Its failure must not be charged to the
            // track the user is waiting for.
            boolean newerAttemptInFlight = clock.eofAttempt() == 0L
                    && (state == PlaybackState.RESOLVING_SOURCE
                        || state == PlaybackState.RESOLVING_METADATA);
            if (newerAttemptInFlight) {
                Log.i(TAG, "event=STALE_ENGINE_ERROR dropped while resolving");
                return;
            }
            setError(PlaybackError.classify(PlaybackError.Stage.OUTPUT,
                    message == null ? "Playback failed" : message));
            traceEvent("ERROR");
            return;
        }
        if (state == PlaybackState.ERROR) {
            // An error is sticky until the user acts or a new attempt starts.
            // Without this, the engine's idle callback erased the message and
            // its Retry/Skip actions within a poll interval, leaving a silent
            // failure the user could see nothing about.
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

    /**
     * One line per meaningful playback transition.
     *
     * <p>Carries track, generation and attempt so a long session can be
     * reconstructed from logs alone: which song, which attempt, which callback
     * and which state. Low volume by construction (one line per transition, never
     * per tick), so it is safe to keep on.
     */
    private void traceEvent(String event) {
        MediaTrack track = queue.current();
        Log.i(TAG, "event=" + event
                + " track=" + (track == null ? "-" : track.key())
                + " gen=" + clock.generation()
                + " attempt=" + controller.attempt()
                + " state=" + state
                + " pos=" + positionMs());
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

    /**
     * Position of the <em>active</em> track, in milliseconds.
     *
     * <p>Deliberately not a raw engine reading. The engine keeps reporting the
     * previous stream's position until the new source has actually produced
     * audio, so reading it directly paired the old position with the new track's
     * duration. Until the current track owns the engine clock, the session
     * reports 0, which is the truth for a track that has not started producing
     * audio yet.
     */
    long positionMs() {
        if (queue.current() == null) return 0L;
        return clock.positionMs(controller.positionMs());
    }

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