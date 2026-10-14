package com.tunefold.app;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNull;
import static org.junit.Assert.assertSame;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

import java.util.ArrayList;

/** Playback queue (§12, §13, §29). */
public class PlaybackQueueTest {

    private static final long RESTART_THRESHOLD_MS = 3_000L;

    @Test public void appendThenPeekNextWalksTheQueue() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.playNow(TestTracks.of("YouTube", "a"));
        queue.append(TestTracks.of("YouTube", "b"));
        queue.append(TestTracks.of("YouTube", "c"));

        assertEquals(3, queue.size());
        assertEquals("b", TrackKey.of(queue.peekNext()).providerTrackId());
        assertTrue(queue.advance());
        assertEquals("b", TrackKey.of(queue.current()).providerTrackId());
        assertEquals("c", TrackKey.of(queue.peekNext()).providerTrackId());
    }

    @Test public void emptyQueueHasNoNextAndNoPrevious() {
        PlaybackQueue queue = new PlaybackQueue();
        assertTrue(queue.isEmpty());
        assertNull(queue.peekNext());
        assertNull(queue.peekPrevious(0, RESTART_THRESHOLD_MS));
        assertFalse(queue.advance());
        assertFalse(queue.retreat());
        assertNull(queue.current());
    }

    @Test public void sameTrackIsNotQueuedTwice() {
        PlaybackQueue queue = new PlaybackQueue();
        assertEquals(0, queue.playNow(TestTracks.of("YouTube", "a")));
        assertEquals("appending the same identity is refused", -1, queue.append(TestTracks.of("YouTube", "a")));
        assertEquals(1, queue.size());
    }

    @Test public void trackWithoutIdentityIsRejected() {
        PlaybackQueue queue = new PlaybackQueue();
        assertEquals(-1, queue.append(TestTracks.of("YouTube", "")));
        queue.playNow(TestTracks.of("YouTube", ""));
        assertTrue("an unidentifiable track must not become current", queue.isEmpty());
    }

    @Test public void previousRestartsTheTrackAfterTheThreshold() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.playNow(TestTracks.of("YouTube", "a"));
        queue.append(TestTracks.of("YouTube", "b"));
        queue.advance();

        assertEquals("late in the track, Previous restarts it",
                "b", TrackKey.of(queue.peekPrevious(9_000, RESTART_THRESHOLD_MS)).providerTrackId());
        assertEquals("early in the track, Previous goes back",
                "a", TrackKey.of(queue.peekPrevious(500, RESTART_THRESHOLD_MS)).providerTrackId());
    }

    @Test public void latePreviousReturnsTheCurrentTrackSoTheCallerRestarts() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.playNow(TestTracks.of("YouTube", "a"));
        queue.append(TestTracks.of("YouTube", "b"));
        queue.advance();

        // Past the threshold the target IS the current track, so the caller must
        // restart without moving the cursor. Identity equality is the contract.
        assertSame("restart means the same item",
                queue.current(), queue.peekPrevious(9_000, RESTART_THRESHOLD_MS));
        assertEquals("b", TrackKey.of(queue.current()).providerTrackId());
    }

    @Test public void previousAtTheStartHasNoTargetWhenRepeatIsOff() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.playNow(TestTracks.of("YouTube", "a"));
        queue.append(TestTracks.of("YouTube", "b"));
        queue.retreat(); // back to "a", the first item
        assertNull(queue.peekPrevious(0, RESTART_THRESHOLD_MS));
    }

    @Test public void repeatAllWrapsBothWays() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.setRepeatMode(PlaybackQueue.RepeatMode.ALL);
        queue.playNow(TestTracks.of("YouTube", "a"));
        queue.append(TestTracks.of("YouTube", "b"));
        queue.advance();
        assertTrue(queue.advance());
        assertEquals("wraps to the first item", "a",
                TrackKey.of(queue.current()).providerTrackId());
        assertEquals("wraps to the last item", "b",
                TrackKey.of(queue.peekPrevious(0, RESTART_THRESHOLD_MS)).providerTrackId());
    }

    @Test public void repeatOneAlwaysReturnsTheSameTrack() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.setRepeatMode(PlaybackQueue.RepeatMode.ONE);
        queue.playNow(TestTracks.of("YouTube", "a"));
        queue.append(TestTracks.of("YouTube", "b"));
        assertEquals("a", TrackKey.of(queue.peekNext()).providerTrackId());
        queue.advance();
        assertEquals("a", TrackKey.of(queue.current()).providerTrackId());
    }

    @Test public void exhaustedQueueReturnsNoNextSoAutoplayCanDecide() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.playNow(TestTracks.of("YouTube", "a"));
        assertNull("the caller decides between autoplay and stop",
                queue.peekNext());
        assertFalse(queue.advance());
    }

    @Test public void removeKeepsCurrentPointingAtTheSameTrack() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.playNow(TestTracks.of("YouTube", "a"));
        queue.append(TestTracks.of("YouTube", "b"));
        queue.append(TestTracks.of("YouTube", "c"));
        queue.advance(); // current = b

        assertTrue(queue.remove(TrackKey.of("YouTube", "a")));
        assertEquals("current is still b", "b", TrackKey.of(queue.current()).providerTrackId());
        assertEquals(2, queue.size());
        assertTrue(queue.remove(TrackKey.of("YouTube", "b")));
        assertEquals("removing the current falls back to a neighbour",
                "c", TrackKey.of(queue.current()).providerTrackId());
    }

    @Test public void removeAllFiltersIdentitiesForRecommendations() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.playNow(TestTracks.of("YouTube", "a"));
        queue.append(TestTracks.of("YouTube", "b"));
        java.util.Set<TrackKey> unwanted = new java.util.LinkedHashSet<>();
        unwanted.add(TrackKey.of("YouTube", "b"));
        assertEquals(1, queue.removeAll(unwanted));
        assertEquals(1, queue.size());
    }

    @Test public void clearEmptiesEverything() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.playNow(TestTracks.of("YouTube", "a"));
        queue.append(TestTracks.of("YouTube", "b"));
        queue.clear();
        assertTrue(queue.isEmpty());
        assertEquals(-1, queue.currentIndex());
        assertNull(queue.current());
    }

    @Test public void setCurrentMovesToAnAlreadyQueuedTrack() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.playNow(TestTracks.of("YouTube", "a"));
        queue.append(TestTracks.of("YouTube", "b"));
        queue.setCurrent(TestTracks.of("YouTube", "a"));
        assertEquals("no duplicate was added", 2, queue.size());
        assertEquals("a", TrackKey.of(queue.current()).providerTrackId());
    }

    @Test public void autoplayToggleIsObservable() {
        PlaybackQueue queue = new PlaybackQueue();
        assertTrue(queue.isAutoplayEnabled());
        queue.setAutoplayEnabled(false);
        assertFalse(queue.isAutoplayEnabled());
        queue.setRepeatMode(null);
        assertEquals(PlaybackQueue.RepeatMode.OFF, queue.repeatMode());
    }

    @Test public void queuedKeysExposeEveryIdentity() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.playNow(TestTracks.of("YouTube", "a"));
        queue.append(TestTracks.of("YouTube", "b"));
        assertTrue(queue.contains(TrackKey.of("YouTube", "a")));
        assertTrue(queue.queuedKeys().contains(TrackKey.of("YouTube", "b")));
        assertFalse(queue.contains(TrackKey.of("YouTube", "zzz")));
    }

    // ------------------------------------------------------- playing from a list

    private static java.util.List<MediaTrack> tracks(String... ids) {
        java.util.List<MediaTrack> list = new java.util.ArrayList<>();
        for (String id : ids) list.add(TestTracks.of("YouTube", id));
        return list;
    }

    /**
     * The regression this class exists for: a tap on a row used to queue exactly one
     * track, so the song ended and there was nothing behind it.
     */
    @Test public void playingFromAListQueuesTheWholeList() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.replaceWith(tracks("a", "b", "c"), TrackKey.of("YouTube", "a"));

        assertEquals(3, queue.size());
        assertEquals("b", TrackKey.of(queue.peekNext()).providerTrackId());
        assertTrue(queue.advance());
        assertEquals("c", TrackKey.of(queue.peekNext()).providerTrackId());
        assertTrue(queue.advance());
        assertEquals("the end of the list is the end of the queue",
                null, queue.peekNext());
    }

    /** The tapped track is the one that starts, wherever it was in the list. */
    @Test public void theTappedTrackIsTheOneThatStarts() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.replaceWith(tracks("a", "b", "c"), TrackKey.of("YouTube", "c"));
        assertEquals("c", TrackKey.of(queue.current()).providerTrackId());
        assertEquals("and nothing follows it", null, queue.peekNext());
    }

    @Test public void theListOrderIsTheOrderThatIsKept() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.replaceWith(tracks("c", "a", "b"), TrackKey.of("YouTube", "c"));

        assertEquals("nothing is sorted or shuffled behind the user's back",
                "c", TrackKey.of(queue.current()).providerTrackId());
        queue.advance();
        assertEquals("a", TrackKey.of(queue.current()).providerTrackId());
    }

    /** A list replaces the queue rather than adding to whatever was playing. */
    @Test public void replacingLeavesNothingOfThePreviousQueue() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.playNow(TestTracks.of("YouTube", "old-1"));
        queue.append(TestTracks.of("YouTube", "old-2"));
        queue.replaceWith(tracks("a", "b"), TrackKey.of("YouTube", "b"));

        assertEquals(2, queue.size());
        assertEquals("b", TrackKey.of(queue.current()).providerTrackId());
        assertFalse("nothing from the previous queue survives",
                queue.contains(TrackKey.of("YouTube", "old-1")));
    }

    /** Two rows showing the same song must not make it play twice in a row. */
    @Test public void aRepeatedIdentityIsQueuedOnce() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.replaceWith(tracks("a", "b", "a", "c"), TrackKey.of("YouTube", "a"));
        assertEquals(3, queue.size());
        assertEquals("b", TrackKey.of(queue.peekNext()).providerTrackId());
    }

    /** A row with no identity could never be stepped to, so it is not queued. */
    @Test public void unidentifiableEntriesAreNotQueued() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.replaceWith(tracks("a", ""), TrackKey.of("YouTube", "a"));
        assertEquals(1, queue.size());
    }

    /** An unusable list must still leave something playing. */
    @Test public void anEmptyListLeavesNothingToPlay() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.playNow(TestTracks.of("YouTube", "a"));
        assertEquals("nothing was queued", -1,
                queue.replaceWith(new ArrayList<>(), TrackKey.of("YouTube", "a")));
        assertTrue(queue.isEmpty());
        assertNull(queue.current());
    }

    @Test public void aNullListIsTheSameAsAnEmptyOne() {
        PlaybackQueue queue = new PlaybackQueue();
        assertEquals(-1, queue.replaceWith(null, TrackKey.of("YouTube", "a")));
        assertTrue(queue.isEmpty());
    }

    /** A start that is not in the list falls back to the front, never to nothing. */
    @Test public void anUnknownStartFallsBackToTheFirstEntry() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.replaceWith(tracks("a", "b"), TrackKey.of("YouTube", "gone"));
        assertEquals(0, queue.currentIndex());
        assertEquals("a", TrackKey.of(queue.current()).providerTrackId());
    }

    @Test public void noStartMeansTheFrontOfTheList() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.replaceWith(tracks("a", "b"), null);
        assertEquals("a", TrackKey.of(queue.current()).providerTrackId());
    }

    /** The bound holds whatever the screen hands over. */
    @Test public void theQueueIsBounded() {
        PlaybackQueue queue = new PlaybackQueue();
        java.util.List<MediaTrack> huge = new ArrayList<>();
        for (int index = 0; index < PlaybackQueue.MAX_ENTRIES + 250; index++) {
            huge.add(TestTracks.of("YouTube", "id" + index));
        }
        queue.replaceWith(huge, TrackKey.of("YouTube", "id0"));
        assertEquals(PlaybackQueue.MAX_ENTRIES, queue.size());
    }

    /** A replaced queue can still be stepped through in both directions. */
    @Test public void previousWalksBackThroughTheList() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.replaceWith(tracks("a", "b", "c"), TrackKey.of("YouTube", "b"));
        queue.retreat();
        assertEquals("a", TrackKey.of(queue.current()).providerTrackId());
        assertEquals("and Next still walks forward through the list",
                "b", TrackKey.of(queue.peekNext()).providerTrackId());
    }
}