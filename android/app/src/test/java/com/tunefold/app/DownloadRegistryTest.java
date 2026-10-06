package com.tunefold.app;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

import org.junit.Before;
import org.junit.Test;

import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Set;

/**
 * Download deduplication and state derivation (§6, §7, §29).
 *
 * <p>The central guarantee: a track reachable through several playlists still
 * has exactly one download state, so it can never produce two files.
 */
public class DownloadRegistryTest {

    private DownloadRegistry registry;
    private Set<TrackKey> onDisk;

    @Before public void setUp() {
        onDisk = new HashSet<>();
        registry = new DownloadRegistry(onDisk::contains);
    }

    private static TrackKey key(String id) { return TrackKey.of("YouTube", id); }

    @Test public void idleForAnUnknownTrack() {
        DownloadState state = registry.stateOf(key("a"));
        assertEquals(DownloadState.Phase.IDLE, state.phase);
        assertEquals("unknown total must not fabricate a percentage", -1, state.progressPercent());
        assertFalse(registry.isDownloaded(key("a")));
        assertTrue(registry.canStart(key("a")));
    }

    @Test public void aStoredFileWinsOverIdle() {
        onDisk.add(key("a"));
        DownloadState state = registry.stateOf(key("a"));
        assertEquals(DownloadState.Phase.COMPLETED, state.phase);
        assertTrue(registry.isDownloaded(key("a")));
        assertFalse("a downloaded track must not start another download",
                registry.canStart(key("a")));
    }

    @Test public void secondRequestWhileDownloadingIsRefused() {
        TrackKey key = key("a");
        assertTrue(registry.canStart(key));
        registry.beginResolving(key);
        assertTrue(registry.isActive(key));
        assertFalse("no concurrent second download", registry.canStart(key));

        registry.progress(key, 500, 2000, 100);
        assertTrue(registry.isActive(key));
        assertFalse("still refused while bytes move", registry.canStart(key));
    }

    @Test public void completedBlocksAFreshDownload() {
        TrackKey key = key("a");
        registry.beginResolving(key);
        registry.complete(key, 2000);
        assertTrue(registry.isDownloaded(key));
        assertFalse(registry.canStart(key));
    }

    @Test public void failedStateIsRetryableAndUnblocks() {
        TrackKey key = key("a");
        registry.fail(key, "network");
        DownloadState state = registry.stateOf(key);
        assertEquals(DownloadState.Phase.FAILED, state.phase);
        assertTrue(state.isRetryable());
        assertFalse(registry.isActive(key));
        assertTrue("a failed download may be retried", registry.canStart(key));
    }

    @Test public void cancelledStateIsNotDownloadedAndNotAFile() {
        TrackKey key = key("a");
        registry.beginResolving(key);
        registry.progress(key, 900, 2000, 100);
        registry.cancel(key);
        DownloadState state = registry.stateOf(key);
        assertEquals(DownloadState.Phase.CANCELLED, state.phase);
        assertFalse("a cancelled download never claims a file", state.isDownloaded());
        assertFalse(registry.isDownloaded(key));
        assertTrue(registry.canStart(key));
    }

    @Test public void oneTrackAcrossManyDimensionsHasOneStateAndOneFile() {
        // The same song reached from search, L1K3D, Descargadas and the queue.
        TrackKey key = key("dQw4w9WgXcQ");
        registry.beginResolving(key);
        registry.complete(key, 3_449_447);

        // Membership in a playlist does not create a second identity.
        assertTrue(registry.isDownloaded(key));
        assertFalse(registry.canStart(key));
        assertFalse(registry.canStart(key));
        assertEquals(DownloadState.Phase.COMPLETED, registry.stateOf(key).phase);
    }

    @Test public void forgettingReConsultsTheStoreSoDescargadasUpdates() {
        TrackKey key = key("a");
        registry.beginResolving(key);
        registry.complete(key, 2000);
        assertTrue(registry.isDownloaded(key));

        // The file is deleted: Descargadas must drop it immediately.
        onDisk.remove(key);
        DownloadState afterDelete = registry.forget(key);
        assertEquals(DownloadState.Phase.IDLE, afterDelete.phase);
        assertFalse(registry.isDownloaded(key));
    }

