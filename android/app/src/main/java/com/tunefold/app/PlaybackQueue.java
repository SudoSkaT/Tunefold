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

    /**
     * Most entries {@link #replaceWith} will hold.
     *
     * <p>A bound, because the list comes from the screen and a screen's list is only
     * bounded by what the user collected. A queue that grows without limit is the one
     * thing in this class that can take the process down with it.
     */
    static final int MAX_ENTRIES = 500;

    private final Object mutex = new Object();
    private final List<MediaTrack> items = new ArrayList<>();
    private int currentIndex = -1;
    private boolean autoplayEnabled = true;
    private RepeatMode repeatMode = RepeatMode.OFF;

    /** Lock guarding every field. Callers hold it across read-modify-write. */
    Object lock() { return mutex; }

    // ------------------------------------------------------------- inspection

    boolean isEmpty() { synchronized (mutex) { return items.isEmpty(); } }

    /**
     * The track at {@code index}, or {@code null} when there is none.
     *
     * <p>An accessor rather than {@link #snapshot()} because "what is coming next" is
     * asked by Home on every render, and copying the queue to read its first few
     * entries is the kind of allocation this class was changed to stop making.
     */
    MediaTrack at(int index) {
        synchronized (mutex) {
            return index < 0 || index >= items.size() ? null : items.get(index);
        }
    }

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

    /**
     * Makes a whole list the queue and points the cursor at one of its entries.
     *
     * <p>This is what "play from a list" means: the list the user is looking at becomes
     * the order things play in, in the order they are shown, so the track after the one
     * they tapped is the one the list shows after it and the end of the list is the end
     * of the queue — where {@link AutoplayPolicy} decides between a recommendation and
     * stopping. Playing a row used to call {@link #playNow}, which queued exactly one
     * track: the song ended and there was nothing behind it to continue with.
     *
     * <p>Order is the caller's order and nothing is sorted or reordered, so what plays
     * next is what the list shows next. Entries without an identity are dropped — they
     * could never be stepped to — and a repeated identity is kept once, because two
     * rows showing the same song would otherwise play it twice in a row.
     *
     * @param tracks the list, in display order; {@code null} or empty empties the queue
     * @param start  the entry to play first; the first one when it is not in the list
     * @return the index the cursor ended up at, or {@code -1} when nothing was queued
     */
    int replaceWith(List<MediaTrack> tracks, TrackKey start) {
        synchronized (mutex) {
            items.clear();
            currentIndex = -1;
            if (tracks != null) {
                for (MediaTrack track : tracks) {
                    if (items.size() >= MAX_ENTRIES) break;
                    if (track == null || TrackKey.of(track) == null) continue;
                    if (indexOf(TrackKey.of(track)) >= 0) continue;
                    items.add(track);
                }
            }
            // A start that is not in the list still has to leave something playing, so
            // the cursor falls back to the front rather than to "nothing".
            currentIndex = start == null ? -1 : indexOf(start);
            if (currentIndex < 0) currentIndex = items.isEmpty() ? -1 : 0;
            return currentIndex;
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

    /** Where an identity sits, or {@code -1}; for jumping to what the player lists. */
    int indexOfKey(TrackKey key) {
        if (key == null) return -1;
        synchronized (mutex) { return indexOf(key); }
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

    /**
     * Puts the cursor on a known index.
     *
     * <p>For jumping: the caller found the index, so the queue does not look it up
     * again, and a jump is not a step — it may go backwards.
     */
    boolean moveTo(int index) {
        synchronized (mutex) {
            if (index < 0 || index >= items.size()) return false;
            currentIndex = index;
            return true;
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