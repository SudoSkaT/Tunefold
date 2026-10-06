package com.tunefold.app;

/** Builds tracks for unit tests through the same canonical constructor production uses. */
final class TestTracks {
    private TestTracks() {}

    static MediaTrack of(String provider, String id) {
        return of(provider, id, "Track " + id, 213_000L);
    }

    static MediaTrack of(String provider, String id, String title, long durationMs) {
        return new MediaTrack(provider, id, title, "Artist " + id, "", "", "artist",
                durationMs, "", "", "{}");
    }
}
