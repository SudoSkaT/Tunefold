package com.tunefold.app;

/**
 * Observable download state for one track identity (§7).
 *
 * <p>A track has exactly one download state at a time, keyed by identity. That
 * is what makes §6 hold by construction: a track present in L1K3D, in
 * `Descargadas` and in the queue is still one {@code TrackKey}, so there is one
 * state, one job and at most one file.
 *
 * <p>{@link #progressPercent()} returns {@code -1} rather than a fabricated
 * number when the total size is unknown: progress becomes indeterminate instead
 * of lying (§7).
 */
final class DownloadState {
    enum Phase {
        /** Nothing requested. */
        IDLE,
        /** Resolving a fresh playable source before any bytes move. */
        RESOLVING_SOURCE,
        /** Bytes are being written to the {@code .part} file. */
        DOWNLOADING,
        /** Committed atomically and registered in the LocalMediaStore. */
        COMPLETED,
        /** Cancelled by the user; no file exists. */
        CANCELLED,
        /** Failed; {@link #error} explains why and {@link #isRetryable()} whether to retry. */
        FAILED,
        /** The track already had a valid local file, so nothing was started. */
        ALREADY_DOWNLOADED
    }

    final TrackKey key;
    final Phase phase;
    final long bytesReceived;
    /** {@code -1} while the total size is unknown. */
    final long totalBytes;
    /** Bytes per second, or {@code -1} when it cannot be derived yet. */
    final long bytesPerSecond;
    final String error;

    DownloadState(TrackKey key, Phase phase, long bytesReceived, long totalBytes,
                  long bytesPerSecond, String error) {
        this.key = key;
        this.phase = phase == null ? Phase.IDLE : phase;
        this.bytesReceived = Math.max(0, bytesReceived);
        this.totalBytes = totalBytes;
        this.bytesPerSecond = bytesPerSecond;
        this.error = error == null ? "" : error;
    }

    static DownloadState idle(TrackKey key) {
        return new DownloadState(key, Phase.IDLE, 0, -1, -1, "");
    }

    /** The store already holds a valid file for this identity. */
    static DownloadState downloaded(TrackKey key, long size) {
        return new DownloadState(key, Phase.COMPLETED, size, size, -1, "");
    }

    /**
     * 0..100 when the total is known, otherwise {@code -1}.
     *
     * <p>Never returns a made-up percentage: an unknown total must render as
     * indeterminate progress.
     */
    int progressPercent() {
        if (totalBytes <= 0) return -1;
        return (int) Math.min(100L, bytesReceived * 100L / totalBytes);
    }

    boolean isTerminal() {
        return phase == Phase.COMPLETED || phase == Phase.CANCELLED
                || phase == Phase.FAILED || phase == Phase.ALREADY_DOWNLOADED;
    }

    /** A download is in flight and must not be started again. */
    boolean isActive() { return phase == Phase.DOWNLOADING || phase == Phase.RESOLVING_SOURCE; }

    boolean isDownloaded() {
        return phase == Phase.COMPLETED || phase == Phase.ALREADY_DOWNLOADED;
    }

    boolean isRetryable() { return phase == Phase.FAILED || phase == Phase.CANCELLED; }

    /** Short label for the UI. */
    String label() {
        switch (phase) {
            case RESOLVING_SOURCE: return "Resolving source";
            case DOWNLOADING: {
                int percent = progressPercent();
                return percent >= 0 ? "Downloading " + percent + "%" : "Downloading";
            }
            case COMPLETED: return "Downloaded";
            case ALREADY_DOWNLOADED: return "Downloaded";
            case CANCELLED: return "Download cancelled";
            case FAILED: return "Download failed";
            default: return "Download";
        }
    }

    @Override public String toString() {
        return phase + "(" + bytesReceived + "/" + (totalBytes < 0 ? "?" : totalBytes) + ")";
    }
}