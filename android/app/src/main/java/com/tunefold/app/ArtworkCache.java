package com.tunefold.app;

import android.util.Log;

import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.util.Arrays;
import java.util.Comparator;
import java.util.LinkedHashMap;
import java.util.Map;

/** Bounded image-only cache. Runs on ProviderRegistry's artwork executor, never the decoder. */
final class ArtworkCache {
    private static final long MAX_BYTES = 128L * 1024L * 1024L;
    private static final long MAX_IMAGE_BYTES = 10L * 1024L * 1024L;
    private static final long TTL_MS = 30L * 24L * 60L * 60L * 1000L;
    /**
     * Decoded covers kept in memory.
     *
     * <p>Two, not more: a decoded cover is the largest bitmap in the app (~1.6 MB
     * at the Now Playing size), and the case this exists for is a recreated
     * Activity or a rotation re-reading the same file, where one entry would
     * already cover it.
     */
    private static final int MEMORY_SLOTS = 2;
    /**
     * Bytes of decoded <b>thumbnails</b> kept in memory.
     *
     * <p>A list row draws at 48dp, which on a 3x screen is a 144² bitmap: about
     * 83 KB. A screenful is eight of them, and a flick through a list is dozens, so a
     * two-slot cache sized for the player's 640² cover would decode every row again
     * on every pass — the same file, over and over, from disk.
     *
     * <p>4 MB is about fifty thumbnails, which is several screens of scrolling plus
     * the rows the user is likely to come back to, and it is a byte budget rather
     * than a count so the bound holds on any density: at 1x a thumbnail is nine
     * times smaller and the cache simply holds more of them.
     */
    private static final long MEMORY_THUMB_BYTES = 4L * 1024L * 1024L;
    /** Side at or above which a cover is "the big one" and gets its own slots. */
    private static final int BIG_SIDE_PX = 256;

    private final File directory;
    /** The player's covers: few, large, and re-read after every rotation. */
    private final DecodedMemory big = new DecodedMemory(MEMORY_SLOTS, Long.MAX_VALUE);
    /** List covers: many, small, bounded by bytes rather than by count. */
    private final DecodedMemory thumbs = new DecodedMemory(0, MEMORY_THUMB_BYTES);

    /**
     * Decoded bitmaps, insertion-ordered, trimmed by whichever bound applies.
     *
     * <p>Never {@code recycle()}s what it drops: a view may still be drawing the
     * bitmap it was handed, and a recycled bitmap drawn later is a hard crash. The
     * garbage collector is the thing that reclaims them.
     */
    private static final class DecodedMemory {
        private final int slots;
        private final long bytes;
        private long used;
        private final LinkedHashMap<String, android.graphics.Bitmap> entries =
                new LinkedHashMap<String, android.graphics.Bitmap>(16, 0.75f, true);

        DecodedMemory(int slots, long bytes) {
            this.slots = slots;
            this.bytes = bytes;
        }

        synchronized android.graphics.Bitmap get(String identity) {
            return identity == null ? null : entries.get(identity);
        }

        synchronized void put(String identity, android.graphics.Bitmap bitmap) {
            if (identity == null || bitmap == null) return;
            android.graphics.Bitmap previous = entries.put(identity, bitmap);
            if (previous != null) used -= byteSize(previous);
            used += byteSize(bitmap);
            trim();
        }

        synchronized void clear() {
            entries.clear();
            used = 0;
        }

        synchronized int size() { return entries.size(); }

        private void trim() {
            java.util.Iterator<Map.Entry<String, android.graphics.Bitmap>> it =
                    entries.entrySet().iterator();
            while (it.hasNext()
                    && (entries.size() > slots || (slots == 0 && used > bytes))) {
                Map.Entry<String, android.graphics.Bitmap> eldest = it.next();
                used -= byteSize(eldest.getValue());
                it.remove();
            }
        }

        private static long byteSize(android.graphics.Bitmap bitmap) {
            return (long) bitmap.getWidth() * bitmap.getHeight() * 4L;
        }
    }

    ArtworkCache(File directory) {
        this.directory = directory;
    }

