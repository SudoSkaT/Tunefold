package com.tunefold.app;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;

import org.junit.Before;
import org.junit.Test;

import java.io.File;
import java.nio.file.Files;
import java.util.concurrent.atomic.AtomicInteger;

/**
 * Guards the downloader's relationship with the native engine.
 *
 * <p>The downloader used to take a {@code long} engine handle as a constructor
 * argument and keep it forever. That single decision produced two separate, both
 * real, failures:
 *
 * <ul>
 *   <li>the engine is created on the controller's worker thread, so a handle read
 *       during startup could be a cached {@code 0} — and a cached zero means every
 *       download is refused with "could not start", forever;</li>
 *   <li>the handle used to be freed on every stop, error and end-of-track, so a
 *       cached handle became a dangling pointer and the next {@code startDownload}
 *       dereferenced released memory. There is a tombstone for it:
 *       {@code SIGSEGV in Java_com_tunefold_app_TunefoldBridge_startDownload}.</li>
 * </ul>
 *
 * <p>Both are the same mistake, so both are guarded by the same assertion: the handle
 * is resolved per call and never captured at construction.
 */
public class TrackDownloaderEngineTest {

    /** Counts how often the handle was actually asked for. */
    private static final class CountingEngines implements TrackDownloader.EngineSource {
        final AtomicInteger reads = new AtomicInteger();
        volatile long value;

        @Override public long handle() {
            reads.incrementAndGet();
            return value;
        }
    }

    private File directory;
    private LocalMediaStore store;
    private CountingEngines engines;

    @Before public void setUp() throws Exception {
        directory = Files.createTempDirectory("tunefold-download-engine-test").toFile();
        store = new FileLocalMediaStore(new File(directory, "local_media"));
        engines = new CountingEngines();
    }

    private TrackDownloader newDownloader() {
        return new TrackDownloader(new ProviderRegistry(), store, engines);
    }

    /**
     * The regression itself: constructing the downloader must not read the engine.
     *
     * <p>A read here is the cached-zero bug, because nothing would ever refresh it.
     */
    @Test public void constructionNeverReadsTheEngine() {
        newDownloader();
        assertEquals("the engine handle must not be captured at construction",
                0, engines.reads.get());
    }

    /** Construction must also stay off the filesystem: it runs on the main looper. */
    @Test public void constructionDoesNotTouchTheDownloadDirectory() {
        newDownloader();
        File[] created = directory.listFiles();
        assertEquals("construction must not walk or create download files",
                0, created == null ? 0 : created.length);
    }

    /** Every native call must resolve the handle at that moment, not from a field. */
    @Test public void handleIsResolvedLazilyOnEveryRead() {
        engines.value = 4242L;
        assertEquals(4242L, engines.handle());
        assertEquals(4242L, engines.handle());
        assertEquals("each native call needs its own read", 2, engines.reads.get());
    }

    /** A changing handle is exactly what a cached long got wrong. */
    @Test public void aMovedHandleIsFollowed() {
        engines.value = 1L;
        long first = engines.handle();
        engines.value = 2L;
        long second = engines.handle();
        assertFalse("a stale handle must not be reused", first == second);
    }

    /** The engine may not exist yet; that must read as zero, not as a bogus pointer. */
    @Test public void missingEngineReadsAsZero() {
        engines.value = 0L;
        assertEquals(0L, engines.handle());
    }

    @Test public void downloaderStartsIdle() {
        TrackDownloader downloader = newDownloader();
        assertFalse(downloader.isDownloaded(TestTracks.of("YouTube", "anything")));
        assertEquals(com.tunefold.app.DownloadState.Phase.IDLE,
                downloader.stateOf(TestTracks.of("YouTube", "anything")).phase);
    }

    /** The store owns the download directory; the downloader must not invent one. */
    @Test public void downloaderUsesTheStoreDirectory() {
        TrackDownloader downloader = newDownloader();
        assertEquals(new File(directory, "local_media").getAbsolutePath(),
                store.directory().getAbsolutePath());
        assertFalse(downloader.isDownloaded(TestTracks.of("YouTube", "x")));
    }
}