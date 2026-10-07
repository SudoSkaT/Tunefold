package com.tunefold.app;

import org.junit.Before;
import org.junit.Test;

import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.Arrays;
import java.util.Collections;
import java.util.List;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNull;
import static org.junit.Assert.assertTrue;

/** L1K3D membership, persistence and recents (§5, §22, §29). */
public class LibraryTest {

    private File directory;

    @Before public void setUp() throws Exception {
        directory = Files.createTempDirectory("tunefold-library-test").toFile();
    }

    private static MediaTrack track(String id) { return TestTracks.of("YouTube", id); }

    @Test public void likeAndUnlikeAreIdempotent() {
        Library library = new Library(directory);

        assertFalse(library.isLiked(track("a")));
        assertTrue(library.setLiked(track("a"), true));
        assertTrue(library.isLiked(track("a")));

        assertFalse(library.setLiked(track("a"), false));
        assertFalse(library.isLiked(track("a")));
    }

    @Test public void membershipDependsOnIdentityNotOnTheObject() {
        Library library = new Library(directory);
        library.setLiked(track("a"), true);

        // A different object carrying the same identity is still liked.
        MediaTrack sameIdentityDifferentMetadata =
                TestTracks.of("YouTube", "a", "Renamed", 1L);
        assertTrue(library.isLiked(sameIdentityDifferentMetadata));
    }

    @Test public void toggleFlipsMembership() {
        Library library = new Library(directory);
        assertTrue(library.toggleLiked(track("a")));
        assertTrue(library.isLiked(track("a")));
        assertFalse(library.toggleLiked(track("a")));
        assertFalse(library.isLiked(track("a")));
    }

    @Test public void unidentifiableTrackCannotBeLiked() {
        Library library = new Library(directory);
        assertFalse(library.setLiked(track(""), true));
        assertFalse(library.isLiked(track("")));
        assertEquals(0, library.likedCount());
    }

    @Test public void likedListingIsDeterministicAndPreservesInsertionOrder() {
        Library library = new Library(directory);
        library.setLiked(track("c"), true);
        library.setLiked(track("a"), true);
        library.setLiked(track("b"), true);

        List<TrackKey> keys = library.likedKeys();
        assertEquals(Arrays.asList(TrackKey.of("YouTube", "c"),
                TrackKey.of("YouTube", "a"), TrackKey.of("YouTube", "b")), keys);
        assertEquals("repeated reads are stable", keys, library.likedKeys());
    }

    @Test public void removingFromLikedDoesNotTouchTheFileSystem() {
        Library library = new Library(directory);
        library.setLiked(track("a"), true);
        library.setLiked(track("a"), false);
        assertTrue("L1K3D is membership, not media", library.isEmpty());
    }

    @Test public void membershipSurvivesProcessRestart() throws Exception {
        Library first = new Library(directory);
        first.setLiked(track("a"), true);
        first.setLiked(track("b"), true);
        first.markPlayed(TrackKey.of("YouTube", "z"));
        first.awaitWritesForTest();

        // A brand new instance reading the same directory is the restart case.
        Library restarted = new Library(directory);
        restarted.load();
        assertTrue(restarted.isLiked(track("a")));
        assertTrue(restarted.isLiked(track("b")));
        assertEquals(2, restarted.likedCount());
        assertTrue(restarted.wasPlayedRecently(TrackKey.of("YouTube", "z")));
    }

    @Test public void recentsAreBoundedAndMostRecentFirst() {
        Library library = new Library(directory);
        for (int index = 0; index < 40; index++) {
            library.markPlayed(TrackKey.of("YouTube", "id" + index));
        }
        List<TrackKey> recent = library.recentlyPlayed();
        assertTrue("recents do not grow without bound", recent.size() <= 20);
        assertEquals("the newest entry is first",
                TrackKey.of("YouTube", "id39"), recent.get(0));
        assertFalse("an evicted entry is forgotten",
                library.wasPlayedRecently(TrackKey.of("YouTube", "id0")));
    }