    /** A decoded cover already in memory for {@code identity}, or {@code null}. */
    android.graphics.Bitmap memoryHit(String identity) {
        android.graphics.Bitmap hit = big.get(identity);
        return hit != null ? hit : thumbs.get(identity);
    }

    /**
     * Remembers {@code bitmap} for {@code identity}, evicting by whichever bound fits
     * its size.
     *
     * <p>Classified by the bitmap itself rather than by who asked for it, so a cover
     * decoded small and a cover decoded large cannot end up fighting over the same
     * slots and evicting each other.
     */
    void rememberDecoded(String identity, android.graphics.Bitmap bitmap) {
        if (identity == null || bitmap == null) return;
        if (Math.max(bitmap.getWidth(), bitmap.getHeight()) >= BIG_SIDE_PX) {
            big.put(identity, bitmap);
        } else {
            thumbs.put(identity, bitmap);
        }
    }

    /** Decoded thumbnails currently held, for the memory assertions. */
    int thumbnailCount() { return thumbs.size(); }

    /** Decoded player covers currently held. */
    int coverCount() { return big.size(); }

    /** Drops every decoded bitmap; called when the screen goes away. */
    void forgetDecoded() {
        big.clear();
        thumbs.clear();
    }

    File getOrDownload(String reference) {
        if (reference == null || !(reference.startsWith("https://") || reference.startsWith("http://"))) {
            return null;
        }
        File target = new File(directory, digest(reference) + ".image");
        if (target.isFile() && target.length() > 0 && target.length() <= MAX_IMAGE_BYTES
                && System.currentTimeMillis() - target.lastModified() <= TTL_MS) {
            target.setLastModified(System.currentTimeMillis());
            return target;
        }
        target.delete();
        HttpURLConnection connection = null;
        File temporary = new File(directory, target.getName() + ".part");
        try {
            if (!directory.isDirectory() && !directory.mkdirs()) return null;
            connection = (HttpURLConnection) new URL(reference).openConnection();
            connection.setConnectTimeout(5_000);
            connection.setReadTimeout(5_000);
            connection.setInstanceFollowRedirects(true);
            connection.setRequestProperty("User-Agent", "Tunefold/0.1 artwork-cache");
            int status = connection.getResponseCode();
            String type = connection.getContentType();
            if (status < 200 || status >= 300 || type == null || !type.toLowerCase().startsWith("image/")) {
                return null;
            }
            long declaredLength = connection.getContentLengthLong();
            if (declaredLength > MAX_IMAGE_BYTES) return null;
            long total = 0;
            try (InputStream input = connection.getInputStream();
                 FileOutputStream output = new FileOutputStream(temporary)) {
                byte[] buffer = new byte[16 * 1024];
                int count;
                while ((count = input.read(buffer)) != -1) {
                    total += count;
                    if (total > MAX_IMAGE_BYTES) return null;
                    output.write(buffer, 0, count);
                }
                output.getFD().sync();
            }
            if (total == 0 || !temporary.renameTo(target)) return null;
            evictOldFiles();
            return target;
        } catch (Exception failure) {
            Log.d("TunefoldCache", "Artwork fetch failed: " + failure.getClass().getSimpleName());
            return null;
        } finally {
            if (connection != null) connection.disconnect();
            temporary.delete();
        }
    }

    private void evictOldFiles() {
        File[] files = directory.listFiles(file -> file.isFile() && file.getName().endsWith(".image"));
        if (files == null) return;
        Arrays.sort(files, Comparator.comparingLong(File::lastModified));
        long total = 0;
        for (File file : files) total += file.length();
        for (File file : files) {
            if (total <= MAX_BYTES) break;
            total -= file.length();
            file.delete();
        }
    }

    private static String digest(String value) {
        try {
            byte[] hash = MessageDigest.getInstance("SHA-256").digest(value.getBytes(StandardCharsets.UTF_8));
            StringBuilder result = new StringBuilder(hash.length * 2);
            for (byte item : hash) result.append(String.format(java.util.Locale.ROOT, "%02x", item));
            return result.toString();
        } catch (Exception impossible) {
            throw new IllegalStateException("SHA-256 unavailable", impossible);
        }
    }
}
