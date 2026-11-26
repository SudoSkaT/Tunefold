package com.tunefold.app;

import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

public class SearchGuardTest {

    @Test public void onlyTheNewestSubmissionIsCurrent() {
        SearchGuard guard = new SearchGuard();
        long first = guard.begin();
        long second = guard.begin();
        assertFalse(guard.isCurrent(first));
        assertTrue(guard.isCurrent(second));
    }

    @Test public void invalidateDropsEverythingInFlight() {
        SearchGuard guard = new SearchGuard();
        long first = guard.begin();
        guard.invalidate();
        assertFalse(guard.isCurrent(first));
    }
}
