package com.tunefold.app;

import android.content.Context;
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
 * <p>Reflects the whole observable session at once: track, artist, artwork,
 * state, progress, duration, liked, downloaded and error (§11). Nothing here
 * holds state of its own — it renders whatever {@link PlaybackSession} reports,
 * which is why a recreated Activity looks identical to the one it replaced.
 *
 * <p>Built in code rather than XML so every dimension is expressed in
 * {@code dp}/{@code sp} and the layout re-measures for whatever window it is
 * given, instead of relying on fixed sizes.
 *
 * <p>Technical diagnostics never appear here; they belong to
 * {@link PlaybackDebugPanel}.
 */
final class NowPlayingView {
    /** Callbacks to the Activity. All invoked on the UI thread. */
    interface Actions {
        void onPlayPause();
        void onNext();
        void onPrevious();
        void onStop();
        void onLike();
        void onDownload();
        void onCancelDownload();
        void onRetryDownload();
        void onRetryPlayback();
        void onSkipToNext();
        void onToggleAutoplay();
    }

    private final LinearLayout root;
    private final LinearLayout nowPlaying;
    private final FrameLayout artworkFrame;
    private final LinearLayout details;
    private final ImageView artwork;
    private final TextView title;
    private final TextView subtitle;
    private final TextView stateLabel;
    private final TextView errorLabel;
    private final ProgressBar buffering;
    private final ProgressBar progress;
    private final TextView elapsed;
    private final TextView total;
    private final Button previous;
    private final Button playPause;
    private final Button next;
    private final Button like;
    private final Button download;
    private final Button errorAction;
    private final Button autoplay;

    private String activeArtworkId = "";
    private Drawable placeholder;
    private boolean landscape;
    private int artworkSidePx;

    /** Bounds for the cover, in dp, applied to the window-derived size. */
    private static final int MIN_ARTWORK_DP = 160;
    private static final int MAX_ARTWORK_DP = 420;

