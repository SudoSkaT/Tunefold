package com.tunefold.app;

import android.graphics.Color;
import android.graphics.drawable.ColorDrawable;
import android.graphics.drawable.Drawable;
import android.text.TextUtils;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.widget.Button;
import android.widget.FrameLayout;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.ProgressBar;
import android.widget.TextView;

/**
 * Android now-playing surface.
 *
 * <p>Built as real views (no XML) so every dimension is expressed in
 * {@code dp}/{@code sp} and the layout adapts to the window it is given:
 * portrait, landscape and large screens all re-measure instead of relying on
 * fixed sizes.
 *
 * <p>Hierarchy, in order:
 * <ol>
 *   <li>artwork</li>
 *   <li>title / artist</li>
 *   <li>playback state</li>
 *   <li>progress</li>
 *   <li>transport controls</li>
 *   <li>secondary actions (download)</li>
 * </ol>
 *
 * <p>Technical diagnostics are NOT rendered here: they belong to
 * {@link PlaybackDebugPanel}.
 */
final class NowPlayingView {
    /** Callbacks to the Activity. All invoked on the UI thread. */
    interface Actions {
        void onPlayPause();
        void onStop();
        void onDownload();
        void onRemoveDownload();
        void onCancelDownload();
    }

    private final LinearLayout root;
    private final LinearLayout nowPlaying;
    private final FrameLayout artworkFrame;
    private final LinearLayout details;
    private final ImageView artwork;
    private final TextView title;
    private final TextView subtitle;
    private final TextView stateLabel;
    private final ProgressBar buffering;
    private final ProgressBar progress;
    private final TextView elapsed;
    private final TextView total;
    private final Button playPause;
    private final Button stop;
    private final Button download;
    private final Actions actions;

    private String activeArtworkId = "";
    private Drawable placeholder;
    private boolean landscape;
    private int artworkSidePx;

    /** Bounds for the cover, in dp, applied to the window-derived size. */
    private static final int MIN_ARTWORK_DP = 160;
    private static final int MAX_ARTWORK_DP = 420;

