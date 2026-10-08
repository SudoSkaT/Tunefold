package com.tunefold.app;

import android.util.Log;

import org.json.JSONObject;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.Comparator;
import java.util.List;

/** Bounded, atomic local media store; callers opt in by supplying the bytes. */
final class FileLocalMediaStore implements LocalMediaStore {
    private static final long MAX_TOTAL_BYTES = 512L * 1024L * 1024L;
    private static final long MAX_ITEM_BYTES = 200L * 1024L * 1024L;
    private final File directory;

    FileLocalMediaStore(File directory) { this.directory = directory; }

    @Override public File directory() { return directory; }

    @Override public synchronized boolean contains(String provider, String id) {
        return get(provider, id) != null;
    }

    @Override public synchronized File get(String provider, String id) {
        File audio = audioFile(provider, id);
        File metadata = metadataFile(provider, id);
        if (audio == null || metadata == null || !audio.isFile()) return null;
        if (!metadata.isFile()) {
            audio.delete();
            return null;
        }
        try {
            JSONObject entry = new JSONObject(readText(metadata));
            if (!provider.equals(entry.optString("provider"))
                    || !id.equals(entry.optString("id"))
                    || entry.optLong("size", -1) != audio.length()
                    || audio.length() <= 0 || audio.length() > MAX_ITEM_BYTES) {
                remove(provider, id);
                return null;
            }
            audio.setLastModified(System.currentTimeMillis());
            return audio;
        } catch (Exception corrupt) {
            remove(provider, id);
            return null;
        }
    }

    /**
     * Enumerates every valid stored entry.
     *
     * <p>This is what makes `Descargadas` a derived view rather than a second
     * copy of the library: the playlist is exactly the set of entries the store
     * still considers valid, so removing a file removes the track from it and a
     * download never has to be "added" to it.
     *
     * <p>Validation is identical to {@link #get}, including pruning of corrupt or
     * inconsistent entries, so a half-written or orphaned file can never be
     * reported as downloaded.
     */
    @Override public synchronized List<Entry> entries() {
        File[] sidecars = directory.listFiles(file -> file.isFile() && file.getName().endsWith(".json"));
        if (sidecars == null) return Collections.emptyList();
        List<Entry> found = new ArrayList<>(sidecars.length);
        for (File sidecar : sidecars) {
            try {
                JSONObject entry = new JSONObject(readText(sidecar));
                String provider = entry.optString("provider", "");
                String id = entry.optString("id", "");
                TrackKey key = TrackKey.of(provider, id);
                if (key == null) { sidecar.delete(); continue; }
                File audio = audioFile(provider, id);
                if (audio == null || !audio.isFile()) { remove(provider, id); continue; }
                long declared = entry.optLong("size", -1);
                long actual = audio.length();
                if (declared != actual || actual <= 0 || actual > MAX_ITEM_BYTES) {
                    remove(provider, id);
                    continue;
                }
                String title = entry.optString("title", "").trim();
                found.add(new Entry(key, actual, audio, title.isEmpty() ? null : title));
            } catch (Exception corrupt) {
                sidecar.delete();
            }
        }
        // Deterministic order so the UI and its tests are reproducible.
        found.sort(Comparator.comparing(entry -> entry.key));
        return found;
    }

    @Override public synchronized boolean store(String provider, String id, InputStream audio) {
        if (provider == null || id == null || audio == null) return false;
        File target = audioFile(provider, id);
        File metadata = metadataFile(provider, id);
        if (target == null || metadata == null || !directory.isDirectory() && !directory.mkdirs()) return false;
        File part = new File(directory, target.getName() + ".part");
        File metadataPart = new File(directory, metadata.getName() + ".part");
        try {
            long size = 0;
            try (FileOutputStream output = new FileOutputStream(part)) {
                byte[] buffer = new byte[64 * 1024];
                int read;
                while ((read = audio.read(buffer)) != -1) {
                    size += read;
                    if (size > MAX_ITEM_BYTES) return false;
                    output.write(buffer, 0, read);
                }
                output.getFD().sync();
            }
            if (size == 0) return false;
            JSONObject entry = new JSONObject();
            entry.put("provider", provider);
            entry.put("id", id);
            entry.put("size", size);
            // Remembered so `Descargadas` can show a real title after a restart; a
            // blank title is left out entirely rather than stored as an empty string.
            if (title != null && !title.trim().isEmpty()) entry.put("title", title.trim());
            entry.put("updated_at_ms", System.currentTimeMillis());
            try (FileOutputStream output = new FileOutputStream(metadataPart)) {
                output.write(entry.toString().getBytes(StandardCharsets.UTF_8));
                output.getFD().sync();
            }
            if (!part.renameTo(target)) return false;
            if (!metadataPart.renameTo(metadata)) {
                target.delete();
                return false;
            }
            evictOldEntries();
            return true;
        } catch (Exception failure) {
            Log.d("TunefoldCache", "Local media store failed: " + failure.getClass().getSimpleName());
            return false;
        } finally {
            part.delete();
            metadataPart.delete();
        }
    }

