package com.tunefold.app;

import org.junit.Before;
import org.junit.Test;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.List;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertNull;
import static org.junit.Assert.assertTrue;

/** Recommendation filtering, deduplication and failure tolerance (§15, §16, §29). */
public class RecommendationsTest {

    private static MediaTrack track(String id) { return TestTracks.of("YouTube", id); }

    private static List<MediaTrack> list(MediaTrack... tracks) {
        return new ArrayList<>(Arrays.asList(tracks));
    }

    private Library library;
    private PlaybackQueue queue;
    private Recommendations recommendations;

    @Before public void setUp() {
        library = new Library(new java.io.File(System.getProperty("java.io.tmpdir"),
                "recs-test-" + System.nanoTime()));
        queue = new PlaybackQueue();
        recommendations = new Recommendations(null, library, queue);
    }

    @Test public void originIsNeverRecommended() {
        List<MediaTrack> kept = recommendations.filter(TrackKey.of("YouTube", "a"),
                list(track("a"), track("b")));
        assertEquals(1, kept.size());
        assertEquals("b", TrackKey.of(kept.get(0)).providerTrackId());
    }

    @Test public void duplicatesInOneResponseAreCollapsed() {
        List<MediaTrack> kept = recommendations.filter(TrackKey.of("YouTube", "a"),
                list(track("b"), track("b"), track("c")));
        assertEquals(2, kept.size());
        assertEquals("b", TrackKey.of(kept.get(0)).providerTrackId());
        assertEquals("c", TrackKey.of(kept.get(1)).providerTrackId());
    }

    @Test public void alreadyQueuedCandidatesAreRemoved() {
        queue.playNow(track("a"));
        queue.append(track("b"));
        List<MediaTrack> kept = recommendations.filter(TrackKey.of("YouTube", "a"),
                list(track("b"), track("c")));
        assertEquals(1, kept.size());
        assertEquals("c", TrackKey.of(kept.get(0)).providerTrackId());
    }

    @Test public void recentlyPlayedCandidatesAreRemoved() {
        library.markPlayed(TrackKey.of("YouTube", "b"));
        List<MediaTrack> kept = recommendations.filter(TrackKey.of("YouTube", "a"),
                list(track("b"), track("c")));
        assertEquals(1, kept.size());
        assertEquals("c", TrackKey.of(kept.get(0)).providerTrackId());
    }

    @Test public void likedCandidatesAreRemoved() {
        library.setLiked(track("b"), true);
        List<MediaTrack> kept = recommendations.filter(TrackKey.of("YouTube", "a"),
                list(track("b"), track("c")));
        assertEquals(1, kept.size());
        assertEquals("c", TrackKey.of(kept.get(0)).providerTrackId());
    }

    // ------------------------------------------------- display (discovery) filter

    /**
     * The shelf is for discovery, not for the autoplay policy: liked and
     * recently played tracks stay visible, only the current track, the queue
     * and duplicates go.
     */
    @Test public void displayKeepsLikedAndRecentButDropsCurrentQueuedAndDupes() {
        library.setLiked(track("b"), true);
        library.markPlayed(TrackKey.of("YouTube", "c"));
        queue.playNow(track("a"));
        queue.append(track("d"));
        List<MediaTrack> shown = recommendations.filterForDisplay(TrackKey.of("YouTube", "a"),
                list(track("a"), track("b"), track("c"), track("d"), track("d"), track("e")));
        assertEquals(3, shown.size());
        assertEquals("b", TrackKey.of(shown.get(0)).providerTrackId());
        assertEquals("c", TrackKey.of(shown.get(1)).providerTrackId());
        assertEquals("e", TrackKey.of(shown.get(2)).providerTrackId());
    }

    @Test public void displayDropsUnidentifiableCandidates() {
        List<MediaTrack> shown = recommendations.filterForDisplay(TrackKey.of("YouTube", "a"),
                list(track("b"), new MediaTrack("", "", "No id", "", "", "", "artist",
                        -1, "", "", "{}")));
        assertEquals(1, shown.size());
        assertEquals("b", TrackKey.of(shown.get(0)).providerTrackId());
    }

    @Test public void displayOnEmptyStaysEmpty() {
        assertTrue(recommendations.filterForDisplay(TrackKey.of("YouTube", "a"),
                new ArrayList<>()).isEmpty());
        assertTrue(recommendations.filterForDisplay(TrackKey.of("YouTube", "a"), null)
                .isEmpty());
    }

