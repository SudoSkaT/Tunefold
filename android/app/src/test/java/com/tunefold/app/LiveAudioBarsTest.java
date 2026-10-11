package com.tunefold.app;

import static org.junit.Assert.assertArrayEquals;
import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertSame;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

/**
 * The rule that makes a 20 Hz surface affordable: read often, paint only on change.
 *
 * <p>The engine publishes a new window about 86 times a second and the surface asks
 * for it 20 times a second, so most polls do bring something new. The ones that
 * matter are the rest: a paused track, a long buffering, a screen nobody is looking
 * at. Those must cost nothing at all, and the only thing standing between them and
 * a battery of useless repaints is the generation the source hands back.
 */
public class LiveAudioBarsTest {

    /** A reader that hands out the windows it is given, and counts the calls. */
    private static final class Fake implements LiveAudioBars.Reader {
        private final long[] generations;
        private int index;
        int reads;
        int lastBars = -1;
        float[] lastBuffer;

        Fake(long... generations) { this.generations = generations; }

        @Override public long read(float[] out, int bars) {
            reads++;
            lastBars = bars;
            lastBuffer = out;
            for (int bar = 0; bar < bars; bar++) {
                for (int value = 0; value < LiveAudioBars.BAR_VALUES; value++) {
                    out[LiveAudioBars.BAR_VALUES * bar + value] = bar + value / 10f;
                }
            }
            return index < generations.length ? generations[index++] : generations[
                    generations.length - 1];
        }
    }

    /** New audio is new audio: the caller learns it has something to draw. */
    @Test public void aNewWindowIsReportedAsNew() {
        Fake fake = new Fake(1L, 2L);
        LiveAudioBars bars = new LiveAudioBars(fake);
        assertTrue(bars.poll(32));
        assertTrue(bars.poll(32));
        assertEquals(2, bars.updateCount());
    }

    /**
     * The same window twice is the whole point.
     *
     * <p>Without this the view would invalidate twenty times a second on a paused
     * track, which is how a screen looks alive while nothing is happening.
     */
    @Test public void theSameWindowIsNotNewAudio() {
        Fake fake = new Fake(7L);
        LiveAudioBars bars = new LiveAudioBars(fake);
        assertTrue(bars.poll(32));
        assertFalse("same window, nothing to repaint", bars.poll(32));
        assertFalse(bars.poll(32));
        assertEquals("polls and updates are different numbers on purpose",
                3, bars.pollCount());
        assertEquals(1, bars.updateCount());
    }

    /** Generation zero is a real answer (silence before the engine warms up). */
    @Test public void silenceIsStillAWindow() {
        Fake fake = new Fake(0L, 0L, 1L);
        LiveAudioBars bars = new LiveAudioBars(fake);
        assertTrue(bars.poll(32));
        assertFalse(bars.poll(32));
        assertTrue("the engine came back", bars.poll(32));
    }

    /** The buffer is reused for the life of the source: one array, no churn. */
    @Test public void theBufferIsAllocatedOnce() {
        LiveAudioBars bars = new LiveAudioBars(new Fake(1L, 2L, 3L));
        float[] first = bars.values();
        bars.poll(64);
        assertSame("a 20 Hz screen cannot allocate per tick", first, bars.values());
        assertSame(first, bars.values());
    }

    /** Whatever the layout asks for, the engine is asked for no more than the cap. */
    @Test public void theBarCountNeverExceedsTheCap() {
        Fake fake = new Fake(1L);
        LiveAudioBars bars = new LiveAudioBars(fake);
        bars.poll(10_000);
        assertEquals(LiveAudioBars.MAX_BARS, fake.lastBars);
        assertEquals(LiveAudioBars.MAX_BARS, bars.bars());
        assertEquals("the array is sized for the cap, not for the request",
                LiveAudioBars.BAR_VALUES * LiveAudioBars.MAX_BARS, bars.values().length);
    }

    /** A request that does not fit is a request that was never made. */
    @Test public void nothingIsReadWithoutRoomToShowIt() {
        Fake fake = new Fake(1L);
        LiveAudioBars bars = new LiveAudioBars(fake);
        assertFalse(bars.poll(0));
        assertFalse(bars.poll(-4));
        assertEquals("not even the read happened", 0, fake.reads);
    }

    /** No engine means no read: a detached surface must not call into nothing. */
    @Test public void aSourceWithoutAReaderRests() {
        LiveAudioBars bars = new LiveAudioBars((LiveAudioBars.Reader) null);
        assertFalse(bars.poll(32));
        assertEquals(0, bars.pollCount());
    }

    /** Clearing is a full reset: the next track starts from silence, not from bars. */
    @Test public void clearingForgetsEverything() {
        Fake fake = new Fake(5L, 5L);
        LiveAudioBars bars = new LiveAudioBars(fake);
        assertTrue(bars.poll(16));
        bars.clear();
        assertEquals(0L, bars.generation());
        assertEquals(0, bars.bars());
        assertArrayEquals(new float[LiveAudioBars.BAR_VALUES * LiveAudioBars.MAX_BARS],
                bars.values(), 0f);
        // And the window that was current before the clear counts as new again:
        // after a track change even the same number is a different track.
        assertTrue("the first read after a clear is always new", bars.poll(16));
    }

    /**
     * The bar count is derived from the width, not from the layout's leftovers.
     *
     * <p>A fractional step would make the figure depend on a sub-pixel width, and the
     * same screen would draw two different shapes after a rotation.
     */
    @Test public void barCountIsAWholeNumberOfSteps() {
        assertEquals(8, LiveAudioBars.barsFor(1, 10));
        assertEquals(10, LiveAudioBars.barsFor(100, 10));
        assertEquals("a step of a pixel is not a step", 10, LiveAudioBars.barsFor(109, 10));
        assertEquals(11, LiveAudioBars.barsFor(110, 10));
        assertEquals("nothing to draw in nothing", 0, LiveAudioBars.barsFor(0, 10));
        assertEquals("no width, no bars, ever", 0, LiveAudioBars.barsFor(100, 0));
    }

    /** The cap is a cap, and it is the same cap the buffer was sized for. */
    @Test public void theCapHoldsForEveryWidth() {
        assertEquals(LiveAudioBars.MAX_BARS, LiveAudioBars.barsFor(100_000, 1));
        assertEquals(LiveAudioBars.MAX_BARS, LiveAudioBars.barsFor(10_000, 10));
    }
}