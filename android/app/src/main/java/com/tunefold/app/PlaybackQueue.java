package com.tunefold.app;

import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Set;

/**
 * Explicit playback queue: the single source of truth for what plays next.
 *
 * <p>Entries are {@link MediaTrack} objects keyed by {@link TrackKey}, so the
 * same song arriving from search, a playlist, a recommendation or autoplay is
 * recognised as the same item and never queued twice in a row by accident.
 *
 * <p>Pure state: no I/O, no Android types, no threads. Every mutation happens on
 * the caller's thread under {@link #lock()}, which keeps ordering deterministic
 * and makes the whole class unit-testable without a device.
 *
 * <p>Not implemented here on purpose (§12): drag reordering, cloud sync,
 * multi-device queues.
 */
final class PlaybackQueue {
    /** What to do when the queue runs out. */
    enum RepeatMode {
        /** Stop at the end. */
        OFF,
        /** Loop the whole queue forever. */
        ALL,
        /** Loop the current track forever. */
        ONE
    }

    private final Object mutex = new Object();
    private final List<MediaTrack> items = new ArrayList<>();
    private int currentIndex = -1;
    private boolean autoplayEnabled = true;
    private RepeatMode repeatMode = RepeatMode.OFF;

    /** Lock guarding every field. Callers hold it across read-modify-write. */
    Object lock() { return mutex; }

    // ------------------------------------------------------------- inspection

    boolean isEmpty() { synchronized (mutex) { return items.isEmpty(); } }

    int size() { synchronized (mutex) { return items.size(); } }

    int currentIndex() { synchronized (mutex) { return currentIndex; } }

    /** The track currently playing, or {@code null}. */
    MediaTrack current() { synchronized (mutex) { return currentAt(currentIndex); } }

    MediaTrack currentAt(int index) {
        synchronized (mutex) {
            return index >= 0 && index < items.size() ? items.get(index) : null;
        }
    }

    /** A snapshot of the queue, safe to iterate while the queue changes. */
    List<MediaTrack> snapshot() { synchronized (mutex) { return new ArrayList<>(items); } }

    boolean isAutoplayEnabled() { synchronized (mutex) { return autoplayEnabled; } }

    void setAutoplayEnabled(boolean value) {
        synchronized (mutex) { autoplayEnabled = value; }
    }

    RepeatMode repeatMode() { synchronized (mutex) { return repeatMode; } }

    void setRepeatMode(RepeatMode mode) {
        synchronized (mutex) { repeatMode = mode == null ? RepeatMode.OFF : mode; }
    }

    boolean contains(TrackKey key) {
        if (key == null) return false;
        synchronized (mutex) {
            for (MediaTrack track : items) if (key.equals(TrackKey.of(track))) return true;
        }
        return false;
    }

    // --------------------------------------------------------------- mutation

    /**
     * Appends a track unless it is already queued.
     *
     * @return the index of the item, or {@code -1} when the track was rejected
     *     (no stable identity) or already present.
     */
    int append(MediaTrack track) {
        TrackKey key = TrackKey.of(track);
        if (key == null) return -1;
        synchronized (mutex) {
            for (int index = 0; index < items.size(); index++) {
                if (key.equals(TrackKey.of(items.get(index)))) return -1;
            }
            items.add(track);
            return items.size() - 1;
        }
    }

    /** Appends a track and makes it current, replacing any previous current. */
    void setCurrent(MediaTrack track) {
        TrackKey key = TrackKey.of(track);
        if (key == null) return;
        synchronized (mutex) {
            int existing = indexOf(key);
            if (existing >= 0) { currentIndex = existing; return; }
            items.add(track);
            currentIndex = items.size() - 1;
        }
    }

    /** Ensures the track is queued and current, returning its index. */
    int playNow(MediaTrack track) {
        TrackKey key = TrackKey.of(track);
        if (key == null) return -1;
        synchronized (mutex) {
            int existing = indexOf(key);
            if (existing >= 0) { currentIndex = existing; return existing; }
            items.add(track);
            currentIndex = items.size() - 1;
            return currentIndex;
        }
    }

