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
 * this track" list, filtered — and, for {@link #forTrack}, optionally
 * reordered by the shared taste ranker ({@link #applyRank}, same scoring as
 * the desktop TUI) which fails open to the provider order on any problem.
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

    /**
     * Reorders candidates by personal taste, or returns {@code null} to leave
     * the provider order alone.
     *
     * <p>Production wires this to {@link TunefoldBridge#rankRelated} (same
     * scoring weights as the desktop TUI, Fase 5); tests inject a fake.
     */
    interface Ranker {
        String rank(List<MediaTrack> candidates);
    }

    static final Telemetry NO_TELEMETRY = (event, detail) -> { };

    private final RelatedSource source;
    private final Library library;
    private final PlaybackQueue queue;
    private final Telemetry telemetry;
    private final Ranker ranker;
    private final Set<TrackKey> recentlyOffered = new LinkedHashSet<>();

    Recommendations(RelatedSource source, Library library, PlaybackQueue queue) {
        this(source, library, queue, NO_TELEMETRY, null);
    }

    Recommendations(RelatedSource source, Library library, PlaybackQueue queue,
                     Telemetry telemetry) {
        this(source, library, queue, telemetry, null);
    }

    Recommendations(RelatedSource source, Library library, PlaybackQueue queue,
                     Telemetry telemetry, Ranker ranker) {
        this.source = source;
        this.library = library;
        this.queue = queue;
        this.telemetry = telemetry == null ? NO_TELEMETRY : telemetry;
        this.ranker = ranker;
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
                // Personalisation happens before the anti-bucle filter: the
                // ranker only reorders what the provider answered (indices
                // stay meaningful), then the filter decides what may be shown.
                List<MediaTrack> ordered = applyRank(raw);
                List<MediaTrack> filtered = filter(originKey, ordered);
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
     * Reorders with the shared ranker, failing open to the provider order.
     *
     * <p>Every failure mode keeps the list untouched: no ranker injected, an
     * empty list, a {@code null} response, an {@code ok:false} envelope, a
     * malformed or empty {@code order}, out-of-range indices, or any thrown
     * {@link Throwable} — including the {@link UnsatisfiedLinkError} a binary
     * built without {@code rankRelated} raises. Recommendation quality may
     * degrade to plain provider order; playback must never break.
     */
    List<MediaTrack> applyRank(List<MediaTrack> raw) {
        if (ranker == null || raw == null || raw.isEmpty()) {
            return raw;
        }
        try {
            int[] order = orderOf(ranker.rank(raw));
            if (order == null) {
                return raw;
            }
            // The order must be a permutation of the input: duplicates or a
            // hole would silently drop candidates, so anything short of that
            // fails open instead of risking a lost track.
            boolean[] seen = new boolean[raw.size()];
            boolean bijective = order.length == raw.size();
            for (int i = 0; bijective && i < order.length; i++) {
                int index = order[i];
                bijective = index >= 0 && index < raw.size() && !seen[index];
                if (bijective) {
                    seen[index] = true;
                }
            }
            if (!bijective) {
                return raw;
            }
            List<MediaTrack> ranked = new ArrayList<>(order.length);
            for (int index : order) {
                ranked.add(raw.get(index));
            }
            return ranked;
        } catch (Throwable failure) {
            Log.i(TAG, "event=RANK_UNAVAILABLE " + failure);
            return raw;
        }
    }

    /**
     * Extracts {@code order} from a {@code {"ok":true,"order":[...]}}
     * response without org.json (the unit-test android.jar stubs it out).
     *
     * @return one index per input candidate, or {@code null} for every
     *         failure mode so the caller keeps the provider order
     */
    static int[] orderOf(String response) {
        if (response == null || !response.contains("\"ok\":true")) {
            return null;
        }
        int key = response.indexOf("\"order\"");
        int open = key < 0 ? -1 : response.indexOf('[', key);
        int close = open < 0 ? -1 : response.indexOf(']', open);
        if (open < 0 || close <= open + 1) {
            return null;
        }
        String body = response.substring(open + 1, close);
        // Java's split would silently drop a trailing empty token, so a
        // dangling or doubled comma must be rejected before splitting.
        if (body.isEmpty() || body.startsWith(",") || body.endsWith(",")
                || body.contains(",,")) {
            return null;
        }
        String[] parts = body.split(",");
        int[] order = new int[parts.length];
        for (int i = 0; i < parts.length; i++) {
            try {
                String token = parts[i].trim();
                if (token.isEmpty()) {
                    return null;
                }
                order[i] = Integer.parseInt(token);
            } catch (NumberFormatException malformed) {
                return null;
            }
        }
        return order;
    }

    /**
     * Fetches discovery candidates for display, e.g. the player's shelf.
     *
     * <p>Deliberately <b>non-personalised</b>: unlike {@link #forTrack}, which
     * feeds autoplay and must never repeat anything the user owns or just
     * heard, this only removes what cannot be offered honestly — the track
     * itself, what is already queued, and duplicates. Liked and recently
     * played tracks stay visible: a shelf for discovery shows associated
     * tracks to pick from, not a second copy of the autoplay policy.
     *
     * <p>Always completes, with an empty list on any failure.
     */
    void forDisplay(MediaTrack origin, Callback callback) {
        TrackKey originKey = TrackKey.of(origin);
        if (source == null || originKey == null) {
            deliver(callback, new ArrayList<>(), null);
            return;
        }
        telemetry.mark(PlaybackTrace.RECOMMENDATIONS_START, "id=" + originKey + " mode=display");
        try {
            source.related(originKey.providerTrackId(), (raw, error) -> {
                List<MediaTrack> shown = filterForDisplay(originKey, raw);
                telemetry.mark(PlaybackTrace.RECOMMENDATIONS_END,
                        "kept=" + shown.size() + " raw=" + (raw == null ? 0 : raw.size())
                                + (error == null ? "" : " error=" + error));
                if (error != null) {
                    Log.i(TAG, "event=RECOMMENDATIONS_FAILED " + error);
                }
                deliver(callback, shown, null);
            });
        } catch (Throwable failure) {
            telemetry.mark(PlaybackTrace.RECOMMENDATIONS_END, "error=unavailable");
            deliver(callback, new ArrayList<>(), null);
        }
    }

    /**
     * Display filtering: current track, queued tracks and duplicates go.
     *
     * <p>Package-visible and pure so the discovery rule is directly testable,
     * next to {@link #filter}.
     */
    List<MediaTrack> filterForDisplay(TrackKey originKey, List<MediaTrack> raw) {
        List<MediaTrack> result = new ArrayList<>();
        if (raw == null || raw.isEmpty()) return result;
        Set<TrackKey> blocked = new HashSet<>();
        if (originKey != null) blocked.add(originKey);
        if (queue != null) blocked.addAll(queue.queuedKeys());
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