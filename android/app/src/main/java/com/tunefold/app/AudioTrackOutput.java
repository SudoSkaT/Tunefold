package com.tunefold.app;

import android.media.AudioAttributes;
import android.media.AudioFormat;
import android.media.AudioTrack;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;

final class AudioTrackOutput {
    interface Listener {
        void onOutputMessage(String message);
    }

    private static final int CHUNK_FRAMES = 1024;
    private final Listener listener;
    private volatile boolean running;
    private volatile AudioTrack track;
    private volatile Thread worker;

    AudioTrackOutput(Listener listener) {
        this.listener = listener;
    }

    void start(long engine) {
        start(engine, null);
    }

    void start(long engine, PlaybackTrace trace) {
        requestStop();
        joinWorker(0);
        synchronized (this) {
            running = true;
            worker = new Thread(() -> pump(engine, trace), "tunefold-audio-output");
            worker.start();
        }
    }

    void pause() {
        AudioTrack current = track;
        if (current != null && current.getPlayState() == AudioTrack.PLAYSTATE_PLAYING) {
            current.pause();
        }
    }

    void requestStop() {
        running = false;
        Thread currentWorker = worker;
        if (currentWorker != null) {
            currentWorker.interrupt();
        }
        AudioTrack currentTrack = track;
        if (currentTrack != null) {
            try {
                currentTrack.pause();
                currentTrack.flush();
                currentTrack.stop();
            } catch (IllegalStateException ignored) {
                // The output thread releases a track that failed to initialize.
            }
        }
    }

    void awaitStopped() {
        joinWorker(0);
    }

    private void joinWorker(long timeoutMs) {
        Thread current = worker;
        if (current == null || current == Thread.currentThread()) return;
        try {
            if (timeoutMs == 0) current.join();
            else current.join(timeoutMs);
        } catch (InterruptedException interrupted) {
            Thread.currentThread().interrupt();
        }
        if (!current.isAlive()) worker = null;
    }

    private void pump(long engine, PlaybackTrace trace) {
        AudioTrack localTrack = null;
        boolean firstWriteMarked = false;
        ByteBuffer pcm = ByteBuffer.allocateDirect(CHUNK_FRAMES * 2 * Float.BYTES)
                .order(ByteOrder.nativeOrder());
        try {
            while (running) {
                int state = TunefoldBridge.getPlaybackState(engine);
                if (state == PlaybackController.ERROR) {
                    message(TunefoldBridge.getLastError(engine));
                    return;
                }

                int sampleRate = TunefoldBridge.getSampleRate(engine);
                int available = TunefoldBridge.getAvailableFrames(engine);
                if (sampleRate <= 0 || available <= 0) {
                    if (TunefoldBridge.isDecoderFinished(engine)) {
                        if (state == PlaybackController.LOADING) {
                            message(nonEmptyError(engine, "Decoder finished before audio output started"));
                        } else {
                            TunefoldBridge.setOutputState(engine, PlaybackController.STOPPED);
                        }
                        return;
                    }
                    sleepBriefly();
                    continue;
                }

                if (localTrack == null) {
                    localTrack = createTrack(sampleRate);
                    track = localTrack;
                    localTrack.play();
                    TunefoldBridge.setOutputState(engine, PlaybackController.PLAYING);
                    if (trace != null) trace.mark(PlaybackTrace.AUDIOTRACK_START,
                            "sample_rate=" + sampleRate);
                    message("AudioTrack started at " + sampleRate + " Hz");
                }

                state = TunefoldBridge.getPlaybackState(engine);
                if (state == PlaybackController.PAUSED) {
                    if (localTrack.getPlayState() == AudioTrack.PLAYSTATE_PLAYING) {
                        localTrack.pause();
                    }
                    sleepBriefly();
                    continue;
                }
                if (state == PlaybackController.LOADING
                        && localTrack.getPlayState() != AudioTrack.PLAYSTATE_PLAYING) {
                    localTrack.play();
                    TunefoldBridge.setOutputState(engine, PlaybackController.PLAYING);
                }

                pcm.clear();
                int frames = TunefoldBridge.readPcm(engine, pcm, Math.min(CHUNK_FRAMES, available));
                if (frames <= 0) {
                    sleepBriefly();
                    continue;
                }
                int bytes = frames * 2 * Float.BYTES;
                pcm.position(0);
                pcm.limit(bytes);
                while (running && pcm.hasRemaining()) {
                    int written = localTrack.write(pcm, pcm.remaining(), AudioTrack.WRITE_BLOCKING);
                    if (!firstWriteMarked && written > 0) {
                        firstWriteMarked = true;
                        if (trace != null) trace.markFirstPositiveWrite(written);
                    }
                    if (written < 0) {
                        throw new IllegalStateException("AudioTrack.write failed: " + written);
                    }
                }
            }
        } catch (Exception error) {
            if (running) {
                TunefoldBridge.reportAudioError(engine, error.toString());
                message(error.toString());
            }
        } finally {
            track = null;
            if (localTrack != null) {
                try {
                    if (localTrack.getPlayState() == AudioTrack.PLAYSTATE_PLAYING) {
                        localTrack.stop();
                    }
                } catch (IllegalStateException ignored) {
                    // Release is still required after a failed output transition.
                }
                localTrack.release();
            }
            running = false;
            synchronized (this) {
                if (worker == Thread.currentThread()) worker = null;
                notifyAll();
            }
        }
    }

    private static AudioTrack createTrack(int sampleRate) {
        int minBytes = AudioTrack.getMinBufferSize(
                sampleRate,
                AudioFormat.CHANNEL_OUT_STEREO,
                AudioFormat.ENCODING_PCM_FLOAT);
        if (minBytes <= 0) {
            throw new IllegalStateException("Unsupported AudioTrack format at " + sampleRate + " Hz");
        }
        int requestedBytes = Math.max(minBytes, sampleRate * 2 * Float.BYTES / 5);
        AudioFormat format = new AudioFormat.Builder()
                .setSampleRate(sampleRate)
                .setChannelMask(AudioFormat.CHANNEL_OUT_STEREO)
                .setEncoding(AudioFormat.ENCODING_PCM_FLOAT)
                .build();
        AudioAttributes attributes = new AudioAttributes.Builder()
                .setUsage(AudioAttributes.USAGE_MEDIA)
                .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                .build();
        AudioTrack result = new AudioTrack.Builder()
                .setAudioAttributes(attributes)
                .setAudioFormat(format)
                .setBufferSizeInBytes(requestedBytes)
                .setTransferMode(AudioTrack.MODE_STREAM)
                .build();
        if (result.getState() != AudioTrack.STATE_INITIALIZED) {
            result.release();
            throw new IllegalStateException("AudioTrack initialization failed");
        }
        return result;
    }

    private void message(String text) {
        if (listener != null && text != null && !text.isEmpty()) listener.onOutputMessage(text);
    }

    private static String nonEmptyError(long engine, String fallback) {
        String error = TunefoldBridge.getLastError(engine);
        return error == null || error.isEmpty() ? fallback : error;
    }

    private static void sleepBriefly() {
        try {
            Thread.sleep(8);
        } catch (InterruptedException interrupted) {
            Thread.currentThread().interrupt();
        }
    }
}
