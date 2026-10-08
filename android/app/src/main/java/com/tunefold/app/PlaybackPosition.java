package com.tunefold.app;

/**
 * A single, consistent read of "where is the active track right now".
 *
 * <p>Mutable and reusable on purpose. The position display is polled twice a second
 * for as long as audio plays, and the alternative to a reusable holder was allocating
 * a small object on every tick — forever, for a value that is three numbers wide.
 *
 * <p>Reusing one instance is also how the consistency guarantee survives: the session
 * fills a caller-owned holder in one go, so a tick can never pair one track's identity
 * with another track's elapsed time.
 */
final class PlaybackPosition {

    /** Identity the position belongs to, or {@code null} when nothing is loaded. */
    TrackKey key;
    /** Elapsed time of {@link #key}, in milliseconds. */
    long positionMs;
    /** Length of {@link #key}, or a non-positive value when unknown. */
    long durationMs;

    /**
     * Fills this holder.
     *
     * <p>Package-private so only the session can produce one: a UI that invented its
     * own holder could desynchronise it from the session's clock, which is exactly the
     * bug {@link PlaybackClock} exists to prevent.
     */
    void set(TrackKey trackKey, long position, long duration) {
        key = trackKey;
        positionMs = position;
        durationMs = duration;
    }
}