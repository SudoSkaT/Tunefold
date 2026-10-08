package com.tunefold.app;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNull;
import static org.junit.Assert.assertTrue;

import org.junit.Before;
import org.junit.Test;

import java.io.ByteArrayInputStream;
import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.Collections;
import java.util.List;

/**
 * The store's sidecar is what a download remembers about itself (§5, §6).
 *
 * <p>It carried provider, id and size, but not the title, so the one thing that makes
 * {@code Descargadas} readable was lost the moment the process restarted: the row came
 * back as the bare provider id, which is all the identity survives. The title is written
 * on commit and read back on scan, and it is deleted with the file it describes.
 */
public class LocalMediaStoreTest {

    private File directory;
    private LocalMediaStore store;

    @Before public void setUp() throws Exception {
        directory = Files.createTempDirectory("tunefold-store-test").toFile();
        store = new FileLocalMediaStore(new File(directory, "local_media"));
    }

    private static TrackKey key(String id) { return TrackKey.of("YouTube", id); }

    private boolean putAudio(String id) {
        byte[] audio = new byte[4096];
        return store.store("YouTube", id,
                new ByteArrayInputStream(audio));
    }

    private LocalMediaStore.Entry onlyEntry() {
        List<LocalMediaStore.Entry> entries = store.entries();
        assertEquals("expected exactly one stored entry", 1, entries.size());
        return entries.get(0);
    }

    /** The commit path is what carries the title. */
    @Test public void aRegisteredTitleComesBackOnTheNextScan() {
        assertTrue(putAudio("dl"));
        File audio = onlyEntry().audio();

        assertTrue(store.register("YouTube", "dl", audio, "Never Gonna Give You Up"));

        // A new store instance is a restart: the file survived, so the title must too.
        FileLocalMediaStore reopened =
                new FileLocalMediaStore(new File(directory, "local_media"));
        List<LocalMediaStore.Entry> entries = reopened.entries();
        assertEquals(1, entries.size());
        assertEquals("Never Gonna Give You Up", entries.get(0).title());
    }

    /** A store fed only bytes has no title to remember, and must not invent one. */
    @Test public void anEntryWithoutATitleReportsNull() {
        assertTrue(putAudio("plain"));
        assertNull(onlyEntry().title());
    }

    /** A blank title is stored as no title, not as an empty row label. */
    @Test public void aBlankTitleIsNotWrittenToTheSidecar() {
        assertTrue(putAudio("blank"));
        File audio = onlyEntry().audio();

        assertTrue(store.register("YouTube", "blank", audio, "   "));

        assertNull(onlyEntry().title());
    }

    /** Titles are trimmed before they are written, so a stray newline cannot wrap a row. */
    @Test public void aTitleIsTrimmedOnTheWayIn() {
        assertTrue(putAudio("messy"));
        File audio = onlyEntry().audio();

        assertTrue(store.register("YouTube", "messy", audio, "  Nice Title\\n"));

        assertEquals("Nice Title", onlyEntry().title());
    }

    /** The title is metadata about the sidecar's own entry, never a new identity. */
    @Test public void aTitleCannotChangeTheIdentity() {
        assertTrue(putAudio("stable"));
        File audio = onlyEntry().audio();
        assertTrue(store.register("YouTube", "stable", audio, "Renamed | Track"));

        LocalMediaStore.Entry entry = onlyEntry();
        assertEquals(key("stable"), entry.key);
        assertEquals("Renamed | Track", entry.title());
    }

    /** Removing the file takes the title with it rather than leaving a dangling label. */
    @Test public void removingAFileRemovesItsTitle() {
        assertTrue(putAudio("temp"));
        File audio = onlyEntry().audio();
        assertTrue(store.register("YouTube", "temp", audio, "Temporary"));
        assertEquals("Temporary", onlyEntry().title());

        assertTrue(store.remove("YouTube", "temp"));

        assertEquals(Collections.emptyList(), store.entries());
    }

    /** An entry whose audio vanished is pruned, title or no title. */
    @Test public void anOrphanedEntryIsPrunedOnScan() throws Exception {
        assertTrue(putAudio("orphan"));
        File audio = onlyEntry().audio();
        assertTrue(store.register("YouTube", "orphan", audio, "Orphan"));
        assertTrue(audio.delete());

        assertEquals("a missing file cannot keep its row alive",
                Collections.emptyList(), store.entries());
    }

    /** A corrupt sidecar is dropped rather than allowed to render a half-known row. */
    @Test public void aCorruptSidecarIsDropped() throws Exception {
        assertTrue(putAudio("corrupt"));
        File sidecar = null;
        for (File candidate : store.directory().listFiles()) {
            if (candidate.getName().endsWith(".json")) sidecar = candidate;
        }
        assertTrue("expected a sidecar next to the audio", sidecar != null);
        Files.write(sidecar.toPath(), "{\"provider\":".getBytes(StandardCharsets.UTF_8));

        assertEquals("an unparseable sidecar must not become a row",
                Collections.emptyList(), store.entries());
    }
}