    NowPlayingView(Context context, Actions actions) {
        int pad = dp(context, 20);
        root = new LinearLayout(context);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setPadding(pad, dp(context, 4), pad, dp(context, 4));

        // `nowPlaying` holds artwork and details as SIBLINGS from the start, so
        // an orientation change only re-weights them: no view is ever re-parented
        // and nothing can end up hidden in the wrong mode.
        nowPlaying = new LinearLayout(context);
        nowPlaying.setOrientation(LinearLayout.VERTICAL);
        root.addView(nowPlaying, new LinearLayout.LayoutParams(-1, -2));

        // ---- 1. Artwork ----
        artworkFrame = new SquareFrameLayout(context);
        artworkFrame.setBackground(new ColorDrawable(Color.parseColor("#1F2430")));
        artwork = new ImageView(context);
        artwork.setScaleType(ImageView.ScaleType.CENTER_CROP);
        artwork.setContentDescription("Artwork");
        placeholder = new ColorDrawable(Color.parseColor("#2A3140"));
        artwork.setImageDrawable(placeholder);
        artworkFrame.addView(artwork, new FrameLayout.LayoutParams(-1, -1));
        nowPlaying.addView(artworkFrame, new LinearLayout.LayoutParams(-1, -2));

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

        // Errors get their own line plus an action, never a silent dead end (§19).
        errorLabel = new TextView(context);
        errorLabel.setTextSize(TypedValue.COMPLEX_UNIT_SP, 13);
        errorLabel.setTextColor(Color.parseColor("#B3261E"));
        errorLabel.setVisibility(View.GONE);
        details.addView(errorLabel, new LinearLayout.LayoutParams(-1, -2));

        errorAction = button(context, "Retry", actions::onRetryPlayback);
        errorAction.setVisibility(View.GONE);
        LinearLayout.LayoutParams errorActionParams = new LinearLayout.LayoutParams(-2, dp(context, 40));
        errorActionParams.topMargin = dp(context, 4);
        details.addView(errorAction, errorActionParams);

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

        // ---- 5. Transport ----
        LinearLayout transport = new LinearLayout(context);
        transport.setOrientation(LinearLayout.HORIZONTAL);
        transport.setGravity(Gravity.CENTER);
        int gap = dp(context, 10);
        previous = button(context, "Previous", actions::onPrevious);
        playPause = button(context, "Play", actions::onPlayPause);
        next = button(context, "Next", actions::onNext);
        transport.addView(previous, new LinearLayout.LayoutParams(0, dp(context, 52), 1f));
        LinearLayout.LayoutParams playParams = new LinearLayout.LayoutParams(0, dp(context, 52), 1.4f);
        playParams.leftMargin = gap;
        transport.addView(playPause, playParams);
        LinearLayout.LayoutParams nextParams = new LinearLayout.LayoutParams(0, dp(context, 52), 1f);
        nextParams.leftMargin = gap;
        transport.addView(next, nextParams);
        LinearLayout.LayoutParams transportParams = new LinearLayout.LayoutParams(-1, -2);
        transportParams.topMargin = dp(context, 12);
        details.addView(transport, transportParams);

        LinearLayout transportSecondary = new LinearLayout(context);
        transportSecondary.setOrientation(LinearLayout.HORIZONTAL);
        transportSecondary.setGravity(Gravity.CENTER);
        Button stop = button(context, "Stop", actions::onStop);
        transportSecondary.addView(stop, new LinearLayout.LayoutParams(0, dp(context, 44), 1f));
        LinearLayout.LayoutParams autoplayRow = new LinearLayout.LayoutParams(0, dp(context, 44), 1f);
        autoplayRow.leftMargin = gap;
        autoplay = button(context, "Autoplay: on", actions::onToggleAutoplay);
        transportSecondary.addView(autoplay, autoplayRow);
        LinearLayout.LayoutParams secondaryParams = new LinearLayout.LayoutParams(-1, -2);
        secondaryParams.topMargin = gap;
        details.addView(transportSecondary, secondaryParams);

        // ---- 6. Secondary actions ----
        LinearLayout actionsRow = new LinearLayout(context);
        actionsRow.setOrientation(LinearLayout.HORIZONTAL);
        like = button(context, "Like", actions::onLike);
        download = button(context, "Download", actions::onDownload);
        actionsRow.addView(like, new LinearLayout.LayoutParams(0, dp(context, 48), 1f));
        LinearLayout.LayoutParams downloadParams = new LinearLayout.LayoutParams(0, dp(context, 48), 1f);
        downloadParams.leftMargin = gap;
        actionsRow.addView(download, downloadParams);
        LinearLayout.LayoutParams actionsRowParams = new LinearLayout.LayoutParams(-1, -2);
        actionsRowParams.topMargin = gap;
        details.addView(actionsRow, actionsRowParams);
    }

    /** Keeps cover art square without ever exceeding the space it was given. */
    private static final class SquareFrameLayout extends FrameLayout {
        SquareFrameLayout(Context context) { super(context); }

        @Override protected void onMeasure(int widthMeasureSpec, int heightMeasureSpec) {
            int side = Math.min(MeasureSpec.getSize(widthMeasureSpec),
                    MeasureSpec.getSize(heightMeasureSpec));
            int exact = MeasureSpec.makeMeasureSpec(side, MeasureSpec.EXACTLY);
            super.onMeasure(exact, exact);
        }
    }

    private static TextView timeLabel(Context context) {
        TextView view = new TextView(context);
        view.setTextSize(TypedValue.COMPLEX_UNIT_SP, 12);
        view.setTextColor(Color.parseColor("#5B6472"));
        view.setText("0:00");
        return view;
    }

