package com.tunefold.app;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNull;

import org.junit.Test;

/**
 * Guards for YouTube-gated JNI entries (portrait-player crash).
 *
 * <p>Binaries without the {@code youtube} feature have no such natives: calling
 * one throws UnsatisfiedLinkError — an Error, invisible to every
 * {@code catch (Exception)} — and the uncaught throw kills the whole process.
 * Opening the player asks for lyrics once per track with no other gate, so
 * that one missing guard was a crash on every player open. Every gated entry
 * must answer "unavailable" before touching native code.
 */
public class ProviderGatesTest {

    @Test public void missingNativesReportUnavailable() {
        assertEquals("YouTube provider is not initialized",
                ProviderRegistry.youtubeMissingError(false));
    }

    @Test public void presentNativesReportNothing() {
        assertNull(ProviderRegistry.youtubeMissingError(true));
    }

    @Test public void gateAgreesWithTheBuildFlag() {
        // The call sites pass BuildConfig.YOUTUBE_ENABLED; the gate must be
        // the identity over it, never an independent opinion.
        assertEquals(BuildConfig.YOUTUBE_ENABLED,
                ProviderRegistry.youtubeMissingError(BuildConfig.YOUTUBE_ENABLED) == null);
    }
}