    NowPlayingView(android.content.Context context, Actions actions) {
        this.actions = actions;
        int pad = dp(context, 20);

        root = new LinearLayout(context);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setPadding(pad, dp(context, 12), pad, dp(context, 12));

        // `nowPlaying` holds artwork + details as SIBLINGS from the start, so
        // orientation changes only re-weight them: no view is ever re-parented
        // and nothing can end up hidden in the wrong mode.
        nowPlaying = new LinearLayout(context);
        nowPlaying.setOrientation(LinearLayout.VERTICAL);
        // The Activity hosts this inside a ScrollView, so the panel is sized by
        // the window (see setArtworkSidePx) rather than by a weight that could
        // collapse to zero in a short window.
        root.addView(nowPlaying, new LinearLayout.LayoutParams(-1, -2));

        // ---- 1. Artwork: the dominant element ----
        artworkFrame = new SquareFrameLayout(context);
        artworkFrame.setBackground(new ColorDrawable(Color.parseColor("#1F2430")));
        artwork = new ImageView(context);
        artwork.setScaleType(ImageView.ScaleType.CENTER_CROP);
        artwork.setContentDescription("Artwork");
        placeholder = new ColorDrawable(Color.parseColor("#2A3140"));
        artwork.setImageDrawable(placeholder);
        artworkFrame.addView(artwork, new FrameLayout.LayoutParams(-1, -1));
        nowPlaying.addView(artworkFrame, new LinearLayout.LayoutParams(-1, 0, 1f));

        details = new LinearLayout(context);
        details.setOrientation(LinearLayout.VERTICAL);
        nowPlaying.addView(details, new LinearLayout.LayoutParams(-1, -2));

        // ---- 2. Title / artist ----
        title = new TextView(context);
        title.setTextSize(TypedValue.COMPLEX_UNIT_SP, 20);
        title.setTextColor(Color.parseColor("#101317"));
        title.setMaxLines(2);
        title.setEllipsize(TextUtils.TruncateAt.END);
        title.setText("Nothing playing");
        LinearLayout.LayoutParams titleParams = new LinearLayout.LayoutParams(-1, -2);
        titleParams.topMargin = dp(context, 16);
        details.addView(title, titleParams);

        subtitle = new TextView(context);
        subtitle.setTextSize(TypedValue.COMPLEX_UNIT_SP, 15);
        subtitle.setTextColor(Color.parseColor("#5B6472"));
        subtitle.setMaxLines(1);
        subtitle.setEllipsize(TextUtils.TruncateAt.END);
        details.addView(subtitle, new LinearLayout.LayoutParams(-1, -2));

        // ---- 3. Playback state ----
        stateLabel = new TextView(context);
        stateLabel.setTextSize(TypedValue.COMPLEX_UNIT_SP, 13);
        stateLabel.setTextColor(Color.parseColor("#5B6472"));
        LinearLayout.LayoutParams stateParams = new LinearLayout.LayoutParams(-1, -2);
        stateParams.topMargin = dp(context, 10);
        details.addView(stateLabel, stateParams);

        buffering = new ProgressBar(context);
        buffering.setIndeterminate(true);
        buffering.setVisibility(View.GONE);
        LinearLayout.LayoutParams bufferingParams = new LinearLayout.LayoutParams(-1, dp(context, 3));
        bufferingParams.topMargin = dp(context, 6);
        details.addView(buffering, bufferingParams);

        // ---- 4. Progress ----
        LinearLayout progressRow = new LinearLayout(context);
        progressRow.setOrientation(LinearLayout.HORIZONTAL);
        progressRow.setGravity(Gravity.CENTER_VERTICAL);
        progress = new ProgressBar(context, null,
                android.R.attr.progressBarStyleHorizontal);
        progress.setMax(1000);
        progress.setProgress(0);
        elapsed = timeLabel(context);
        total = timeLabel(context);
        total.setGravity(Gravity.END);
        int timeWidth = dp(context, 52);
        progressRow.addView(elapsed, new LinearLayout.LayoutParams(timeWidth, -2));
        progressRow.addView(progress, new LinearLayout.LayoutParams(0, -2, 1f));
        progressRow.addView(total, new LinearLayout.LayoutParams(timeWidth, -2));
        LinearLayout.LayoutParams progressParams = new LinearLayout.LayoutParams(-1, -2);
        progressParams.topMargin = dp(context, 8);
        details.addView(progressRow, progressParams);

        // ---- 5. Transport controls (touch targets >= 48dp) ----
        LinearLayout transport = new LinearLayout(context);
        transport.setOrientation(LinearLayout.HORIZONTAL);
        transport.setGravity(Gravity.CENTER);
        int gap = dp(context, 12);
        playPause = button(context, "Play", actions::onPlayPause);
        stop = button(context, "Stop", actions::onStop);
        transport.addView(playPause, new LinearLayout.LayoutParams(0, dp(context, 52), 2f));
        LinearLayout.LayoutParams stopParams = new LinearLayout.LayoutParams(0, dp(context, 52), 1f);
        stopParams.leftMargin = gap;
        transport.addView(stop, stopParams);
        LinearLayout.LayoutParams transportParams = new LinearLayout.LayoutParams(-1, -2);
        transportParams.topMargin = dp(context, 12);
        details.addView(transport, transportParams);

        // ---- 6. Secondary actions ----
        download = button(context, "Download", actions::onDownload);
        LinearLayout.LayoutParams downloadParams = new LinearLayout.LayoutParams(-1, dp(context, 48));
        downloadParams.topMargin = gap;
        details.addView(download, downloadParams);
    }

