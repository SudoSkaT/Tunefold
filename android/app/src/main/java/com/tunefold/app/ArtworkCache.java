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

/** Bounded image-only cache. Runs on ProviderRegistry's artwork executor, never the decoder. */
final class ArtworkCache {
    private static final long MAX_BYTES = 128L * 1024L * 1024L;
    private static final long MAX_IMAGE_BYTES = 10L * 1024L * 1024L;
    private static final long TTL_MS = 30L * 24L * 60L * 60L * 1000L;
    private final File directory;

    ArtworkCache(File directory) {
        this.directory = directory;
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
