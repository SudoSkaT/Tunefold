package com.tunefold.app;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

/**
 * The rules the new surfaces are built on, tested where they are pure.
 *
 * <p>Each of these decides something a person would describe as "obviously right", and
 * each of them is a line of code that can be wrong without anything looking wrong on
 * screen: which of the four states a section is in, whether a query is a stream or a
 * search, where a scroll may legally stop, and how much of a library the app is
 * willing to describe at once.
 */
public class NavigationRulesTest {

    // ------------------------------------------------------------ scroll host

    /** The offset cannot go above the content, ever, whatever asked for it. */
    @Test public void scrollStopsAtTheContent() {
        assertEquals(0, ScrollHost.clampScroll(-500, 1200));
        assertEquals(1200, ScrollHost.clampScroll(9999, 1200));
        assertEquals(600, ScrollHost.clampScroll(600, 1200));
    }

    /** Content that fits needs no scrolling at all, and a negative range is nothing. */
    @Test public void contentThatFitsCannotScroll() {
        assertEquals(0, ScrollHost.clampScroll(50, 0));
        assertEquals(0, ScrollHost.clampScroll(-50, -1));
    }

    // -------------------------------------------------------------- search

    /** A link is a link: the two hosts this app can actually resolve. */
    @Test public void linksAreRecognisedAsLinks() {
        assertTrue(SearchSurface.isUrl("https://www.youtube.com/watch?v=abc"));
        assertTrue(SearchSurface.isUrl("https://youtu.be/abc"));
        assertTrue(SearchSurface.isUrl("https://www.youtube.com/shorts/abc"));
        assertTrue(SearchSurface.isUrl("HTTPS://WWW.YOUTUBE.COM/WATCH?V=ABC"));
    }

    /**
     * A sentence that happens to contain the word is a sentence.
     *
     * <p>Searching for one is a normal thing to type, and treating it as a stream would
     * send it to a resolver that cannot play it.
     */
    @Test public void wordsAreNotLinks() {
        assertFalse(SearchSurface.isUrl("get lucky"));
        assertFalse(SearchSurface.isUrl("how to post on youtube.com/watch"));
        assertFalse(SearchSurface.isUrl(""));
        assertFalse(SearchSurface.isUrl(null));
    }

    // -------------------------------------------------------------- sections

    /** The four states, decided from what is known and not from what is convenient. */
    @Test public void aSectionKnowsWhichOfTheFourItIs() {
        assertEquals(SectionState.Phase.LOADING, SectionState.phaseOf(true, false, 0));
        assertEquals(SectionState.Phase.CONTENT, SectionState.phaseOf(false, false, 3));
        assertEquals(SectionState.Phase.EMPTY, SectionState.phaseOf(false, false, 0));
        assertEquals(SectionState.Phase.ERROR, SectionState.phaseOf(true, true, 3));
    }

    /**
     * A failure outranks everything.
     *
     * <p>Showing "Loading…" over a request that already failed is the kind of small
     * lie a state machine makes when its branches are in the wrong order.
     */
    @Test public void aFailureIsNeverHiddenByAnotherState() {
        assertEquals(SectionState.Phase.ERROR, SectionState.phaseOf(false, true, 0));
    }

    /** A loading section that already has rows is not loading any more. */
    @Test public void rowsEndTheLoadingState() {
        assertEquals(SectionState.Phase.CONTENT, SectionState.phaseOf(true, false, 1));
    }

    // -------------------------------------------------------------- library

    /**
     * The bound on a list that cannot be scrolled through.
     *
     * <p>A thousand tracks would be a thousand descriptions resolved over a network to
     * show the first eight. The bound is a promise about how much work a destination
     * may do, and it has to be a number someone can point at.
     */
    @Test public void aLibraryIsDescribedUpToItsBound() {
        assertEquals(10, LibrarySurface.limit(10));
        assertEquals(LibrarySurface.MAX_TRACKS, LibrarySurface.limit(100_000));
        assertEquals(0, LibrarySurface.limit(0));
    }

    // ----------------------------------------------------------------- rows

    /** A row's status is what the track IS, not a menu of what it could become. */
    @Test public void aTrackWithoutAnyStateSaysNothing() {
        assertEquals("", TrackRow.statusOf(DownloadState.Phase.IDLE, -1, false));
        assertEquals("LIKED", TrackRow.statusOf(DownloadState.Phase.IDLE, -1, true));
    }

    /** A running download shows its own word: it is the only place it is visible. */
    @Test public void aRunningDownloadSaysSo() {
        assertEquals("SAVING", TrackRow.statusOf(
                DownloadState.Phase.DOWNLOADING, -1, false));
        assertEquals("SAVING 42%", TrackRow.statusOf(
                DownloadState.Phase.DOWNLOADING, 42, false));
        assertEquals("PREPARING", TrackRow.statusOf(
                DownloadState.Phase.RESOLVING_SOURCE, -1, false));
    }

    /** What a download finished as is worth more than what it was liked for. */
    @Test public void aFinishedDownloadOutranksALike() {
        assertEquals("SAVED", TrackRow.statusOf(
                DownloadState.Phase.COMPLETED, -1, true));
        assertEquals("FAILED", TrackRow.statusOf(
                DownloadState.Phase.FAILED, -1, false));
    }
}