    /**
     * Keeps cover art square without ever exceeding the space it was given.
     *
     * <p>The side is {@code min(width, height)} of the incoming specs, so in
     * portrait (where the panel is taller than it is wide) the art is bounded by
     * the available height and the controls below it are never pushed off
     * screen, and in landscape it is bounded by the artwork column width.
     */
    private static final class SquareFrameLayout extends FrameLayout {
        SquareFrameLayout(android.content.Context context) { super(context); }

        @Override protected void onMeasure(int widthMeasureSpec, int heightMeasureSpec) {
            int side = Math.min(
                    MeasureSpec.getSize(widthMeasureSpec),
                    MeasureSpec.getSize(heightMeasureSpec));
            int exact = MeasureSpec.makeMeasureSpec(side, MeasureSpec.EXACTLY);
            super.onMeasure(exact, exact);
        }
    }

    private static TextView timeLabel(android.content.Context context) {
        TextView view = new TextView(context);
        view.setTextSize(TypedValue.COMPLEX_UNIT_SP, 12);
        view.setTextColor(Color.parseColor("#5B6472"));
        view.setText("0:00");
        return view;
    }

    private static Button button(android.content.Context context, String label, Runnable onClick) {
        Button view = new Button(context);
        view.setText(label);
        view.setAllCaps(false);
        view.setTextSize(TypedValue.COMPLEX_UNIT_SP, 15);
        view.setOnClickListener(ignored -> onClick.run());
        return view;
    }

    static int dp(android.content.Context context, float value) {
        return Math.round(value * context.getResources().getDisplayMetrics().density);
    }

    View view() { return root; }

    /**
     * Artwork is loaded off the UI thread by the caller; this only accepts a
     * bitmap that still matches the current track, so a slow load cannot
     * overwrite a newer one.
     */
    void setArtwork(MediaTrack track, android.graphics.Bitmap bitmap) {
        if (bitmap == null) {
            artwork.setImageDrawable(placeholder);
            return;
        }
        if (track != null && !track.providerId.equals(activeArtworkId)) return;
        artwork.setImageBitmap(bitmap);
    }

    void setTrack(MediaTrack track) {
        activeArtworkId = track == null ? "" : track.providerId;
        artwork.setImageDrawable(placeholder);
        if (track == null) {
            title.setText("Nothing playing");
            subtitle.setText("");
            total.setText("0:00");
            return;
        }
        title.setText(track.title);
        String attribution = track.artist.isEmpty() ? track.channel : track.artist;
        subtitle.setText(attribution);
        if (track.durationMs > 0) total.setText(formatTime(track.durationMs));
    }

    /**
     * Renders the real playback state. Technical diagnostics never appear
     * here; see {@link PlaybackDebugPanel}.
     */
    void setPlaybackState(int state, String error, TrackDownloader.Status downloadStatus) {
        boolean downloading = downloadStatus != null
                && (downloadStatus.state == TrackDownloader.State.RUNNING);
        boolean busy = state == PlaybackController.LOADING || state == PlaybackController.IDLE;
        buffering.setVisibility(busy || downloading ? View.VISIBLE : View.GONE);

        String label;
        switch (state) {
            case PlaybackController.PLAYING: label = "Playing"; break;
            case PlaybackController.PAUSED: label = "Paused"; break;
            case PlaybackController.LOADING: label = "Buffering"; break;
            case PlaybackController.STOPPED: label = "Stopped"; break;
            case PlaybackController.ERROR: label = "Error"; break;
            default: label = "Idle";
        }
        if (downloading) {
            int percent = downloadStatus.percent();
            label = percent >= 0 ? "Downloading " + percent + "%" : "Downloading";
        }
        stateLabel.setText(label);

        if (error != null && !error.isEmpty() && state == PlaybackController.ERROR) {
            stateLabel.setText("Error · " + error);
        }

        playPause.setText(state == PlaybackController.PAUSED ? "Resume" : "Play");
        playPause.setEnabled(state != PlaybackController.IDLE);
        stop.setEnabled(state != PlaybackController.IDLE && state != PlaybackController.STOPPED);

        // The download action reflects reality: run / cancel / remove / start.
        if (downloading) {
            download.setText("Cancel download");
        } else if (downloadStatus != null
                && downloadStatus.state == TrackDownloader.State.COMPLETED) {
            download.setText("Remove download");
        } else {
            download.setText("Download");
        }
    }