    @Test public void seededEntriesBecomeDownloaded() {
        List<LocalMediaStore.Entry> entries = new ArrayList<>();
        entries.add(new LocalMediaStore.Entry(key("a"), 111L, null));
        entries.add(new LocalMediaStore.Entry(key("b"), 222L, null));
        registry.seedStored(entries);

        assertTrue(registry.isDownloaded(key("a")));
        assertTrue(registry.isDownloaded(key("b")));
        assertFalse(registry.isDownloaded(key("c")));
    }

    @Test public void progressPercentIsHonestAboutUnknownTotals() {
        TrackKey key = key("a");
        registry.progress(key, 4096, -1, -1);
        assertEquals(-1, registry.stateOf(key).progressPercent());

        registry.progress(key, 500, 1000, 250);
        assertEquals(50, registry.stateOf(key).progressPercent());

        registry.progress(key, 5000, 1000, 250);
        assertEquals("never exceeds 100", 100, registry.stateOf(key).progressPercent());
    }

    @Test public void alreadyDownloadedIsDistinctFromCompleted() {
        TrackKey key = key("a");
        registry.alreadyDownloaded(key, 500);
        DownloadState state = registry.stateOf(key);
        assertEquals(DownloadState.Phase.ALREADY_DOWNLOADED, state.phase);
        assertTrue(state.isDownloaded());
        assertEquals("Downloaded", state.label());
    }

    @Test public void labelsDescribeEachPhase() {
        assertEquals("Download", DownloadState.idle(key("a")).label());
        assertEquals("Resolving source",
                new DownloadState(key("a"), DownloadState.Phase.RESOLVING_SOURCE, 0, -1, -1, "").label());
        assertEquals("Downloading",
                new DownloadState(key("a"), DownloadState.Phase.DOWNLOADING, 10, -1, -1, "").label());
        assertEquals("Downloading 40%",
                new DownloadState(key("a"), DownloadState.Phase.DOWNLOADING, 40, 100, 1, "").label());
        assertEquals("Download failed",
                new DownloadState(key("a"), DownloadState.Phase.FAILED, 0, -1, -1, "x").label());
    }

    @Test public void listenerSeesEveryTransition() {
        List<String> events = new ArrayList<>();
        registry.setListener((key, state) -> events.add(state.phase + ":" + key));
        TrackKey key = key("a");
        registry.beginResolving(key);
        registry.progress(key, 1, 2, 1);
        registry.complete(key, 2);
        assertEquals(3, events.size());
        assertEquals("RESOLVING_SOURCE:" + key, events.get(0));
        assertEquals("COMPLETED:" + key, events.get(2));
    }

    @Test public void storedKeysComeFromKnownStateNotDisk() {
        registry.seedStored(entries(key("a"), key("b")));
        assertEquals(2, registry.storedKeys().size());
        assertTrue(registry.storedKeys().contains(key("a")));
        assertEquals("seeded sizes survive", 42, registry.storedSize(key("a")));
        assertEquals("absent identity is not stored", 0, registry.storedSize(key("z")));
    }

    @Test public void afterAScanTheDiskIsNeverAskedAgain() {
        // A probe that throws stands in for "filesystem access on the UI thread":
        // once the store has been enumerated, no read may reach it.
        DownloadRegistry guarded = new DownloadRegistry(key -> {
            throw new AssertionError("disk consulted after the store scan: " + key);
        });
        guarded.seedStored(entries(key("a")));
        assertTrue(guarded.isDownloaded(key("a")));
        assertFalse("unknown means not stored once the scan is complete",
                guarded.isDownloaded(key("z")));
        assertTrue(guarded.canStart(key("z")));
    }

    @Test public void beforeAScanTheStoreIsStillAsked() {
        onDisk.add(key("a"));
        assertTrue(registry.isDownloaded(key("a")));
        assertFalse(registry.isDownloaded(key("z")));
    }

    private static List<LocalMediaStore.Entry> entries(TrackKey... keys) {
        List<LocalMediaStore.Entry> entries = new ArrayList<>();
        for (TrackKey key : keys) {
            entries.add(new LocalMediaStore.Entry(key, 42, null));
        }
        return entries;
    }

    @Test public void nullIdentityIsNeverDownloadable() {
        assertFalse(registry.canStart(null));
        assertFalse(registry.isDownloaded((TrackKey) null));
        assertFalse(registry.isActive((TrackKey) null));
    }
}