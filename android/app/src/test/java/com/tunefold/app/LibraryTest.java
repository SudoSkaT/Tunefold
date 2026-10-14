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

    // ------------------------------------------------------- durability

    /**
     * A like must be on disk by the time the call returns.
     *
     * <p>This is the regression guard for a real loss of data: membership used to be
     * written on a background executor, so liking a track and immediately swiping the
     * app away discarded the write. {@code awaitWritesForTest()} would have hidden the
     * bug — it drains the queue — so the assertion deliberately does not call it.
     */
    @Test public void aLikeIsOnDiskBeforeTheCallReturns() {
        Library library = new Library(directory);
        assertTrue(library.toggleLiked(track("durable")));

        String onDisk = readLibraryFile();
        assertTrue("like was not durable: " + onDisk, onDisk.contains("l|YouTube|durable"));
    }

    @Test public void setLikedIsAlsoDurable() {
        Library library = new Library(directory);
        assertTrue(library.setLiked(track("durable-set"), true));
        assertTrue(readLibraryFile().contains("l|YouTube|durable-set"));
    }

    @Test public void unlikingIsAlsoDurable() {
        Library library = new Library(directory);
        library.setLiked(track("gone"), true);
        assertFalse(library.setLiked(track("gone"), false));
        assertFalse(readLibraryFile().contains("l|YouTube|gone|"));
    }

    /** Recents are bookkeeping, not user data: losing the last few costs nothing. */
    @Test public void recentsStillPersistWithoutBlocking() {
        Library library = new Library(directory);
        library.markPlayed(TrackKey.of("YouTube", "recent-1"));
        library.awaitWritesForTest();
        assertTrue(readLibraryFile().contains("r|YouTube|recent-1"));
    }

    /** A crash mid-write must never leave a truncated document behind. */
    @Test public void noPartialDocumentIsLeftBehind() throws Exception {
        Library library = new Library(directory);
        library.toggleLiked(track("a"));
        File[] leftovers = directory.listFiles(file -> file.getName().endsWith(".part"));
        assertEquals("a temporary file survived the write",
                0, leftovers == null ? 0 : leftovers.length);
    }

    /** The remembered label is what L1K3D renders after a restart. */
    @Test public void rememberedLabelSurvivesAReload() {
        Library writer = new Library(directory);
        writer.toggleLiked(TestTracks.of("YouTube", "labelled", "Nice Title", 1L));

        Library reader = new Library(directory);
        reader.load();
        assertEquals("Nice Title", reader.labelFor(TrackKey.of("YouTube", "labelled")));
    }

    // ------------------------------------------------------------ cover and source

    /** A track as the provider describes it: cover and source URL included. */
    private static MediaTrack described(String id) {
        return new MediaTrack("YouTube", id, "Nice Title", "Artist", "", "", "artist",
                213_000L, "https://img.example/" + id + ".jpg",
                "https://provider.example/watch?v=" + id, "{}");
    }

    /**
     * The cover a member was liked with has to outlive the session that fetched it.
     *
     * <p>This is the whole L1K3D-cover bug: membership is an identity, the queue holds
     * real tracks only for what is playing right now, and a row synthesised from the
     * identity alone asks for no cover and renders as a grey square for ever.
     */
    @Test public void aLikedCoverSurvivesAReload() {
        Library writer = new Library(directory);
        writer.toggleLiked(described("a"));

        Library reader = new Library(directory);
        reader.load();
        assertEquals("https://img.example/a.jpg",
                reader.thumbnailFor(TrackKey.of("YouTube", "a")));
    }

    /** The provider URL is what makes that same member playable again. */
    @Test public void aLikedSourceSurvivesAReload() {
        Library writer = new Library(directory);
        writer.toggleLiked(described("a"));

        Library reader = new Library(directory);
        reader.load();
        assertEquals("https://provider.example/watch?v=a",
                reader.sourceFor(TrackKey.of("YouTube", "a")));
    }

    @Test public void coverLabelAndSourceRoundTripTogether() {
        Library writer = new Library(directory);
        writer.toggleLiked(described("a"));
        writer.toggleLiked(described("b"));
        writer.awaitWritesForTest();

        Library reader = new Library(directory);
        reader.load();
        for (String id : new String[] {"a", "b"}) {
            TrackKey key = TrackKey.of("YouTube", id);
            assertEquals("https://img.example/" + id + ".jpg", reader.thumbnailFor(key));
            assertEquals("https://provider.example/watch?v=" + id, reader.sourceFor(key));
            assertEquals("Nice Title", reader.labelFor(key));
            assertTrue(reader.isLiked(TestTracks.of("YouTube", id)));
        }
    }

    /** Unliking forgets everything remembered about the identity. */
    @Test public void unlikingForgetsTheCoverAndTheSource() {
        Library library = new Library(directory);
        library.toggleLiked(described("a"));
        library.toggleLiked(described("a"));
        assertNull(library.thumbnailFor(TrackKey.of("YouTube", "a")));
        assertNull(library.sourceFor(TrackKey.of("YouTube", "a")));
        assertNull(library.labelFor(TrackKey.of("YouTube", "a")));
    }

    /** A blank cover is "unknown", not "an empty URL to fetch". */
    @Test public void aBlankCoverIsNotRemembered() {
        Library library = new Library(directory);
        library.toggleLiked(TestTracks.of("YouTube", "a"));
        assertNull(library.thumbnailFor(TrackKey.of("YouTube", "a")));
        assertNull(library.sourceFor(TrackKey.of("YouTube", "a")));
    }

    /**
     * Files written before covers and sources existed must keep loading.
     *
     * <p>They are whole-line records of their own, so nothing about the membership
     * record's own shape changed and a title containing the separator still reads back
     * as one label rather than as a label and a cover.
     */
    @Test public void legacyDocumentsStillLoadWithoutCovers() {
        Library reader = new Library(directory);
        reader.loadFromForTest("l|YouTube|legacy\n"
                + "l|YouTube|separated|Odd | Title\n"
                + "r|YouTube|played\n");
        assertTrue(reader.isLiked(TestTracks.of("YouTube", "legacy")));
        assertTrue(reader.isLiked(TestTracks.of("YouTube", "separated")));
        assertEquals("a separator in a title is still part of the title",
                "Odd | Title", reader.labelFor(TrackKey.of("YouTube", "separated")));
        assertTrue(reader.wasPlayedRecently(TrackKey.of("YouTube", "played")));
        assertNull("and there is simply no cover", reader.thumbnailFor(TrackKey.of("YouTube", "legacy")));
        assertNull(reader.sourceFor(TrackKey.of("YouTube", "legacy")));
    }

    /** A cover or source record for an identity that is no longer liked is ignored. */
    @Test public void orphanedCoverRecordsAreIgnored() {
        Library reader = new Library(directory);
        reader.loadFromForTest("t|YouTube|ghost|https://img.example/g.jpg\n"
                + "u|YouTube|ghost|https://provider.example/g\n"
                + "t|YouTube|noCover\n");
        assertTrue(reader.isEmpty());
        assertNull(reader.thumbnailFor(TrackKey.of("YouTube", "ghost")));
        assertNull(reader.sourceFor(TrackKey.of("YouTube", "ghost")));
        assertNull("an empty field is not a URL", reader.thumbnailFor(TrackKey.of("YouTube", "noCover")));
    }

    /** Nothing a cover URL contains may forge a second record. */
    @Test public void aCoverSurvivesAsOneRecord() {
        Library writer = new Library(directory);
        writer.toggleLiked(described("a"));
        writer.awaitWritesForTest();

        String document = readLibraryFile();
        assertEquals("three records: membership, cover and source",
                3, document.split("\n", -1).length - 1);

        Library reader = new Library(directory);
        reader.loadFromForTest(document);
        assertEquals("https://img.example/a.jpg",
                reader.thumbnailFor(TrackKey.of("YouTube", "a")));
        assertTrue("membership survived too", reader.isLiked(TestTracks.of("YouTube", "a")));
    }

    private String readLibraryFile() {
        File file = new File(directory, "library.json");
        if (!file.isFile()) return "";
        try {
            return new String(java.nio.file.Files.readAllBytes(file.toPath()),
                    StandardCharsets.UTF_8);
        } catch (Exception unreadable) {
            throw new AssertionError("library file unreadable", unreadable);
        }
    }
}
