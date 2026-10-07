package com.tunefold.app;

import android.os.Handler;
import android.os.Looper;

import org.json.JSONArray;
import org.json.JSONObject;

import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/** Async Android facade over Rust's provider-neutral CatalogRegistry and StreamResolver. */
final class ProviderRegistry implements AutoCloseable {
    interface Callback<T> { void complete(T value, String error); }

    private final ExecutorService worker = Executors.newSingleThreadExecutor(r ->
            new Thread(r, "tunefold-provider"));
    /**
     * Source resolution runs on its own thread, never on {@link #worker}.
     *
     * <p>A resolution that never returns would otherwise occupy the single
     * provider thread and take search, metadata and recommendations down with
     * it: cancelling the download changes the UI but cannot unblock a call that
     * is already stuck inside the resolver.
     */
    private final ExecutorService resolutionWorker = Executors.newSingleThreadExecutor(r ->
            new Thread(r, "tunefold-resolve"));
    private final ExecutorService artworkWorker = Executors.newSingleThreadExecutor(r ->
            new Thread(r, "tunefold-artwork-cache"));
    private final Handler main = new Handler(Looper.getMainLooper());
    private volatile boolean available;
    private volatile ArtworkCache artworkCache;
    private volatile LocalMediaStore localMediaStore;

    /**
     * @param cacheDir purgeable storage for caches (artwork, provider internals)
     * @param filesDir durable storage for what the user explicitly saved
     */
    void initialize(String cacheDir, String filesDir, Callback<Boolean> callback) {
        worker.execute(() -> {
            artworkCache = new ArtworkCache(new java.io.File(cacheDir, "artwork"));
            // Downloads are user data, not a cache: under cacheDir Android is free
            // to delete them when storage runs low, which would silently drop
            // tracks out of `Descargadas` and lose an explicit user action.
            localMediaStore = new FileLocalMediaStore(new java.io.File(filesDir, "local_media"));
            boolean ready = BuildConfig.YOUTUBE_ENABLED
                    && TunefoldBridge.initializeYoutube(cacheDir);
            available = ready;
            deliver(callback, ready, ready ? null : "YouTube provider is disabled or unavailable");
        });
    }

    /** Artwork is fetched and decoded independently from source resolution. */
    void loadArtwork(MediaTrack track, Callback<android.graphics.Bitmap> callback) {
        ArtworkCache cache = artworkCache;
        if (cache == null || track.thumbnail.isEmpty()) {
            deliver(callback, null, "Artwork is unavailable");
            return;
        }
        artworkWorker.execute(() -> {
            long started = android.os.SystemClock.elapsedRealtimeNanos();
            java.io.File file = cache.getOrDownload(track.thumbnail);
            if (file != null) track.artworkCachePath = file.getAbsolutePath();
            android.graphics.Bitmap bitmap = file == null ? null : decodeArtwork(file);
            android.util.Log.i("TunefoldPerf", "event=artwork_cache_"
                    + (file == null ? "miss" : "ready") + " elapsed_ms="
                    + ((android.os.SystemClock.elapsedRealtimeNanos() - started) / 1_000_000L));
            deliver(callback, bitmap, bitmap == null ? "Artwork unavailable" : null);
        });
    }

    /** The bounded local-media store, owned by this registry. */
    LocalMediaStore localMediaStore() { return localMediaStore; }

