package com.tunefold.app;

import android.content.Context;
import android.view.Gravity;
import android.view.View;
import android.widget.LinearLayout;
import android.widget.TextView;

import java.util.ArrayList;
import java.util.Collections;
import java.util.List;

final class AlbumDetailSurface extends SectionSurface {

    private final ArtworkView artwork;
    private final TextView title;
    private final TextView artist;
    private final TextView meta;
    private final TrackGroup tracks;
    private final IconButton back;
    private final IconButton play;

    private MediaTrack album;
    private List<MediaTrack> loadedTracks = new ArrayList<>();

    AlbumDetailSurface(Context context, Host host) {
        super(context, host);
        setVisibility(View.GONE);

        LinearLayout header = new LinearLayout(context);
        header.setOrientation(LinearLayout.HORIZONTAL);
        header.setGravity(Gravity.CENTER_VERTICAL);
        back = new IconButton(context);
        back.setAction(Icon.BACK, "Back");
        back.setOnClickListener(view -> ((MainActivity) context).closeAlbumDetail());
        header.addView(back, new LinearLayout.LayoutParams(
                DesignTokens.dp(DesignTokens.CONTROL_TARGET),
                DesignTokens.dp(DesignTokens.CONTROL_TARGET)));
        title = new TextView(context);
        title.setTextSize(android.util.TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_TITLE_SIZE);
        title.setTextColor(DesignTokens.Palette.ON_SURFACE);
        title.setMaxLines(2);
        title.setEllipsize(android.text.TextUtils.TruncateAt.END);
        LinearLayout.LayoutParams titleParams = new LinearLayout.LayoutParams(0,
                LinearLayout.LayoutParams.WRAP_CONTENT, 1f);
        titleParams.leftMargin = DesignTokens.dp(DesignTokens.SPACE_2);
        header.addView(title, titleParams);
        addView(header, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT));

        artwork = new ArtworkView(context);
        artwork.setCornerRadiusDp(DesignTokens.RADIUS_LARGE);
        int side = DesignTokens.dp(160);
        LinearLayout.LayoutParams artParams = new LinearLayout.LayoutParams(side, side);
        artParams.gravity = Gravity.CENTER_HORIZONTAL;
        artParams.topMargin = DesignTokens.dp(DesignTokens.SPACE_3);
        addView(artwork, artParams);

        artist = new TextView(context);
        artist.setTextSize(android.util.TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_BODY_SIZE);
        artist.setTextColor(DesignTokens.Palette.ON_SURFACE_MUTED);
        artist.setGravity(Gravity.CENTER_HORIZONTAL);
        addView(artist, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT));

        meta = new TextView(context);
        meta.setTextSize(android.util.TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_CAPTION_SIZE);
        meta.setTextColor(DesignTokens.Palette.ON_SURFACE_MUTED);
        meta.setGravity(Gravity.CENTER_HORIZONTAL);
        addView(meta, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT));

        LinearLayout controls = new LinearLayout(context);
        controls.setOrientation(LinearLayout.HORIZONTAL);
        controls.setGravity(Gravity.CENTER);
        play = new IconButton(context);
        play.setAction(Icon.PLAY, "Play album");
        play.setOnClickListener(view -> playAlbum(false));
        controls.addView(play, new LinearLayout.LayoutParams(
                DesignTokens.dp(DesignTokens.CONTROL_TARGET),
                DesignTokens.dp(DesignTokens.CONTROL_TARGET)));
        IconButton shuffle = new IconButton(context);
        shuffle.setAction(Icon.SHUFFLE, "Play album shuffled");
        shuffle.setOnClickListener(view -> playAlbum(true));
        controls.addView(shuffle, new LinearLayout.LayoutParams(
                DesignTokens.dp(DesignTokens.CONTROL_TARGET),
                DesignTokens.dp(DesignTokens.CONTROL_TARGET)));
        addView(controls, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT));

        tracks = new TrackGroup(context, "Tracks", host.rowPool(), host, host.scrollHost());
        tracks.setRangeListener((first, count) -> { });
        addView(tracks, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT));
    }

    void open(MediaTrack value) {
        album = value;
        loadedTracks.clear();
        title.setText(value == null ? "" : value.title);
        artist.setText(value == null ? "" : value.artist);
        meta.setText("");
        tracks.showLoading();
        tracks.setVisibleGroup(true);
        if (value != null) {
            ((MainActivity) getContext()).loadAlbumArtwork(value, artwork);
            artwork.expectCover(value.key() == null ? "" : value.key().toString());
        }
    }

    void setTracks(List<MediaTrack> value, String error) {
        if (album == null) return;
        loadedTracks = value == null ? new ArrayList<>() : new ArrayList<>(value);
        if (error != null) {
            tracks.showError(error, () -> { });
            return;
        }
        if (loadedTracks.isEmpty()) {
            tracks.showEmpty("No tracks");
            meta.setText("0 songs");
            return;
        }
        long totalMs = 0;
        for (MediaTrack t : loadedTracks) if (t.durationMs > 0) totalMs += t.durationMs;
        meta.setText(loadedTracks.size() + " songs · " + formatDuration(totalMs));
        tracks.show(loadedTracks, false);
        tracks.setRangeListener((first, count) -> { });
    }

    private void playAlbum(boolean shuffleMode) {
        if (loadedTracks.isEmpty()) return;
        List<MediaTrack> toPlay = new ArrayList<>(loadedTracks);
        if (shuffleMode) Collections.shuffle(toPlay);
        ((MainActivity) getContext()).playAlbumTracks(toPlay);
    }

    private static String formatDuration(long ms) {
        long totalSeconds = ms / 1000L;
        long minutes = totalSeconds / 60L;
        long seconds = totalSeconds % 60L;
        return minutes + ":" + (seconds < 10 ? "0" : "") + seconds;
    }

    @Override void onEnter() { }

    @Override void onShow() { }

    @Override void setActiveKey(TrackKey key) {
        tracks.setActiveTrack(key);
    }

}