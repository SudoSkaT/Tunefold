package com.tunefold.app;

import android.util.Log;

import java.util.ArrayList;
import java.util.Collections;
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
        /**
         * The queue itself, not a copy of it.
         *
         * <p>A {@code List} copy of the queue was made on every snapshot, and a
         * snapshot is taken several times a second, so a queue of any size was copied
         * continuously for data nothing but the diagnostics panel ever read. Handing
         * out the live queue and reading its size or index instead makes a snapshot
         * allocation-free apart from the snapshot itself.
         */
        final PlaybackQueue queue;
        final int queueIndex;
        final boolean autoplayEnabled;
        final boolean canGoNext;
        final boolean canGoPrevious;
        /**
         * Position of {@link #currentTrack}, in milliseconds.
         *
         * <p>Carried inside the snapshot on purpose: reading the position separately
         * and pairing it with a track from another call is what let a new track be shown
         * with the previous track's elapsed time.
         */
        final long positionMs;

        Snapshot(PlaybackState state, MediaTrack currentTrack, TrackKey currentKey,
                 PlaybackError error, boolean liked, boolean downloaded,
                 DownloadState download, PlaybackQueue queue, int queueIndex,
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
    /** Metadata learned from the provider this process, most-recently-used first. */
    private final java.util.Map<TrackKey, MediaTrack> learned =
            new java.util.LinkedHashMap<>(64, 0.75f, true);
    /** How many learned tracks one process keeps; the data itself is the bound. */
    private static final int LEARNED_LIMIT = 200;
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
        // One engine read per snapshot: the previous code asked twice, once to decide
        // whether Previous can act and once to report, and the two could disagree if
        // the position advanced between them.
        long position = positionMs();
        boolean canNext;
        boolean canPrevious;
        synchronized (queue.lock()) {
            canNext = queue.peekNext() != null;
            canPrevious = queue.peekPrevious(0, PREVIOUS_RESTART_MS) != null
                    || (position > PREVIOUS_RESTART_MS && track != null);
        }
        return new Snapshot(state, track, key, error,
                library.isLiked(key), download.isDownloaded(), download,
                queue, queue.currentIndex(), queue.isAutoplayEnabled(),
                canNext, canPrevious, position);
    }

    /**
     * Fills {@code out} with the active track's identity, position and duration.
     *
     * <p>The cheap read, for the position poll. {@link #snapshot()} is the complete
     * answer and is correct, but it is more than a 500&nbsp;ms tick needs: the tick
     * only ever draws a time and a scrubber, and taking a full snapshot there made
     * every tick build a queue copy and a download state for data no one on that code
     * path could see.
     *
     * <p>One consistent read: identity, position and duration all come from the same
     * instant, so a tick can never pair one track with another track's time.
     *
     * @param out a caller-owned holder, reused across ticks
     * @return {@code out}
     */
    PlaybackPosition readPosition(PlaybackPosition out) {
        MediaTrack track = queue.current();
        out.set(track == null ? null : track.key(), positionMs(),
                track == null ? 0L : track.durationMs);
        return out;
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
        // One track is a list of one: playing something replaces the queue rather than
        // appending to whatever was there, so "continue listening" is what the user
        // last chose to play and not an accumulation of everything they ever tapped.
        queue.replaceWith(Collections.singletonList(track), track.key());
        startCurrent();
    }

    /**
     * Plays {@code selected} out of {@code tracks}, in {@code tracks}' order.
     *
     * <p>The list becomes the queue, so a song played from L1K3D or from
     * {@code Descargadas} is followed by the next one in that list when it ends
     * instead of stopping there. The order is the order the rows are shown in, which is
     * the only order the user can see, and it is preserved exactly: nothing is sorted
     * and nothing is shuffled behind their back.
     *
     * <p>Falls back to playing {@code selected} alone when the list is unusable, so a
     * tap always produces a track playing rather than a row that does nothing.
     */
    void playList(List<MediaTrack> tracks, MediaTrack selected) {
        if (selected == null) return;
        if (selected.key() == null) {
            setError(PlaybackError.classify(PlaybackError.Stage.SOURCE_RESOLUTION,
                    "Track has no identity"));
            return;
        }
        if (tracks == null || queue.replaceWith(tracks, selected.key()) < 0) play(selected);
        else startCurrent();
    }

    /** Appends to the queue without interrupting playback. */
    void enqueue(MediaTrack track) {
        if (track == null) return;
        if (queue.append(track) >= 0) notifyChanged();
    }

    /** Whether this identity is somewhere in the queue, playing or not. */
    boolean isQueued(TrackKey key) { return queue.contains(key); }

    /**
     * Takes one entry out of the queue.
     *
     * <p>"Continue listening" on the discovery screen is the queue shown as a list, so
     * a list the user cannot edit is a picture of a decision they cannot change. This
     * is that change: the row goes, and the cursor moves to whatever now sits where it
     * was rather than to nothing.
     *
     * @return {@code true} when something was removed
     */
    boolean removeFromQueue(TrackKey key) {
        if (key == null || !queue.remove(key)) return false;
        PlaybackTrace.markCurrent(PlaybackTrace.QUEUE_NEXT, "removed=" + key);
        notifyChanged();
        return true;
    }

    /**
     * Empties the queue.
     *
     * <p>What is playing stays in it. Clearing a queue is a statement about what comes
     * <em>next</em>, not about stopping: a user who empties the queue wants the song to
     * end and then stop, and taking the current track out of its own queue while its
     * audio is still running would blank the player mid-song and make Home believe
     * nothing is playing.
     *
     * @return {@code true} when there was something ahead to clear
     */
    boolean clearQueue() {
        MediaTrack current = queue.current();
        synchronized (queue.lock()) {
            if (queue.size() <= 1) return false;
        }
        queue.replaceWith(Collections.singletonList(current), TrackKey.of(current));
        PlaybackTrace.markCurrent(PlaybackTrace.QUEUE_NEXT, "cleared");
        notifyChanged();
        return true;
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
            queue.replaceWith(Collections.singletonList(track), track.key());
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

    /**
     * Seeks the active track to {@code positionMs}, when the pipeline can.
     *
     * <p>The request comes from the waveform, which is the one control that can
     * express a position precisely. Two rules make it safe:
     *
     * <ul>
     *   <li>it is refused, silently and without touching the position, unless the
     *       engine reports that it can seek — a scrubber that moves and snaps back
     *       is worse than a progress display;</li>
     *   <li>the accepted path opens a new {@link PlaybackClock} generation, exactly
     *       like a track change, so the old attempt's callbacks and its clock are
     *       refused afterwards. A seek that did not do this would let the previous
     *       decoder report the position of the new one.</li>
     * </ul>
     *
     * @return {@code true} when a seek was attempted on a pipeline that can do it
     */
    /**
     * Salta a una pista que ya está en la cola y la reproduce.
     *
     * <p>Es para lo que sirve una lista de "sigue": el cursor se mueve y la pista
     * elegida suena, en vez de quitar de en medio lo que ya se estaba oyendo.
     *
     * @return {@code true} si la identidad estaba en la cola
     */
    boolean playQueued(TrackKey key) {
        int index = queue.indexOfKey(key);
        if (index < 0) return false;
        MediaTrack target = queue.at(index);
        if (target == null) return false;
        PlaybackTrace.markCurrent(PlaybackTrace.QUEUE_NEXT, "jump=" + key);
        if (!queue.moveTo(index)) return false;
        startCurrent();
        return true;
    }

    /**
     * La letra sincronizada de lo que suena, una vez por pista y por proceso.
     *
     * <p>Es una ida y vuelta a la red, así que se recuerda lo que respondió: la misma
     * canción vuelve veinte segundos después y no se vuelve a preguntar. Una pista sin
     * letra se recuerda como "no hay", no como error: una respuesta vacía es un
     * resultado, y volver a insistir en ella cada vez que se repinta sería gastar
     * batería por un "no" que ya se sabe.
     */
    void lyrics(MediaTrack track, ProviderRegistry.Callback<LyricTrack> callback) {
        TrackKey key = track == null ? null : track.key();
        if (key == null) {
            if (callback != null) callback.complete(null, "The track has no identity");
            return;
        }
        boolean asked;
        synchronized (lyricCache) {
            asked = lyricCache.containsKey(key);
            if (!asked) lyricCache.put(key, null);
        }
        if (asked) {
            LyricTrack known;
            synchronized (lyricCache) { known = lyricCache.get(key); }
            if (callback != null) callback.complete(known, null);
            return;
        }
        providers.syncedLyrics(track, (found, error) -> {
            synchronized (lyricCache) {
                if (lyricCache.size() >= LYRIC_CACHE_LIMIT) {
                    java.util.Iterator<TrackKey> oldest = lyricCache.keySet().iterator();
                    if (oldest.hasNext()) oldest.next();
                    oldest.remove();
                }
                lyricCache.put(key, found);
            }
            if (callback != null) callback.complete(found, error);
        });
    }

    /** Letra por identidad ya preguntada; acotado, porque son unos pocos textos. */
    private final java.util.Map<TrackKey, LyricTrack> lyricCache =
            new java.util.LinkedHashMap<>();
    private static final int LYRIC_CACHE_LIMIT = 8;

    boolean seekTo(long positionMs) {
        MediaTrack track = queue.current();
        if (track == null || positionMs < 0L) return false;
        long duration = track.durationMs;
        if (duration > 0L && positionMs >= duration) {
            positionMs = Math.max(0L, duration - 1_000L);
        }
        // Asked BEFORE the generation opens: a new generation whose attempt never
        // starts would freeze this track's position and refuse its end-of-track.
        if (!controller.canSeek(positionMs)) {
            PlaybackTrace.markCurrent(PlaybackTrace.SEEK_REFUSED, "ms=" + positionMs);
            return false;
        }
        // The generation is what makes the old attempt's callbacks and its clock
        // stale: after this line, nothing belonging to the previous decoder can be
        // reported as the new one's position.
        clock.beginTrack();
        clock.detachEngine();
        boolean started = controller.seekTo(positionMs);
        if (started) {
            PlaybackTrace.markCurrent(PlaybackTrace.SEEK_APPLIED, "ms=" + positionMs);
            clock.expectEofFrom(controller.attempt());
            notifyChanged();
        }
        return started;
    }

    /** Whether the waveform may be used as a scrubber at all. */
    boolean supportsSeek() { return controller != null && controller.supportsSeek(); }

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

    /**
     * Downloads, described from what is remembered about each one.
     *
     * <p>Derived from the registry's identities rather than from
     * {@link LocalMediaStore#entries()}: the enumeration is disk I/O and this is read
     * on the main thread, twice a second, for every notification (§27). The registry
     * holds the same identities in memory and is seeded from that same store scan.
     *
     * <p>Describing them goes through {@link #describeRemembered(TrackKey)}, so a
     * download that is also liked is described once, from the freshest copy of its
     * metadata.
     */
    List<MediaTrack> downloadedTracks() {
        List<MediaTrack> result = new ArrayList<>();
        for (TrackKey key : downloadedKeys()) {
            MediaTrack track = describeRemembered(key);
            if (track != null) result.add(track);
        }
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

    /**
     * Changes whenever {@code Descargadas} gains or loses a track.
     *
     * <p>Exists so the view can distinguish "a row's progress moved" from "the list is
     * a different list". That distinction was missing, and it is why the view stayed
     * empty after a restart: it was rendered once, before the store scan had finished,
     * and the scan's own notification only re-bound rows — of which there were none. The
     * files were on disk the whole time.
     *
     * <p>Cheap on purpose: it is read on every session notification, including one per
     * download-progress tick.
     */
    int downloadsRevision() {
        return downloader == null ? 0 : downloader.registry().revision();
    }

    /**
     * How many times L1K3D has changed, for the same reason {@link #downloadsRevision()}
     * exists: a screen that re-derives its libraries on every notification needs to
     * know whether the answer would be different this time.
     *
     * <p>Read on every notification too, and it is a field load.
     */
    int libraryRevision() { return library == null ? 0 : library.revision(); }

    private String trackTitle(TrackKey key) {
        MediaTrack track = findKnown(key);
        if (track != null) return track.title;
        // After a restart the live track is gone, so the title has to come from
        // something that outlives it: the download's own sidecar first, because it is
        // the fresher of the two, then the like's label. Either beats showing the user
        // a bare provider id.
        String downloaded = downloader == null ? null : downloader.registry().titleOf(key);
        if (downloaded != null && !downloaded.isEmpty()) return downloaded;
        String remembered = library.labelFor(key);
        return remembered == null || remembered.isEmpty()
                ? key.providerTrackId() : remembered;
    }

    /**
     * What a remembered identity actually is, asked once per process.
     *
     * <p>Everything saved before the app kept its metadata has none of it: an identity,
     * a title and nothing else. The provider is the only thing that can turn an
     * identity back into a cover and a playable source, so the row that needs one asks
     * for it — and the answer is remembered everywhere it belongs (the library for a
     * member, the sidecar for a download), so this happens once for the life of the
     * data and not once per launch.
     *
     * @param key      an identity with no usable metadata
     * @param callback the real track, or {@code null} when the provider cannot say
     */
    void learnMetadata(TrackKey key, ProviderRegistry.Callback<MediaTrack> callback) {
        if (key == null) {
            if (callback != null) callback.complete(null, "Track has no identity");
            return;
        }
        // Already known — either live, or learned earlier in this process.
        MediaTrack known = findKnown(key);
        if (known != null && !known.thumbnail.isEmpty()) {
            if (callback != null) callback.complete(known, null);
            return;
        }
        synchronized (learned) {
            MediaTrack cached = learned.get(key);
            if (cached != null) {
                if (callback != null) callback.complete(cached, null);
                return;
            }
        }
        providers.trackMetadata(key, (track, error) -> {
            if (track != null) {
                remember(track);
                synchronized (learned) {
                    learned.put(key, track);
                    // Bounded: a library of five hundred identities is the data, and a
                    // process must not turn into a second copy of it.
                    if (learned.size() > LEARNED_LIMIT) {
                        java.util.Iterator<TrackKey> oldest = learned.keySet().iterator();
                        if (oldest.hasNext()) oldest.next();
                        oldest.remove();
                    }
                }
            } else {
                Log.i(TAG, "event=METADATA_UNAVAILABLE id=" + key);
            }
            if (callback != null) callback.complete(track, error);
        });
    }

    /** Where learned metadata is written, so the next launch does not ask again. */
    private void remember(MediaTrack track) {
        TrackKey key = track.key();
        if (library.learnMetadata(key, track)) return;
        if (downloader != null) downloader.learnMetadata(track);
    }

    /**
     * The cover URL remembered for an identity, or the empty string.
     *
     * <p>A remembered identity is all L1K3D and {@code Descargadas} hold after a
     * restart, and an identity has no artwork: without this every one of their rows
     * asked for nothing and stayed a grey square for ever, even though the cover URL
     * was sitting in the very sidecar written when the track was saved.
     *
     * <p>The download sidecar first, because it describes a file that is still on disk
     * and was written when the cover was known; the like's copy is a label remembered
     * for a membership.
     */
    private String thumbnailFor(TrackKey key) {
        String downloaded = downloader == null ? null : downloader.registry().thumbnailOf(key);
        if (downloaded != null && !downloaded.isEmpty()) return downloaded;
        String remembered = library.thumbnailFor(key);
        return remembered == null ? "" : remembered;
    }

    /**
     * The provider URL remembered for an identity, or the empty string.
     *
     * <p>What makes a member that nothing else knows about playable: source resolution
     * needs provider metadata, and the only copy of it that survives the process is the
     * one the library kept.
     */
    private String sourceFor(TrackKey key) {
        String remembered = library.sourceFor(key);
        return remembered == null ? "" : remembered;
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
     * A track for a remembered identity, reconstructed from what outlived it.
     *
     * <p>L1K3D is membership and {@code Descargadas} is a directory, so after a restart
     * there is no live track to find; without this the rows would read
     * "Resolving &lt;provider/id&gt;". Everything a row cannot derive from the key —
     * what to read, what to draw, what to play — is handed in from the library and the
     * download registry.
     *
     * @param key a liked or downloaded identity; {@code null} yields {@code null}
     */
    MediaTrack describeRemembered(TrackKey key) {
        MediaTrack known = findKnown(key);
        if (known != null) return known;
        if (key == null) return null;
        // providerId must stay the real identity: it is what the download registry
        // and the queue key on. Only the display fields are synthesised.
        return new MediaTrack(key.provider(), key.providerTrackId(), trackTitle(key),
                "", "", "", "", -1, thumbnailFor(key), sourceFor(key), "{}");
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
        // Iterated directly, not over a defensive copy. The observer set and this
        // method are both main-thread only — that is the invariant the whole session
        // already relies on — and copying the set on every notification allocated a
        // list several times a second purely to protect against a mutation that cannot
        // happen. An observer must not add or remove observers while being notified.
        for (Observer observer : observers) {
            observer.onSessionChanged();
        }
    }

    private static android.os.Handler main() {
        return MAIN;
    }

    private static final android.os.Handler MAIN =
            new android.os.Handler(android.os.Looper.getMainLooper());
}