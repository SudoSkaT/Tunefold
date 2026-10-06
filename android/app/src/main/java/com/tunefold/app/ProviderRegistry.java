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
    private final ExecutorService artworkWorker = Executors.newSingleThreadExecutor(r ->
            new Thread(r, "tunefold-artwork-cache"));
    private final Handler main = new Handler(Looper.getMainLooper());
    private volatile boolean available;
    private volatile ArtworkCache artworkCache;
    private volatile LocalMediaStore localMediaStore;

    void initialize(String cacheDir, Callback<Boolean> callback) {
        worker.execute(() -> {
            artworkCache = new ArtworkCache(new java.io.File(cacheDir, "artwork"));
            localMediaStore = new FileLocalMediaStore(new java.io.File(cacheDir, "local_media"));
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
        worker.execute(() -> {
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
                JSONObject root = new JSONObject(TunefoldBridge.resolveYoutubeSource(track.rawJson));
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
        artworkWorker.shutdownNow();
        available = false;
    }
}
