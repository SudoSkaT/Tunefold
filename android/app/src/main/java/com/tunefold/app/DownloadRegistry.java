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
    /**
     * Title each download was committed under.
     *
     * <p>Memory only, and that is the point: the row's label has to be readable on the
     * main thread several times a second, so it may not come from the sidecar on demand.
     * The store writes the title once at commit and seeds it back on every scan.
     */
    private final Map<TrackKey, String> titles = new LinkedHashMap<>();
    private final StoredProbe probe;
    /** True once a full store enumeration has been seeded. */
    private volatile boolean scanComplete;
    /**
     * Bumped whenever the set of downloaded identities changes.
     *
     * <p>Exists so a view derived from this registry can tell "a row's state changed"
     * from "the list gained or lost a track" in O(1). Without it, {@code Descargadas}
     * was rendered once at bind time — empty, because the store scan had not finished
     * — and the seed notification only re-bound rows, of which there were none, so the
     * downloads that were sitting on disk never appeared.
     */
    private final java.util.concurrent.atomic.AtomicInteger revision =
            new java.util.concurrent.atomic.AtomicInteger();

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

    /**
     * Marks the download finished and committed, remembering the title it was
     * committed under.
     *
     * @param title title to remember for the row, or {@code null} when unknown
     */
    DownloadState complete(TrackKey key, long size, String title) {
        rememberTitle(key, title);
        // Membership, not just state: this identity is now part of the derived view,
        // and a view that only rebinds rows cannot make a row appear.
        boolean wasDownloaded = isDownloaded(key);
        DownloadState next = put(key, DownloadState.downloaded(key, size));
        if (!wasDownloaded) revision.incrementAndGet();
        return next;
    }

    /** Marks the download failed with a retryable error. */
    /**
     * Remembers a title for an identity, replacing any earlier one.
     *
     * <p>A blank title is ignored rather than stored: an empty string is what a row
     * would render as, and remembering one would turn "no title" into "empty title".
     *
     * @return {@code true} when the remembered title changed
     */
    boolean rememberTitle(TrackKey key, String title) {
        if (key == null || title == null || title.trim().isEmpty()) return false;
        String cleaned = title.trim();
        synchronized (states) {
            if (cleaned.equals(titles.get(key))) return false;
            titles.put(key, cleaned);
        }
        return true;
    }

    /** Title remembered for an identity, or {@code null}. Memory only, never disk. */
    String titleOf(TrackKey key) {
        if (key == null) return null;
        synchronized (states) { return titles.get(key); }
    }

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
        synchronized (states) {
            previous = states.remove(key);
            // The title described a file that no longer exists; keeping it would
            // leave a label behind for a track `Descargadas` no longer lists.
            titles.remove(key);
        }
        DownloadState next = stateOf(key);
        if (previous != null && previous.isDownloaded()) revision.incrementAndGet();
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
        List<TrackKey> seeded = new ArrayList<>();
        for (LocalMediaStore.Entry entry : entries) {
            // Known state only: this enumeration IS the disk answer, so asking
            // the probe for an identity it is about to seed would be redundant I/O.
            DownloadState current = stateOfKnown(entry.key);
            if (current == null || !current.isDownloaded()) {
                synchronized (states) {
                    states.put(entry.key, DownloadState.downloaded(entry.key, entry.size));
                }
                seeded.add(entry.key);
            }
            // Seeded from the sidecar, so the title survives a restart exactly like
            // the file does.
            rememberTitle(entry.key, entry.title());
        }
        // Every stored identity is now known, so later reads answer from memory.
        scanComplete = true;
        if (!seeded.isEmpty()) revision.incrementAndGet();
        // Rows built before the scan finished showed "Download" for tracks that were
        // already on disk; without this they stayed wrong until the next reload.
        for (TrackKey key : seeded) notifyChanged(key, stateOfKnown(key));
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

    /**
     * Changes whenever an identity becomes downloaded for the first time.
     *
     * <p>Cheap by design: a derived view polls this instead of diffing its own rows,
     * which would be O(n) per download-progress notification.
     */
    int revision() { return revision.get(); }

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