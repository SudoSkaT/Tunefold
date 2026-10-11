package com.tunefold.app;

import android.content.Context;
import android.text.TextUtils;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * The persistent strip that says what is playing without being the player.
 *
 * <p>It exists because playback is the centre of this app: a track started anywhere
 * keeps a handle on screen for as long as it lasts, on every destination, without the
 * user leaving what they were doing.
 *
 * <h3>It holds no state of its own</h3>
 * Everything here comes from a {@link PlaybackSession.Snapshot} passed in by the
 * caller, and every setter compares before it writes. There is no second playback
 * model, no cached position and no timer: a mini player that keeps its own copy of
 * the truth is a mini player that is wrong twice a day.
 *
 * <h3>It is thin</h3>
 * 56dp, one row, and the artist line disappears rather than wrap. A strip that grows
 * to fit its content is a strip that pushes the content the user came for off screen.
 *
 * <p>Tapping anywhere that is not the transport opens the full surface. That is the
 * one gesture the strip owns, and it is why the strip is clickable as a whole and not
 * only through a chevron nobody would find.
 */
@android.annotation.SuppressLint("ViewConstructor")
final class MiniPlayer extends LinearLayout {

    /** What the strip asks the screen to do. */
    interface Actions {
        /** Play or pause whatever is loaded. */
        void onPlayPause();

        /** Open the full Now Playing surface. */
        void onExpand();
    }

    private final ArtworkView artwork;
    private final TextView title;
    private final TextView artist;
    private final IconButton playPause;
    private final Actions actions;

    private CharSequence renderedTitle = "";
    private CharSequence renderedArtist = "";
    private boolean renderedPlaying;
    private boolean renderedVisible;
    private String renderedCover = "";
    private final StringBuilder scratch = new StringBuilder(96);

    MiniPlayer(Context context, Actions actions) {
        super(context);
        DesignTokens.init(context);
        this.actions = actions;
        setOrientation(HORIZONTAL);
        setGravity(Gravity.CENTER_VERTICAL);
        int height = DesignTokens.dp(DesignTokens.MINI_PLAYER_HEIGHT);
        setMinimumHeight(height);
        setPadding(DesignTokens.dp(DesignTokens.CONTENT_PADDING_HORIZONTAL), 0,
                DesignTokens.dp(DesignTokens.CONTENT_PADDING_HORIZONTAL), 0);
        setBackground(new android.graphics.drawable.ColorDrawable(
                DesignTokens.Palette.SURFACE_RAISED));
        setClickable(true);
        setFocusable(true);
        setElevation(DesignTokens.dp(DesignTokens.ELEVATION_CARD));

        artwork = new ArtworkView(context);
        artwork.setCornerRadiusDp(DesignTokens.RADIUS_ARTWORK_THUMB);
        int side = DesignTokens.dp(DesignTokens.MINI_PLAYER_ARTWORK);
        LayoutParams artworkParams = new LayoutParams(side, side);
        artworkParams.rightMargin = DesignTokens.dp(DesignTokens.MINI_PLAYER_GAP);
        addView(artwork, artworkParams);

        title = new TextView(context);
        title.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_BODY_SIZE);
        title.setTextColor(DesignTokens.Palette.ON_SURFACE);
        title.setMaxLines(1);
        title.setEllipsize(TextUtils.TruncateAt.END);
        title.setIncludeFontPadding(false);