    void clear() {
        synchronized (mutex) {
            items.clear();
            currentIndex = -1;
        }
    }

    /** Removes one entry, keeping the current index pointing at the same track. */
    boolean remove(TrackKey key) {
        if (key == null) return false;
        synchronized (mutex) {
            int index = indexOf(key);
            if (index < 0) return false;
            items.remove(index);
            if (currentIndex == index) {
                // The removed item was current: fall back to whatever now sits
                // at this position, else the previous one.
                if (currentIndex >= items.size()) currentIndex = items.size() - 1;
            } else if (currentIndex > index) {
                currentIndex--;
            }
            return true;
        }
    }

    private int indexOf(TrackKey key) {
        for (int index = 0; index < items.size(); index++) {
            if (key.equals(TrackKey.of(items.get(index)))) return index;
        }
        return -1;
    }

    // ---------------------------------------------------------------- stepping

    /**
     * The item Next should play, honouring the repeat mode.
     *
     * @return {@code null} when the queue is exhausted, so the caller can decide
     *     between autoplay and stopping.
     */
    MediaTrack peekNext() {
        synchronized (mutex) {
            if (items.isEmpty()) return null;
            if (repeatMode == RepeatMode.ONE) return currentAt(currentIndex);
            int next = currentIndex + 1;
            if (next < items.size()) return items.get(next);
            if (repeatMode == RepeatMode.ALL && !items.isEmpty()) return items.get(0);
            return null;
        }
    }

    /**
     * The item Previous should play.
     *
     * <p>{@code restartThresholdMs} implements the common player rule: after a
     * few seconds, Previous restarts the current track instead of skipping back.
     *
     * @return the track to play, or {@code null} when there is nothing before.
     */
    MediaTrack peekPrevious(long positionMs, long restartThresholdMs) {
        synchronized (mutex) {
            if (items.isEmpty()) return null;
            if (positionMs > restartThresholdMs) return currentAt(currentIndex);
            int previous = currentIndex - 1;
            if (previous >= 0) return items.get(previous);
            if (repeatMode == RepeatMode.ALL) return items.get(items.size() - 1);
            return null;
        }
    }

    /** Moves the cursor to the item after the current one. */
    boolean advance() {
        MediaTrack next = peekNext();
        if (next == null) return false;
        synchronized (mutex) {
            int index = indexOf(TrackKey.of(next));
            if (index < 0) return false;
            currentIndex = index;
            return true;
        }
    }

    /** Moves the cursor to the item before the current one. */
    boolean retreat() {
        synchronized (mutex) {
            if (items.isEmpty()) return false;
            int previous = currentIndex - 1;
            if (previous < 0) previous = repeatMode == RepeatMode.ALL ? items.size() - 1 : -1;
            if (previous < 0) return false;
            currentIndex = previous;
            return true;
        }
    }

    /**
     * Removes identities the caller wants to avoid.
     *
     * <p>Used by recommendations and autoplay so a candidate that is already
     * queued, already liked or just played is never inserted twice.
     */
    int removeAll(Set<TrackKey> keys) {
        if (keys == null || keys.isEmpty()) return 0;
        synchronized (mutex) {
            int before = items.size();
            items.removeIf(track -> keys.contains(TrackKey.of(track)));
            currentIndex = clampCurrent();
            return before - items.size();
        }
    }

    private int clampCurrent() {
        if (items.isEmpty()) return -1;
        if (currentIndex < 0) return -1;
        if (currentIndex >= items.size()) return items.size() - 1;
        return currentIndex;
    }

    /** Identities currently queued, for filtering recommendations. */
    Set<TrackKey> queuedKeys() {
        synchronized (mutex) {
            Set<TrackKey> keys = new LinkedHashSet<>();
            for (MediaTrack track : items) {
                TrackKey key = TrackKey.of(track);
                if (key != null) keys.add(key);
            }
            return keys;
        }
    }

    static List<MediaTrack> none() { return Collections.emptyList(); }
}