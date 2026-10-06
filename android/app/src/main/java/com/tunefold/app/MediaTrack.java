package com.tunefold.app;

import org.json.JSONArray;
import org.json.JSONObject;

/**
 * Provider-neutral subset of the Rust domain Track, retaining its canonical JSON
 * identity.
 *
 * <p>Identity is {@code provider + providerTrackId} (see {@link TrackKey}) and
 * is deliberately independent of this object's title, artist or any URL.
 */
final class MediaTrack {
    final String title, provider, providerId, providerUrl, thumbnail, rawJson;
    final String artist, channel, album, artistRole;
    final long durationMs;
    volatile String artworkCachePath = "";

    /**
     * Canonical construction.
     *
     * <p>{@link #fromJson} delegates here, so provider data and test data go
     * through exactly one path and cannot drift apart.
     */
    MediaTrack(String provider, String providerId, String title, String artist, String channel,
               String album, String artistRole, long durationMs, String thumbnail,
               String providerUrl, String rawJson) {
        this.provider = provider == null ? "" : provider;
        this.providerId = providerId == null ? "" : providerId;
        this.title = title == null ? "Untitled" : title;
        this.artist = artist == null ? "" : artist;
        this.channel = channel == null ? "" : channel;
        this.album = album == null ? "" : album;
        this.artistRole = artistRole == null ? "artist" : artistRole;
        this.durationMs = durationMs;
        this.thumbnail = thumbnail == null ? "" : thumbnail;
        this.providerUrl = providerUrl == null ? "" : providerUrl;
        this.rawJson = rawJson == null ? "{}" : rawJson;
    }

    /** Parses the provider's JSON representation. */
    static MediaTrack fromJson(JSONObject value) {
        String title = value.optString("title", "Untitled");
        String provider = value.optString("source", "unknown");
        String providerId = value.optString("external_id", "");
        String providerUrl = value.optString("url", "");
        JSONObject image = value.optJSONObject("thumbnail");
        String thumbnail = image == null ? "" : image.optString("url", "");
        JSONObject duration = value.optJSONObject("duration");
        long durationMs = duration == null ? -1
                : duration.optLong("secs", 0) * 1000 + duration.optLong("nanos", 0) / 1_000_000;
        JSONArray artists = value.optJSONArray("artists");
        String name = "";
        String role = "artist";
        if (artists != null && artists.length() > 0) {
            JSONObject first = artists.optJSONObject(0);
            if (first != null) {
                name = first.optString("name", "");
                role = first.optString("role", "artist");
            }
        }
        // A channel is an attribution, not a performing artist.
        String artist = "channel".equals(role) ? "" : name;
        String channel = "channel".equals(role) ? name : "";
        JSONObject collection = value.optJSONObject("album");
        String album = collection == null ? "" : collection.optString("name", "");
        return new MediaTrack(provider, providerId, title, artist, channel, album, role,
                durationMs, thumbnail, providerUrl, value.toString());
    }

    /** Stable identity of this track, or {@code null} when it has none. */
    TrackKey key() { return TrackKey.of(provider, providerId); }

    String displayText() {
        String attribution = artist.isEmpty() ? channel : artist;
        return attribution.isEmpty() ? title : title + " — " + attribution;
    }
}