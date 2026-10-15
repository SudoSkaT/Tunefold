package com.tunefold.app;

import static org.junit.Assert.assertEquals;

import org.junit.Test;

/**
 * Shelf swipe direction mapping (Now Playing tabs).
 *
 * <p>Left goes forward (Siguiente → Recomendados → Radio), right goes back,
 * and anything vertical or short stays put so the scroll never fights a tap.
 */
public class ShelfTabsTest {

    @Test public void leftFromQueueOpensRecommended() {
        assertEquals(NowPlayingView.Tab.RECOMMENDED,
                NowPlayingView.swipeTab(NowPlayingView.Tab.QUEUE, -200f, 10f));
    }

    @Test public void rightFromRecommendedReturnsToQueue() {
        assertEquals(NowPlayingView.Tab.QUEUE,
                NowPlayingView.swipeTab(NowPlayingView.Tab.RECOMMENDED, 200f, 10f));
    }

    @Test public void leftFromRecommendedReachesRadio() {
        assertEquals(NowPlayingView.Tab.RADIO,
                NowPlayingView.swipeTab(NowPlayingView.Tab.RECOMMENDED, -200f, 0f));
    }

    @Test public void radioShelvesCycle() {
        assertEquals(NowPlayingView.Tab.RECOMMENDED,
                NowPlayingView.swipeTab(NowPlayingView.Tab.RADIO, 200f, 0f));
        assertEquals(NowPlayingView.Tab.RADIO,
                NowPlayingView.swipeTab(NowPlayingView.Tab.RADIO, -200f, 0f));
    }

    @Test public void verticalOrShortGesturesStay() {
        assertEquals(NowPlayingView.Tab.QUEUE,
                NowPlayingView.swipeTab(NowPlayingView.Tab.QUEUE, -200f, 150f));
        assertEquals(NowPlayingView.Tab.RECOMMENDED,
                NowPlayingView.swipeTab(NowPlayingView.Tab.RECOMMENDED, -10f, 0f));
        assertEquals(NowPlayingView.Tab.QUEUE,
                NowPlayingView.swipeTab(null, -500f, 0f));
    }
}
