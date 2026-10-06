package com.tunefold.app;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Set;

/**
 * Persistent user library: L1K3D membership and a short recently-played memory.
 *
 * <p><b>L1K3D</b> is membership of identities, never audio. Adding a track to
 * L1K3D and downloading it are independent dimensions: a track can be liked
 * without a file, downloaded without being liked, or both — and there is still
 * exactly one file per track.
 *
 * <p>Persistence is a small JSON document written atomically. A library of
 * hundreds of tracks is far below the point where a database would help, and
 * staying on plain files keeps the storage semantics identical to the rest of
 * the app (see {@code FileLocalMediaStore}).
 *
 * <p>All writes happen off the UI thread: callers hand in a directory and this
 * class performs its I/O on its own single executor, publishing the resulting
 * state back on the main thread.
 */
final class Library {
    /** How many tracks are remembered as recently played. */
    private static final int RECENT_LIMIT = 20;
    private static final String FILE_NAME = "library.json";
    /**
     * Field separator inside a persisted record.
     *
     * <p>Deliberately a printable character that can never appear in a provider
     * or a provider track id. A control character (U+001F and friends) would be
     * silently eaten by {@code String.trim()}, corrupting every record.
     */
    private static final char SEPARATOR = '|';

    interface Observer { void onLibraryChanged(); }

    private final File file;
    private final Object lock = new Object();
    /** Insertion-ordered so listings are deterministic. */
    private final LinkedHashSet<TrackKey> liked = new LinkedHashSet<>();
    private final LinkedHashSet<TrackKey> recent = new LinkedHashSet<>();
    private volatile Observer observer;
    private volatile boolean loaded;

    Library(File directory) {
        this.file = new File(directory, FILE_NAME);
    }

    void setObserver(Observer observer) { this.observer = observer; }

    // ------------------------------------------------------------ membership

    boolean isLiked(TrackKey key) {
        if (key == null) return false;
        synchronized (lock) { return liked.contains(key); }
    }

    boolean isLiked(MediaTrack track) { return isLiked(TrackKey.of(track)); }

    /**
     * Adds or removes L1K3D membership for a track.
     *
     * @return {@code true} when the track is liked afterwards.
     */
    boolean setLiked(MediaTrack track, boolean value) {
        TrackKey key = TrackKey.of(track);
        if (key == null) return false;
        boolean changed;
        synchronized (lock) {
            changed = value ? liked.add(key) : liked.remove(key);
        }
        if (changed) {
            persistAsync();
            notifyChanged();
        }
        return value;
    }

    /** Toggles L1K3D membership, returning the resulting state. */
    boolean toggleLiked(MediaTrack track) {
        TrackKey key = TrackKey.of(track);
        if (key == null) return false;
        boolean nowLiked;
        synchronized (lock) {
            nowLiked = !liked.contains(key);
            if (nowLiked) liked.add(key); else liked.remove(key);
        }
        persistAsync();
        notifyChanged();
        return nowLiked;
    }

    /** Number of L1K3D members. */
    int likedCount() {
        synchronized (lock) { return liked.size(); }
    }

    /** Whether L1K3D holds any track. */
    boolean isEmpty() {
        synchronized (lock) { return liked.isEmpty(); }
    }

    /** Every liked identity, in insertion order. */
    List<TrackKey> likedKeys() {
        synchronized (lock) { return new ArrayList<>(liked); }
    }

    // -------------------------------------------------------------- recents

    /** Records that a track was played, most recent first. */
    void markPlayed(TrackKey key) {
        if (key == null) return;
        synchronized (lock) {
            recent.remove(key);
            recent.add(key);
            while (recent.size() > RECENT_LIMIT) {
                TrackKey oldest = recent.iterator().next();
                recent.remove(oldest);
            }
        }
        persistAsync();
    }

    boolean wasPlayedRecently(TrackKey key) {
        if (key == null) return false;
        synchronized (lock) { return recent.contains(key); }
    }

    /**
     * Recently played identities, newest first.
     *
     * <p>{@code recent} is insertion-ordered so eviction can drop the oldest
     * cheaply; the caller-facing view is reversed because "what did I just hear"
     * reads newest-first.
     */
    List<TrackKey> recentlyPlayed() {
        synchronized (lock) {
            List<TrackKey> newestFirst = new ArrayList<>(recent);
            java.util.Collections.reverse(newestFirst);
            return newestFirst;
        }
    }

    /**
     * Every remembered identity: L1K3D plus recents.
     *
     * <p>Used to keep a recommendation from replaying something the user just
     * heard or already owns.
     */
    Set<TrackKey> knownIdentities() {
        synchronized (lock) {
            Set<TrackKey> all = new LinkedHashSet<>(liked);
            all.addAll(recent);
            return all;
        }
    }

    // ----------------------------------------------------------- persistence

    /** Reads the library from disk. Safe to call more than once. */
    void load() {
        synchronized (lock) {
            if (loaded) return;
            loaded = true;
            try {
                if (file.isFile()) parse(readAll(file));
            } catch (Exception unreadable) {
                // A corrupt library must not break the app: start empty rather
                // than crash. The next write repairs the file.
                android.util.Log.w("Tunefold", "library unreadable: " + unreadable);
            }
        }
    }