    /**
     * Tracks related to a video, for Home and autoplay (§15).
     *
     * <p>Uses the provider's own related list. Deduplication is NOT done here:
     * {@link Recommendations} owns that policy so it can be unit-tested without a
     * provider, and so there is exactly one set of rules.
     *
     * <p>Always completes; a failure yields an empty list, because a failed
     * recommendation must never become a global error (§25).
     */
    void related(String videoId, Callback<List<MediaTrack>> callback) {
        worker.execute(() -> {
            if (!available || videoId == null || videoId.isEmpty()) {
                deliver(callback, new ArrayList<>(), "Provider is not initialized");
                return;
            }
            long started = android.os.SystemClock.elapsedRealtimeNanos();
            String response = TunefoldBridge.relatedYoutube(videoId, 20);
            parseTracks(response, (tracks, error) -> {
                if (error != null) {
                    android.util.Log.i("TunefoldPerf", "event=RECOMMENDATIONS_FAILED " + error);
                    deliver(callback, new ArrayList<>(), null);
                    return;
                }
                android.util.Log.i("TunefoldPerf", "event=RECOMMENDATIONS_COMPLETE count="
                        + (tracks == null ? 0 : tracks.size()) + " elapsed_ms="
                        + ((android.os.SystemClock.elapsedRealtimeNanos() - started) / 1_000_000L));
                deliver(callback, tracks, null);
            });
        });
    }

    /**
     * The committed local audio file for a track, or {@code null}.
     *
     * <p>Reads off the UI thread: {@link LocalMediaStore} touches the filesystem
     * and must never be called from the UI thread (§27).
     */
    void localFile(MediaTrack track, Callback<java.io.File> callback) {
        worker.execute(() -> {
            LocalMediaStore store = localMediaStore;
            java.io.File file = (store == null || track == null)
                    ? null : store.get(track.provider, track.providerId);
            deliver(callback, file, null);
        });
    }

    /** Enumerates committed local media, backing the derived `Descargadas` view. */
    void localEntries(Callback<List<LocalMediaStore.Entry>> callback) {
        worker.execute(() -> {
            LocalMediaStore store = localMediaStore;
            List<LocalMediaStore.Entry> entries =
                    store == null ? java.util.Collections.emptyList() : store.entries();
            deliver(callback, entries, null);
        });
    }

    private static android.graphics.Bitmap decodeArtwork(java.io.File file) {
        android.graphics.BitmapFactory.Options bounds = new android.graphics.BitmapFactory.Options();
        bounds.inJustDecodeBounds = true;
        android.graphics.BitmapFactory.decodeFile(file.getAbsolutePath(), bounds);
        int sample = 1;
        while (bounds.outWidth / sample > 256 || bounds.outHeight / sample > 256) sample *= 2;
        android.graphics.BitmapFactory.Options options = new android.graphics.BitmapFactory.Options();
        options.inSampleSize = sample;
        return android.graphics.BitmapFactory.decodeFile(file.getAbsolutePath(), options);
    }

    void search(String query, Callback<List<MediaTrack>> callback) {
        worker.execute(() -> {
            if (!available) { deliver(callback, null, "YouTube provider is not initialized"); return; }
            long start = android.os.SystemClock.elapsedRealtimeNanos();
            String response = TunefoldBridge.searchYoutube(query, 20);
            android.util.Log.i("TunefoldPerf", "event=search_complete elapsed_ms="
                    + ((android.os.SystemClock.elapsedRealtimeNanos() - start) / 1_000_000L));
            parseTracks(response, callback);
        });
    }

    void resolve(String url, PlaybackTrace trace, Callback<MediaTrack> callback) {
        worker.execute(() -> {
            if (!available) { deliver(callback, null, "YouTube provider is not initialized"); return; }
            if (trace != null) trace.mark("METADATA_RESOLUTION_START");
            String response = TunefoldBridge.resolveYoutubeUrl(url);
            parseTracks(response, (tracks, error) -> {
                if (trace != null) trace.mark(PlaybackTrace.METADATA_AVAILABLE);
                deliver(callback,
                        error == null && tracks != null && !tracks.isEmpty() ? tracks.get(0) : null,
                        error != null ? error : tracks == null || tracks.isEmpty() ? "No track found" : null);
            });
        });
    }