    void setDownloadStatus(TrackDownloader.Status status) {
        if (status == null) return;
        boolean downloading = status.state == TrackDownloader.State.RUNNING;
        boolean completed = status.state == TrackDownloader.State.COMPLETED;
        if (downloading) {
            int percent = status.percent();
            String text = percent >= 0 ? "Downloading " + percent + "%" : "Downloading";
            if (status.total > 0) {
                text = text + " · " + formatBytes(status.received) + " / "
                        + formatBytes(status.total);
            }
            download.setText(text);
        } else if (completed) {
            download.setText("Downloaded · remove");
        } else if (status.state == TrackDownloader.State.FAILED) {
            download.setText("Download failed");
        } else if (status.state == TrackDownloader.State.CANCELLED) {
            download.setText("Download");
        }
    }

    void setPosition(long positionMs, long durationMs) {
        if (durationMs > 0) {
            progress.setProgress((int) (positionMs * 1000 / durationMs));
            total.setText(formatTime(durationMs));
        } else {
            progress.setProgress(0);
        }
        elapsed.setText(formatTime(Math.max(0, positionMs)));
    }

    /**
     * Orientation switch: portrait stacks artwork above the details, landscape
     * puts them side by side with a bounded artwork column so the controls keep
     * a usable height on a short window.
     */
    void setLandscape(boolean value) {
        if (landscape == value) return;
        landscape = value;
        nowPlaying.setOrientation(value ? LinearLayout.HORIZONTAL : LinearLayout.VERTICAL);

        LinearLayout.LayoutParams detailsParams =
                (LinearLayout.LayoutParams) details.getLayoutParams();
        detailsParams.width = value ? 0 : -1;
        detailsParams.height = -2;
        detailsParams.weight = value ? 1f : 0f;
        details.setLayoutParams(detailsParams);
        setArtworkSidePx(artworkSidePx);
    }

    /**
     * Artwork side in PIXELS, computed by the Activity from the current window.
     *
     * <p>Derived rather than hard-coded so portrait, landscape, small phones and
     * large screens all get a sensible cover without the controls ever being
     * squeezed out of the layout. The value is clamped to a sane dp range so a
     * very small window still shows a usable cover and a tablet-sized one does
     * not dwarf the controls.
     */
    void setArtworkSidePx(int requestedPx) {
        android.content.Context context = root.getContext();
        int side = Math.max(dp(context, MIN_ARTWORK_DP),
                Math.min(dp(context, MAX_ARTWORK_DP), requestedPx));
        artworkSidePx = side;
        LinearLayout.LayoutParams params =
                (LinearLayout.LayoutParams) artworkFrame.getLayoutParams();
        params.width = side;
        params.height = side;
        params.rightMargin = landscape ? dp(context, 16) : 0;
        params.bottomMargin = landscape ? 0 : dp(context, 8);
        artworkFrame.setLayoutParams(params);
    }

    static String formatTime(long ms) {
        long total = Math.max(0, ms) / 1000;
        long hours = total / 3600;
        long minutes = (total % 3600) / 60;
        long seconds = total % 60;
        if (hours > 0) return String.format(java.util.Locale.ROOT, "%d:%02d:%02d",
                hours, minutes, seconds);
        return String.format(java.util.Locale.ROOT, "%d:%02d", minutes, seconds);
    }

    static String formatBytes(long bytes) {
        if (bytes <= 0) return "0 B";
        double mb = bytes / (1024.0 * 1024.0);
        if (mb >= 1.0) return String.format(java.util.Locale.ROOT, "%.1f MB", mb);
        return String.format(java.util.Locale.ROOT, "%.0f KB", bytes / 1024.0);
    }
}