    @Test public void aProviderThatAlwaysAnswersTheSamePairCannotLoop() {
        // A provider that keeps answering with the same two tracks must not make
        // autoplay oscillate between them forever. Each pass offers something
        // new until the pair is exhausted, and then it stops.
        List<MediaTrack> providerSays = list(track("x"), track("y"));

        MediaTrack first = recommendations.select(
                recommendations.filter(TrackKey.of("YouTube", "a"), providerSays));
        assertNotNull(first);
        assertEquals("x", TrackKey.of(first).providerTrackId());

        MediaTrack second = recommendations.select(
                recommendations.filter(TrackKey.of("YouTube", "x"), providerSays));
        assertNotNull("the second pass walks away from the first choice",
                second);
        assertEquals("y", TrackKey.of(second).providerTrackId());

        assertNull("once every candidate has been offered, autoplay stops instead of looping",
                recommendations.select(
                        recommendations.filter(TrackKey.of("YouTube", "y"), providerSays)));
    }

    @Test public void resetAllowsTheSameCandidateAgain() {
        recommendations.select(list(track("x")));
        recommendations.reset();
        List<MediaTrack> after = recommendations.filter(TrackKey.of("YouTube", "a"),
                list(track("x")));
        assertEquals(1, after.size());
    }

    @Test public void selectOnEmptyReturnsNullSoAutoplayStops() {
        assertNull(recommendations.select(Collections.emptyList()));
        assertNull(recommendations.select(null));
    }

    @Test public void unidentifiableCandidatesAreDropped() {
        List<MediaTrack> kept = recommendations.filter(TrackKey.of("YouTube", "a"),
                list(track(""), track("b")));
        assertEquals(1, kept.size());
        assertEquals("b", TrackKey.of(kept.get(0)).providerTrackId());
    }

    @Test public void filterHandlesNullAndEmptyInput() {
        assertTrue(recommendations.filter(TrackKey.of("YouTube", "a"), null).isEmpty());
        assertTrue(recommendations.filter(TrackKey.of("YouTube", "a"),
                Collections.emptyList()).isEmpty());
    }

    /** §25: a failing recommendation must not become a global error. */
    @Test public void providerFailureYieldsAnEmptyListAndNoError() {
        Recommendations failing = new Recommendations((id, callback) -> {
            callback.onRecommendations(null, "provider exploded");
        }, library, queue);

        List<MediaTrack>[] captured = new List[1];
        failing.forTrack(track("a"), (tracks, error) -> {
            captured[0] = tracks;
            assertNull("recommendation failure is not surfaced as a user error", error);
        });
        assertNotNull(captured[0]);
        assertTrue(captured[0].isEmpty());
    }

    @Test public void providerExceptionIsContained() {
        Recommendations throwing = new Recommendations((id, callback) -> {
            throw new IllegalStateException("boom");
        }, library, queue);

        List<MediaTrack>[] captured = new List[1];
        throwing.forTrack(track("a"), (tracks, error) -> {
            captured[0] = tracks;
            assertNull(error);
        });
        assertNotNull(captured[0]);
        assertTrue(captured[0].isEmpty());
    }

    @Test public void missingSourceCompletesEmpty() {
        List<MediaTrack>[] captured = new List[1];
        recommendations.forTrack(track("a"), (tracks, error) -> {
            captured[0] = tracks;
            assertNull(error);
        });
        assertNotNull(captured[0]);
        assertTrue(captured[0].isEmpty());
    }

    @Test public void telemetrySeesRecommendationBoundaries() {
        List<String> events = new ArrayList<>();
        Recommendations traced = new Recommendations((id, callback) ->
                callback.onRecommendations(list(track("z")), null),
                library, queue, (event, detail) -> events.add(event));

        traced.forTrack(track("a"), (tracks, error) -> { });
        traced.select(list(track("z")));

        assertTrue(events.contains(PlaybackTrace.RECOMMENDATIONS_START));
        assertTrue(events.contains(PlaybackTrace.RECOMMENDATIONS_END));
        assertTrue(events.contains(PlaybackTrace.RECOMMENDATION_SELECTED));
    }

