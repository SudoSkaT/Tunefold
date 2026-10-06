package com.tunefold.app;

import android.util.Log;

import java.util.ArrayList;
import java.util.HashSet;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Set;

/**
 * Recommendations for Home and autoplay (§15).
 *
 * <p>Deliberately simple and deterministic: the provider's own "related to
 * this track" list, filtered. No model, no scoring, no cloud.
 *
 * <p>Two guarantees matter more than variety:
 * <ul>
 *   <li><b>Never the same track.</b> A candidate is rejected when it is the
 *       current track, already queued, already liked or was just played.</li>
 *   <li><b>Never a loop.</b> {@link #recentlyOffered} remembers what this engine
 *       already handed out, so repeated EOFs walk away from a fixed point
 *       instead of oscillating between two tracks.</li>
 * </ul>
 *
 * <p>A recommendation failure is never a global error: it yields an empty list
 * so Home keeps working (§25).
 */
final class Recommendations {
    private static final String TAG = "TunefoldPerf";
    /** How many previously offered candidates to remember. */
    private static final int OFFER_MEMORY = 10;

    /** Fetches related tracks for a video id. Asynchronous by contract. */
    interface RelatedSource {
        void related(String providerTrackId, Callback callback);
    }

    interface Callback {
        void onRecommendations(List<MediaTrack> tracks, String error);
    }

    /** Where trace events go; a no-op seam so this stays unit-testable. */
    interface Telemetry { void mark(String event, String detail); }

    static final Telemetry NO_TELEMETRY = (event, detail) -> { };

    private final RelatedSource source;
    private final Library library;
    private final PlaybackQueue queue;
    private final Telemetry telemetry;
    private final Set<TrackKey> recentlyOffered = new LinkedHashSet<>();

    Recommendations(RelatedSource source, Library library, PlaybackQueue queue) {
        this(source, library, queue, NO_TELEMETRY);
    }

    Recommendations(RelatedSource source, Library library, PlaybackQueue queue,
                     Telemetry telemetry) {
        this.source = source;
        this.library = library;
        this.queue = queue;
        this.telemetry = telemetry == null ? NO_TELEMETRY : telemetry;
    }

    /**
     * Fetches and filters recommendations for the given track.
     *
     * <p>Always completes, with an empty list on any failure.
     */
    void forTrack(MediaTrack origin, Callback callback) {
        TrackKey originKey = TrackKey.of(origin);
        if (source == null || originKey == null) {
            deliver(callback, new ArrayList<>(), null);
            return;
        }
        telemetry.mark(PlaybackTrace.RECOMMENDATIONS_START, "id=" + originKey);
        try {
            source.related(originKey.providerTrackId(), (raw, error) -> {
                List<MediaTrack> filtered = filter(originKey, raw);
                telemetry.mark(PlaybackTrace.RECOMMENDATIONS_END,
                        "kept=" + filtered.size() + " raw=" + (raw == null ? 0 : raw.size())
                                + (error == null ? "" : " error=" + error));
                if (error != null) {
                    // A failed recommendation is not a playback error.
                    Log.i(TAG, "event=RECOMMENDATIONS_FAILED " + error);
                }
                deliver(callback, filtered, null);
            });
        } catch (Throwable failure) {
            telemetry.mark(PlaybackTrace.RECOMMENDATIONS_END, "error=unavailable");
            deliver(callback, new ArrayList<>(), null);
        }
    }

    /**
     * Applies every deduplication rule (§15, §16).
     *
     * <p>Package-private and pure so the rules are directly testable.
     */
    List<MediaTrack> filter(TrackKey originKey, List<MediaTrack> raw) {
        List<MediaTrack> result = new ArrayList<>();
        if (raw == null || raw.isEmpty()) return result;

        Set<TrackKey> blocked = new HashSet<>();
        if (originKey != null) blocked.add(originKey);
        if (library != null) blocked.addAll(library.knownIdentities());
        if (queue != null) blocked.addAll(queue.queuedKeys());
        synchronized (recentlyOffered) { blocked.addAll(recentlyOffered); }

        for (MediaTrack candidate : raw) {
            TrackKey key = TrackKey.of(candidate);
            if (key == null) continue;
            if (blocked.contains(key)) continue;
            blocked.add(key);
            result.add(candidate);
        }
        return result;
    }

    /**
     * Picks the first usable candidate and records it as offered.
     *
     * @return the chosen track, or {@code null} when nothing survived filtering.
     */
    MediaTrack select(List<MediaTrack> candidates) {
        if (candidates == null || candidates.isEmpty()) return null;
        MediaTrack chosen = candidates.get(0);
        TrackKey key = TrackKey.of(chosen);
        if (key == null) return null;
        synchronized (recentlyOffered) {
            recentlyOffered.add(key);
            while (recentlyOffered.size() > OFFER_MEMORY) {
                TrackKey oldest = recentlyOffered.iterator().next();
                recentlyOffered.remove(oldest);
            }
        }
        telemetry.mark(PlaybackTrace.RECOMMENDATION_SELECTED, "id=" + key);
        return chosen;
    }

    /** Forgets what was offered, e.g. when the user changes track deliberately. */
    void reset() {
        synchronized (recentlyOffered) { recentlyOffered.clear(); }
    }

    private static void deliver(Callback callback, List<MediaTrack> tracks, String error) {
        if (callback != null) callback.onRecommendations(tracks, error);
    }
}