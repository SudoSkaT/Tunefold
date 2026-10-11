package com.tunefold.app;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNotEquals;
import static org.junit.Assert.assertNull;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

/** Track identity (§4, §29). */
public class TrackKeyTest {

    @Test public void sameProviderAndIdIsTheSameTrack() {
        TrackKey first = TrackKey.of(TestTracks.of("YouTube", "dQw4w9WgXcQ", "Never Gonna Give You Up", 213_000L));
        TrackKey second = TrackKey.of(TestTracks.of("YouTube", "dQw4w9WgXcQ", "A completely different title", 1L));
        assertEquals("same provider+id is the same identity even if metadata differs",
                first, second);
        assertEquals(first.hashCode(), second.hashCode());
    }

    @Test public void differentIdIsADifferentTrack() {
        assertNotEquals(TrackKey.of("YouTube", "a"), TrackKey.of("YouTube", "b"));
    }

    @Test public void differentProviderIsADifferentTrack() {
        assertNotEquals(TrackKey.of("YouTube", "a"), TrackKey.of("Other", "a"));
    }

    @Test public void titleIsNeverPartOfIdentity() {
        TrackKey original = TrackKey.of(TestTracks.of("YouTube", "x", "Original Title", 1L));
        TrackKey renamed = TrackKey.of(TestTracks.of("YouTube", "x", "Renamed Title", 2L));
        assertEquals(original, renamed);
    }

    @Test public void trackWithoutIdentityHasNoKey() {
        assertNull("a track with no provider id cannot be identified",
                TrackKey.of("YouTube", ""));
        assertNull(TrackKey.of("", "x"));
        assertNull(TrackKey.of(null, "x"));
        assertNull(TrackKey.of((MediaTrack) null));
        assertNull(TrackKey.of(TestTracks.of("YouTube", "")));
    }

    @Test public void orderingIsDeterministic() {
        TrackKey a = TrackKey.of("YouTube", "aaa");
        TrackKey b = TrackKey.of("YouTube", "bbb");
        TrackKey c = TrackKey.of("Zeta", "aaa");
        // Provider is compared first, then the provider track id.
        assertTrue(a.compareTo(b) < 0);
        assertTrue(b.compareTo(c) < 0);
        assertTrue("same provider, higher id sorts later",
                TrackKey.of("YouTube", "zzz").compareTo(TrackKey.of("YouTube", "aaa")) > 0);
        assertEquals(0, a.compareTo(TrackKey.of("YouTube", "aaa")));
    }

    @Test public void keyWorksAsAMapKey() {
        java.util.Map<TrackKey, String> map = new java.util.HashMap<>();
        map.put(TrackKey.of("YouTube", "x"), "value");
        assertEquals("value", map.get(TrackKey.of("YouTube", "x")));
        assertFalse(map.containsKey(TrackKey.of("YouTube", "y")));
    }

    /**
     * The comparison every identity-guarded cache has to use.
     *
     * <p>{@code MediaTrack.key()} returns a fresh instance per call, so a tick hands
     * the surface an equal but distinct key. Reference comparison here made every
     * cache throw itself away on every tick; this test is the reminder.
     */
    @Test public void sameIdentitySurvivesAFreshInstance() {
        TrackKey first = TrackKey.of("YouTube", "abc");
        TrackKey second = TrackKey.of("YouTube", "abc");
        org.junit.Assert.assertNotSame("key() builds a new instance every call", first, second);
        org.junit.Assert.assertTrue("and they are the same track", TrackKey.same(first, second));
    }

    @Test public void sameIdentityIsNullSafe() {
        assertTrue(TrackKey.same(null, null));
        assertFalse(TrackKey.same(null, TrackKey.of("YouTube", "abc")));
        assertFalse(TrackKey.same(TrackKey.of("YouTube", "abc"), null));
        assertFalse(TrackKey.same(TrackKey.of("YouTube", "abc"), TrackKey.of("YouTube", "abd")));
        assertFalse(TrackKey.same(TrackKey.of("YouTube", "abc"), TrackKey.of("Other", "abc")));
    }
}
