package com.tunefold.app;

import android.os.Handler;
import android.os.Looper;
import android.util.Log;

import java.io.File;
import java.util.concurrent.ConcurrentHashMap;

/**
 * Explicit, user-initiated track download.
 *
 * <p>This is a capability of its own, deliberately separated from playback:
 * nothing here runs during Play, metadata resolution, artwork fetch, preload or
 * buffering. A download starts only when the user asks for it.
 *
 * <p>It reuses the existing Range transport and Symphonia/AudioTrack pipeline —
 * there is no second player. Once the file is committed, the very same
 * {@code PlayableSource} contract serves it as a {@code file:} source, which
 * {@link AndroidDecoder} already handles.
 *
 * <p>Ownership rules:
 * <ul>
 *   <li>identity is {@code provider + track id}, never the resolved URL;</li>
 *   <li>Rust writes to {@code .part}, validates the final size and commits
 *       atomically; Java then registers it in the existing bounded
 *       {@link LocalMediaStore};</li>
 *   <li>a temporary URL is never stored as the file's identity.</li>
 * </ul>
 */
final class TrackDownloader implements AutoCloseable {
    private static final String TAG = "TunefoldPerf";
    /** How often the UI is told about progress. */
    private static final long POLL_MS = 200;

    enum State { IDLE, RUNNING, COMPLETED, CANCELLED, FAILED }

    /** Immutable snapshot handed to the UI thread. */
    static final class Status {
        final State state;
        final long received;
        final long total;
        final String error;

        Status(State state, long received, long total, String error) {
            this.state = state;
            this.received = received;
            this.total = total;
            this.error = error;
        }

        /** 0..100 while the total size is known, otherwise -1. */
        int percent() {
            if (total <= 0) return -1;
            return (int) Math.min(100L, received * 100L / total);
        }
    }

    interface Observer { void onDownloadStatus(MediaTrack track, Status status); }

    private static final class Job {
        final MediaTrack track;
        final long id;
        volatile State state = State.RUNNING;
        volatile long received;
        volatile long total = -1;
        volatile String error = "";

        Job(MediaTrack track, long id) {
            this.track = track;
            this.id = id;
        }

        Status snapshot() {
            return new Status(state, received, total, error);
        }
    }

    private final ProviderRegistry providers;
    private final LocalMediaStore store;
    private final File directory;
    private final long engineHandle;
    private volatile Observer observer;
    private final Handler main = new Handler(Looper.getMainLooper());
    private final ConcurrentHashMap<String, Job> jobs = new ConcurrentHashMap<>();
    private volatile boolean closed;

    TrackDownloader(ProviderRegistry providers, LocalMediaStore store, long engineHandle,
                    Observer observer) {
        this.providers = providers;
        this.store = store;
        // The store owns the location; the downloader must not invent one.
        this.directory = store.directory();
        this.engineHandle = engineHandle;
        this.observer = observer;
        // Abandoned `.part` files are cleaned at construction, never during Play.
        try {
            int removed = TunefoldBridge.cleanAbandonedDownloads(directory.getAbsolutePath());
            if (removed > 0) Log.i(TAG, "event=DOWNLOAD_CLEAN_PARTS removed=" + removed);
        } catch (Throwable failure) {
            Log.w(TAG, "could not clean abandoned downloads: " + failure);
        }
    }

    /** Re-points the observer when the Activity is recreated. */
    void setObserver(Observer value) { observer = value; }

    private static String key(MediaTrack track) {
        return track.provider + "/" + track.providerId;
    }

    /**
     * Current status for a track.
     *
     * <p>The store is the authority: a track that is already on disk reports
     * COMPLETED even with no job in memory, so the UI offers "remove" after an
     * app restart instead of pretending it was never downloaded.
     */
    Status statusOf(MediaTrack track) {
        Job job = jobs.get(key(track));
        if (job != null && job.state != State.IDLE) return job.snapshot();
        long size = sizeOf(track);
        if (size > 0) return new Status(State.COMPLETED, size, size, "");
        return new Status(State.IDLE, 0, -1, "");
    }

    /** True when a valid local file already exists for this track. */
    boolean isDownloaded(MediaTrack track) {
        return store.get(track.provider, track.providerId) != null;
    }