    @Test public void autoplayNeverDownloadsARecommendation() {
        // A recommendation is only queued and played; download is never implied.
        Recommendations source = new Recommendations((id, callback) ->
                callback.onRecommendations(list(track("z")), null), library, queue);
        List<MediaTrack>[] captured = new List[1];
        source.forTrack(track("a"), (tracks, error) -> captured[0] = tracks);

        MediaTrack chosen = recommendations.select(captured[0]);
        assertNotNull(chosen);
        assertFalse("the engine exposes no download side effect",
                source.toString().contains("startDownload"));
    }

    // ------------------------------------------- shared ranking (Fase 5)

    /** Builds a forTrack whose provider always answers [a, b]. */
    private List<MediaTrack> forTrackWithRanker(Recommendations.Ranker ranker) {
        Recommendations ranked = new Recommendations((id, callback) ->
                callback.onRecommendations(list(track("a"), track("b")), null),
                library, queue, Recommendations.NO_TELEMETRY, ranker);
        List<MediaTrack>[] captured = new List[1];
        ranked.forTrack(track("x"), (tracks, error) -> captured[0] = tracks);
        assertNotNull(captured[0]);
        return captured[0];
    }

    @Test public void rankerReordersProviderOrder() {
        List<MediaTrack> ranked = forTrackWithRanker(candidates -> "{\"ok\":true,\"order\":[1,0]}");
        assertEquals(2, ranked.size());
        assertEquals("b", TrackKey.of(ranked.get(0)).providerTrackId());
        assertEquals("a", TrackKey.of(ranked.get(1)).providerTrackId());
    }

    @Test public void noRankerKeepsProviderOrder() {
        List<MediaTrack> ranked = forTrackWithRanker(null);
        assertEquals("a", TrackKey.of(ranked.get(0)).providerTrackId());
        assertEquals("b", TrackKey.of(ranked.get(1)).providerTrackId());
    }

    /** Every failure mode falls open to the provider order (§25 in spirit). */
    @Test public void rankerFailuresKeepProviderOrder() {
        List<String> brokenResponses = Arrays.asList(
                null,
                "garbage",
                "{\"ok\":false,\"category\":\"invalid_request\",\"error\":\"x\"}",
                "{\"ok\":true}",
                "{\"ok\":true,\"order\":[]}",
                "{\"ok\":true,\"order\":[0]}",            // hole: not a permutation
                "{\"ok\":true,\"order\":[0,0]}",          // duplicate index
                "{\"ok\":true,\"order\":[0,99]}",         // out of range
                "{\"ok\":true,\"order\":[0,notanint]}");  // malformed body
        for (String response : brokenResponses) {
            List<MediaTrack> ranked = forTrackWithRanker(candidates -> response);
            assertEquals("response must not reorder: " + response, 2, ranked.size());
            assertEquals("a", TrackKey.of(ranked.get(0)).providerTrackId());
            assertEquals("b", TrackKey.of(ranked.get(1)).providerTrackId());
        }
    }

    @Test public void rankerThrowableKeepsProviderOrder() {
        // Both are unchecked, so the Ranker lambda may throw them without
        // declaring anything: a bug and a binary without rankRelated alike.
        Recommendations.Ranker[] broken = {
                candidates -> { throw new IllegalStateException("boom"); },
                candidates -> { throw new UnsatisfiedLinkError("rankRelated missing"); },
        };
        for (Recommendations.Ranker ranker : broken) {
            List<MediaTrack> ranked = forTrackWithRanker(ranker);
            assertEquals(2, ranked.size());
            assertEquals("a", TrackKey.of(ranked.get(0)).providerTrackId());
            assertEquals("b", TrackKey.of(ranked.get(1)).providerTrackId());
        }
    }

    @Test public void orderOfParsesOnlyWellFormedOkResponses() {
        int[] order = Recommendations.orderOf("{\"ok\":true,\"order\":[2,0,1]}");
        assertNotNull(order);
        assertEquals(3, order.length);
        assertEquals(2, order[0]);
        assertEquals(0, order[1]);
        assertEquals(1, order[2]);

        assertNull(Recommendations.orderOf(null));
        assertNull(Recommendations.orderOf("not json"));
        assertNull(Recommendations.orderOf("{\"ok\":false,\"order\":[1,0]}"));
        assertNull(Recommendations.orderOf("{\"ok\":true}"));
        assertNull(Recommendations.orderOf("{\"ok\":true,\"order\":[]}"));
        assertNull(Recommendations.orderOf("{\"ok\":true,\"order\":[0,]}}"));
    }
}