    private static Button button(Context context, String label, Runnable onClick) {
        Button view = new Button(context);
        view.setText(label);
        view.setAllCaps(false);
        view.setTextSize(TypedValue.COMPLEX_UNIT_SP, 13);
        // Transport labels must stay on one line: a wrapped "Previous" makes the
        // row look broken and shrinks the touch target.
        view.setMaxLines(1);
        view.setEllipsize(TextUtils.TruncateAt.END);
        view.setPadding(dp(context, 4), 0, dp(context, 4), 0);
        view.setOnClickListener(ignored -> onClick.run());
        return view;
    }

    static int dp(Context context, float value) {
        return Math.round(value * context.getResources().getDisplayMetrics().density);
    }

    View view() { return root; }

    void setArtwork(MediaTrack track, android.graphics.Bitmap bitmap) {
        if (bitmap == null) {
            artwork.setImageDrawable(placeholder);
            return;
        }
        if (track != null && track.providerId != null
                && !track.providerId.equals(activeArtworkId)) return;
        artwork.setImageBitmap(bitmap);
    }

    /**
     * Renders the entire session state (§11).
     *
     * <p>One entry point so the screen can never show a half-updated mix of old
     * and new state.
     */
    void render(PlaybackSession.Snapshot snapshot, long positionMs) {
        MediaTrack track = snapshot.currentTrack;

        if (track != null) {
            activeArtworkId = track.providerId;
            title.setText(track.title);
            subtitle.setText(attribution(track));
            if (track.durationMs > 0) total.setText(formatTime(track.durationMs));
        } else {
            activeArtworkId = "";
            title.setText("Nothing playing");
            subtitle.setText("");
            total.setText("0:00");
            artwork.setImageDrawable(placeholder);
        }

        stateLabel.setText(snapshot.state.label());
        boolean waiting = snapshot.state == PlaybackState.BUFFERING
                || snapshot.state == PlaybackState.RESOLVING_METADATA
                || snapshot.state == PlaybackState.RESOLVING_SOURCE
                || snapshot.state == PlaybackState.SEARCHING;
        buffering.setVisibility(waiting ? View.VISIBLE : View.GONE);

        renderError(snapshot);
        renderTransport(snapshot);
        // §26: an action must always show the result of its own state, so the
        // autoplay button states what it will do, not a static label.
        autoplay.setText(snapshot.autoplayEnabled ? "Autoplay: on" : "Autoplay: off");
        autoplay.setContentDescription(snapshot.autoplayEnabled
                ? "Autoplay is on. Tap to turn it off."
                : "Autoplay is off. Tap to turn it on.");
        renderLike(snapshot);
        renderDownload(snapshot);

        long duration = track == null ? 0 : track.durationMs;
        if (duration > 0) {
            progress.setProgress((int) Math.min(1000, positionMs * 1000 / duration));
        } else {
            progress.setProgress(0);
        }
        elapsed.setText(formatTime(Math.max(0, positionMs)));
    }

    private void renderError(PlaybackSession.Snapshot snapshot) {
        PlaybackError error = snapshot.error;
        if (error == null || snapshot.state != PlaybackState.ERROR) {
            errorLabel.setVisibility(View.GONE);
            errorAction.setVisibility(View.GONE);
            return;
        }
        errorLabel.setVisibility(View.VISIBLE);
        errorLabel.setText(error.message);
        // Recoverable failures offer Retry; an unavailable track offers Next,
        // because retrying a 404 forever would be dishonest (§19).
        if (error.retryable) {
            errorAction.setVisibility(View.VISIBLE);
            errorAction.setText("Retry");
            errorAction.setOnClickListener(ignored -> { });
        } else {
            errorAction.setVisibility(View.VISIBLE);
            errorAction.setText("Next track");
            errorAction.setOnClickListener(ignored -> { });
        }
    }

