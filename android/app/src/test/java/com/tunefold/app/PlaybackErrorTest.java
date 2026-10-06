package com.tunefold.app;

import org.junit.Test;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

/** Error classification and visible behaviour (§19, §29). */
public class PlaybackErrorTest {

    @Test public void http404IsTerminalAndNotRetryable() {
        PlaybackError error = PlaybackError.classify(
                PlaybackError.Stage.SOURCE_RESOLUTION, "el stream no existe (HTTP 404 byte 0)");
        assertEquals(PlaybackError.Kind.TRACK_UNAVAILABLE, error.kind);
        assertFalse("an unavailable track must not be retried forever", error.retryable);
        assertEquals("This track is unavailable.", error.message);
    }

    @Test public void notFoundVariantsAreAlsoTerminal() {
        for (String raw : new String[]{"HTTP 404", "not found", "404",
                "el stream no existe (HTTP 404 byte 0)"}) {
            PlaybackError error = PlaybackError.classify(PlaybackError.Stage.SOURCE_RESOLUTION, raw);
            assertEquals(raw, PlaybackError.Kind.TRACK_UNAVAILABLE, error.kind);
            assertFalse(raw, error.retryable);
        }
    }

    @Test public void vagueWordingStaysRecoverableSoThereIsAlwaysARetry() {
        // "unsupported" alone is too vague to call the track unavailable: it is
        // also how the decoder reports an unsupported CODEC, which is a local
        // problem. Treating an unrecognised failure as recoverable guarantees
        // the user is never left with a dead end.
        PlaybackError vague = PlaybackError.classify(
                PlaybackError.Stage.SOURCE_RESOLUTION, "unsupported");
        assertEquals(PlaybackError.Kind.RECOVERABLE, vague.kind);
        assertTrue(vague.retryable);
    }

    @Test public void networkFailuresAreRecoverable() {
        for (String raw : new String[]{"error de red", "timeout de red", "connection reset",
                "network failure", "temporary HTTP failure"}) {
            PlaybackError error = PlaybackError.classify(PlaybackError.Stage.SOURCE_RESOLUTION, raw);
            assertEquals(raw, PlaybackError.Kind.RECOVERABLE, error.kind);
            assertTrue(raw, error.retryable);
        }
    }

    @Test public void sourceExpiredIsRecoverableBecauseReResolvingCanHelp() {
        PlaybackError error = PlaybackError.classify(
                PlaybackError.Stage.SOURCE_RESOLUTION, "el stream no existe");
        // A stream that is gone permanently must not loop; an expired one is
        // distinguishable by the retry budget, which is bounded elsewhere.
        assertFalse(error.retryable);
        PlaybackError expired = PlaybackError.classify(
                PlaybackError.Stage.SOURCE_RESOLUTION, "stream caducado");
        assertEquals(PlaybackError.Kind.RECOVERABLE, expired.kind);
        assertTrue(expired.retryable);
    }

    @Test public void localFailuresAreNotRetryableWithoutUserAction() {
        for (String raw : new String[]{"local media open failed", "empty stream",
                "no audio track", "unsupported codec"}) {
            PlaybackError error = PlaybackError.classify(PlaybackError.Stage.DECODE, raw);
            assertEquals(raw, PlaybackError.Kind.LOCAL_FAILURE, error.kind);
            assertFalse(raw, error.retryable);
        }
    }

    @Test public void unrecognisedFailuresStayRecoverableSoThereIsAlwaysAWayForward() {
        PlaybackError error = PlaybackError.classify(PlaybackError.Stage.UNKNOWN, "something odd");
        assertEquals(PlaybackError.Kind.RECOVERABLE, error.kind);
        assertTrue(error.retryable);
    }

    @Test public void emptyMessageIsSilentRatherThanAUserFacingError() {
        assertTrue(PlaybackError.isSilent(PlaybackError.Stage.SOURCE_RESOLUTION, ""));
        assertTrue(PlaybackError.isSilent(PlaybackError.Stage.SOURCE_RESOLUTION, null));
    }

    @Test public void recommendationFailuresAreAlwaysSilent() {
        assertTrue("a failed recommendation must not break Home",
                PlaybackError.isSilent(PlaybackError.Stage.RECOMMENDATION, "provider exploded"));
    }

    @Test public void stageIsPreservedForDiagnostics() {
        assertEquals(PlaybackError.Stage.DOWNLOAD,
                PlaybackError.classify(PlaybackError.Stage.DOWNLOAD, "error de red").stage);
        assertEquals(PlaybackError.Stage.OUTPUT,
                PlaybackError.classify(PlaybackError.Stage.OUTPUT, "AudioTrack.write failed").stage);
    }

    @Test public void stateMappingCoversEveryEngineState() {
        assertEquals(PlaybackState.PLAYING, PlaybackState.fromEngine(PlaybackController.PLAYING));
        assertEquals(PlaybackState.PAUSED, PlaybackState.fromEngine(PlaybackController.PAUSED));
        assertEquals(PlaybackState.BUFFERING, PlaybackState.fromEngine(PlaybackController.LOADING));
        assertEquals(PlaybackState.STOPPED, PlaybackState.fromEngine(PlaybackController.STOPPED));
        assertEquals(PlaybackState.ERROR, PlaybackState.fromEngine(PlaybackController.ERROR));
        assertEquals(PlaybackState.IDLE, PlaybackState.fromEngine(PlaybackController.IDLE));
    }

    @Test public void everyStateHasAUserFacingLabel() {
        for (PlaybackState state : PlaybackState.values()) {
            assertFalse(state.name(), state.label() == null || state.label().isEmpty());
        }
    }
}