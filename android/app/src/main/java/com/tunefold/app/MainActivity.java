package com.tunefold.app;

import android.app.Activity;
import android.content.ComponentName;
import android.content.Intent;
import android.content.ServiceConnection;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.widget.Button;
import android.widget.EditText;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.TextView;

import java.util.List;

public final class MainActivity extends Activity {
    private final Handler mainHandler = new Handler(Looper.getMainLooper());
    private PlaybackController controller;
    private ProviderRegistry providers;
    private boolean bound;
    private boolean destroyed;
    private EditText urlInput;
    private TextView stateText;
    private ImageView artworkView;
    private LinearLayout results;
    private String providerMessage = "";
    private String activeArtworkId = "";

    private final ServiceConnection connection = new ServiceConnection() {
        @Override public void onServiceConnected(ComponentName name, IBinder service) {
            controller = ((ForegroundPlaybackService.LocalBinder) service).controller();
            providers = ((ForegroundPlaybackService.LocalBinder) service).providers();
            if (getIntent().getBooleanExtra("runtime_smoke_test", false)) resolveAndPlay();
        }
        @Override public void onServiceDisconnected(ComponentName name) { controller = null; providers = null; }
    };

    private final Runnable statePoll = new Runnable() {
        @Override public void run() {
            if (destroyed) return;
            PlaybackController active = controller;
            if (active != null) {
                active.refresh();
                String label = stateLabel(active.state());
                String details = active.error();
                if (details != null && !details.isEmpty()) label += " · " + details;
                if (!providerMessage.isEmpty()) label += "\n" + providerMessage;
                String diagnostics = active.diagnostics();
                if (diagnostics != null && !diagnostics.isEmpty()) label += "\n" + diagnostics;
                stateText.setText(label);
            }
            mainHandler.postDelayed(this, 400);
        }
    };

