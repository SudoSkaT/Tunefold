package com.tunefold.app;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

/**
 * The window rule: which rows exist while the user looks at eight of them.
 *
 * <p>This is the arithmetic behind the whole list, and it is arithmetic a device can
 * only confirm visually — which is exactly the kind of arithmetic that should not be
 * left to a device.
 */
public class TrackListWindowTest {

    private static final int ROW = 192;   // 64dp at density 3.0
    private static final int PREFETCH = 3;

    private static int[] window(int tracks, int scrollTop, int viewport) {
        return TrackList.windowFor(tracks, ROW, scrollTop, viewport, PREFETCH);
    }

    /** The first screen shows the first rows, plus what a fling would need. */
    @Test public void theTopOfTheListIsTheTopOfTheWindow() {
        int[] range = window(500, 0, 1000);
        assertEquals(0, range[0]);
        assertEquals("a screenful plus the prefetch on both sides", 9, range[1]);
    }

    /** The bound is the point: a library is not built to be scrolled. */
    @Test public void theWindowIsTheViewportAndNotTheLibrary() {
        int[] range = window(5000, 0, 1000);
        assertTrue("bound rows are the viewport plus prefetch, never the data: " + range[1],
                range[1] < 30);
    }

    /** Scrolling down moves the window down, keeping the prefetch above it. */
    @Test public void scrollingMovesTheWindowByExactlyWhatScrolled() {
        int[] range = window(500, 960, 1000);
        assertEquals("three rows of prefetch above the first visible one", 2, range[0]);
        // Six visible rows (1000px / 192), three above, three below, and the partial
        // row at the bottom edge is why the count is one higher than the sum.
        assertEquals(6 + 2 * PREFETCH, range[1]);
    }

    /**
     * The last screen cannot run past the end.
     *
     * <p>The offset is the one the host would actually produce: the content's height
     * minus the viewport, never its full height.
     */
    @Test public void theWindowStopsAtTheLastRow() {
        int maxScroll = 500 * ROW - 1000;
        int[] range = window(500, maxScroll, 1000);
        assertEquals("the window ends exactly at the data", 500, range[0] + range[1]);
        assertTrue("and it does not start past it", range[0] < 500);
        assertTrue("nor before the prefetch boundary", range[0] >= 500 - 12);
    }

    /** Scrolling past the end is the host's clamp, not this arithmetic's business. */
    @Test public void anEmptyListHasNoWindow() {
        int[] range = window(0, 0, 1000);
        assertEquals(0, range[0]);
        assertEquals(0, range[1]);
    }

    /** A list shorter than the screen shows all of it, once. */
    @Test public void aShortListIsNotPaddedWithNothing() {
        int[] range = window(3, 0, 1000);
        assertEquals(0, range[0]);
        assertEquals(3, range[1]);
    }

    /** A viewport smaller than a row still shows one: an empty list is not a result. */
    @Test public void aViewportSmallerThanARowStillShowsOne() {
        int[] range = TrackList.windowFor(10, ROW, 0, 10, PREFETCH);
        assertEquals(0, range[0]);
        assertTrue(range[1] >= 1);
    }

    /** A degenerate list is a degenerate result, not an exception on the UI thread. */
    @Test public void degenerateGeometryStaysInsideTheData() {
        assertEquals("a zero row height cannot index the data", 10,
                TrackList.windowFor(10, 0, 0, 1000, PREFETCH)[1]);
        assertEquals("no rows, no window", 0,
                TrackList.windowFor(-4, ROW, 0, 1000, PREFETCH)[1]);
        int[] backwards = TrackList.windowFor(10, ROW, -500, 1000, PREFETCH);
        assertTrue("a negative offset still starts at the first row", backwards[0] >= 0);
        assertTrue(backwards[0] + backwards[1] <= 10);
    }

    /** The same window twice is the same window, which is what keeps scroll free. */
    @Test public void theWindowIsStableForAStableViewport() {
        int[] first = window(500, 1920, 1000);
        int[] second = window(500, 1920, 1000);
        assertEquals(first[0], second[0]);
        assertEquals(first[1], second[1]);
    }

    /**
     * The same tracks twice are not a new list.
     *
     * <p>This is the rule that stops the screen redrawing itself twice a second. It
     * is compared by identity, because identity is the only thing a row renders: the
     * same song with a newly resolved title is the same row and is refreshed in place
     * by {@code refresh()}, not by being torn down and built again.
     */
    @Test public void theSameTracksAreNotANewList() {
        java.util.List<MediaTrack> first = tracks("a", "b", "c");
        java.util.List<MediaTrack> same = tracks("a", "b", "c");
        assertTrue(TrackList.sameTracks(first, same));
    }

    /** Different objects, same identities: still the same list. */
    @Test public void identityDecidesNotObjectIdentity() {
        java.util.List<MediaTrack> first = tracks("a", "b");
        java.util.List<MediaTrack> rebuilt = new java.util.ArrayList<>();
        for (MediaTrack track : tracks("a", "b")) {
            rebuilt.add(TestTracks.of(track.provider, track.providerId, "Another title", 1L));
        }
        assertTrue(TrackList.sameTracks(first, rebuilt));
    }

    @Test public void aDifferentSetIsANewList() {
        assertFalse(TrackList.sameTracks(tracks("a", "b", "c"), tracks("a", "c", "b")));
        assertFalse(TrackList.sameTracks(tracks("a", "b"), tracks("a", "b", "c")));
        assertFalse(TrackList.sameTracks(tracks("a", "b"), tracks("a", "z")));
        assertFalse(TrackList.sameTracks(null, tracks("a")));
    }

    /** Clearing a list and putting the same thing back is still the same list. */
    @Test public void emptyIsEmpty() {
        assertTrue(TrackList.sameTracks(null, null));
        assertTrue(TrackList.sameTracks(new java.util.ArrayList<>(), null));
        assertFalse(TrackList.sameTracks(new java.util.ArrayList<>(), tracks("a")));
    }

    private static java.util.List<MediaTrack> tracks(String... ids) {
        java.util.List<MediaTrack> list = new java.util.ArrayList<>();
        for (String id : ids) list.add(TestTracks.of("yt", id));
        return list;
    }
}