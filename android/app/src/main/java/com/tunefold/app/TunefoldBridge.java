package com.tunefold.app;

import java.nio.ByteBuffer;

final class TunefoldBridge {
    static {
        System.loadLibrary("tunefold");
    }

    private TunefoldBridge() {}

    static native long createEngine();
    static native void destroyEngine(long handle);
    static native boolean playStream(long handle, String url, String headersJson, long tapToDecoderUs);
    static native void pauseAudio(long handle);
    static native void resumeAudio(long handle);
    static native void stopAudio(long handle);
    static native int getPlaybackState(long handle);
    static native int getSampleRate(long handle);
    static native int getAvailableFrames(long handle);
    static native long getPositionMs(long handle);
    static native boolean isDecoderFinished(long handle);
    /**
     * True once when the last track reached its end of media; false for a
     * user-initiated stop. Take-once so one EOF cannot fire autoplay twice.
     */
    static native boolean takeTrackFinished(long handle);
    static native int readPcm(long handle, ByteBuffer destination, int maxFrames);
    static native void setOutputState(long handle, int state);
    static native String getLastError(long handle);
    static native void reportAudioError(long handle, String error);
    static native void getVisualState(long handle, float[] features, float[] bars);
    /**
     * Copies {@code columns} columns of the stereo track envelope into {@code out}.
     *
     * <p>Fills {@code out} with 4 floats per bar (left min/max, right min/max) of
     * the last measured window and returns the bus generation. The caller
     * compares the number against the one it already had: unchanged means no new
     * audio and nothing to repaint, which is what lets the surface keep
     * answering at 20 Hz while genuinely resting.
     */
    static native long readLiveWaveform(long handle, float[] out, int bars);
    static native String getRuntimeDiagnostics(long handle);
    static native String drainPlaybackTrace(long handle);

    /** Starts an explicit full-track download. Returns a download id (>0) or 0. */
    static native long startDownload(long handle, String directory, String provider,
                                     String trackId, String url, String headersJson);
    /** Progress as "received\ttotal" (total -1 while unknown), or null when finished. */
    static native String getDownloadProgress(long handle, long downloadId);
    static native boolean cancelDownload(long handle, long downloadId);
    static native int cleanAbandonedDownloads(String directory);
    /** Canonical path Rust uses for a downloaded track. */
    static native String localMediaPath(String directory, String provider, String trackId);

    static native boolean initializeYoutube(String cacheDir);
    static native String searchYoutube(String query, int limit);
    static native String resolveYoutubeUrl(String url);
    /**
     * Provider metadata for a video id, for a remembered track.
     *
     * <p>{@link #resolveYoutubeUrl} needs a link to parse; L1K3D holds identities and
     * {@code Descargadas} holds files, and neither holds a cover. This is the half that
     * turns either of those back into something that can be drawn and played.
     */
    static native String metadataYoutube(String videoId);
    /**
     * Synchronised lyrics as timed lines: {@code {"ok":true,"lines":[{"t":ms,"s":text}]}}.
     *
     * <p>LRCLIB, the same source the console uses, parsed by the same parser — so the
     * Android karaoke cannot disagree with the desktop one about which line is being
     * sung. A plain lyric is never mixed in: an untimed line cannot be the timed line.
     */
    static native String syncedLyrics(String title, String artist, int durationMs);
    static native String resolveYoutubeSource(String trackJson);
    /** Provider "related to this video" list, for Home and autoplay. */
    static native String relatedYoutube(String videoId, int limit);
}
