package com.tunefold.app;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNotEquals;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

import java.util.Collections;

/**
 * Notification content identity and action mapping (§17, Fase 4).
 *
 * <p>Guards the two Fase-4 contracts: the notification re-notifies only when
 * something visible changed (track, transport state, like, download phase),
 * and the Like/Download actions render from live state.
 */
public class NotificationActionsTest {

    private static MediaTrack track(String id) {
        return new MediaTrack("YouTube", id, "Title " + id, "Artist", "", "",
                "artist", 180_000L, "https://img.example/" + id + ".jpg", "", "{}");
    }

    private static PlaybackSession.Snapshot snapshot(PlaybackState state, String id,
                                                     boolean liked, DownloadState download) {
        MediaTrack track = id == null ? null : track(id);
        TrackKey key = track == null ? null : track.key();
        return new PlaybackSession.Snapshot(state, track, key, null, liked,
                download != null && download.isDownloaded(), download == null
                        ? DownloadState.idle(key) : download,
                new PlaybackQueue(), 0, true, false, false, 0L);
    }

    @Test public void sameSnapshotHasTheSameContentKey() {
        String left = PlaybackNotification.contentKey(
                snapshot(PlaybackState.PLAYING, "a", false, DownloadState.idle(null)));
        String right = PlaybackNotification.contentKey(
                snapshot(PlaybackState.PLAYING, "a", false, DownloadState.idle(null)));
        assertEquals(left, right);
    }

    @Test public void trackChangeMovesTheContentKey() {
        String playing = PlaybackNotification.contentKey(
                snapshot(PlaybackState.PLAYING, "a", false, DownloadState.idle(null)));
        String next = PlaybackNotification.contentKey(
                snapshot(PlaybackState.PLAYING, "b", false, DownloadState.idle(null)));
        assertNotEquals(playing, next);
    }

    @Test public void transportStateMovesTheContentKey() {
        String playing = PlaybackNotification.contentKey(
                snapshot(PlaybackState.PLAYING, "a", false, DownloadState.idle(null)));
        String paused = PlaybackNotification.contentKey(
                snapshot(PlaybackState.PAUSED, "a", false, DownloadState.idle(null)));
        assertNotEquals(playing, paused);
    }

    @Test public void likeMovesTheContentKey() {
        TrackKey key = TrackKey.of("YouTube", "a");
        String unliked = PlaybackNotification.contentKey(snapshot(
                PlaybackState.PLAYING, "a", false, DownloadState.idle(key)));
        String liked = PlaybackNotification.contentKey(snapshot(
                PlaybackState.PLAYING, "a", true, DownloadState.idle(key)));
        assertNotEquals(unliked, liked);
    }

    @Test public void downloadPhaseMovesTheContentKey() {
        TrackKey key = TrackKey.of("YouTube", "a");
        String idle = PlaybackNotification.contentKey(snapshot(
                PlaybackState.PLAYING, "a", false, DownloadState.idle(key)));
        String busy = PlaybackNotification.contentKey(snapshot(
                PlaybackState.PLAYING, "a", false,
                new DownloadState(key, DownloadState.Phase.DOWNLOADING, 50, 100, 1, "")));
        assertNotEquals(idle, busy);
    }

    @Test public void downloadProgressAloneDoesNotReNotify() {
        // The notification shows the download as a state, never as a number:
        // a 200ms progress tick must not rebuild it.
        TrackKey key = TrackKey.of("YouTube", "a");
        String half = PlaybackNotification.contentKey(snapshot(
                PlaybackState.PLAYING, "a", false,
                new DownloadState(key, DownloadState.Phase.DOWNLOADING, 50, 100, 1, "")));
        String nearly = PlaybackNotification.contentKey(snapshot(
                PlaybackState.PLAYING, "a", false,
                new DownloadState(key, DownloadState.Phase.DOWNLOADING, 90, 100, 1, "")));
        assertEquals(half, nearly);
    }

    @Test public void likeIconFollowsMembership() {
        assertEquals(Icon.HEART_FILLED, PlaybackNotification.likeIconFor(true));
        assertEquals(Icon.HEART, PlaybackNotification.likeIconFor(false));
    }

    @Test public void downloadIconOffersCancelWhileActive() {
        assertEquals(Icon.CLOSE, PlaybackNotification.downloadIconFor(
                DownloadState.Phase.RESOLVING_SOURCE));
        assertEquals(Icon.CLOSE, PlaybackNotification.downloadIconFor(
                DownloadState.Phase.DOWNLOADING));
    }

    @Test public void downloadIconOffersRetryWhenRetryable() {
        assertEquals(Icon.RETRY, PlaybackNotification.downloadIconFor(
                DownloadState.Phase.FAILED));
        assertEquals(Icon.RETRY, PlaybackNotification.downloadIconFor(
                DownloadState.Phase.CANCELLED));
    }

    @Test public void downloadIconShowsDoneWhenStored() {
        assertEquals(Icon.DOWNLOADED, PlaybackNotification.downloadIconFor(
                DownloadState.Phase.COMPLETED));
        assertEquals(Icon.DOWNLOADED, PlaybackNotification.downloadIconFor(
                DownloadState.Phase.ALREADY_DOWNLOADED));
        assertEquals(Icon.DOWNLOAD, PlaybackNotification.downloadIconFor(
                DownloadState.Phase.IDLE));
    }

    @Test public void downloadTitlesMatchTheInAppControl() {
        assertEquals("Cancel download", PlaybackNotification.downloadTitleFor(
                DownloadState.Phase.DOWNLOADING));
        assertEquals("Retry download", PlaybackNotification.downloadTitleFor(
                DownloadState.Phase.FAILED));
        assertEquals("Downloaded", PlaybackNotification.downloadTitleFor(
                DownloadState.Phase.COMPLETED));
        assertEquals("Download", PlaybackNotification.downloadTitleFor(
                DownloadState.Phase.IDLE));
    }

    @Test public void likeAndDownloadActionsAreDistinct() {
        assertTrue(!PlaybackNotification.ACTION_LIKE.equals(PlaybackNotification.ACTION_DOWNLOAD));
        assertTrue(!PlaybackNotification.ACTION_LIKE.equals(PlaybackNotification.ACTION_TOGGLE));
        assertTrue(!PlaybackNotification.ACTION_DOWNLOAD.equals(PlaybackNotification.ACTION_NEXT));
        assertTrue(Collections.unmodifiableList(java.util.Arrays.asList(
                PlaybackNotification.ACTION_PREVIOUS, PlaybackNotification.ACTION_TOGGLE,
                PlaybackNotification.ACTION_NEXT, PlaybackNotification.ACTION_LIKE,
                PlaybackNotification.ACTION_DOWNLOAD)).size() == 5);
    }
}
