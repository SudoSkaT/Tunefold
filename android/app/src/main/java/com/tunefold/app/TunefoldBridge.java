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
    static native String resolveYoutubeSource(String trackJson);
    /** Provider "related to this video" list, for Home and autoplay. */
    static native String relatedYoutube(String videoId, int limit);
}