    /**
     * Track → PlayableSource.
     *
     * <p>Strictly a resolution step: a hit in the LocalMediaStore short-circuits
     * it with a {@code file:} source, otherwise the provider resolves a
     * temporary URL. Resolution never downloads the whole track, and a resolved
     * URL is never stored as a permanent media reference.
     */
    void playableSource(MediaTrack track, PlaybackTrace trace, Callback<PlayableSource> callback) {
        resolutionWorker.execute(() -> {
            LocalMediaStore local = localMediaStore;
            java.io.File localFile = local == null ? null : local.get(track.provider, track.providerId);
            if (localFile != null) {
                if (trace != null) trace.mark(PlaybackTrace.SOURCE_RESOLUTION_START,
                        "hit=local_media bytes=" + localFile.length());
                if (trace != null) trace.mark(PlaybackTrace.PLAYABLE_SOURCE_AVAILABLE,
                        "kind=local_media");
                deliver(callback, new PlayableSource(localFile.toURI().toString(), "[]"), null);
                return;
            }
            if (trace != null) trace.mark(PlaybackTrace.SOURCE_RESOLUTION_START, "hit=none");
            if (!available) { deliver(callback, null, "YouTube provider is not initialized"); return; }
            try {
                long start = android.os.SystemClock.elapsedRealtimeNanos();
                // Entry/exit markers: a resolution that never returns is otherwise
                // indistinguishable from one that was never requested.
                android.util.Log.i("TunefoldPerf", "event=RESOLVE_ENTER id="
                        + track.provider + "/" + track.providerId);
                JSONObject root = new JSONObject(TunefoldBridge.resolveYoutubeSource(track.rawJson));
                android.util.Log.i("TunefoldPerf", "event=RESOLVE_EXIT id="
                        + track.provider + "/" + track.providerId + " ms="
                        + ((android.os.SystemClock.elapsedRealtimeNanos() - start) / 1_000_000L));
                if (!root.optBoolean("ok")) {
                    deliver(callback, null, root.optString("category", "source_resolution_failed")
                            + ": " + root.optString("error", "Source resolution failed"));
                    return;
                }
                JSONObject source = root.getJSONObject("source");
                long elapsedMs = (android.os.SystemClock.elapsedRealtimeNanos() - start) / 1_000_000L;
                if (trace != null) trace.mark(PlaybackTrace.PLAYABLE_SOURCE_AVAILABLE,
                        "source_cache=" + root.optString("source_cache", "unknown")
                                + " resolution_ms=" + elapsedMs);
                deliver(callback, new PlayableSource(source.getString("uri"),
                        source.optJSONArray("headers") == null ? "[]"
                                : source.getJSONArray("headers").toString()), null);
            } catch (Exception error) {
                deliver(callback, null, error.toString());
            }
        });
    }

    /** Explicit local save hook; playback itself never downloads a complete file. */
    void storeLocalMedia(MediaTrack track, java.io.InputStream audio, Callback<Boolean> callback) {
        worker.execute(() -> {
            LocalMediaStore local = localMediaStore;
            boolean stored = local != null && local.store(track.provider, track.providerId, audio);
            deliver(callback, stored, stored ? null : "Local media could not be stored");
        });
    }

    private void parseTracks(String response, Callback<List<MediaTrack>> callback) {
        try {
            JSONObject root = new JSONObject(response);
            if (!root.optBoolean("ok")) {
                deliver(callback, null, root.optString("category", "provider_error")
                        + ": " + root.optString("error", "Provider request failed"));
                return;
            }
            JSONArray values = root.optJSONArray("tracks");
            List<MediaTrack> result = new ArrayList<>();
            if (values != null) for (int index = 0; index < values.length(); index++) {
                JSONObject item = values.optJSONObject(index);
                if (item != null) result.add(MediaTrack.fromJson(item));
            }
            deliver(callback, result, null);
        } catch (Exception error) { deliver(callback, null, error.toString()); }
    }

    private <T> void deliver(Callback<T> callback, T value, String error) {
        main.post(() -> callback.complete(value, error));
    }

    @Override public void close() {
        worker.shutdownNow();
        resolutionWorker.shutdownNow();
        artworkWorker.shutdownNow();
        available = false;
    }
}
