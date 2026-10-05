package com.tunefold.app;

import android.app.Activity;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.view.ViewGroup;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.TextView;

import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

public final class MainActivity extends Activity implements AudioTrackOutput.Listener {
    static final int STATE_IDLE = 0;
    static final int STATE_BUFFERING = 1;
    static final int STATE_PLAYING = 2;
    static final int STATE_PAUSED = 3;
    static final int STATE_STOPPED = 4;
    static final int STATE_ERROR = 5;

    private final Handler mainHandler = new Handler(Looper.getMainLooper());
    private final ExecutorService commands = Executors.newSingleThreadExecutor();
    private volatile long engine;
    private volatile boolean destroyed;
    private EditText urlInput;
    private TextView stateText;
    private AudioTrackOutput output;
    private final float[] visualFeatures = new float[13];
    private final float[] waveformBars = new float[24];

    private final Runnable statePoll = new Runnable() {
        @Override public void run() {
            if (destroyed) return;
            if (engine != 0) {
                int state = TunefoldBridge.getPlaybackState(engine);
                int rate = TunefoldBridge.getSampleRate(engine);
                String label = stateLabel(state);
                if (rate > 0) label += " · " + rate + " Hz";
                TunefoldBridge.getVisualState(engine, visualFeatures, waveformBars);
                float peak = 0.0f;
                for (float bar : waveformBars) peak = Math.max(peak, bar);
                if (peak > 0.0f || visualFeatures[1] > 0.0f) {
                    label += " · analysis peak " + Math.round(peak * 1000.0f) + "‰";
                }
                String diagnostics = TunefoldBridge.getRuntimeDiagnostics(engine);
                if (diagnostics != null && !diagnostics.isEmpty()) {
                    label += "\n" + diagnostics;
                }
                if (state == STATE_ERROR) {
                    String error = TunefoldBridge.getLastError(engine);
                    if (error != null && !error.isEmpty()) label += " · " + error;
                    output.requestStop();
                }
                stateText.setText(label);
            }
            mainHandler.postDelayed(this, 300);
        }
    };