        artist = new TextView(context);
        artist.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_CAPTION_SIZE);
        artist.setTextColor(DesignTokens.Palette.ON_SURFACE_MUTED);
        artist.setMaxLines(1);
        artist.setEllipsize(TextUtils.TruncateAt.END);
        artist.setIncludeFontPadding(false);

        LinearLayout text = new LinearLayout(context);
        text.setOrientation(VERTICAL);
        LayoutParams textParams = new LayoutParams(0, LayoutParams.WRAP_CONTENT, 1f);
        textParams.rightMargin = DesignTokens.dp(DesignTokens.GAP);
        addView(text, textParams);
        text.addView(title, new LayoutParams(LayoutParams.MATCH_PARENT,
                LayoutParams.WRAP_CONTENT));
        text.addView(artist, new LayoutParams(LayoutParams.MATCH_PARENT,
                LayoutParams.WRAP_CONTENT));

        playPause = new IconButton(context);
        playPause.setAction(Icon.PLAY, "Start playback");
        playPause.setOnClickListener(view -> {
            if (actions != null) actions.onPlayPause();
        });
        addView(playPause, new LayoutParams(DesignTokens.dp(DesignTokens.CONTROL_TARGET),
                DesignTokens.dp(DesignTokens.CONTROL_TARGET)));

        setOnClickListener(view -> {
            if (actions != null) actions.onExpand();
        });
        setVisibility(View.GONE);
    }

    /** The cover sub-view, so the screen can feed it the same bitmap the player has. */
    ArtworkView artwork() { return artwork; }

    /** Applies a session snapshot; the only way this strip changes. */
    void apply(PlaybackSession.Snapshot snapshot) {
        MediaTrack track = snapshot == null ? null : snapshot.currentTrack;
        boolean visible = track != null;
        if (renderedVisible != visible) {
            renderedVisible = visible;
            setVisibility(visible ? View.VISIBLE : View.GONE);
        }
        if (!visible) return;

        CharSequence name = track.title;
        if (!TextUtils.equals(name, renderedTitle)) {
            renderedTitle = name;
            title.setText(renderedTitle);
        }
        CharSequence who = attributionOf(track);
        boolean hasWho = who.length() > 0;
        if (hasWho != (renderedArtist.length() > 0)
                || (hasWho && !TextUtils.equals(who, renderedArtist))) {
            renderedArtist = hasWho ? who : "";
            artist.setText(renderedArtist);
            artist.setVisibility(hasWho ? View.VISIBLE : View.GONE);
        }
        boolean playing = snapshot.state == PlaybackState.PLAYING;
        if (renderedPlaying != playing) {
            renderedPlaying = playing;
            playPause.setAction(playing ? Icon.PAUSE : Icon.PLAY,
                    playing ? "Pause playback" : "Start playback");
        }
        String cover = track.thumbnail.isEmpty() ? "" : identityOf(track);
        if (!cover.equals(renderedCover)) {
            renderedCover = cover;
            artwork.expectCover(cover);
        }
        applyDescription(track, snapshot);
    }

    /** Declares which cover this strip will accept. */
    void expectArtwork(String identity) {
        if (identity == null || identity.equals(renderedCover)) return;
        renderedCover = identity;
        artwork.expectCover(identity);
    }

    /** Applies a cover already decoded for the player. */
    void setArtwork(android.graphics.Bitmap bitmap) {
        artwork.setCover(renderedCover, bitmap);
    }

    /**
     * One node, one sentence.
     *
     * <p>The strip is one thing to a screen reader: what it is, who by, and that
     * opening it shows the player. The transport inside it keeps its own node because
     * it is a control, not a caption.
     *
     * <p>Written only when it changed. This strip is handed a snapshot twice a second
     * for every reason at all, and {@code setContentDescription} is not free: it is an
     * accessibility event, which is a tree walk by another process, twice a second,
     * for a sentence that is almost always the same sentence.
     */
    private void applyDescription(MediaTrack track, PlaybackSession.Snapshot snapshot) {
        scratch.setLength(0);
        scratch.append(track.title);
        CharSequence who = attributionOf(track);
        if (who.length() > 0) scratch.append(", ").append(who);
        scratch.append(". Double tap to open the player.");
        String sentence = scratch.toString();
        if (sentence.equals(renderedDescription)) return;
        renderedDescription = sentence;
        setContentDescription(sentence);
    }

    /** Sentence this strip last published; compared so it is written once. */
    private String renderedDescription = "";

    private static CharSequence attributionOf(MediaTrack track) {
        return track.artist.isEmpty() ? track.channel : track.artist;
    }

    private static String identityOf(MediaTrack track) {
        TrackKey key = track.key();
        return key == null ? "" : key.toString();
    }
}