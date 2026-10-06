package com.tunefold.app;

import java.io.File;
import java.io.InputStream;

/** Optional stable-ID store for explicitly saved audio, separate from stream URL and metadata caches. */
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
    boolean register(String provider, String id, java.io.File path);

    /**
     * Directory this store owns. The downloader must use it verbatim: writing
     * anywhere else produces a file the store would not recognise.
     */
    java.io.File directory();

    boolean remove(String provider, String id);
}