    @Override protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        buildMinimalHost();
        output = new AudioTrackOutput(this);
        stateText.setText("Initializing Rust engine…");
        commands.execute(() -> {
            long created;
            try {
                created = TunefoldBridge.createEngine();
            } catch (Throwable error) {
                String message = "JNI initialization failed: " + error;
                mainHandler.post(() -> {
                    if (!destroyed) stateText.setText(message);
                });
                return;
            }
            mainHandler.post(() -> {
                if (destroyed) {
                    // Initialization may finish while the Activity is shutting down.
                    new Thread(() -> TunefoldBridge.destroyEngine(created),
                            "tunefold-engine-cleanup").start();
                    return;
                }
                engine = created;
                stateText.setText(created == 0
                        ? "Rust engine initialization failed" : "Ready");
                if (created != 0 && getIntent().getBooleanExtra("runtime_smoke_test", false)) {
                    startPlayback();
                }
            });
        });
        mainHandler.post(statePoll);
    }

    private void buildMinimalHost() {
        int padding = (int) (20 * getResources().getDisplayMetrics().density);
        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setPadding(padding, padding, padding, padding);

        TextView title = new TextView(this);
        title.setText("Tunefold Android Runtime Test");
        title.setTextSize(20);
        root.addView(title, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));

        urlInput = new EditText(this);
        urlInput.setSingleLine(true);
        urlInput.setHint("HTTPS audio URL (AAC/M4A, FLAC, WAV, or Ogg Vorbis)");
        urlInput.setInputType( android.text.InputType.TYPE_CLASS_TEXT
                | android.text.InputType.TYPE_TEXT_VARIATION_URI);
        // Small real CC0 Vorbis sample used for the runtime smoke test.
        urlInput.setText("https://upload.wikimedia.org/wikipedia/commons/e/e3/Example_sound_file_in_Ogg_Vorbis_format.ogg");
        String runtimeUrl = getIntent().getStringExtra("runtime_stream_url");
        if (runtimeUrl != null) urlInput.setText(runtimeUrl);
        root.addView(urlInput, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));

        LinearLayout buttons = new LinearLayout(this);
        Button play = new Button(this);
        play.setText("Play URL");
        play.setOnClickListener(view -> startPlayback());
        buttons.addView(play, new LinearLayout.LayoutParams(0,
                ViewGroup.LayoutParams.WRAP_CONTENT, 1));

        Button pauseResume = new Button(this);
        pauseResume.setText("Pause / Resume");
        pauseResume.setOnClickListener(view -> togglePause());
        buttons.addView(pauseResume, new LinearLayout.LayoutParams(0,
                ViewGroup.LayoutParams.WRAP_CONTENT, 1));

        Button stop = new Button(this);
        stop.setText("Stop");
        stop.setOnClickListener(view -> stopPlayback());
        buttons.addView(stop, new LinearLayout.LayoutParams(0,
                ViewGroup.LayoutParams.WRAP_CONTENT, 1));
        root.addView(buttons);

        stateText = new TextView(this);
        stateText.setTextIsSelectable(true);
        stateText.setText("Initializing");
        root.addView(stateText, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        setContentView(root);
    }

    private void startPlayback() {
        String url = urlInput.getText().toString().trim();
        if (!(url.startsWith("https://") || url.startsWith("http://"))) {
            stateText.setText("Enter an http or https audio URL");
            return;
        }
        long activeEngine = engine;
        if (activeEngine == 0) {
            stateText.setText("Rust engine is unavailable");
            return;
        }
        stateText.setText("Opening stream");
        commands.execute(() -> {
            output.requestStop();
            output.awaitStopped();
            TunefoldBridge.stopAudio(activeEngine);
            if (!TunefoldBridge.playStream(activeEngine, url)) {
                postMessage(errorOrDefault(activeEngine, "Could not start stream"));
                return;
            }
            output.start(activeEngine);
        });
    }

    private void togglePause() {
        long activeEngine = engine;
        if (activeEngine == 0) return;
        commands.execute(() -> {
            int state = TunefoldBridge.getPlaybackState(activeEngine);
            if (state == STATE_PLAYING || state == STATE_BUFFERING) {
                TunefoldBridge.pauseAudio(activeEngine);
                output.pause();
            } else if (state == STATE_PAUSED) {
                TunefoldBridge.resumeAudio(activeEngine);
            }
        });
    }

    private void stopPlayback() {
        long activeEngine = engine;
        if (activeEngine == 0) return;
        output.requestStop();
        commands.execute(() -> {
            output.awaitStopped();
            TunefoldBridge.stopAudio(activeEngine);
            postMessage("Stopped");
        });
    }

    @Override public void onOutputMessage(String message) {
        postMessage(message);
    }

    private void postMessage(String message) {
        mainHandler.post(() -> {
            if (!destroyed && stateText != null) stateText.setText(message);
        });
    }

    private static String errorOrDefault(long handle, String fallback) {
        String error = TunefoldBridge.getLastError(handle);
        return error == null || error.isEmpty() ? fallback : error;
    }

    private static String stateLabel(int state) {
        switch (state) {
            case STATE_BUFFERING: return "Buffering";
            case STATE_PLAYING: return "Playing";
            case STATE_PAUSED: return "Paused";
            case STATE_STOPPED: return "Stopped";
            case STATE_ERROR: return "Error";
            default: return "Idle";
        }
    }

    @Override protected void onDestroy() {
        destroyed = true;
        mainHandler.removeCallbacks(statePoll);
        output.requestStop();
        long oldEngine = engine;
        engine = 0;
        if (oldEngine != 0) {
            commands.execute(() -> {
                output.awaitStopped();
                TunefoldBridge.stopAudio(oldEngine);
                TunefoldBridge.destroyEngine(oldEngine);
            });
        }
        commands.shutdown();
        super.onDestroy();
    }
}
