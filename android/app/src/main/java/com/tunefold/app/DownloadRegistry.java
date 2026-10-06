package com.tunefold.app;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Authoritative download state for every known track identity (§6).
 *
 * <p>This is what makes double downloads impossible: the registry is keyed by
 * {@link TrackKey}, so a track that appears in search results, in L1K3D, in
 * `Descargadas` and in the queue resolves to the <em>same</em> entry. A second
 * request for an active or completed download is answered from existing state
 * instead of starting new work.
 *
 * <p>Pure state with no I/O: the LocalMediaStore is consulted through the
 * {@link #isStored} predicate and by seeding with {@link #seedStored}, which
 * keeps the policy unit-testable and keeps filesystem access where it belongs.
 *
 * <p>Once {@link #seedStored} has enumerated the store, the disk probe is not
 * consulted again: every stored identity is known, so an unknown key is known not
 * to be stored. That is what keeps {@code Descargadas} and every row button off
 * the filesystem when the UI asks (§27).
 */
final class DownloadRegistry {
    /** Answers "does the store already hold a valid file for this identity?". */
    interface StoredProbe { boolean isStored(TrackKey key); }

    /** Notified whenever any identity's state changes. */
    interface Listener { void onDownloadChanged(TrackKey key, DownloadState state); }

    private final Map<TrackKey, DownloadState> states = new LinkedHashMap<>();
    private final StoredProbe probe;
    /** True once a full store enumeration has been seeded. */
    private volatile boolean scanComplete;

    DownloadRegistry(StoredProbe probe) {
        this.probe = probe;
    }

    /** State for an identity from memory alone; {@code null} when unknown. */
    private DownloadState stateOfKnown(TrackKey key) {
        if (key == null) return null;
        synchronized (states) { return states.get(key); }
    }

    /** Current state for an identity; never {@code null}. */
    DownloadState stateOf(TrackKey key) {
        if (key == null) return DownloadState.idle(null);
        synchronized (states) {
            DownloadState known = states.get(key);
            if (known != null) return known;
        }
        // Not tracked yet: the store is still the authority on disk.
        if (isStoredOnDisk(key)) return DownloadState.downloaded(key, 0);
        return DownloadState.idle(key);
    }

    DownloadState stateOf(MediaTrack track) { return stateOf(TrackKey.of(track)); }

    /** True when a valid local file exists for this identity. */
    boolean isDownloaded(TrackKey key) {
        if (key == null) return false;
        synchronized (states) {
            DownloadState known = states.get(key);
            if (known != null && known.isDownloaded()) return true;
        }
        return isStoredOnDisk(key);
    }

    boolean isDownloaded(MediaTrack track) { return isDownloaded(TrackKey.of(track)); }

    /** True when a download for this identity is already running. */
    boolean isActive(TrackKey key) {
        synchronized (states) {
            DownloadState known = states.get(key);
            return known != null && known.isActive();
        }
    }

    boolean isActive(MediaTrack track) { return isActive(TrackKey.of(track)); }

    /**
     * Decides whether a download may start for this identity.
     *
     * <p>The three rejection reasons are distinct on purpose, because the UI
     * reacts differently to each:
     * <ul>
     *   <li>{@code null} key → the track cannot be identified;</li>
     *   <li>{@code false} + {@code ALREADY_DOWNLOADED} → nothing to do;</li>
     *   <li>{@code false} + current state → a download is already running.</li>
     * </ul>
     */
    boolean canStart(TrackKey key) {
        if (key == null) return false;
        synchronized (states) {
            DownloadState known = states.get(key);
            if (known != null && known.isActive()) return false;
            if (known != null && known.isDownloaded()) return false;
        }
        return !isStoredOnDisk(key);
    }

    /** Marks that source resolution has begun for this identity. */
    DownloadState beginResolving(TrackKey key) {
        return put(key, new DownloadState(key, DownloadState.Phase.RESOLVING_SOURCE,
                0, -1, -1, ""));
    }

    /** Records progress. Returns the state that was stored. */
    DownloadState progress(TrackKey key, long received, long total, long speed) {
        return put(key, new DownloadState(key, DownloadState.Phase.DOWNLOADING,
                received, total, speed, ""));
    }

    /** Marks the download finished and committed. */
    DownloadState complete(TrackKey key, long size) {
        return put(key, DownloadState.downloaded(key, size));
    }

    /** Marks the download failed with a retryable error. */
    DownloadState fail(TrackKey key, String error) {
        DownloadState next = new DownloadState(key, DownloadState.Phase.FAILED,
                0, -1, -1, error == null ? "Download failed" : error);
        return put(key, next);
    }

    /** Marks the download cancelled by the user. */
    DownloadState cancel(TrackKey key) {
        return put(key, new DownloadState(key, DownloadState.Phase.CANCELLED,
                0, -1, -1, ""));
    }

    /**
     * Records that a download was refused because a valid file already exists.
     *
     * <p>Kept distinct from {@link #complete} so the UI can explain that nothing
     * was downloaded rather than pretending a transfer happened.
     */
    DownloadState alreadyDownloaded(TrackKey key, long size) {
        return put(key, new DownloadState(key, DownloadState.Phase.ALREADY_DOWNLOADED,
                size, size, -1, ""));
    }

    /**
     * Forgets in-memory state so the store is consulted again.
     *
     * <p>Called after a file is deleted: the track must leave `Descargadas`
     * immediately, and there must be no stale COMPLETED state claiming otherwise.
     */
    DownloadState forget(TrackKey key) {
        if (key == null) return DownloadState.idle(null);
        DownloadState previous;
        synchronized (states) { previous = states.remove(key); }
        DownloadState next = stateOf(key);
        if (previous != null) notifyChanged(key, next);
        return next;
    }

    /**
     * Seeds committed files discovered on disk.
     *
     * <p>Called when the store is enumerated: this is how `Descargadas` becomes a
     * derived view instead of a second copy of the library.
     */
    void seedStored(Iterable<LocalMediaStore.Entry> entries) {
        for (LocalMediaStore.Entry entry : entries) {
            // Known state only: this enumeration IS the disk answer, so asking
            // the probe for an identity it is about to seed would be redundant I/O.
            DownloadState current = stateOfKnown(entry.key);
            if (current == null || !current.isDownloaded()) {
                synchronized (states) {
                    states.put(entry.key, DownloadState.downloaded(entry.key, entry.size));
                }
            }
        }
        // Every stored identity is now known, so later reads answer from memory.
        scanComplete = true;
    }

    /**
     * Every identity currently stored, derived from known state only.
     *
     * <p>This is what backs the {@code Descargadas} view without a filesystem
     * walk on the caller's thread.
     */
    List<TrackKey> storedKeys() {
        List<TrackKey> keys = new ArrayList<>();
        synchronized (states) {
            for (DownloadState state : states.values()) {
                if (state.isDownloaded()) keys.add(state.key);
            }
        }
        return keys;
    }

    /** Validated size of a stored file, or 0 when it is not stored. */
    long storedSize(TrackKey key) {
        if (key == null) return 0;
        synchronized (states) {
            DownloadState known = states.get(key);
            if (known != null && known.isDownloaded()) return known.totalBytes;
        }
        return 0;
    }

    /** True when the file exists on disk, but only when the disk must be asked. */
    private boolean isStoredOnDisk(TrackKey key) {
        if (scanComplete) return false;
        return probe != null && probe.isStored(key);
    }

    private DownloadState put(TrackKey key, DownloadState next) {
        if (key == null) return next;
        synchronized (states) { states.put(key, next); }
        notifyChanged(key, next);
        return next;
    }

    private volatile Listener listener;

    void setListener(Listener value) { listener = value; }

    private void notifyChanged(TrackKey key, DownloadState state) {
        Listener current = listener;
        if (current != null) current.onDownloadChanged(key, state);
    }
}