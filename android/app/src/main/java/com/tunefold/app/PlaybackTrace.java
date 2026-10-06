package com.tunefold.app;

import android.os.SystemClock;
import android.util.Log;

/** Per-play monotonic timeline. Never includes a signed stream URL or request headers. */
final class PlaybackTrace {
    private final long startedNs = SystemClock.elapsedRealtimeNanos();
    private final String trackId;

    PlaybackTrace(String trackId) {
        this.trackId = trackId == null ? "unknown" : trackId;
        mark("T0_PLAY_TAP");
    }

    void mark(String event) {
        long elapsedMs = (SystemClock.elapsedRealtimeNanos() - startedNs) / 1_000_000L;
        Log.i("TunefoldPerf", "track=" + trackId + " event=" + event + " elapsed_ms=" + elapsedMs);
    }
}
