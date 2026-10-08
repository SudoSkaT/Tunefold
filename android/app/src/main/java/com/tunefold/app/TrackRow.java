package com.tunefold.app;

import android.content.Context;
import android.graphics.Typeface;
import android.text.TextUtils;
import android.util.TypedValue;
import android.view.Gravity;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * One row in a track list: cover, title, attribution, like and download.
 *
 * <p>Built for reuse, because a list here used to be built from scratch every time it
 * was rendered. Each row is constructed once and then re-pointed at a different track
 * by {@link #bind}; {@link TrackList} keeps the row objects so scrolling and section
 * refreshes never inflate a view again.
 *
 * <p>{@link #bind} is a full state application, and every setter inside it is
 * equality-guarded. That means binding a row to a track whose visible state has not
 * changed costs a handful of comparisons and no allocation, which is what makes it
 * safe to re-bind a whole visible list on every download tick.
 */
@android.annotation.SuppressLint("ViewConstructor")
final class TrackRow extends LinearLayout {

    /** What a row offers, so the click handlers never re-read live state to decide. */
    interface Callbacks {
        /** Play this row's track. */
        void onPlay(TrackRow row);

        /** Toggle L1K3D membership for this row's track. */
        void onToggleLike(TrackRow row);

        /** Advance this row's download by one step: start, cancel, retry or remove. */
        void onAdvanceDownload(TrackRow row);
    }

    private final ArtworkView artwork;
    private final TextView title;
    private final TextView attribution;
    private final SecondaryActionButton like;
    private final SecondaryActionButton download;
    private final Callbacks callbacks;

    /** Reused so composing an accessibility description allocates nothing. */
    private final StringBuilder scratch = new StringBuilder(96);
    private MediaTrack track;
    private TrackKey key;
    private CharSequence renderedTitle = "";
    private CharSequence renderedAttribution = "";
    private boolean liked;
    private DownloadState.Phase downloadPhase = DownloadState.Phase.IDLE;
    private int downloadPercent = -1;

    TrackRow(Context context, Callbacks callbacks) {
        super(context);
        DesignTokens.init(context);
        this.callbacks = callbacks;
        setOrientation(HORIZONTAL);
        setGravity(Gravity.CENTER_VERTICAL);
        int rowHeight = DesignTokens.dp(DesignTokens.ROW_HEIGHT);
        setMinimumHeight(rowHeight);
        setClickable(true);
        setFocusable(true);
        setBackground(IconButton.ripple(DesignTokens.Palette.RIPPLE, false));

        artwork = new ArtworkView(context);
        artwork.setCornerRadiusDp(DesignTokens.RADIUS_ARTWORK_THUMB);
        int thumb = DesignTokens.dp(DesignTokens.ARTWORK_THUMB);
        LayoutParams artworkParams = new LayoutParams(thumb, thumb);
        artworkParams.rightMargin = DesignTokens.dp(DesignTokens.ROW_GAP);
        addView(artwork, artworkParams);

        LinearLayout text = new LinearLayout(context);
        text.setOrientation(VERTICAL);
        LayoutParams textParams = new LayoutParams(0, LayoutParams.WRAP_CONTENT, 1f);
        textParams.rightMargin = DesignTokens.dp(DesignTokens.GAP);
        addView(text, textParams);

        title = new TextView(context);
        title.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_BODY_SIZE);
        title.setTextColor(DesignTokens.Palette.ON_SURFACE);
        title.setMaxLines(1);
        title.setEllipsize(TextUtils.TruncateAt.END);
        text.addView(title, new LayoutParams(LayoutParams.MATCH_PARENT,
                LayoutParams.WRAP_CONTENT));

        attribution = new TextView(context);
        attribution.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_CAPTION_SIZE);
        attribution.setTextColor(DesignTokens.Palette.ON_SURFACE_MUTED);
        attribution.setMaxLines(1);
        attribution.setEllipsize(TextUtils.TruncateAt.END);
        text.addView(attribution, new LayoutParams(LayoutParams.MATCH_PARENT,
                LayoutParams.WRAP_CONTENT));

        like = new SecondaryActionButton(context);
        like.setOnClickListener(view -> {
            if (callbacks != null) callbacks.onToggleLike(this);
        });
        like.setAction(Icon.HEART, "Not liked. Double tap to add to L1K3D.");
        addView(like, new LayoutParams(DesignTokens.dp(DesignTokens.CONTROL_TARGET_SECONDARY),
                DesignTokens.dp(DesignTokens.CONTROL_TARGET_SECONDARY)));

        download = new SecondaryActionButton(context);
        download.setOnClickListener(view -> {
            if (callbacks != null) callbacks.onAdvanceDownload(this);
        });
        download.setAction(Icon.DOWNLOAD, "Not downloaded. Double tap to download.");
        LayoutParams downloadParams = new LayoutParams(
                DesignTokens.dp(DesignTokens.CONTROL_TARGET_SECONDARY),
                DesignTokens.dp(DesignTokens.CONTROL_TARGET_SECONDARY));
        downloadParams.leftMargin = DesignTokens.dp(DesignTokens.SPACE_2);
        addView(download, downloadParams);

        setOnClickListener(view -> {
            if (callbacks != null) callbacks.onPlay(this);
        });
    }

    /** The track this row currently shows. */
    MediaTrack track() { return track; }

    /** The identity this row currently shows. */
    TrackKey key() { return key; }

    /** L1K3D membership as last applied. */
    boolean isLiked() { return liked; }

    /** Download phase as last applied. */
    DownloadState.Phase downloadPhase() { return downloadPhase; }

    /** The artwork sub-view, so a host can feed it decoded covers. */
    ArtworkView artwork() { return artwork; }

    /** Re-points the row at {@code value}, applying every visible piece of state. */
    void bind(MediaTrack value, boolean isLiked, DownloadState download) {
        track = value;
        key = value == null ? null : value.key();
        applyIdentity();
        applyText();
        applyLike(isLiked);
        applyDownload(download);
    }

    /**
     * Declares which cover this row will accept.
     *
     * <p>Separate from {@link #bind} because the cover arrives asynchronously: the row
     * refuses any bitmap whose identity is not the one it declared.
     */
    void expectArtwork(String identity) {
        artwork.expectCover(identity);
    }

    /** Applies a cover decoded for this row. */
    void setArtwork(android.graphics.Bitmap bitmap) {
        artwork.setCover(artworkIdentity(), bitmap);
    }

    /** Identity used to validate this row's artwork. */
    String artworkIdentity() {
        if (key == null) return "";
        return key.provider() + "/" + key.providerTrackId();
    }

    /** Applies L1K3D membership. */
    void applyLike(boolean value) {
        if (liked == value) return;
        liked = value;
        like.setToggleState(value);
        like.setPhase(value ? Icon.HEART_FILLED : Icon.HEART,
                value ? SecondaryActionButton.Phase.DONE : SecondaryActionButton.Phase.IDLE,
                -1);
        like.applyDescription(value
                ? "Liked. Double tap to remove from L1K3D."
                : "Not liked. Double tap to add to L1K3D.");
    }

    /** Applies download state. */
    void applyDownload(DownloadState value) {
        DownloadState state = value == null ? DownloadState.idle(key) : value;
        int percent = state.phase == DownloadState.Phase.DOWNLOADING
                ? state.progressPercent() : -1;
        if (state.phase == downloadPhase && percent == downloadPercent) return;
        downloadPhase = state.phase;
        downloadPercent = percent;
        download.setPhase(downloadIcon(state.phase), phaseFor(state.phase), percent);
        download.applyDescription(downloadDescription(state.phase, percent));
        applyDescription();
    }

    private void applyIdentity() {
        if (key == null) {
            artwork.expectCover(null);
            artwork.clearCover();
        } else {
            artwork.expectCover(artworkIdentity());
        }
    }

    private void applyText() {
        CharSequence nextTitle = track == null ? "" : track.title;
        if (!TextUtils.equals(nextTitle, renderedTitle)) {
            renderedTitle = nextTitle;
            title.setText(renderedTitle);
        }
        CharSequence nextAttribution = track == null ? "" : attributionOf(track);
        if (!TextUtils.equals(nextAttribution, renderedAttribution)) {
            renderedAttribution = nextAttribution;
            attribution.setText(renderedAttribution);
        }
        applyDescription();
    }

    private static CharSequence attributionOf(MediaTrack value) {
        String who = value.artist.isEmpty() ? value.channel : value.artist;
        return who;
    }

    /**
     * One description for the whole row.
     *
     * <p>A row is one accessibility node: title, attribution, and what the two actions
     * will do. Exposing three separate nodes meant a screen reader read "Like" and
     * "Download" with no indication of which track they referred to.
     */
    private void applyDescription() {
        CharSequence name = track == null ? "" : track.title;
        CharSequence who = track == null ? "" : attributionOf(track);
        scratch.setLength(0);
        scratch.append(name);
        if (who.length() > 0) scratch.append(", ").append(who);
        scratch.append(". ");
        scratch.append(liked ? "Liked. " : "Not liked. ");
        scratch.append(downloadDescription(downloadPhase, downloadPercent));
        // Copied out, never handed over as the builder: the View would keep a reference
        // and start announcing text this row has already moved on from.
        setContentDescription(scratch.toString());
    }

    /**
     * What the download action currently offers.
     *
     * <p>Kept in step with the glyph in {@link #downloadIcon}: an action that runs must
     * stay reachable to cancel, because a cancellation the user cannot reach looks
     * broken (§8, §26).
     */
    private static CharSequence downloadDescription(DownloadState.Phase phase, int percent) {
        switch (phase) {
            case RESOLVING_SOURCE: return "Preparing download. Double tap to cancel.";
            case DOWNLOADING:
                return percent >= 0
                        ? "Downloading " + percent + " percent. Double tap to cancel."
                        : "Downloading. Double tap to cancel.";
            case COMPLETED:
            case ALREADY_DOWNLOADED: return "Downloaded. Double tap to remove.";
            case FAILED:
            case CANCELLED: return "Download failed. Double tap to retry.";
            default: return "Not downloaded. Double tap to download.";
        }
    }

    private static Icon downloadIcon(DownloadState.Phase phase) {
        switch (phase) {
            case COMPLETED:
            case ALREADY_DOWNLOADED: return Icon.DOWNLOADED;
            case DOWNLOADING:
            case RESOLVING_SOURCE: return Icon.DOWNLOAD;
            case FAILED:
            case CANCELLED: return Icon.RETRY;
            default: return Icon.DOWNLOAD;
        }
    }

    private static SecondaryActionButton.Phase phaseFor(DownloadState.Phase phase) {
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

    /** Marks the row as the one currently playing. */
    void setActiveTrack(boolean active) {
        if (isActivated() == active) return;
        setActivated(active);
        title.setTypeface(active ? Typeface.DEFAULT_BOLD : Typeface.DEFAULT);
    }
}