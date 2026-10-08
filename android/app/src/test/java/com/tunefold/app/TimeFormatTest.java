package com.tunefold.app;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNull;

import org.junit.Test;

/**
 * Guards the timestamp and byte formatting the surface renders.
 *
 * <p>This runs on a 500&nbsp;ms tick, so the implementation deliberately avoids
 * {@code String.format}; these cases pin the output that behaviour must preserve.
 */
public class TimeFormatTest {

    private static String time(long ms) { return TimeFormat.time(ms); }

    @Test public void subMinuteShowsSeconds() {
        assertEquals("0:00", time(0L));
        assertEquals("0:01", time(1_000L));
        assertEquals("0:09", time(9_400L));
        assertEquals("0:59", time(59_999L));
    }

    @Test public void minutesAreZeroPaddedToTwoDigits() {
        assertEquals("1:00", time(60_000L));
        assertEquals("1:05", time(65_000L));
        assertEquals("9:59", time(599_999L));
    }

    @Test public void pastAnHourAddsTheHourField() {
        assertEquals("1:00:00", time(3_600_000L));
        assertEquals("1:00:01", time(3_601_000L));
        assertEquals("2:03:04", time(7_384_000L));
    }

    @Test public void veryLongTracksKeepCountingHours() {
        assertEquals("10:00:00", time(36_000_000L));
    }

    @Test public void negativePositionsReadAsZero() {
        assertEquals("0:00", time(-1L));
        assertEquals("0:00", time(Long.MIN_VALUE / 2));
    }

    @Test public void buffersAreClearedBetweenWrites() {
        StringBuilder buffer = new StringBuilder();
        TimeFormat.time(3_601_000L, buffer);
        assertEquals("1:00:01", buffer.toString());
        TimeFormat.time(0L, buffer);
        assertEquals("0:00", buffer.toString());
    }

    @Test public void bufferFormMatchesTheStringForm() {
        StringBuilder buffer = new StringBuilder(8);
        for (long ms : new long[] {0L, 999L, 1_000L, 61_000L, 3_601_000L, 7_384_000L}) {
            TimeFormat.time(ms, buffer);
            assertEquals("mismatch at " + ms, time(ms), buffer.toString());
        }
    }

    @Test public void unknownDurationUsesTheSharedSentinel() {
        assertEquals(-1L, TimeFormat.UNKNOWN);
    }

    @Test public void bytesScaleToKilobytesThenMegabytes() {
        assertEquals("0 B", TimeFormat.bytes(0L));
        assertEquals("0 B", TimeFormat.bytes(-5L));
        assertEquals("1 KB", TimeFormat.bytes(1024L));
        assertEquals("512 KB", TimeFormat.bytes(512L * 1024L));
        assertEquals("1.0 MB", TimeFormat.bytes(1024L * 1024L));
        assertEquals("1.5 MB", TimeFormat.bytes((long) (1.5d * 1024 * 1024)));
        assertEquals("117.7 MB", TimeFormat.bytes(123_456_789L));
    }

    /** An unknown rate is never replaced by a fabricated zero. */
    @Test public void unknownRateIsNullRatherThanZero() {
        assertNull(TimeFormat.rate(-1L));
        assertNull(TimeFormat.rate(0L));
        assertEquals("1 KB/s", TimeFormat.rate(1024L));
        assertEquals("2.0 MB/s", TimeFormat.rate(2L * 1024L * 1024L));
    }
}