    @Override
    public synchronized boolean register(String provider, String id, File path, String title) {
        if (provider == null || provider.isEmpty() || id == null || id.isEmpty()) return false;
        if (path == null || !path.isFile()) return false;
        File target = audioFile(provider, id);
        File metadata = metadataFile(provider, id);
        if (target == null || metadata == null) return false;
        long size = path.length();
        if (size <= 0 || size > MAX_ITEM_BYTES) return false;
        if (!directory.isDirectory() && !directory.mkdirs()) return false;
        File metadataPart = new File(directory, metadata.getName() + ".part");
        try {
            if (!path.getCanonicalFile().equals(target.getCanonicalFile())) {
                // The downloader must write exactly where the store looks; this
                // guards against registering a file the store does not own.
                return false;
            }
            JSONObject entry = new JSONObject();
            entry.put("provider", provider);
            entry.put("id", id);
            entry.put("size", size);
            entry.put("updated_at_ms", System.currentTimeMillis());
            try (FileOutputStream output = new FileOutputStream(metadataPart)) {
                output.write(entry.toString().getBytes(StandardCharsets.UTF_8));
                output.getFD().sync();
            }
            if (!metadataPart.renameTo(metadata)) return false;
            evictOldEntries();
            return true;
        } catch (Exception failure) {
            Log.d("TunefoldCache",
                    "Local media register failed: " + failure.getClass().getSimpleName());
            return false;
        } finally {
            metadataPart.delete();
        }
    }

    @Override public synchronized boolean remove(String provider, String id) {
        File audio = audioFile(provider, id);
        File metadata = metadataFile(provider, id);
        return (audio == null || !audio.exists() || audio.delete())
                && (metadata == null || !metadata.exists() || metadata.delete());
    }

    private File audioFile(String provider, String id) {
        String key = digest(provider, id);
        return key == null ? null : new File(directory, key + ".audio");
    }

    private File metadataFile(String provider, String id) {
        String key = digest(provider, id);
        return key == null ? null : new File(directory, key + ".json");
    }

    private void evictOldEntries() {
        File[] files = directory.listFiles(file -> file.isFile() && file.getName().endsWith(".audio"));
        if (files == null) return;
        Arrays.sort(files, Comparator.comparingLong(File::lastModified));
        long bytes = 0;
        for (File file : files) bytes += file.length();
        for (File file : files) {
            if (bytes <= MAX_TOTAL_BYTES) break;
            bytes -= file.length();
            String name = file.getName();
            file.delete();
            new File(directory, name.substring(0, name.length() - 6) + ".json").delete();
        }
    }

    private static String readText(File file) throws Exception {
        byte[] bytes = new byte[(int) Math.min(file.length(), 64 * 1024)];
        try (FileInputStream input = new FileInputStream(file)) {
            int offset = 0;
            while (offset < bytes.length) {
                int count = input.read(bytes, offset, bytes.length - offset);
                if (count < 0) break;
                offset += count;
            }
            return new String(bytes, 0, offset, StandardCharsets.UTF_8);
        }
    }

    private static String digest(String provider, String id) {
        if (provider == null || provider.isEmpty() || id == null || id.isEmpty()) return null;
        try {
            byte[] input = (provider + "\u0000" + id).getBytes(StandardCharsets.UTF_8);
            byte[] hash = MessageDigest.getInstance("SHA-256").digest(input);
            StringBuilder result = new StringBuilder(hash.length * 2);
            for (byte value : hash) result.append(String.format(java.util.Locale.ROOT, "%02x", value));
            return result.toString();
        } catch (Exception impossible) {
            return null;
        }
    }
}
