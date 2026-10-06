package com.tunefold.app;

import org.junit.Test;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertNull;
import static org.junit.Assert.assertTrue;

/** Autoplay policy and loop prevention (§14, §16, §29). */
public class AutoplayPolicyTest {

    private static MediaTrack track(String id) { return TestTracks.of("YouTube", id); }

    @Test public void eofWithQueuedNextPlaysNext() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.playNow(track("a"));
        queue.append(track("b"));

        AutoplayPolicy.Outcome outcome =
                AutoplayPolicy.afterTrackFinished(queue, false, null);
        assertEquals(AutoplayPolicy.Decision.PLAY_NEXT, outcome.decision);
        assertEquals("b", TrackKey.of(outcome.target).providerTrackId());
    }

    @Test public void eofWithAutoplayDisabledStops() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.playNow(track("a"));

        AutoplayPolicy.Outcome outcome =
                AutoplayPolicy.afterTrackFinished(queue, false, null);
        assertEquals(AutoplayPolicy.Decision.STOP, outcome.decision);
        assertEquals("queue exhausted and autoplay is off", outcome.reason);
    }

    @Test public void eofWithAutoplayEnabledAsksForARecommendation() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.playNow(track("a"));
        MediaTrack candidate = track("z");

        AutoplayPolicy.Outcome outcome =
                AutoplayPolicy.afterTrackFinished(queue, true, candidate);
        assertEquals(AutoplayPolicy.Decision.RECOMMEND, outcome.decision);
        assertEquals("z", TrackKey.of(outcome.target).providerTrackId());
    }

    @Test public void autoplayWithNoCandidateAsksTheProviderOnce() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.playNow(track("a"));

        AutoplayPolicy.Outcome outcome =
                AutoplayPolicy.afterTrackFinished(queue, true, null);
        // "not fetched yet" must mean FETCH_CANDIDATE, never STOP: the caller
        // always passes null on the first call, so STOP here disables autoplay.
        assertEquals(AutoplayPolicy.Decision.FETCH_CANDIDATE, outcome.decision);
        assertNull("fetching has no target yet", outcome.target);
    }

    @Test public void aFetchedCandidateIsPlayedDirectly() {
        PlaybackQueue queue = new PlaybackQueue();
        queue.playNow(track("a"));

        AutoplayPolicy.Outcome outcome =
                AutoplayPolicy.afterTrackFinished(queue, true, track("z"));
        assertEquals(AutoplayPolicy.Decision.RECOMMEND, outcome.decision);
    }

    @Test public void emptyQueueWithAutoplayGoesStraightToRecommendations() {
        PlaybackQueue queue = new PlaybackQueue();
        assertTrue(queue.isEmpty());

        AutoplayPolicy.Outcome outcome =
                AutoplayPolicy.afterTrackFinished(queue, true, track("z"));
        assertEquals(AutoplayPolicy.Decision.RECOMMEND, outcome.decision);
    }

    @Test public void emptyQueueWithAutoplayOffStops() {
        AutoplayPolicy.Outcome outcome =
                AutoplayPolicy.afterTrackFinished(new PlaybackQueue(), false, track("z"));
        assertEquals(AutoplayPolicy.Decision.STOP, outcome.decision);
    }

    @Test public void nullQueueIsHandledDefensively() {
        AutoplayPolicy.Outcome outcome =
                AutoplayPolicy.afterTrackFinished(null, true, track("z"));
        assertEquals(AutoplayPolicy.Decision.RECOMMEND, outcome.decision);
        AutoplayPolicy.Outcome stopped =
                AutoplayPolicy.afterTrackFinished(null, false, null);
        assertEquals(AutoplayPolicy.Decision.STOP, stopped.decision);
    }

    /**
     * The skip budget is what stops a broken provider from looping forever
     * (§20): a permanently failing candidate eventually exhausts it.
     */
    @Test public void repeatedAutoplayFailuresStopInsteadOfLoopingForever() {
        RecoveryPolicy recovery = new RecoveryPolicy();
        int guard = 0;
        while (recovery.takeAutoplaySkip() && guard < 100) guard++;

        assertEquals("autoplay gives up after a bounded number of skips",
                RecoveryPolicy.MAX_AUTOPLAY_SKIPS, guard);
        assertFalse("no further skip is allowed", recovery.takeAutoplaySkip());
        assertEquals(0, recovery.autoplaySkipsLeft());
    }

    @Test public void aSuccessfulAutoplayResetsTheSkipBudget() {
        RecoveryPolicy recovery = new RecoveryPolicy();
        recovery.takeAutoplaySkip();
        assertTrue(recovery.autoplaySkipsLeft() > 0);
        recovery.onAutoplaySucceeded();
        assertEquals(RecoveryPolicy.MAX_AUTOPLAY_SKIPS, recovery.autoplaySkipsLeft());
    }

    @Test public void startingANewTrackResetsTransientBudgets() {
        RecoveryPolicy recovery = new RecoveryPolicy();
        recovery.takeSourceRetry();
        recovery.takePlaybackSourceRetry();
        assertEquals(0, recovery.sourceRetriesLeft());

        recovery.resetForNextTrack();
        assertEquals(RecoveryPolicy.MAX_SOURCE_RETRIES, recovery.sourceRetriesLeft());
        assertEquals(RecoveryPolicy.MAX_PLAYBACK_SOURCE_RETRIES, recovery.playbackSourceRetriesLeft());
        assertEquals(RecoveryPolicy.MAX_AUTOPLAY_SKIPS, recovery.autoplaySkipsLeft());
    }

    @Test public void everyRetryBudgetIsBoundedAndSingleUse() {
        RecoveryPolicy recovery = new RecoveryPolicy();
        assertTrue(recovery.takeSourceRetry());
        assertFalse("a second automatic source retry is refused",
                recovery.takeSourceRetry());
        assertTrue(recovery.takePlaybackSourceRetry());
        assertFalse(recovery.takePlaybackSourceRetry());
        assertTrue(recovery.takeDownloadRetry());
        assertFalse(recovery.takeDownloadRetry());
        assertNotNull(recovery.describe());
    }

    @Test public void successfulPlaybackAndDownloadRefreshTheirBudgets() {
        RecoveryPolicy recovery = new RecoveryPolicy();
        recovery.takeSourceRetry();
        recovery.takeDownloadRetry();
        recovery.onPlaybackStarted();
        recovery.onDownloadFinished();
        assertEquals(RecoveryPolicy.MAX_SOURCE_RETRIES, recovery.sourceRetriesLeft());
        assertEquals(RecoveryPolicy.MAX_DOWNLOAD_RETRIES, recovery.downloadRetriesLeft());
    }
}