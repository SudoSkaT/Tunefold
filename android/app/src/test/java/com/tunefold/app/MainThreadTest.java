package com.tunefold.app;

import static org.junit.Assert.assertEquals;

import org.junit.After;
import org.junit.Test;

import java.util.ArrayList;
import java.util.List;

/**
 * Guards the main-thread hand-off.
 *
 * <p>This exists because of a real failure: the "output began" callback was invoked on
 * the audio worker, the session that owns it notified its observers there, and those
 * observers touched views. Android raised {@code CalledFromWrongThreadException} inside
 * the worker, which then reported itself as an audio error — so a healthy playback
 * surfaced to the user as an error, intermittently, depending on whether the view
 * hierarchy happened to be attached yet.
 *
 * <p>The contract is "never run a caller's work on a worker", so it is directly
 * assertable against the poster seam.
 */
public class MainThreadTest {

    /** Records what would have been posted instead of touching a real looper. */
    private static final class RecordingPoster implements MainThread.Poster {
        final List<Runnable> posted = new ArrayList<>();

        @Override public void post(Runnable action) { posted.add(action); }
    }

    private static final class Counter implements Runnable {
        int runs;

        @Override public void run() { runs++; }
    }

    private final MainThread.Poster previous = MainThread.swapPosterForTest(null);

    @After public void restore() { MainThread.swapPosterForTest(previous); }

    /** Running inline when already on the main thread is what preserves ordering. */
    @Test public void runsInlineWhenAlreadyOnMain() {
        RecordingPoster poster = new RecordingPoster();
        MainThread.swapPosterForTest(poster);
        Counter action = new Counter();

        MainThread.run(action, true);

        assertEquals(1, action.runs);
        assertEquals("main-thread work must not be deferred", 0, poster.posted.size());
    }

    @Test public void defersWhenNotOnMain() {
        RecordingPoster poster = new RecordingPoster();
        MainThread.swapPosterForTest(poster);
        Counter action = new Counter();

        MainThread.run(action, false);

        assertEquals("worker work must not run inline", 0, action.runs);
        assertEquals(1, poster.posted.size());
    }

    @Test public void deferredWorkRunsExactlyOnce() {
        RecordingPoster poster = new RecordingPoster();
        MainThread.swapPosterForTest(poster);
        Counter action = new Counter();

        MainThread.run(action, false);
        MainThread.run(action, false);
        poster.posted.forEach(Runnable::run);

        assertEquals(2, poster.posted.size());
        assertEquals(2, action.runs);
    }

    /** With no looper at all the work must still happen, never be dropped. */
    @Test public void missingPosterStillDeliversWork() {
        MainThread.swapPosterForTest(null);
        Counter action = new Counter();

        MainThread.run(action, false);

        assertEquals("work must never be silently dropped", 1, action.runs);
    }

    @Test public void nullWorkIsIgnored() {
        MainThread.run(null, true);
        MainThread.run(null, false);
    }

    /** Two consecutive transitions must not invert. */
    @Test public void inlineRunsPreserveOrder() {
        StringBuilder order = new StringBuilder();
        MainThread.run(() -> order.append('a'), true);
        MainThread.run(() -> order.append('b'), true);
        assertEquals("ab", order.toString());
    }

    /**
     * The exact shape that broke: a session callback stored where the audio worker
     * could reach it and invoked it directly.
     */
    @Test public void workerCallbacksAreWrappedForTheMainThread() {
        RecordingPoster poster = new RecordingPoster();
        MainThread.swapPosterForTest(poster);
        Counter sessionCallback = new Counter();

        Runnable handedToAudioWorker = () -> MainThread.run(sessionCallback, false);
        handedToAudioWorker.run();

        assertEquals("the session callback must not have run on the worker",
                0, sessionCallback.runs);
        assertEquals(1, poster.posted.size());
    }

    /** Asking must be safe on any thread, because it runs on every hand-off. */
    @Test public void mainThreadDetectionDoesNotThrow() {
        MainThread.isMainThread();
    }
}