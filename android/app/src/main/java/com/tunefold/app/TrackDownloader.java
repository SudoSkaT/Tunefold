package com.tunefold.app;

import android.os.Handler;
import android.os.Looper;
import android.util.Log;

import java.io.File;
import java.util.HashMap;
import java.util.Map;

/**
 * Explicit, user-initiated track download.
 *
 * <p>This is a capability of its own, deliberately separated from playback:
 * nothing here runs during Play, metadata resolution, artwork fetch, preload or
 * buffering. A download starts only when the user asks for it.
 *
 * <p>It reuses the existing Range transport and Symphonia/AudioTrack pipeline —
 * there is no second player. Once the file is committed, the same
 * {@code PlayableSource} contract serves it as a {@code file:} source.
 *
 * <h3>Why downloads cannot be duplicated (§6)</h3>
 * All observable state lives in {@link DownloadRegistry}, keyed by
 * {@link TrackKey}. A track that appears in search results, L1K3D,
 * `Descargadas` and the queue resolves to one key, so:
 * <ul>
 *   <li>a request for an already-downloaded track is answered from state and
 *       starts nothing;</li>
 *   <li>a request while a download is running is refused, so there is never a
 *       second concurrent transfer for the same file;</li>
 *   <li>the file itself is named by the store's digest of the same identity,
 *       so even a hypothetical race would collide on one path rather than
 *       creating a second file.</li>
 * </ul>
 */
final class TrackDownloader implements AutoCloseable {
    private static final String TAG = "TunefoldPerf";
    /** How often the UI is told about progress. */
    private static final long POLL_MS = 200;
    /** Progress is only reported once this many bytes moved, to keep it cheap. */
    private static final long PROGRESS_STEP_BYTES = 64 * 1024;

    interface Observer { void onDownloadChanged(TrackKey key, DownloadState state); }

    /** One in-flight native download. */
    private static final class Job {
        final TrackKey key;
        final MediaTrack track;
        final long id;
        volatile long lastReported;
        final long startedAtMs = android.os.SystemClock.elapsedRealtime();

        Job(TrackKey key, MediaTrack track, long id) {
            this.key = key;
            this.track = track;
            this.id = id;
        }

        long speed(long received) {
            long elapsed = android.os.SystemClock.elapsedRealtime() - startedAtMs;
            if (elapsed <= 0) return -1;
            return received * 1000L / elapsed;
        }
    }

    private final ProviderRegistry providers;
    private final LocalMediaStore store;
    private final File directory;
    private final long engineHandle;
    private final RecoveryPolicy recovery = new RecoveryPolicy();
    private final Handler main = new Handler(Looper.getMainLooper());
    private final Map<TrackKey, Job> jobs = new HashMap<>();
    private final DownloadRegistry registry;
    private volatile Observer observer;
    private volatile boolean closed;

    TrackDownloader(ProviderRegistry providers, LocalMediaStore store, long engineHandle) {
        this.providers = providers;
        this.store = store;
        // The store owns the location; the downloader must not invent one.
        this.directory = store.directory();
        this.engineHandle = engineHandle;
        this.registry = new DownloadRegistry(key -> store.get(key.provider(), key.providerTrackId()) != null);
        this.registry.setListener((key, state) -> notifyChanged(key, state));
        // Abandoned `.part` files are cleaned at construction, never during Play.
        try {
            int removed = TunefoldBridge.cleanAbandonedDownloads(directory.getAbsolutePath());
            if (removed > 0) Log.i(TAG, "event=DOWNLOAD_CLEAN_PARTS removed=" + removed);
        } catch (Throwable failure) {
            Log.w(TAG, "could not clean abandoned downloads: " + failure);
        }
    }

    /** The dedup authority: other components read state from here, never guess. */
    DownloadRegistry registry() { return registry; }

    /** Re-points the observer when the Activity is recreated. */
    void setObserver(Observer value) { observer = value; }

    /** Current download state for a track; never {@code null}. */
    DownloadState stateOf(MediaTrack track) { return registry.stateOf(track); }

    DownloadState stateOf(TrackKey key) { return registry.stateOf(key); }

    /** True when a valid local file exists for this identity. */
    boolean isDownloaded(MediaTrack track) { return registry.isDownloaded(track); }

    /**
     * Seeds committed files discovered on disk.
     *
     * <p>This is what backs `Descargadas`: the view is the store's contents, not
     * a second copy of the library, so a deleted file disappears from it
     * automatically and a downloaded track shows up without ever being added.
     */
    void seedFromStore() {
        providers.localEntries((entries, error) -> {
            if (closed || entries == null) return;
            registry.seedStored(entries);
        });
    }

    /**
     * Starts the download of {@code track}, unless that would duplicate work.
     *
     * <p>Refusals are reported through the observer rather than silently ignored
     * (§26: every action gets a visible response).
     *
     * @return {@code true} when a new download actually started.
     */
    boolean start(MediaTrack track) {
        if (closed) return false;
        TrackKey key = TrackKey.of(track);
        if (key == null) {
            notifyChanged(null, registry.stateOf(key));
            return false;
        }

        // §6: consult the LocalMediaStore through the logical identity FIRST.
        if (registry.isDownloaded(key)) {
            // Size comes from the seeded scan, not from a disk read: this runs
            // on the thread that received the tap (§27).
            registry.alreadyDownloaded(key, registry.storedSize(key));
            Log.i(TAG, "event=DOWNLOAD_ALREADY_EXISTS id=" + key);
            return false;
        }
        if (registry.isActive(key)) {
            Log.i(TAG, "event=DOWNLOAD_ALREADY_RUNNING id=" + key);
            return false;
        }

        synchronized (jobs) {
            if (jobs.containsKey(key)) return false;
        }
        registry.beginResolving(key);
        PlaybackTrace.markCurrent(PlaybackTrace.DOWNLOAD_START, "id=" + key);
        providers.playableSource(track, null, (source, failure) -> {
            if (closed) return;
            if (failure != null || source == null) {
                registry.fail(key, failure == null ? "Source resolution failed" : failure);
                return;
            }
            if (!registry.canStart(key)) return;
            long id = TunefoldBridge.startDownload(engineHandle, directory.getAbsolutePath(),
                    key.provider(), key.providerTrackId(), source.url, source.headersJson);
            if (id <= 0) {
                registry.fail(key, "Download could not start");
                return;
            }
            Job job = new Job(key, track, id);
            synchronized (jobs) { jobs.put(key, job); }
            poll(job);
        });
        return true;
    }

