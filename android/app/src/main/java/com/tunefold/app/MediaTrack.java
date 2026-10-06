package com.tunefold.app;

import org.json.JSONArray;
import org.json.JSONObject;

/** Provider-neutral subset of the Rust domain Track, retaining its canonical JSON identity. */
final class MediaTrack {
    final String title, provider, providerId, providerUrl, thumbnail, rawJson;
    final String artist, channel, album, artistRole;
    final long durationMs;
    volatile String artworkCachePath = "";

    private MediaTrack(JSONObject value) {
        rawJson = value.toString();
        title = value.optString("title", "Untitled");
        provider = value.optString("source", "unknown");
        providerId = value.optString("external_id", "");
        providerUrl = value.optString("url", "");
        JSONObject image = value.optJSONObject("thumbnail");
        thumbnail = image == null ? "" : image.optString("url", "");
        JSONObject duration = value.optJSONObject("duration");
        durationMs = duration == null ? -1
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
        artist = "channel".equals(role) ? "" : name;
        channel = "channel".equals(role) ? name : "";
        artistRole = role;
        JSONObject collection = value.optJSONObject("album");
        album = collection == null ? "" : collection.optString("name", "");
    }

    static MediaTrack fromJson(JSONObject value) { return new MediaTrack(value); }

    String displayText() {
        String attribution = artist.isEmpty() ? channel : artist;
        return attribution.isEmpty() ? title : title + " — " + attribution;
    }
}