    /**
     * Starts the download of {@code track}.
     *
     * <p>Resolves the playable source first (exactly as Play does) so the
     * signed URL is fresh, then streams it to a {@code .part} file. Never
     * called implicitly.
     */
    void start(MediaTrack track) {
        if (closed) return;
        String key = key(track);
        Job existing = jobs.get(key);
        if (existing != null && existing.state == State.RUNNING) return;
        // Already on disk: nothing to do, the UI must offer "remove".
        if (isDownloaded(track)) {
            publish(track, new Status(State.COMPLETED, sizeOf(track), sizeOf(track), ""));
            return;
        }
        providers.playableSource(track, null, (source, failure) -> {
            if (failure != null || source == null) {
                publish(track, new Status(State.FAILED, 0, -1,
                        failure == null ? "Source resolution failed" : failure));
                return;
            }
            if (closed) return;
            long id = TunefoldBridge.startDownload(engineHandle, directory.getAbsolutePath(),
                    track.provider, track.providerId, source.url, source.headersJson);
            if (id <= 0) {
                publish(track, new Status(State.FAILED, 0, -1, "Download could not start"));
                return;
            }
            Job job = new Job(track, id);
            jobs.put(key, job);
            publish(track, job.snapshot());
            poll(job);
        });
    }

    /** Cancels an in-flight download. The {@code .part} file is removed. */
    void cancel(MediaTrack track) {
        Job job = jobs.get(key(track));
        if (job == null || job.state != State.RUNNING) return;
        TunefoldBridge.cancelDownload(engineHandle, job.id);
        job.state = State.CANCELLED;
        publish(track, job.snapshot());
    }

    /** Removes a committed download. Playback falls back to streaming. */
    void remove(MediaTrack track) {
        cancel(track);
        if (!store.remove(track.provider, track.providerId)) {
            publish(track, new Status(State.FAILED, 0, -1, "Download could not be removed"));
            return;
        }
        jobs.remove(key(track));
        publish(track, new Status(State.IDLE, 0, -1, ""));
    }

    /**
     * Polls the native download while it runs and registers the committed file
     * in the bounded {@link LocalMediaStore} when it finishes.
     */
    private void poll(Job job) {
        if (closed || job.state != State.RUNNING) return;
        String raw = TunefoldBridge.getDownloadProgress(engineHandle, job.id);
        if (raw == null) {
            // The native job is gone: it either committed or failed. The store
            // is the authority on whether a valid file exists.
            finish(job);
            return;
        }
        String[] parts = raw.split("\t");
        if (parts.length >= 2) {
            try {
                job.received = Long.parseLong(parts[0]);
                job.total = Long.parseLong(parts[1]);
            } catch (NumberFormatException malformed) {
                Log.w(TAG, "malformed download progress: " + raw);
            }
        }
        publish(job.track, job.snapshot());
        main.postDelayed(() -> poll(job), POLL_MS);
    }

    /**
     * The native job is gone, so it either committed or gave up.
     *
     * <p>Rust already renamed {@code .part} to the final file; the store is
     * still the authority on validity, so we ask it to register the file it
     * owns rather than trusting the download's own success.
     */
    private void finish(Job job) {
        String committed = TunefoldBridge.localMediaPath(directory.getAbsolutePath(),
                job.track.provider, job.track.providerId);
        boolean registered = committed != null
                && store.register(job.track.provider, job.track.providerId, new File(committed));
        if (!registered) {
            // Nothing usable on disk: cancelled, interrupted or rejected.
            String message = readError();
            job.state = message.isEmpty() ? State.CANCELLED : State.FAILED;
            job.error = message;
            publish(job.track, job.snapshot());
            return;
        }
        File audio = store.get(job.track.provider, job.track.providerId);
        long size = audio == null ? 0 : audio.length();
        job.received = size;
        job.total = size;
        job.state = State.COMPLETED;
        job.error = "";
        Log.i(TAG, "event=DOWNLOAD_COMPLETE provider=" + job.track.provider
                + " id=" + job.track.providerId + " bytes=" + size);
        publish(job.track, job.snapshot());
    }

    private long sizeOf(MediaTrack track) {
        File audio = store.get(track.provider, track.providerId);
        return audio == null ? 0 : audio.length();
    }

    private String readError() {
        try {
            String error = TunefoldBridge.getLastError(engineHandle);
            return error == null ? "" : error;
        } catch (Throwable unavailable) {
            return "";
        }
    }

    private void publish(MediaTrack track, Status status) {
        if (observer == null) return;
        main.post(() -> {
            if (!closed) observer.onDownloadStatus(track, status);
        });
    }

    @Override public void close() {
        closed = true;
        for (Job job : jobs.values()) {
            if (job.state == State.RUNNING) TunefoldBridge.cancelDownload(engineHandle, job.id);
        }
        jobs.clear();
    }
}