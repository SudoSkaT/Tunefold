package com.tunefold.app;

import android.content.Context;
import android.view.Gravity;
import android.view.View;
import android.widget.LinearLayout;
import android.widget.TextView;

import java.util.ArrayList;
import java.util.List;

final class ArtistDetailSurface extends SectionSurface {

    private final ArtworkView artwork;
    private final TextView title;
    private final TextView description;
    private final TrackGroup tracks;
    private final TrackGroup albums;
    private final IconButton back;
    private final IconButton play;
    private final IconButton shuffle;
    private final TextView meta;
    private MediaTrack artist;
    private java.util.List<MediaTrack> loadedTracks = new java.util.ArrayList<>();

    ArtistDetailSurface(Context context, Host host) {
        super(context, host);
        setVisibility(View.GONE);

        LinearLayout header = new LinearLayout(context);
        header.setOrientation(LinearLayout.HORIZONTAL);
        header.setGravity(Gravity.CENTER_VERTICAL);
        back = new IconButton(context);
        back.setAction(Icon.BACK, "Back");
        back.setOnClickListener(view -> ((MainActivity) context).closeArtistDetail());
        header.addView(back, new LinearLayout.LayoutParams(
                DesignTokens.dp(DesignTokens.CONTROL_TARGET),
                DesignTokens.dp(DesignTokens.CONTROL_TARGET)));
        title = new TextView(context);
        title.setTextSize(android.util.TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_TITLE_SIZE);
        title.setTextColor(DesignTokens.Palette.ON_SURFACE);
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

        description = new TextView(context);
        description.setTextSize(android.util.TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_BODY_SIZE);
        description.setTextColor(DesignTokens.Palette.ON_SURFACE_MUTED);
        description.setGravity(Gravity.CENTER_HORIZONTAL);
        addView(description, new LinearLayout.LayoutParams(
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
        play.setAction(Icon.PLAY, "Play artist");
        play.setOnClickListener(view -> playTracks(false));
        controls.addView(play, new LinearLayout.LayoutParams(
                DesignTokens.dp(DesignTokens.CONTROL_TARGET),
                DesignTokens.dp(DesignTokens.CONTROL_TARGET)));
        shuffle = new IconButton(context);
        shuffle.setAction(Icon.SHUFFLE, "Play artist shuffled");
        shuffle.setOnClickListener(view -> playTracks(true));
        controls.addView(shuffle, new LinearLayout.LayoutParams(
                DesignTokens.dp(DesignTokens.CONTROL_TARGET),
                DesignTokens.dp(DesignTokens.CONTROL_TARGET)));
        addView(controls, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT));

        tracks = new TrackGroup(context, "Tracks", host.rowPool(), host, host.scrollHost());
        addView(tracks, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT));

        albums = new TrackGroup(context, "Albums", host.rowPool(), host, host.scrollHost());
        addView(albums, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT));
    }

    void open(MediaTrack value) {
        artist = value;
        loadedTracks.clear();
        title.setText(value == null ? "" : value.title);
        description.setText(value == null ? "" : (value.artist.isEmpty() ? value.channel : value.artist));
        meta.setText("");
        tracks.showLoading();
        albums.showLoading();
        tracks.setVisibleGroup(true);
        albums.setVisibleGroup(true);
        if (value != null) {
            ((MainActivity) getContext()).loadAlbumArtwork(value, artwork);
            artwork.expectCover(value.key() == null ? "" : value.key().toString());
            fetch(value);
        }
    }

    private void fetch(MediaTrack value) {
        ((MainActivity) getContext()).fetchArtistContent(value, this);
    }

    void setTracksAndAlbums(List<MediaTrack> trackList, List<MediaTrack> albumList, String error) {
        if (error != null) {
            tracks.showError(error, () -> { });
            albums.showError(error, () -> { });
            return;
        }
        loadedTracks = trackList == null ? new java.util.ArrayList<>() : new java.util.ArrayList<>(trackList);
        if (loadedTracks.isEmpty()) {
            tracks.showEmpty("No tracks");
            meta.setText("0 songs");
        } else {
            tracks.show(loadedTracks, false);
            meta.setText(loadedTracks.size() + " songs");
        }
        if (albumList == null || albumList.isEmpty()) {
            albums.showEmpty("No albums");
        } else {
            albums.show(albumList, false);
        }
    }

    private void playTracks(boolean shuffleMode) {
        if (loadedTracks.isEmpty()) return;
        java.util.List<MediaTrack> toPlay = new java.util.ArrayList<>(loadedTracks);
        if (shuffleMode) java.util.Collections.shuffle(toPlay);
        ((MainActivity) getContext()).playAlbumTracks(toPlay);
    }

    @Override void onEnter() { }

    @Override void onShow() { }

    @Override void setActiveKey(TrackKey key) {
        tracks.setActiveTrack(key);
        albums.setActiveTrack(key);
    }
}
