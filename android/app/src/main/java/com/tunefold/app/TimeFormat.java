package com.tunefold.app;

import java.util.Locale;

/**
 * Formats the timestamps and byte counts the UI shows.
 *
 * <p>Pure logic, no Android types, so it is unit-testable and so the hot path
 * stays allocation-light: {@link #time(long, StringBuilder)} writes into a caller
 * owned buffer instead of going through {@code String.format}, whose varargs box
 * every argument and allocate a {@code Formatter} per call. The progress display
 * runs on a 500&nbsp;ms tick, so four boxed calls per tick is pure garbage over a
 * long listening session.
 *
 * <p>{@link NowPlayingView} delegates here, which also means there is exactly one
 * definition of what {@code 1:02:03} and {@code 3.4 MB} mean.
 */
final class TimeFormat {

    private TimeFormat() { }

    /** Value meaning "not known yet". */
    static final long UNKNOWN = -1L;

    /** Renders {@code ms} as {@code m:ss}, or {@code h:mm:ss} past an hour. */
    static String time(long ms) {
        StringBuilder out = new StringBuilder(8);
        time(ms, out);
        return out.toString();
    }

    /**
     * Writes {@code ms} into {@code out}, which is cleared first.
     *
     * <p>Reuses one buffer per label so a repeating tick costs no garbage: the
     * single unavoidable allocation is the {@code String} the TextView keeps.
     */
    static void time(long ms, StringBuilder out) {
        long total = ms / 1000L;
        if (total < 0) total = 0L;
        long hours = total / 3600L;
        long minutes = (total % 3600L) / 60L;
        long seconds = total % 60L;
        out.setLength(0);
        if (hours > 0L) {
            out.append(hours).append(':');
            twoDigits(out, minutes);
        } else {
            out.append(minutes);
        }
        out.append(':');
        twoDigits(out, seconds);
    }

    /** Renders a byte count with a single decimal, as the download row shows. */
    static String bytes(long bytes) {
        if (bytes <= 0L) return "0 B";
        double megabytes = bytes / (1024.0 * 1024.0);
        if (megabytes >= 1.0d) return String.format(Locale.ROOT, "%.1f MB", megabytes);
        return String.format(Locale.ROOT, "%.0f KB", bytes / 1024.0d);
    }

    /**
     * Renders a transfer rate, or {@code null} when it cannot be derived.
     *
     * <p>An unknown rate is never replaced by a fabricated {@code 0 B/s}: the UI
     * must not claim to know something it does not (§7, §27).
     */
    static String rate(long bytesPerSecond) {
        if (bytesPerSecond <= 0L) return null;
        return bytes(bytesPerSecond) + "/s";
    }

    private static void twoDigits(StringBuilder out, long value) {
        if (value < 10) out.append('0');
        out.append(value);
    }
}