    private void renderTransport(PlaybackSession.Snapshot snapshot) {
        boolean hasTrack = snapshot.currentTrack != null;
        // §26: the label states the action, so it must read "Pause" while audio
        // is coming out. Mirroring the state instead left a playing track showing
        // "Play".
        boolean playing = snapshot.state == PlaybackState.PLAYING;
        playPause.setText(playing ? "Pause" : "Play");
        playPause.setContentDescription(playing
                ? "Pause playback" : "Start playback");
        playPause.setEnabled(hasTrack);
        // Next is never disabled by an empty queue: with autoplay it still acts,
        // and the session decides between the next item, a recommendation and
        // stopping (§13).
        next.setEnabled(hasTrack || snapshot.autoplayEnabled);
        previous.setEnabled(hasTrack);
    }

    private void renderLike(PlaybackSession.Snapshot snapshot) {
        // §26: the label always shows the result of the last action.
        like.setText(snapshot.liked ? "Liked" : "Like");
        like.setEnabled(snapshot.currentKey != null);
    }

    private void renderDownload(PlaybackSession.Snapshot snapshot) {
        DownloadState state = snapshot.download;
        boolean hasTrack = snapshot.currentTrack != null;
        download.setEnabled(hasTrack);

        if (state == null || state.phase == DownloadState.Phase.IDLE) {
            download.setText(snapshot.downloaded ? "Remove download" : "Download");
            return;
        }
        switch (state.phase) {
            case RESOLVING_SOURCE:
                download.setText("Resolving source");
                break;
            case DOWNLOADING:
                download.setText(state.label());
                break;
            case COMPLETED:
                download.setText("Remove download");
                break;
            case ALREADY_DOWNLOADED:
                download.setText("Downloaded");
                download.setEnabled(false);
                break;
            case CANCELLED:
                download.setText("Download");
                break;
            case FAILED:
                download.setText("Retry download");
                break;
            default:
                download.setText("Download");
        }
    }

    void setPosition(long positionMs, long durationMs) {
        if (durationMs > 0) {
            progress.setProgress((int) Math.min(1000, positionMs * 1000 / durationMs));
            total.setText(formatTime(durationMs));
        }
        elapsed.setText(formatTime(Math.max(0, positionMs)));
    }

    /** Sets the error action's behaviour once, from the Activity. */
    void bindErrorAction(Runnable onRetry, Runnable onNext) {
        errorAction.setOnClickListener(ignored -> {
            if (errorAction.getText().toString().startsWith("Retry")) onRetry.run();
            else onNext.run();
        });
    }

    void bindDownloadAction(Runnable onDownload, Runnable onCancel, Runnable onRetry) {
        download.setOnClickListener(ignored -> {
            String label = download.getText().toString();
            if (label.startsWith("Cancel") || label.contains("%")
                    || label.startsWith("Resolving")) {
                onCancel.run();
            } else if (label.startsWith("Retry")) {
                onRetry.run();
            } else {
                onDownload.run();
            }
        });
    }

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
     * squeezed out of the layout.
     */
    void setArtworkSidePx(int requestedPx) {
        Context context = root.getContext();
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

    private static String attribution(MediaTrack track) {
        String value = track.artist.isEmpty() ? track.channel : track.artist;
        return value.isEmpty() ? "" : value;
    }

    static String formatTime(long ms) {
        long total = Math.max(0, ms) / 1000;
        long hours = total / 3600;
        long minutes = (total % 3600) / 60;
        long seconds = total % 60;
        if (hours > 0) {
            return String.format(java.util.Locale.ROOT, "%d:%02d:%02d", hours, minutes, seconds);
        }
        return String.format(java.util.Locale.ROOT, "%d:%02d", minutes, seconds);
    }

    static String formatBytes(long bytes) {
        if (bytes <= 0) return "0 B";
        double megabytes = bytes / (1024.0 * 1024.0);
        if (megabytes >= 1.0) {
            return String.format(java.util.Locale.ROOT, "%.1f MB", megabytes);
        }
        return String.format(java.util.Locale.ROOT, "%.0f KB", bytes / 1024.0);
    }
}