package com.tunefold.app;

import java.io.File;
import java.io.InputStream;
import java.util.List;

/**
 * Optional stable-ID store for explicitly saved audio, separate from stream URL
 * and metadata caches.
 *
 * <p>The store is the authority for "is this track downloaded". Nothing else
 * decides it: the `Descargadas` view is derived from {@link #entries()}, so a
 * track leaves that view exactly when its file stops being valid.
 */
interface LocalMediaStore {
    boolean contains(String provider, String id);

    File get(String provider, String id);

    boolean store(String provider, String id, InputStream audio);

    /**
     * Registers audio already committed to disk at {@code path} by the
     * downloader.
     *
     * <p>No copies bytes: the downloader already wrote them atomically to a
     * {@code .part} and renamed them. This only runs the store's own validation
     * and writes the identity/size sidecar that {@link #get} trusts, so the
     * existing limits and eviction budget stay authoritative.
     */
    boolean register(String provider, String id, File path);

    boolean remove(String provider, String id);

    /**
     * Every valid entry currently stored, in deterministic (identity) order.
     *
     * <p>Entries whose sidecar or audio file is missing, inconsistent or over
     * the size limit are pruned while enumerating, so the result only ever
     * contains genuinely playable files.
     */
    List<Entry> entries();

    /**
     * Directory this store owns. The downloader must use it verbatim: writing
     * anywhere else produces a file the store would not recognise.
     */
    File directory();

    /** A stored, validated local media file. */
    final class Entry {
        /** Identity of the track this audio belongs to. */
        public final TrackKey key;
        /** Validated size in bytes. */
        public final long size;
        private final File audio;

        Entry(TrackKey key, long size, File audio) {
            this.key = key;
            this.size = size;
            this.audio = audio;
        }

        /** The audio file. Guaranteed to exist: the entry was validated. */
        public File audio() { return audio; }

        @Override public String toString() { return key + " (" + size + " bytes)"; }
    }
}