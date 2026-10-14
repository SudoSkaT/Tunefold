package com.tunefold.app;

import android.content.Context;
import android.graphics.Typeface;
import android.text.TextUtils;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
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

    /**
     * Where covers come from, as one function.
     *
     * <p>A row asks for its cover when it is bound, so a row that was never built can
     * never ask for one: that is the whole memory argument for a windowed list, and it
     * only holds if the ask happens here and nowhere else.
     */
    interface ArtworkLoader {
        /**
         * @param track    the track whose cover is wanted
         * @param identity the identity the answer must carry
         * @param sidePx   the side the cover will be drawn at
         */
        void load(MediaTrack track, String identity, int sidePx);
    }

    /** Asks the row for its contextual actions; long press and overflow share it. */
    interface OverflowHost {
        /**
         * @param row     the row the actions belong to
         * @param anchor  the view to show them next to
         */
        void onOverflow(TrackRow row, View anchor);
    }

    private final ArtworkView artwork;
    private final TextView title;
    private final TextView attribution;
    private final TextView status;
    private final TextView duration;
    private final IconButton overflow;
    private final Callbacks callbacks;
    private ArtworkLoader loader;
    private OverflowHost overflowHost;

    /** Reused so composing an accessibility description allocates nothing. */
    private final StringBuilder scratch = new StringBuilder(96);
    private MediaTrack track;
    private TrackKey key;
    /** The list this row belongs to, which is what a tap plays. */
    private TrackList list;
    private CharSequence renderedTitle = "";
    private CharSequence renderedAttribution = "";
    private boolean liked;
    private DownloadState.Phase downloadPhase = DownloadState.Phase.IDLE;
    private int downloadPercent = -1;
    private CharSequence renderedStatus = "";
    private CharSequence renderedDuration = "";
    private int artworkSidePx;

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

        // Status: what this track already IS, not what it could be. A row is scanned,
        // not operated, and the two actions it does offer live in one place: the
        // overflow. Two always-present buttons per row is also two always-present
        // focus stops in a list of fifty.
        status = new TextView(context);
        status.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_CAPTION_SIZE);
        status.setLetterSpacing(DesignTokens.TYPE_LABEL_TRACKING);
        status.setMaxLines(1);
        status.setTextColor(DesignTokens.Palette.ON_SURFACE_FAINT);
        status.setGravity(Gravity.CENTER);
        LayoutParams statusParams = new LayoutParams(LayoutParams.WRAP_CONTENT,
                DesignTokens.dp(DesignTokens.ROW_STATUS_HEIGHT));
        statusParams.rightMargin = DesignTokens.dp(DesignTokens.SPACE_2);
        text.addView(status, statusParams);

        duration = new TextView(context);
        duration.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_CAPTION_SIZE);
        duration.setTextColor(DesignTokens.Palette.ON_SURFACE_MUTED);
        duration.setGravity(Gravity.END | Gravity.CENTER_VERTICAL);
        duration.setMaxLines(1);
        LayoutParams durationParams = new LayoutParams(
                DesignTokens.dp(DesignTokens.ROW_DURATION_WIDTH),
                DesignTokens.dp(DesignTokens.ROW_STATUS_HEIGHT));
        durationParams.rightMargin = DesignTokens.dp(DesignTokens.SPACE_1);
        addView(duration, durationParams);

        overflow = new IconButton(context, DesignTokens.CONTROL_TARGET_OVERFLOW,
                DesignTokens.ICON_SIZE_SMALL);
        overflow.setAction(Icon.MORE, "More actions");
        overflow.setOnClickListener(view -> {
            if (overflowHost != null) overflowHost.onOverflow(this, overflow);
        });
        addView(overflow, new LayoutParams(
                DesignTokens.dp(DesignTokens.CONTROL_TARGET_OVERFLOW),
                DesignTokens.dp(DesignTokens.CONTROL_TARGET_OVERFLOW)));

        setOnClickListener(view -> {
            if (callbacks != null) callbacks.onPlay(this);
        });
        // A long press opens the same menu as the button: the gesture is the one people
        // already know, and the button is the one a screen reader can reach.
        setOnLongClickListener(view -> {
            if (overflowHost != null) overflowHost.onOverflow(this, overflow);
            return true;
        });
    }

    /** The track this row currently shows. */
    MediaTrack track() { return track; }

    /**
     * Declares which list this row belongs to.
     *
     * <p>On the row, and re-declared on every binding, because a row is pooled: the
     * same object is handed to whichever list asks for one next, and a row that
     * remembered the previous list would make "play this track" mean "play a list the
     * user is no longer looking at".
     */
    void setList(TrackList value) { list = value; }

    /**
     * The tracks of the list this row belongs to, in display order.
     *
     * <p>What a tap plays: the row's own track, plus the list it was tapped in, so
     * the song that plays next is the one the list shows next. Empty when the row
     * belongs to no list, which the caller treats as "just this track".
     */
    java.util.List<MediaTrack> listTracks() {
        return list == null ? java.util.Collections.<MediaTrack>emptyList() : list.tracks();
    }

    /** The identity this row currently shows. */
    TrackKey key() { return key; }

    /** L1K3D membership as last applied. */
    boolean isLiked() { return liked; }

    /** Download phase as last applied. */
    DownloadState.Phase downloadPhase() { return downloadPhase; }

    /** The artwork sub-view, so a host can feed it decoded covers. */
    ArtworkView artwork() { return artwork; }

    /** The one path covers take, set once per screen. */
    void setArtworkLoader(ArtworkLoader value) { loader = value; }

    /** Where the contextual actions are shown from. */
    void setOverflowHost(OverflowHost value) { overflowHost = value; }

    /** The action that advances this row's download by one step, for a menu. */
    void advanceDownload() {
        if (callbacks != null) callbacks.onAdvanceDownload(this);
    }

    /** Toggles L1K3D membership for this row's track, for a menu. */
    void toggleLike() {
        if (callbacks != null) callbacks.onToggleLike(this);
    }

    /** Re-points the row at {@code value}, applying every visible piece of state. */
    void bind(MediaTrack value, boolean isLiked, DownloadState download) {
        track = value;
        key = value == null ? null : value.key();
        applyIdentity();
        applyText();
        applyDuration();
        applyLike(isLiked);
        applyDownload(download);
        requestCover();
    }

    /**
     * Asks for the cover of whatever this row now shows.
     *
     * <p>Through the pool's loader, once per binding: a row that is rebound to the
     * track it already showed asks again only because the loader answers from its
     * cache, which is where that answer belongs.
     *
     * <p><b>Asked even when the track has no cover URL.</b> A track remembered from a
     * session that is gone carries only its identity, and the loader is the one that
     * can turn that into a cover — so refusing to ask here is what left a whole
     * library of grey squares on screen for ever.
     */
    private void requestCover() {
        if (track == null || loader == null) return;
        artworkSidePx = DesignTokens.dp(DesignTokens.ARTWORK_THUMB);
        loader.load(track, artworkIdentity(), artworkSidePx);
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

    /** Applies L1K3D membership, which the row shows as status, not as a control. */
    void applyLike(boolean value) {
        if (liked == value) return;
        liked = value;
        applyStatus();
        applyDescription();
    }

    /** Applies download state. */
    void applyDownload(DownloadState value) {
        DownloadState state = value == null ? DownloadState.idle(key) : value;
        int percent = state.phase == DownloadState.Phase.DOWNLOADING
                ? state.progressPercent() : -1;
        if (state.phase == downloadPhase && percent == downloadPercent) return;
        downloadPhase = state.phase;
        downloadPercent = percent;
        applyStatus();
        applyDescription();
    }

    /**
     * The optional status line: what this track already is.
     *
     * <p>Empty when there is nothing to say, so a plain track's row is title, artist,
     * duration and one button. Liked and downloaded are the two states a row earns,
     * and a running download shows its own word because that is the only place it is
     * visible without opening the player.
     */
    private void applyStatus() {
        CharSequence next = statusOf(downloadPhase, downloadPercent, liked);
        if (TextUtils.equals(next, renderedStatus)) return;
        renderedStatus = next;
        status.setText(renderedStatus);
        status.setVisibility(renderedStatus.length() == 0 ? View.GONE : View.VISIBLE);
    }

    /** The words a row shows for what this track already is; package-visible to test. */
    static CharSequence statusOf(DownloadState.Phase phase, int percent, boolean liked) {
        String word;
        switch (phase) {
            case RESOLVING_SOURCE: word = "PREPARING"; break;
            case DOWNLOADING:
                word = percent >= 0 ? "SAVING " + percent + "%" : "SAVING";
                break;
            case COMPLETED:
            case ALREADY_DOWNLOADED: word = "SAVED"; break;
            case FAILED:
            case CANCELLED: word = "FAILED"; break;
            default:
                word = liked ? "LIKED" : "";
                break;
        }
        return word;
    }

    /**
     * The track's own duration, when the provider told us.
     *
     * <p>Unknown stays blank rather than becoming {@code --:--}: a row that does not
     * know how long a song is has no business pretending it is zero.
     */
    private void applyDuration() {
        long milliseconds = track == null ? -1L : track.durationMs;
        CharSequence next = milliseconds > 0L
                ? TimeFormat.time(milliseconds) : "";
        if (TextUtils.equals(next, renderedDuration)) return;
        renderedDuration = next;
        duration.setText(renderedDuration);
        duration.setVisibility(renderedDuration.length() == 0 ? View.GONE : View.VISIBLE);
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
        applyStatus();
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
        CharSequence length = renderedDuration;
        if (length.length() > 0) scratch.append(", ").append(length);
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