package com.tunefold.app;

import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

/** The lifecycle and audio gates that keep a hidden or paused screen at rest. */
public class UiCadenceTest {

    @Test public void aStartedActivityPolls() {
        assertTrue(UiCadence.uiPollEnabled(true, false));
    }

    @Test public void aStoppedOrDestroyedActivityDoesNotPoll() {
        assertFalse(UiCadence.uiPollEnabled(false, false));
        assertFalse(UiCadence.uiPollEnabled(true, true));
        assertFalse(UiCadence.uiPollEnabled(false, true));
    }

    @Test public void onlyPlayingAudioProducesLiveBars() {
        assertTrue(UiCadence.soundProducesBars(PlaybackState.PLAYING));
        assertFalse(UiCadence.soundProducesBars(PlaybackState.PAUSED));
        assertFalse(UiCadence.soundProducesBars(PlaybackState.BUFFERING));
        assertFalse(UiCadence.soundProducesBars(PlaybackState.RESOLVING_SOURCE));
        assertFalse(UiCadence.soundProducesBars(PlaybackState.RESOLVING_METADATA));
        assertFalse(UiCadence.soundProducesBars(PlaybackState.STOPPED));
        assertFalse(UiCadence.soundProducesBars(PlaybackState.IDLE));
        assertFalse(UiCadence.soundProducesBars(PlaybackState.ERROR));
    }

    @Test public void theVisualTickNeedsAStartedVisiblePlayerWithAudio() {
        assertTrue(UiCadence.visualTickEnabled(true, true, true));
        assertFalse("background", UiCadence.visualTickEnabled(false, true, true));
        assertFalse("player closed", UiCadence.visualTickEnabled(true, false, true));
        assertFalse("paused", UiCadence.visualTickEnabled(true, true, false));
    }
}