    @Override protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        buildMinimalHost();
        Intent service = new Intent(this, ForegroundPlaybackService.class);
        bound = bindService(service, connection, BIND_AUTO_CREATE);
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
        root.addView(title, new LinearLayout.LayoutParams(-1, -2));
        artworkView = new ImageView(this);
        artworkView.setScaleType(ImageView.ScaleType.CENTER_CROP);
        int artworkSize = (int) (112 * getResources().getDisplayMetrics().density);
        root.addView(artworkView, new LinearLayout.LayoutParams(artworkSize, artworkSize));
        urlInput = new EditText(this);
        urlInput.setSingleLine(true);
        urlInput.setHint("YouTube URL or search query");
        urlInput.setInputType(android.text.InputType.TYPE_CLASS_TEXT
                | android.text.InputType.TYPE_TEXT_VARIATION_URI);
        urlInput.setText("https://www.youtube.com/watch?v=");
        String runtimeUrl = getIntent().getStringExtra("runtime_stream_url");
        if (runtimeUrl != null) urlInput.setText(runtimeUrl);
        root.addView(urlInput, new LinearLayout.LayoutParams(-1, -2));
        LinearLayout buttons = new LinearLayout(this);
        Button search = new Button(this);
        search.setText("Search / Play URL");
        search.setOnClickListener(view -> searchOrResolve());
        buttons.addView(search, new LinearLayout.LayoutParams(0, -2, 1));
        Button pauseResume = new Button(this);
        pauseResume.setText("Pause / Resume");
        pauseResume.setOnClickListener(view -> { if (controller != null) controller.togglePause(); });
        buttons.addView(pauseResume, new LinearLayout.LayoutParams(0, -2, 1));
        Button stop = new Button(this);
        stop.setText("Stop");
        stop.setOnClickListener(view -> { if (controller != null) controller.stop(); });
        buttons.addView(stop, new LinearLayout.LayoutParams(0, -2, 1));
        root.addView(buttons);
        results = new LinearLayout(this);
        results.setOrientation(LinearLayout.VERTICAL);
        root.addView(results, new LinearLayout.LayoutParams(-1, -2));
        stateText = new TextView(this);
        stateText.setTextIsSelectable(true);
        stateText.setText("Connecting to playback service…");
        root.addView(stateText, new LinearLayout.LayoutParams(-1, -2));
        setContentView(root);
    }

    private void searchOrResolve() {
        String query = urlInput.getText().toString().trim();
        if (providers == null) { stateText.setText("Provider service is not connected"); return; }
        if (query.contains("youtube.com/") || query.contains("youtu.be/")) {
            resolve(query, new PlaybackTrace("url"));
        } else if (query.startsWith("https://") || query.startsWith("http://")) {
            playDirectUrl(query);
        } else {
            providerMessage = "Searching YouTube…";
            stateText.setText("Searching YouTube…");
            providers.search(query, (tracks, error) -> {
                results.removeAllViews();
                if (error != null) { providerMessage = error; stateText.setText(error); return; }
                providerMessage = "";
                showTracks(tracks);
            });
        }
    }

    private void resolveAndPlay() {
        String url = getIntent().getStringExtra("runtime_stream_url");
        if (url == null) return;
        if (url.contains("youtube.com/") || url.contains("youtu.be/")) {
            resolve(url, new PlaybackTrace("url"));
        }
        else playDirectUrl(url);
    }

    private void playDirectUrl(String url) {
        if (controller == null) { stateText.setText("Playback service is not connected"); return; }
        Intent service = new Intent(this, ForegroundPlaybackService.class);
        if (Build.VERSION.SDK_INT >= 26) startForegroundService(service);
        else startService(service);
        providerMessage = "Playing direct HTTP audio source";
        PlaybackTrace trace = new PlaybackTrace("direct-http");
        trace.mark("T1_CONTROLLER_RECEIVED_PLAY");
        controller.play(new PlayableSource(url, "[]"), trace);
    }

    private void resolve(String url, PlaybackTrace trace) {
        providerMessage = "Resolving YouTube metadata…";
        stateText.setText("Resolving YouTube metadata…");
        if (controller == null) { stateText.setText("Playback service is not connected"); return; }
        startPlaybackService();
        controller.playUrl(url, providers, trace, (track, error) -> {
            if (error != null) { providerMessage = error; stateText.setText(error); return; }
            providerMessage = "Metadata: " + track.title
                    + (track.artist.isEmpty() ? "" : " — " + track.artist);
            showTrackArtwork(track);
            showTracks(java.util.Collections.singletonList(track));
        });
    }

    private void showTracks(List<MediaTrack> tracks) {
        results.removeAllViews();
        for (MediaTrack track : tracks) {
            Button row = new Button(this);
            row.setText(track.displayText());
            row.setOnClickListener(view -> playTrack(track));
            results.addView(row, new LinearLayout.LayoutParams(-1, -2));
        }
        if (tracks.isEmpty()) providerMessage = "No results";
        else if (providerMessage.isEmpty()) providerMessage = tracks.size() + " result(s)";
        stateText.setText(stateLabel(controller == null ? PlaybackController.IDLE : controller.state())
                + "\n" + providerMessage);
    }

    private void playTrack(MediaTrack track) {
        if (controller == null || providers == null) return;
        PlaybackTrace trace = new PlaybackTrace(track.providerId);
        showTrackArtwork(track);
        providerMessage = "Resolving playable source for " + track.title;
        stateText.setText("Resolving playable source: " + track.displayText());
        startPlaybackService();
        controller.playTrack(track, providers, trace, (resolvedTrack, error) -> {
            if (error != null) { providerMessage = "Source error: " + error; stateText.setText(error); return; }
            providerMessage = "Source resolved; starting decoder for " + track.displayText();
            stateText.setText(providerMessage);
        });
    }

    private void startPlaybackService() {
        Intent service = new Intent(this, ForegroundPlaybackService.class);
        if (Build.VERSION.SDK_INT >= 26) startForegroundService(service);
        else startService(service);
    }

    private void showTrackArtwork(MediaTrack track) {
        if (providers == null || artworkView == null) return;
        String expectedId = track.providerId;
        activeArtworkId = expectedId;
        artworkView.setImageDrawable(null);
        providers.loadArtwork(track, (bitmap, error) -> {
            if (!destroyed && bitmap != null && expectedId.equals(activeArtworkId)) {
                artworkView.setImageBitmap(bitmap);
            }
        });
    }

    private static String stateLabel(int state) {
        switch (state) {
            case PlaybackController.LOADING: return "Loading";
            case PlaybackController.PLAYING: return "Playing";
            case PlaybackController.PAUSED: return "Paused";
            case PlaybackController.STOPPED: return "Stopped";
            case PlaybackController.ERROR: return "Error";
            default: return "Idle";
        }
    }

    @Override protected void onDestroy() {
        destroyed = true;
        mainHandler.removeCallbacks(statePoll);
        if (bound) unbindService(connection);
        controller = null;
        super.onDestroy();
    }
}