    /** Cancels an in-flight download. The {@code .part} file is removed. */
    boolean cancel(MediaTrack track) { return cancel(TrackKey.of(track)); }

    boolean cancel(TrackKey key) {
        if (key == null) return false;
        Job job;
        synchronized (jobs) { job = jobs.remove(key); }
        if (job != null) TunefoldBridge.cancelDownload(engineHandle, job.id);
        DownloadState state = registry.cancel(key);
        PlaybackTrace.markCurrent(PlaybackTrace.DOWNLOAD_CANCELLED, "id=" + key);
        return state.phase == DownloadState.Phase.CANCELLED;
    }

    /** Retries a failed or cancelled download, subject to the retry budget. */
    boolean retry(MediaTrack track) {
        TrackKey key = TrackKey.of(track);
        if (key == null) return false;
        if (!recovery.takeDownloadRetry()) {
            // Budget exhausted: the failure becomes terminal rather than looping.
            registry.fail(key, "Too many download attempts");
            return false;
        }
        return start(track);
    }

    /** Removes a committed download. Playback falls back to streaming. */
    boolean remove(MediaTrack track) {
        TrackKey key = TrackKey.of(track);
        if (key == null) return false;
        cancel(key);
        if (!store.remove(key.provider(), key.providerTrackId())) {
            registry.fail(key, "Download could not be removed");
            return false;
        }
        // Forget the in-memory state so `Descargadas` drops the track at once.
        registry.forget(key);
        return true;
    }

    /**
     * Polls the native download and registers the committed file when it ends.
     */
    private void poll(Job job) {
        if (closed || job.key == null) return;
        String raw = TunefoldBridge.getDownloadProgress(engineHandle, job.id);
        if (raw == null) {
            synchronized (jobs) { jobs.remove(job.key); }
            finish(job);
            return;
        }
        String[] parts = raw.split("\t");
        if (parts.length >= 2) {
            try {
                long received = Long.parseLong(parts[0]);
                long total = Long.parseLong(parts[1]);
                if (received - job.lastReported >= PROGRESS_STEP_BYTES) {
                    job.lastReported = received;
                    registry.progress(job.key, received, total, job.speed(received));
                    PlaybackTrace.markCurrent(PlaybackTrace.DOWNLOAD_PROGRESS,
                            "id=" + job.key + " received=" + received + " total=" + total);
                }
            } catch (NumberFormatException malformed) {
                Log.w(TAG, "malformed download progress: " + raw);
            }
        }
        main.postDelayed(() -> poll(job), POLL_MS);
    }

    /**
     * The native job is gone, so it either committed or gave up.
     *
     * <p>Rust already renamed {@code .part} to the final file; the store is
     * still the authority on validity, so we ask it to register the file it
     * owns rather than trusting the download's own success. A track is only
     * reported as downloaded once that registration succeeded — never before the
     * atomic commit (§9).
     */
    private void finish(Job job) {
        recovery.onDownloadFinished();
        TrackKey key = job.key;
        String committed = TunefoldBridge.localMediaPath(directory.getAbsolutePath(),
                key.provider(), key.providerTrackId());
        boolean registered = committed != null
                && store.register(key.provider(), key.providerTrackId(), new File(committed));
        if (!registered) {
            String message = readError();
            if (message.isEmpty()) {
                // No file and no error: the user cancelled.
                registry.cancel(key);
                PlaybackTrace.markCurrent(PlaybackTrace.DOWNLOAD_CANCELLED, "id=" + key);
            } else {
                registry.fail(key, message);
                PlaybackTrace.markCurrent(PlaybackTrace.DOWNLOAD_FAILED,
                        "id=" + key + " error=" + message);
            }
            return;
        }
        File audio = store.get(key.provider(), key.providerTrackId());
        long size = audio == null ? 0 : audio.length();
        registry.complete(key, size);
        PlaybackTrace.markCurrent(PlaybackTrace.DOWNLOAD_COMPLETE, "id=" + key + " bytes=" + size);
        Log.i(TAG, "event=DOWNLOAD_COMPLETE id=" + key + " bytes=" + size);
    }

    private String readError() {
        try {
            String error = TunefoldBridge.getLastError(engineHandle);
            return error == null ? "" : error;
        } catch (Throwable unavailable) {
            return "";
        }
    }

    private void notifyChanged(TrackKey key, DownloadState state) {
        Observer current = observer;
        if (current == null) return;
        main.post(() -> {
            if (!closed && current != null) current.onDownloadChanged(key, state);
        });
    }

    /** A short summary of the recovery budget, for the diagnostics panel. */
    String recoverySummary() { return recovery.describe(); }

    @Override public void close() {
        closed = true;
        synchronized (jobs) {
            for (Job job : jobs.values()) {
                TunefoldBridge.cancelDownload(engineHandle, job.id);
            }
            jobs.clear();
        }
    }
}