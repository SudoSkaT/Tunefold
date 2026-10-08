package com.tunefold.app;

import android.os.Handler;
import android.os.Looper;

/**
 * The one place work crosses onto the main thread.
 *
 * <p>Most of Tunefold is main-thread code by construction: {@link PlaybackSession}
 * documents that it runs entirely on the main thread, and the UI reads its state
 * without locking. That invariant is easy to break from a worker — and the symptom is
 * not a clean crash but a {@code CalledFromWrongThreadException} raised deep inside a
 * worker, which then reports itself as an audio failure. That is how a perfectly
 * healthy playback ended up shown to the user as an error.
 *
 * <p>Routing every hand-off through here makes the crossing explicit and reviewable,
 * instead of implicit in whichever callback happened to be stored where.
 */
final class MainThread {

    /** How deferred work actually gets scheduled; a seam so the rule can be tested. */
    interface Poster {
        /** Schedules {@code action} on the main thread. */
        void post(Runnable action);
    }

    /**
     * Installed on first use.
     *
     * <p>Stays {@code null} when there is no main looper — a JVM unit test, or a
     * process tearing down. {@link #run} then executes inline rather than dropping the
     * work, because a dropped state transition leaves a machine half-moved.
     */
    private static volatile Poster poster;

    private MainThread() { }

    /**
     * Runs {@code action} on the main thread, inline when already there.
     *
     * <p>Inline when possible is deliberate: posting unconditionally would add a frame of
     * latency to transitions that are already on the right thread, and would reorder
     * them against work that legitimately has to happen first.
     */
    static void run(Runnable action) {
        run(action, isMainThread());
    }

    /**
     * The decision, separated from the Looper so it can be asserted.
     *
     * @param alreadyOnMain whether the caller is already on the main thread
     */
    static void run(Runnable action, boolean alreadyOnMain) {
        if (action == null) return;
        if (alreadyOnMain) {
            action.run();
            return;
        }
        Poster current = poster;
        if (current == null) {
            current = install();
        }
        if (current == null) {
            action.run();
            return;
        }
        current.post(action);
    }

    /** True when the calling thread is the main thread. */
    static boolean isMainThread() {
        return Looper.myLooper() == Looper.getMainLooper();
    }

    private static synchronized Poster install() {
        if (poster != null) return poster;
        Looper looper = Looper.getMainLooper();
        if (looper == null) return null;
        Handler handler = new Handler(looper);
        poster = action -> {
            // A quit looper refuses posts; run inline rather than lose the transition.
            if (!handler.post(action)) action.run();
        };
        return poster;
    }

    /** Test seam: overrides the poster and returns the previous one. */
    static Poster swapPosterForTest(Poster replacement) {
        Poster previous = poster;
        poster = replacement;
        return previous;
    }
}