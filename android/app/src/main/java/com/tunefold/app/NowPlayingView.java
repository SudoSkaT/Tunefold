package com.tunefold.app;

import android.content.Context;
import android.content.res.ColorStateList;
import android.graphics.Bitmap;
import android.graphics.drawable.ColorDrawable;
import android.text.TextUtils;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.widget.LinearLayout;
import android.widget.ProgressBar;
import android.widget.TextView;

/**
 * The now-playing surface: artwork, then identity, then state, then progress, then the
 * transport, then the secondary actions.
 *
 * <p>Reflects whatever {@link PlaybackSession} reports and holds no state of its own
 * beyond the last value each part rendered, which is what lets a recreated Activity
 * look identical to the one it replaced.
 *
 * <h3>One entry point per kind of change</h3>
 * This surface used to have a single {@code render(snapshot)} that re-applied
 * everything: metadata, state, progress, download and error, on every notification. A
 * position tick therefore re-set six labels and a progress bar, and a state change
 * re-read the track title. Each of those is a {@code setText} or {@code setProgress},
 * and each requests layout and then a draw.
 *
 * <p>So the update is split into {@link #applyTrack}, {@link #applyPlaybackState},
 * {@link #applyLibraryState}, {@link #applyDownloadState}, {@link #applyError} and
 * {@link #setPosition}, and every one of them compares against what it last rendered
 * before it writes. A notification that changes one thing now touches one view, and a
 * tick that cannot change anything visible costs a handful of comparisons.
 *
 * <h3>Built in code, from tokens</h3>
 * Every dimension and colour comes from {@link DesignTokens}, so nothing here can drift
 * from the rest of the surface. There are no fixed sizes: the cover is derived from the
 * window by the Activity and clamped here.
 *
 * <p>Technical diagnostics never appear here; they belong to {@link PlaybackDebugPanel}.
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

    /** What the error action offers; decides its glyph, its description and its effect. */
    private enum ErrorAction { NONE, RETRY, SKIP }

    private static final CharSequence NOTHING_PLAYING = "Nothing playing";

    private final Actions actions;
    private final LinearLayout root;
    private final LinearLayout stage;
    private final ArtworkView artwork;
    private final LinearLayout details;
    private final TextView title;
    private final TextView subtitle;
    private final TextView stateLabel;
    private final TextView errorLabel;
    private final IconButton errorAction;
    private final ProgressBar buffering;
    private final ProgressControl progress;
    private final IconButton previous;
    private final PrimaryPlaybackButton playPause;
    private final IconButton next;
    private final IconButton stop;
    private final SecondaryActionButton autoplay;
    private final SecondaryActionButton like;
    private final SecondaryActionButton download;

    // ---- last rendered values; every apply* method compares before it writes ----

    private TrackKey activeTrackKey;
    private boolean metadataShown;
    private PlaybackState renderedState;
    private boolean renderedWaiting;
    private boolean renderedPlaying;
    private boolean renderedAutoplay;
    private boolean renderedLiked;
    private DownloadState.Phase renderedDownloadPhase;
    private int renderedDownloadPercent = Integer.MIN_VALUE;
    private ErrorAction renderedErrorAction = ErrorAction.NONE;
    private CharSequence renderedErrorText = "";
    private boolean landscape;
    private int artworkSidePx;
    private String artworkSideKey = "";

    NowPlayingView(Context context, Actions actions) {
        DesignTokens.init(context);
        this.actions = actions;
        int pad = DesignTokens.dp(DesignTokens.CARD_PADDING);
        root = new LinearLayout(context);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setPadding(pad, pad, pad, pad);
        root.setBackground(new ColorDrawable(DesignTokens.Palette.SURFACE_CARD));
        root.setElevation(DesignTokens.dp(DesignTokens.ELEVATION_CARD));

        // `stage` holds artwork and details as SIBLINGS from the start, so an
        // orientation change only re-weights them: no view is ever re-parented and
        // nothing can end up hidden in the wrong mode.
        stage = new LinearLayout(context);
        stage.setOrientation(LinearLayout.VERTICAL);
        root.addView(stage, new LinearLayout.LayoutParams(-1, -2));

        artwork = new ArtworkView(context);
        stage.addView(artwork, new LinearLayout.LayoutParams(-1, -2));

        details = new LinearLayout(context);
        details.setOrientation(LinearLayout.VERTICAL);
        stage.addView(details, new LinearLayout.LayoutParams(-1, -2));

        title = label(context, DesignTokens.TYPE_TITLE_SIZE, DesignTokens.Palette.ON_SURFACE);
        title.setMaxLines(2);
        title.setEllipsize(TextUtils.TruncateAt.END);
        title.setText(NOTHING_PLAYING);
        addDetails(title, DesignTokens.BLOCK_GAP);

        subtitle = label(context, DesignTokens.TYPE_BODY_SIZE,
                DesignTokens.Palette.ON_SURFACE_MUTED);
        subtitle.setMaxLines(1);
        subtitle.setEllipsize(TextUtils.TruncateAt.END);
        addDetails(subtitle, 0f);

        stateLabel = label(context, DesignTokens.TYPE_LABEL_SIZE,
                DesignTokens.Palette.ON_SURFACE_MUTED);
        stateLabel.setLetterSpacing(DesignTokens.TYPE_LABEL_TRACKING);
        stateLabel.setAllCaps(true);
        addDetails(stateLabel, DesignTokens.GAP);

        // A failure gets its own line plus an action, never a silent dead end (§19).
        errorLabel = label(context, DesignTokens.TYPE_BODY_SIZE, DesignTokens.Palette.ERROR);
        errorLabel.setVisibility(View.GONE);
        addDetails(errorLabel, 0f);

        errorAction = new IconButton(context);
        errorAction.setVisibility(View.GONE);
        addDetails(errorAction, DesignTokens.GAP_TIGHT, -2);

        buffering = new ProgressBar(context);
        buffering.setIndeterminate(true);
        buffering.setVisibility(View.GONE);
        buffering.setIndeterminateTintList(ColorStateList.valueOf(ArtworkTheme.accent()));
        buffering.setContentDescription("Buffering");
        addDetails(buffering, DesignTokens.GAP);

        progress = new ProgressControl(context);
        addDetails(progress, DesignTokens.BLOCK_GAP);

        // ---- transport: previous, the one control that matters, next ----
        LinearLayout transport = row(context, Gravity.CENTER);
        int target = DesignTokens.dp(DesignTokens.CONTROL_TARGET);
        previous = new IconButton(context);
        previous.setAction(Icon.PREVIOUS, "Previous track");
        previous.setOnClickListener(ignored -> actions.onPrevious());
        transport.addView(previous, new LinearLayout.LayoutParams(target, target));

        playPause = new PrimaryPlaybackButton(context);
        playPause.setAction(Icon.PLAY, "Start playback");
        playPause.setOnClickListener(ignored -> actions.onPlayPause());
        LinearLayout.LayoutParams playParams = new LinearLayout.LayoutParams(
                DesignTokens.dp(DesignTokens.CONTROL_TARGET_PRIMARY),
                DesignTokens.dp(DesignTokens.CONTROL_TARGET_PRIMARY));
        playParams.leftMargin = DesignTokens.dp(DesignTokens.SECTION_GAP);
        transport.addView(playPause, playParams);

        next = new IconButton(context);
        next.setAction(Icon.NEXT, "Next track");
        next.setOnClickListener(ignored -> actions.onNext());
        LinearLayout.LayoutParams nextParams = new LinearLayout.LayoutParams(target, target);
        nextParams.leftMargin = DesignTokens.dp(DesignTokens.SECTION_GAP);
        transport.addView(next, nextParams);
        addDetails(transport, DesignTokens.SECTION_GAP);

        // ---- secondary transport ----
        LinearLayout secondary = row(context, Gravity.CENTER);
        stop = new IconButton(context);
        stop.setAction(Icon.STOP, "Stop playback");
        stop.setOnClickListener(ignored -> actions.onStop());
        secondary.addView(stop, new LinearLayout.LayoutParams(target, target));

        autoplay = new SecondaryActionButton(context);
        autoplay.setAction(Icon.AUTOPLAY, "Autoplay");
        autoplay.setOnClickListener(ignored -> actions.onToggleAutoplay());
        LinearLayout.LayoutParams autoplayParams = new LinearLayout.LayoutParams(target, target);
        autoplayParams.leftMargin = DesignTokens.dp(DesignTokens.SECTION_GAP);
        secondary.addView(autoplay, autoplayParams);
        addDetails(secondary, DesignTokens.GAP);

        // ---- secondary actions ----
        LinearLayout actionRow = row(context, Gravity.START);
        like = new SecondaryActionButton(context);
        like.setAction(Icon.HEART, "Not liked. Double tap to add to L1K3D.");
        like.setOnClickListener(ignored -> actions.onLike());
        actionRow.addView(like, new LinearLayout.LayoutParams(target, target));

        download = new SecondaryActionButton(context);
        download.setAction(Icon.DOWNLOAD, "Download. Double tap to start.");
        // Dispatch is on the last rendered phase, never on wording: matching strings
        // made this control's action depend on its label and silently routed Cancel to
        // a repaint, which made cancellation look broken (§8, §26).
        download.setOnClickListener(ignored -> {
            switch (renderedDownloadPhase) {
                case RESOLVING_SOURCE:
                case DOWNLOADING:
                    actions.onCancelDownload();
                    break;
                case FAILED:
                case CANCELLED:
                    actions.onRetryDownload();
                    break;
                default:
                    actions.onDownload();
                    break;
            }
        });
        LinearLayout.LayoutParams downloadParams = new LinearLayout.LayoutParams(target, target);
        downloadParams.leftMargin = DesignTokens.dp(DesignTokens.GAP);
        actionRow.addView(download, downloadParams);
        addDetails(actionRow, 0f);
    }

    View view() { return root; }

    /** The artwork sub-view, for window-derived sizing and cover identity. */
    ArtworkView artwork() { return artwork; }

    // ---------------------------------------------------------------- artwork

    /**
     * Declares which cover the view will accept.
     *
     * <p>Separate from {@link #setArtwork} because the cover arrives asynchronously and
     * must never land on a track it was not requested for.
     */
    void expectArtwork(String identity) { artwork.expectCover(identity); }

    /** Shows a decoded cover, for the identity the view is currently waiting for. */
    void setArtwork(String identity, Bitmap bitmap) { artwork.setCover(identity, bitmap); }

    // ----------------------------------------------------------------- render

    /**
     * Metadata: title, attribution and cover identity.
     *
     * <p>Guarded by track identity, so a notification that changed only the position
     * never re-read or re-set the title.
     */
    void applyTrack(PlaybackSession.Snapshot snapshot) {
        MediaTrack track = snapshot.currentTrack;
        if (track == null) {
            if (!metadataShown) return;
            metadataShown = false;
            activeTrackKey = null;
            setText(title, NOTHING_PLAYING);
            setText(subtitle, "");
            progress.clear();
            artwork.expectCover(null);
            artwork.clearCover();
            return;
        }
        TrackKey key = track.key();
        // Identity guards the cover, but not the words: the same track can be resolved
        // again with corrected metadata, and a title that never refreshes after that is
        // worse than the extra comparison it costs.
        if (metadataShown && equal(activeTrackKey, key)
                && TextUtils.equals(track.title, title.getText())
                && TextUtils.equals(attributionOf(track), subtitle.getText())) {
            return;
        }
        metadataShown = true;
        activeTrackKey = key;
        setText(title, track.title);
        setText(subtitle, attributionOf(track));
        artwork.expectCover(key == null ? track.providerId : key.toString());
    }

    /** Playback state: status line, buffering indicator, transport, autoplay. */
    void applyPlaybackState(PlaybackSession.Snapshot snapshot) {
        PlaybackState state = snapshot.state;
        if (renderedState != state) {
            renderedState = state;
            setText(stateLabel, state.label());
            boolean waiting = isWaiting(state);
            if (renderedWaiting != waiting) {
                renderedWaiting = waiting;
                buffering.setVisibility(waiting ? View.VISIBLE : View.GONE);
            }
        }
        boolean hasTrack = snapshot.currentTrack != null;
        // §26: an action must state what it will do, so the primary control reads
        // Pause while audio is coming out. Mirroring the state instead once left a
        // playing track showing "Play".
        boolean playing = state == PlaybackState.PLAYING;
        if (renderedPlaying != playing) {
            renderedPlaying = playing;
            playPause.setAction(playing ? Icon.PAUSE : Icon.PLAY,
                    playing ? "Pause playback" : "Start playback");
        }
        playPause.setEnabled(hasTrack);
        // Next is never disabled by an empty queue: with autoplay on it still acts, and
        // the session decides between the next item, a recommendation and stopping (§13).
        next.setEnabled(hasTrack || snapshot.autoplayEnabled);
        previous.setEnabled(hasTrack);
        stop.setEnabled(hasTrack);
        applyAutoplay(snapshot.autoplayEnabled);
    }

    private void applyAutoplay(boolean enabled) {
        if (renderedAutoplay == enabled) return;
        renderedAutoplay = enabled;
        autoplay.setToggleState(enabled);
        autoplay.setPhase(enabled ? Icon.AUTOPLAY : Icon.AUTOPLAY_OFF,
                enabled ? SecondaryActionButton.Phase.IDLE : SecondaryActionButton.Phase.OFF,
                -1);
        autoplay.applyDescription(enabled
                ? "Autoplay is on. Double tap to turn it off."
                : "Autoplay is off. Double tap to turn it on.");
    }

    /** Library membership: the like control. */
    void applyLibraryState(PlaybackSession.Snapshot snapshot) {
        like.setEnabled(snapshot.currentKey != null);
        if (renderedLiked == snapshot.liked) return;
        renderedLiked = snapshot.liked;
        // §26: the control always shows the result of the last action.
        like.setToggleState(snapshot.liked);
        like.setPhase(snapshot.liked ? Icon.HEART_FILLED : Icon.HEART,
                snapshot.liked ? SecondaryActionButton.Phase.DONE
                        : SecondaryActionButton.Phase.IDLE,
                -1);
        like.applyDescription(snapshot.liked
                ? "Liked. Double tap to remove from L1K3D."
                : "Not liked. Double tap to add to L1K3D.");
    }

    /** Download state: glyph, tint and progress ring. */
    void applyDownloadState(PlaybackSession.Snapshot snapshot) {
        DownloadState state = snapshot.download;
        DownloadState.Phase phase = state == null ? DownloadState.Phase.IDLE : state.phase;
        int percent = phase == DownloadState.Phase.DOWNLOADING && state != null
                ? state.progressPercent() : -1;
        download.setEnabled(snapshot.currentTrack != null);
        if (renderedDownloadPhase == phase && renderedDownloadPercent == percent) return;
        renderedDownloadPhase = phase;
        renderedDownloadPercent = percent;
        download.setPhase(downloadIcon(phase), downloadPhaseOf(phase), percent);
        download.applyDescription(downloadDescription(phase, percent));
    }

    /** Failure: message and the one action that can move it forward. */
    void applyError(PlaybackSession.Snapshot snapshot) {
        PlaybackError error = snapshot.error;
        boolean failing = error != null && snapshot.state == PlaybackState.ERROR;
        ErrorAction offer = ErrorAction.NONE;
        CharSequence message = "";
        if (failing) {
            // Recoverable failures offer Retry; an unavailable track offers Next,
            // because retrying a 404 forever would be dishonest (§19).
            offer = error.retryable ? ErrorAction.RETRY : ErrorAction.SKIP;
            message = error.message;
        }
        if (offer == renderedErrorAction && TextUtils.equals(message, renderedErrorText)) return;
        renderedErrorAction = offer;
        renderedErrorText = message;
        if (offer == ErrorAction.NONE) {
            setText(errorLabel, "");
            errorLabel.setVisibility(View.GONE);
            errorAction.setVisibility(View.GONE);
            return;
        }
        setText(errorLabel, message);
        errorLabel.setVisibility(View.VISIBLE);
        errorAction.setVisibility(View.VISIBLE);
        errorAction.setAction(offer == ErrorAction.RETRY ? Icon.RETRY : Icon.NEXT,
                offer == ErrorAction.RETRY ? "Retry playback" : "Skip to the next track");
    }

    /**
     * Applies a position tick.
     *
     * <p>Guarded by identity inside {@link ProgressControl}: a tick computed for a
     * track that is no longer the active one is dropped, so a late update can never
     * repaint the elapsed time of the previous song over the new one. This is the last
     * line of defence behind the session's own generation check.
     */
    void setPosition(TrackKey key, long positionMs, long durationMs) {
        progress.setPosition(key, positionMs, durationMs);
    }

    // -------------------------------------------------------------- geometry

    /**
     * Orientation, applied by re-weighting rather than by re-parenting.
     *
     * <p>Artwork and details are siblings from construction, so this only changes the
     * stage's direction and the details' width: no view is added, removed or moved.
     */
    void setLandscape(boolean value) {
        if (landscape == value) return;
        landscape = value;
        stage.setOrientation(value ? LinearLayout.HORIZONTAL : LinearLayout.VERTICAL);
        LinearLayout.LayoutParams detailsParams =
                (LinearLayout.LayoutParams) details.getLayoutParams();
        detailsParams.width = value ? 0 : -1;
        detailsParams.height = -2;
        detailsParams.weight = value ? 1f : 0f;
        detailsParams.rightMargin = 0;
        detailsParams.leftMargin = value
                ? DesignTokens.dp(DesignTokens.ARTWORK_GUTTER_LANDSCAPE) : 0;
        details.setLayoutParams(detailsParams);
        setArtworkSidePx(artworkSidePx);
    }

    /**
     * Artwork side in PIXELS, computed by the Activity from the current window.
     *
     * <p>Derived rather than hard-coded so portrait, landscape, small phones and large
     * screens all get a sensible cover without the controls ever being squeezed out of
     * the layout. The key makes a repeated call with the same value and orientation
     * free.
     */
    void setArtworkSidePx(int requestedPx) {
        int side = Math.max(DesignTokens.dp(DesignTokens.ARTWORK_MIN),
                Math.min(DesignTokens.dp(DesignTokens.ARTWORK_MAX), requestedPx));
        String key = side + "/" + landscape;
        if (key.equals(artworkSideKey)) return;
        artworkSideKey = key;
        artworkSidePx = side;
        LinearLayout.LayoutParams params = (LinearLayout.LayoutParams) artwork.getLayoutParams();
        params.width = side;
        params.height = side;
        params.rightMargin = landscape
                ? DesignTokens.dp(DesignTokens.ARTWORK_GUTTER_LANDSCAPE) : 0;
        params.bottomMargin = landscape ? 0
                : DesignTokens.dp(DesignTokens.ARTWORK_GUTTER_PORTRAIT);
        artwork.setLayoutParams(params);
    }

    // -------------------------------------------------------------- plumbing

    private TextView label(Context context, float sizeSp, int color) {
        TextView view = new TextView(context);
        view.setTextSize(TypedValue.COMPLEX_UNIT_SP, sizeSp);
        view.setTextColor(color);
        view.setIncludeFontPadding(false);
        return view;
    }

    private LinearLayout row(Context context, int gravity) {
        LinearLayout view = new LinearLayout(context);
        view.setOrientation(LinearLayout.HORIZONTAL);
        view.setGravity(gravity);
        return view;
    }

    private void addDetails(View view, float gapDp) {
        addDetails(view, gapDp, -1);
    }

    /**
     * Adds a child to the details column.
     *
     * <p>The width is a parameter because a control must not be stretched by a
     * MATCH_PARENT default: a 48dp icon button given the full width became a 360dp
     * target whose glyph floated in the middle of it.
     */
    private void addDetails(View view, float gapDp, int width) {
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(width, -2);
        if (gapDp > 0f) params.topMargin = DesignTokens.dp(gapDp);
        details.addView(view, params);
    }

    /**
     * Writes a label only when it would actually change.
     *
     * <p>{@code setText} on a TextView re-measures and re-lays-out; skipping it is the
     * difference between a notification that costs a comparison and one that costs a
     * layout pass.
     */
    private static void setText(TextView view, CharSequence value) {
        if (TextUtils.equals(view.getText(), value)) return;
        view.setText(value);
    }

    private static boolean equal(TrackKey left, TrackKey right) {
        return left == null ? right == null : left.equals(right);
    }

    private static boolean isWaiting(PlaybackState state) {
        return state == PlaybackState.BUFFERING
                || state == PlaybackState.RESOLVING_METADATA
                || state == PlaybackState.RESOLVING_SOURCE
                || state == PlaybackState.SEARCHING;
    }

    private static CharSequence attributionOf(MediaTrack track) {
        String who = track.artist.isEmpty() ? track.channel : track.artist;
        return who;
    }

    private static Icon downloadIcon(DownloadState.Phase phase) {
        switch (phase) {
            case COMPLETED:
            case ALREADY_DOWNLOADED: return Icon.DOWNLOADED;
            case FAILED:
            case CANCELLED: return Icon.RETRY;
            default: return Icon.DOWNLOAD;
        }
    }

    private static SecondaryActionButton.Phase downloadPhaseOf(DownloadState.Phase phase) {
        switch (phase) {
            case RESOLVING_SOURCE: return SecondaryActionButton.Phase.PENDING;
            case DOWNLOADING: return SecondaryActionButton.Phase.RUNNING;
            case COMPLETED:
            case ALREADY_DOWNLOADED: return SecondaryActionButton.Phase.DONE;
            case FAILED:
            case CANCELLED: return SecondaryActionButton.Phase.FAILED;
            default: return SecondaryActionButton.Phase.IDLE;
        }
    }

    /**
     * What the download action currently offers.
     *
     * <p>The glyph is {@link Icon#DOWNLOADED} once a file exists, and the action then
     * removes it: an icon-only control has to keep saying what its tap will do, or
     * cancelling and removing become indistinguishable (§8, §26).
     */
    private static CharSequence downloadDescription(DownloadState.Phase phase, int percent) {
        switch (phase) {
            case RESOLVING_SOURCE: return "Preparing download. Double tap to cancel.";
            case DOWNLOADING:
                return percent >= 0
                        ? "Downloading " + percent + " percent. Double tap to cancel."
                        : "Downloading. Double tap to cancel.";
            case COMPLETED:
            case ALREADY_DOWNLOADED: return "Downloaded. Double tap to remove the download.";
            case FAILED:
            case CANCELLED: return "Download failed. Double tap to try again.";
            default: return "Download. Double tap to start.";
        }
    }
}