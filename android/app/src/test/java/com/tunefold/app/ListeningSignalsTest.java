package com.tunefold.app;

import org.junit.After;
import org.junit.Test;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertNull;
import static org.junit.Assert.assertTrue;

/**
 * Persisted listening feedback and the JSON request handed to the shared
 * ranker (Fase 5). The file is line/tab separated — never org.json, which the
 * stubbed android.jar of unit tests cannot run.
 */
public class ListeningSignalsTest {

    private File file = new File(System.getProperty("java.io.tmpdir"),
            "signals-test-" + System.nanoTime() + ".txt");

    @After public void tearDown() {
        file.delete();
        new File(file.getParentFile(), file.getName() + ".part").delete();
    }

    /** A track whose rawJson is usable; TestTracks' "{}" is deliberately rejected. */
    private static MediaTrack track(String id) {
        return new MediaTrack("YouTube", id, "Track " + id, "Artist " + id, "", "",
                "artist", 213_000L, "", "", "{\"title\":\"Track " + id + "\"}");
    }

    @Test public void recordsPlayAndBuildsRankRequest() {
        ListeningSignals signals = new ListeningSignals(file);
        signals.record(track("a"), ListeningSignals.KIND_PLAY, "manual");

        String request = signals.buildRankRequest(Arrays.asList(track("a"), track("b")));
        assertNotNull(request);
        assertTrue(request, request.contains("\"candidates\":["));
        assertTrue(request, request.contains("{\"title\":\"Track a\"}"));
        assertTrue(request, request.contains("{\"title\":\"Track b\"}"));
        assertTrue(request, request.contains("\"kind\":\"play\""));
        assertTrue(request, request.contains("\"context\":\"manual\""));
        assertTrue(request, request.contains("\"track\":"));
        // No listened-ms on a play: the field must not appear at all.
        assertTrue(request, !request.contains("duration_ms"));
    }

    @Test public void emptyCandidatesHasNothingToRank() {
        ListeningSignals signals = new ListeningSignals(file);
        signals.record(track("a"), ListeningSignals.KIND_PLAY, "manual");
        assertNull(signals.buildRankRequest(new ArrayList<>()));
        assertNull(signals.buildRankRequest(null));
    }

    @Test public void unidentifiableTrackIsNotRecorded() {
        ListeningSignals signals = new ListeningSignals(file);
        signals.record(TestTracks.of("YouTube", "a"), ListeningSignals.KIND_PLAY, "manual");
        signals.record(null, ListeningSignals.KIND_PLAY, "manual");
        assertEquals(0, signals.signalCount());
    }

    @Test public void completedCarriesListenedDuration() {
        ListeningSignals signals = new ListeningSignals(file);
        signals.record(track("a"), ListeningSignals.KIND_COMPLETED, "queue", 213_000L);
        String request = signals.buildRankRequest(Collections.singletonList(track("a")));
        assertNotNull(request);
        assertTrue(request, request.contains("\"kind\":\"completed\""));
        assertTrue(request, request.contains("\"duration_ms\":213000"));
    }

    @Test public void survivesRestartOfTheProcess() {
        new ListeningSignals(file).record(track("a"), ListeningSignals.KIND_SKIP, "manual");
        assertTrue("signal must hit the disk immediately", file.isFile());

        ListeningSignals reloaded = new ListeningSignals(file);
        assertEquals(1, reloaded.signalCount());
        String request = reloaded.buildRankRequest(Collections.singletonList(track("a")));
        assertNotNull(request);
        assertTrue(request, request.contains("\"kind\":\"skip\""));
    }

    @Test public void keepsTheNewestTwoHundredSignals() {
        ListeningSignals signals = new ListeningSignals(file);
        for (int i = 0; i < 205; i++) {
            signals.record(track("a"), ListeningSignals.KIND_PLAY, "manual");
        }
        assertEquals(200, signals.signalCount());

        ListeningSignals reloaded = new ListeningSignals(file);
        assertEquals(200, reloaded.signalCount());
    }

    @Test public void corruptedFileIsIgnoredNotFatal() throws IOException {
        try (FileOutputStream out = new FileOutputStream(file)) {
            out.write("junk\nshort\tline\nnot\talways\tnumber\tx\ty\n".getBytes(StandardCharsets.UTF_8));
        }
        ListeningSignals signals = new ListeningSignals(file);
        assertEquals(0, signals.signalCount());
        signals.record(track("a"), ListeningSignals.KIND_PLAY, "manual");
        assertEquals(1, signals.signalCount());
    }

    @Test public void trackIdentifiersSurviveJsonEscaping() {
        ListeningSignals signals = new ListeningSignals(file);
        MediaTrack quoted = new MediaTrack("YouTube", "a", "Say \"what\"?", "A\tB", "", "",
                "artist", 1000L, "", "", "{\"title\":\"quoted\"}");
        signals.record(quoted, ListeningSignals.KIND_PLAY, "manual");
        // The context/kind columns never contain tabs or quotes in practice,
        // but the stored track raw must round-trip untouched.
        String request = signals.buildRankRequest(Collections.singletonList(track("a")));
        assertNotNull(request);
        assertTrue(request, request.contains("{\"title\":\"quoted\"}"));
    }
}
