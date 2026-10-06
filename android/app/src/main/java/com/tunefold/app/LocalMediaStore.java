package com.tunefold.app;

import java.io.File;
import java.io.InputStream;

/** Optional stable-ID store for explicitly saved audio, separate from stream URL and metadata caches. */
interface LocalMediaStore {
    boolean contains(String provider, String id);
    File get(String provider, String id);
    boolean store(String provider, String id, InputStream audio);
    boolean remove(String provider, String id);
}