    /**
     * Parses the on-disk representation into the in-memory sets.
     *
     * <p>Format: one record per line, {@code <l|r>|<provider>|<id>}, where the
     * leading letter says whether the line is L1K3D membership or a
     * recently-played entry. Unparseable lines are ignored rather than fatal, so
     * one bad record cannot cost the user their whole library.
     */
    private void parse(String text) {
        for (String line : text.split("\n")) {
            // No trim(): the separator and the id must survive verbatim.
            if (line.length() < 4 || line.charAt(1) != SEPARATOR) continue;
            char kind = line.charAt(0);
            String rest = line.substring(2);
            int separator = rest.indexOf(SEPARATOR);
            if (separator <= 0 || separator == rest.length() - 1) continue;
            TrackKey key = TrackKey.of(rest.substring(0, separator), rest.substring(separator + 1));
            if (key == null) continue;
            if (kind == 'l') liked.add(key); else if (kind == 'r') recent.add(key);
        }
    }

    private void persist() { writeAtomically(serialize()); }

    /**
     * Persists off the UI thread.
     *
     * <p>Library writes are triggered by taps, so doing the I/O inline would put
     * a filesystem write on the UI thread. Writes are serialized through a
     * single executor to keep file order deterministic.
     */
    private void persistAsync() {
        final String text = serialize();
        WRITES.execute(() -> writeAtomically(text));
    }

    /**
     * Test hook: blocks until every queued write has reached the disk.
     *
     * <p>Writes are asynchronous by design so a like never blocks the UI, which
     * means a test cannot assert on the file immediately after toggling. The
     * wait is submitted to the same single-threaded executor, so it can only run
     * after all previously queued writes.
     */
    void awaitWritesForTest() {
        final java.util.concurrent.CountDownLatch done =
                new java.util.concurrent.CountDownLatch(1);
        WRITES.execute(done::countDown);
        try {
            done.await(10, java.util.concurrent.TimeUnit.SECONDS);
        } catch (InterruptedException interrupted) {
            Thread.currentThread().interrupt();
        }
    }

    private static final java.util.concurrent.ExecutorService WRITES =
            java.util.concurrent.Executors.newSingleThreadExecutor(r -> {
                Thread thread = new Thread(r, "tunefold-library");
                thread.setDaemon(true);
                return thread;
            });

    /** Writes via a temporary file and a rename so a crash cannot truncate it. */
    private void writeAtomically(String text) {
        File parent = file.getParentFile();
        if (parent != null && !parent.isDirectory() && !parent.mkdirs()) return;
        File temporary = new File(parent, file.getName() + ".part");
        try {
            try (FileOutputStream output = new FileOutputStream(temporary)) {
                output.write(text.getBytes(StandardCharsets.UTF_8));
                output.getFD().sync();
            }
            if (!temporary.renameTo(file)) {
                // renameTo will not overwrite on some filesystems.
                file.delete();
                if (!temporary.renameTo(file)) return;
            }
        } catch (Exception failure) {
            android.util.Log.w("Tunefold", "library save failed: " + failure);
            temporary.delete();
        }
    }

    private static String readAll(File source) throws IOException {
        byte[] bytes = new byte[(int) Math.min(source.length(), 4L * 1024 * 1024)];
        try (FileInputStream input = new FileInputStream(source)) {
            int offset = 0;
            while (offset < bytes.length) {
                int count = input.read(bytes, offset, bytes.length - offset);
                if (count < 0) break;
                offset += count;
            }
            return new String(bytes, 0, offset, StandardCharsets.UTF_8);
        }
    }

    private void notifyChanged() {
        Observer current = observer;
        if (current == null) return;
        MAIN.post(current::onLibraryChanged);
    }

    private static final android.os.Handler MAIN =
            new android.os.Handler(android.os.Looper.getMainLooper());

    /** Test hook: replaces the in-memory state. */
    void restoreForTest(List<TrackKey> likedKeys, List<TrackKey> recentKeys) {
        synchronized (lock) {
            liked.clear();
            recent.clear();
            liked.addAll(likedKeys);
            recent.addAll(recentKeys);
            loaded = true;
        }
    }

    /** The persisted representation; the single writer and parser agree on it. */
    String serialize() {
        StringBuilder text = new StringBuilder();
        synchronized (lock) {
            for (TrackKey key : liked) {
                text.append('l').append(SEPARATOR).append(key.provider()).append(SEPARATOR)
                        .append(key.providerTrackId()).append('\n');
            }
            for (TrackKey key : recent) {
                text.append('r').append(SEPARATOR).append(key.provider()).append(SEPARATOR)
                        .append(key.providerTrackId()).append('\n');
            }
        }
        return text.toString();
    }

    /** Test hook: parses a serialized representation into a fresh library. */
    void loadFromForTest(String text) {
        synchronized (lock) {
            liked.clear();
            recent.clear();
            loaded = true;
            // Delegates to the one parser so tests cannot drift from disk.
            parse(text);
        }
    }

}