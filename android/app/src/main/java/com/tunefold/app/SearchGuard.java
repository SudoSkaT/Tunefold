package com.tunefold.app;

/**
 * Identity of the search currently allowed to paint results.
 *
 * <p>The provider answers on a worker thread; a slow answer for "a" must not
 * replace the results for "b" submitted later. Every submit opens a
 * generation and every callback carries it back.
 */
final class SearchGuard {
    /** Written by the UI thread; read by provider callbacks. */
    private volatile long current;

    long begin() {
        current += 1;
        return current;
    }

    /** Invalidates in-flight work without starting a new search. */
    void invalidate() {
        current += 1;
    }

    boolean isCurrent(long generation) {
        return generation == current;
    }
}
