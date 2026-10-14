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
    void loadArtwork(MediaTrack track, int targetPx, Callback<android.graphics.Bitmap> callback) {
        ArtworkCache cache = artworkCache;
        if (cache == null || track.thumbnail.isEmpty()) {
            deliver(callback, null, "Artwork is unavailable");
            return;
        }
        String identity = track.key() == null ? track.thumbnail : track.key().toString();
        artworkWorker.execute(() -> {
            long started = android.os.SystemClock.elapsedRealtimeNanos();
            // A memory hit still repaints, and still owns the accent: a cover that is
            // already decoded is the common case after a rotation, and skipping it
            // would leave the surface showing the previous track's colour. It is
            // checked HERE, on the artwork worker, because deriving the palette is
            // bitmap work and must never run where a frame is being produced.
            // Who asked decides what the answer has to be good enough for: a
            // thumbnail cannot be stretched into the player's slot, so it is not a hit
            // for a player-sized request, and a row's request never owns the palette.
            boolean ownsPalette = ownsPalette(targetPx);
            android.graphics.Bitmap cached = cache.memoryHit(identity);
            if (cached != null && cached.getWidth() < minSide(targetPx)) cached = null;
            if (cached != null) {
                if (ownsPalette) applyDerivedAccent(cached);
                deliver(callback, cached, null);
                return;
            }
            java.io.File file = cache.getOrDownload(track.thumbnail);
            if (file != null) track.artworkCachePath = file.getAbsolutePath();
            int target = Math.max(DesignTokens.dp(DesignTokens.ARTWORK_THUMB),
                    Math.min(targetPx, DesignTokens.ARTWORK_DECODE_MAX_PX));
            android.graphics.Bitmap bitmap = file == null ? null : decodeArtwork(file, target);
            if (bitmap != null) cache.rememberDecoded(identity, bitmap);
            android.util.Log.i("TunefoldPerf", "event=artwork_cache_"
                    + (file == null ? "miss" : "ready") + " px=" + target
                    + " elapsed_ms="
                    + ((android.os.SystemClock.elapsedRealtimeNanos() - started) / 1_000_000L));
            if (bitmap != null && ownsPalette) applyDerivedAccent(bitmap);
            deliver(callback, bitmap, bitmap == null ? "Artwork unavailable" : null);
        });
    }

    /**
     * Derives the two bar colours this cover implies and applies them on the UI
     * thread.
     *
     * <p>The palette arithmetic runs on the artwork worker, against a 32×32
     * downsample: {@code Bitmap.createScaledBitmap} on a 640² cover costs a
     * millisecond or two of work that must never happen where a frame is being
     * produced (the caller is always the artwork worker, never the UI thread).
     * Only the finished palette crosses to the main thread, and
     * {@link ArtworkTheme#setPalette} is a no-op when nothing moved, so a cover
     * that yields the brand mint repaints nothing at all.
     */
    /**
     * Whether this request is for a cover big enough to BE the player's cover.
     *
     * <p>Asked of the request, not of the decoded bitmap: providers hand out covers
     * of whatever size they like, and a 320px thumbnail of a 320px source is a fine
     * cover for the player — refusing to take its colours because the file was small
     * is refusing the palette, not protecting it.
     */
    private static boolean ownsPalette(int targetPx) {
        return targetPx >= DesignTokens.dp(DesignTokens.ARTWORK_MIN);
    }

    private static int minSide(int targetPx) {
        return Math.max(DesignTokens.dp(DesignTokens.ARTWORK_THUMB),
                Math.min(targetPx, DesignTokens.ARTWORK_DECODE_MAX_PX));
    }

    private void applyDerivedAccent(android.graphics.Bitmap bitmap) {
        int side = DesignTokens.ARTWORK_PALETTE_SAMPLE_PX;
        ArtworkPalette.Palette palette;
        try {
            android.graphics.Bitmap sample =
                    android.graphics.Bitmap.createScaledBitmap(bitmap, side, side, true);
            int[] pixels = new int[side * side];
            sample.getPixels(pixels, 0, side, 0, 0, side, side);
            if (sample != bitmap) sample.recycle();
            palette = ArtworkPalette.derive(pixels);
        } catch (RuntimeException failure) {
            // A palette that cannot be read is not a reason to lose the cover.
            android.util.Log.d("TunefoldPerf",
                    "event=artwork_palette_failed " + failure.getClass().getSimpleName());
            return;
        }
        // Once per cover, and only when something actually moved: the palette is the
        // one thing here whose outcome cannot be read off the screen afterwards.
        android.util.Log.i("TunefoldPerf", "event=artwork_palette derived=" + palette.derived
                + " primary=" + String.format(java.util.Locale.ROOT, "#%08X", palette.primary)
                + " secondary=" + String.format(java.util.Locale.ROOT, "#%08X",
                        palette.secondary));
        main.post(() -> ArtworkTheme.setPalette(palette));
    }

    /** The bounded local-media store, owned by this registry. */
    LocalMediaStore localMediaStore() { return localMediaStore; }

    /**
     * Synchronised lyrics for a track, parsed once per track.
     *
     * <p>The provider worker, because it is a network round trip and the main thread
     * must not spend eight seconds' worth of waiting. Nothing is cached here: the
     * session owns one lyric per identity for the life of the process, and a second
     * cache would be a second copy of the same answer.
     */
    void syncedLyrics(MediaTrack track, Callback<LyricTrack> callback) {
        if (track == null || track.title == null || track.title.trim().isEmpty()) {
            deliver(callback, null, "Track has no title to look lyrics up by");
            return;
        }
        String who = !track.artist.isEmpty() ? track.artist : track.channel;
        worker.execute(() -> {
            long started = android.os.SystemClock.elapsedRealtimeNanos();
            try {
                String response = TunefoldBridge.syncedLyrics(track.title.trim(), who,
                        (int) Math.max(0L, track.durationMs));
                LyricTrack parsed = LyricTrack.parse(identityOf(track), response);
                if (parsed.isEmpty()) {
                    android.util.Log.i("TunefoldPerf", "event=LYRICS_NONE id=" + identityOf(track)
                            + " query=\"" + track.title.trim() + "\" / \"" + who + "\" / "
                            + track.durationMs + "ms bytes="
                            + (response == null ? -1 : response.length())
                            + " head=" + (response == null || response.isEmpty()
                                    ? "<vacio>" : response.substring(0, Math.min(90, response.length()))));
                    deliver(callback, null, "No synchronised lyrics");
                    return;
                }
                android.util.Log.i("TunefoldPerf", "event=LYRICS_READY id=" + identityOf(track)
                        + " lines=" + parsed.lines().size()
                        + " ms="
                        + ((android.os.SystemClock.elapsedRealtimeNanos() - started) / 1_000_000L));
                deliver(callback, parsed, null);
            } catch (Exception failure) {
                android.util.Log.i("TunefoldPerf", "event=LYRICS_FAILED id=" + identityOf(track)
                        + " error=" + failure.getClass().getSimpleName());
                deliver(callback, null, failure.toString());
            }
        });
    }

    /** Identity as the artwork and the logs spell it; the lyric is cached by it. */
    private static String identityOf(MediaTrack track) {
        TrackKey key = track.key();
        return key == null ? "" : key.toString();
    }

    /**
     * Runs disk work away from the main thread.
     *
     * <p>Rewriting a sidecar is I/O and the UI thread must not pay for it (§27). The
     * work goes on the provider worker rather than a second executor, because it is
     * the same kind of work: the file behind what the user is looking at.
     */
    void runOffUi(Runnable action) {
        if (action == null) return;
        worker.execute(action);
    }

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

    /**
     * Decodes a cover no larger than {@code targetPx} on its long side.
     *
     * <p>Two passes: the first reads only the bounds, so the second can pick an
     * {@code inSampleSize} from the real dimensions instead of allocating a full
     * size bitmap and throwing it away. The sample is a power of two, which is all
     * {@code inSampleSize} can express; the remaining scale happens once, in the
     * hardware pipeline, when the view draws it.
     */
    private static android.graphics.Bitmap decodeArtwork(java.io.File file, int targetPx) {
        android.graphics.BitmapFactory.Options bounds = new android.graphics.BitmapFactory.Options();
        bounds.inJustDecodeBounds = true;
        android.graphics.BitmapFactory.decodeFile(file.getAbsolutePath(), bounds);
        if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return null;
        int longest = Math.max(bounds.outWidth, bounds.outHeight);
        int sample = 1;
        while (longest / sample > targetPx) sample *= 2;
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
     *
     * <p>A track reconstructed from a remembered identity carries no provider
     * metadata, only the provider's own URL for it. Rather than refusing those —
     * which is what every member of L1K3D met on a cold start, and every download
     * whose file was removed — the URL is resolved back into full metadata first
     * and the source is resolved from that.
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
                String trackJson = withProviderMetadata(track);
                if (trackJson == null) {
                    deliver(callback, null, "No provider metadata to resolve this track");
                    return;
                }
                long start = android.os.SystemClock.elapsedRealtimeNanos();
                // Entry/exit markers: a resolution that never returns is otherwise
                // indistinguishable from one that was never requested.
                android.util.Log.i("TunefoldPerf", "event=RESOLVE_ENTER id="
                        + track.provider + "/" + track.providerId);
                JSONObject root = new JSONObject(TunefoldBridge.resolveYoutubeSource(trackJson));
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

    /**
     * Metadata for an identity, answered by the provider and nowhere else.
     *
     * <p>For a track remembered from a session that is gone: L1K3D holds identities and
     * {@code Descargadas} holds files, and neither holds a cover, a duration or
     * anything to resolve a source from. The provider is the only thing that knows
     * what its own ids mean.
     */
    void trackMetadata(TrackKey key, Callback<MediaTrack> callback) {
        if (key == null || key.providerTrackId().isEmpty()) {
            deliver(callback, null, "Track has no identity");
            return;
        }
        worker.execute(() -> {
            if (!available) { deliver(callback, null, "YouTube provider is not initialized"); return; }
            long started = android.os.SystemClock.elapsedRealtimeNanos();
            try {
                JSONObject root = new JSONObject(TunefoldBridge.metadataYoutube(key.providerTrackId()));
                JSONArray values = root.optJSONArray("tracks");
                JSONObject first = values == null ? null : values.optJSONObject(0);
                if (first == null || !root.optBoolean("ok")) {
                    android.util.Log.i("TunefoldPerf", "event=METADATA_MISS id=" + key);
                    deliver(callback, null, root.optString("error", "No metadata for this track"));
                    return;
                }
                MediaTrack resolved = MediaTrack.fromJson(first);
                android.util.Log.i("TunefoldPerf", "event=METADATA_READY id=" + key
                        + " ms="
                        + ((android.os.SystemClock.elapsedRealtimeNanos() - started) / 1_000_000L));
                deliver(callback, resolved, null);
            } catch (Exception failure) {
                android.util.Log.i("TunefoldPerf", "event=METADATA_FAILED id=" + key
                        + " error=" + failure.getClass().getSimpleName());
                deliver(callback, null, failure.toString());
            }
        });
    }

    /**
     * The provider metadata {@link #playableSource} needs, or {@code null}.
     *
     * <p>Provider metadata is the track's own JSON, and a track that came from a
     * search or a recommendation carries it. A track that came from a remembered
     * identity does not, so it is asked for again: by the URL that was remembered when
     * there is one, and by the identity itself when there is not — which is the case
     * for everything saved before either was kept.
     */
    private String withProviderMetadata(MediaTrack track) {
        if (track == null) return null;
        String json = track.rawJson;
        if (json != null && !json.isEmpty() && !json.equals("{}")) return json;
        String url = track.providerUrl;
        if (url != null && !url.trim().isEmpty()) {
            String response = TunefoldBridge.resolveYoutubeUrl(url.trim());
            String parsed = firstTrackJson(response);
            if (parsed != null) {
                android.util.Log.i("TunefoldPerf", "event=RESOLVE_FROM_URL id="
                        + track.provider + "/" + track.providerId);
                return parsed;
            }
        }
        String byId = firstTrackJson(TunefoldBridge.metadataYoutube(track.providerId));
        if (byId != null) {
            android.util.Log.i("TunefoldPerf", "event=RESOLVE_FROM_ID id="
                    + track.provider + "/" + track.providerId);
        }
        return byId;
    }

    /** The first track of a provider answer, or {@code null} if there is none. */
    private static String firstTrackJson(String response) {
        try {
            JSONArray values = new JSONObject(response).optJSONArray("tracks");
            JSONObject first = values == null ? null : values.optJSONObject(0);
            return first == null ? null : first.toString();
        } catch (Exception malformed) {
            android.util.Log.i("TunefoldPerf", "event=RESOLVE_PARSE_FAILED error="
                    + malformed.getClass().getSimpleName());
            return null;
        }
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
