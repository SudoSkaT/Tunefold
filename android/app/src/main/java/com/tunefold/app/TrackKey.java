package com.tunefold.app;

import java.util.Objects;

/**
 * Track identity: {@code provider + provider_track_id}.
 *
 * <p>This is the single source of track identity in the Android app. Every
 * dimension of a track — L1K3D membership, download state, LocalMediaStore
 * file, queue membership, current playback and recommendation filtering —
 * keys off this value, so the same song is the same {@code TrackKey} wherever
 * it appears: search results, playlists, recommendations, autoplay or queue.
 *
 * <p>Deliberately NOT part of identity: title, artist, URL, signed URL,
 * position in a playlist or file name. All of those can change for the same
 * musical entity, or collide for different ones.
 *
 * <p>Immutable and safe to use as a map key.
 */
final class TrackKey implements Comparable<TrackKey> {
    private final String provider;
    private final String providerTrackId;

    private TrackKey(String provider, String providerTrackId) {
        this.provider = provider;
        this.providerTrackId = providerTrackId;
    }

    /**
     * Identity of a track, or {@code null} when it cannot be identified.
     *
     * <p>A track without a provider id has no stable identity and must not be
     * liked, queued or downloaded: doing so would create entries that can never
     * be found again.
     */
    static TrackKey of(MediaTrack track) {
        return track == null ? null : of(track.provider, track.providerId);
    }

    /** Identity from its parts, or {@code null} when either part is empty. */
    static TrackKey of(String provider, String providerTrackId) {
        if (provider == null || provider.isEmpty()) return null;
        if (providerTrackId == null || providerTrackId.isEmpty()) return null;
        return new TrackKey(provider, providerTrackId);
    }

String provider() { return provider; }

    String providerTrackId() { return providerTrackId; }

    @Override public boolean equals(Object other) {
        if (this == other) return true;
        if (!(other instanceof TrackKey)) return false;
        TrackKey that = (TrackKey) other;
        return provider.equals(that.provider) && providerTrackId.equals(that.providerTrackId);
    }

    @Override public int hashCode() { return Objects.hash(provider, providerTrackId); }

    /** Stable ordering so deterministic playlist listings are reproducible. */
    @Override public int compareTo(TrackKey other) {
        int byProvider = provider.compareTo(other.provider);
        return byProvider != 0 ? byProvider : providerTrackId.compareTo(other.providerTrackId);
    }

    @Override public String toString() { return provider + "/" + providerTrackId; }
}