    @Test public void replayingMovesATrackToTheFrontWithoutDuplicating() {
        Library library = new Library(directory);
        library.markPlayed(TrackKey.of("YouTube", "a"));
        library.markPlayed(TrackKey.of("YouTube", "b"));
        library.markPlayed(TrackKey.of("YouTube", "a"));

        List<TrackKey> recent = library.recentlyPlayed();
        assertEquals(2, recent.size());
        assertEquals(TrackKey.of("YouTube", "a"), recent.get(0));
    }

    @Test public void knownIdentitiesCoversLikedAndRecents() {
        Library library = new Library(directory);
        library.setLiked(track("liked"), true);
        library.markPlayed(TrackKey.of("YouTube", "played"));

        assertTrue(library.knownIdentities().contains(TrackKey.of("YouTube", "liked")));
        assertTrue(library.knownIdentities().contains(TrackKey.of("YouTube", "played")));
        assertEquals(2, library.knownIdentities().size());
    }

    @Test public void corruptLibraryStartsEmptyRatherThanCrashing() throws Exception {
        File broken = new File(directory, "library.json");
        Files.write(broken.toPath(),
                "this is not the format\nl|missing-part\nx\n||\n".getBytes(StandardCharsets.UTF_8));

        Library library = new Library(directory);
        library.load();
        assertEquals("a corrupt file must not crash or fake membership", 0, library.likedCount());
    }

    @Test public void emptyDirectoryIsAValidEmptyLibrary() {
        Library library = new Library(new File(directory, "does-not-exist"));
        library.load();
        assertTrue(library.isEmpty());
        assertEquals(Collections.emptyList(), library.likedKeys());
    }

    @Test public void likedLabelsSurviveAReload() {
        Library writer = new Library(directory);
        writer.setLiked(track("a"), true);
        writer.awaitWritesForTest();

        Library reader = new Library(directory);
        reader.loadFromForTest(writer.serialize());
        assertEquals("the title comes back, not a bare provider id",
                "Track a", reader.labelFor(TrackKey.of("YouTube", "a")));
    }

    @Test public void escapedLabelComesBackIntact() {
        Library writer = new Library(directory);
        writer.toggleLiked(TestTracks.of("YouTube", "a", "Odd | Title\nSecond line", 1_000L));
        Library reader = new Library(directory);
        reader.loadFromForTest(writer.serialize());
        assertEquals("Odd | Title Second line",
                reader.labelFor(TrackKey.of("YouTube", "a")));
    }

    @Test public void aTitleContainingTheSeparatorStaysParseable() {
        // A title with the record separator and a newline in it.
        Library writer = new Library(directory);
        writer.toggleLiked(TestTracks.of("YouTube", "a", "Odd | Title\nSecond line", 1_000L));
        String document = writer.serialize();
        assertEquals("a line break in a title cannot forge a second record", 1,
                document.split("\n", -1).length - 1);

        Library reader = new Library(directory);
        reader.loadFromForTest(document);
        assertTrue("membership is intact", reader.isLiked(TestTracks.of("YouTube", "a")));
    }

    @Test public void unlikingForgetsTheLabel() {
        Library writer = new Library(directory);
        writer.toggleLiked(TestTracks.of("YouTube", "a"));
        writer.toggleLiked(TestTracks.of("YouTube", "a"));
        assertNull(writer.labelFor(TrackKey.of("YouTube", "a")));
    }

    @Test public void recordsWithoutALabelStillLoad() {
        Library reader = new Library(directory);
        reader.loadFromForTest("l|YouTube|legacy\n");
        assertTrue("older files keep working", reader.isLiked(TestTracks.of("YouTube", "legacy")));
        assertNull(reader.labelFor(TrackKey.of("YouTube", "legacy")));
    }

    @Test public void persistedDocumentRoundTrips() {
        Library writer = new Library(directory);
        writer.setLiked(track("a"), true);
        writer.markPlayed(TrackKey.of("YouTube", "b"));
        writer.awaitWritesForTest();

        Library reader = new Library(directory);
        reader.loadFromForTest(writer.serialize());
        assertTrue(reader.isLiked(track("a")));
        assertTrue(reader.wasPlayedRecently(TrackKey.of("YouTube", "b")));
        assertFalse("a played track is not automatically liked",
                reader.isLiked(track("b")));
    }
}