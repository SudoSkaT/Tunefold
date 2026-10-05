package com.tunefold.app;

import java.nio.ByteBuffer;

final class TunefoldBridge {
    static {
        System.loadLibrary("tunefold");
    }

    private TunefoldBridge() {}

    static native long createEngine();
    static native void destroyEngine(long handle);
    static native boolean playStream(long handle, String url);
    static native void pauseAudio(long handle);
    static native void resumeAudio(long handle);
    static native void stopAudio(long handle);
    static native int getPlaybackState(long handle);
    static native int getSampleRate(long handle);
    static native int getAvailableFrames(long handle);
    static native boolean isDecoderFinished(long handle);
    static native int readPcm(long handle, ByteBuffer destination, int maxFrames);
    static native void setOutputState(long handle, int state);
    static native String getLastError(long handle);
    static native void reportAudioError(long handle, String error);
    static native void getVisualState(long handle, float[] features, float[] bars);
    static native String getRuntimeDiagnostics(long handle);
}
