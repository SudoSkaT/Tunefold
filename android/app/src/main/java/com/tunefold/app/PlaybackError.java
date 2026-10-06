package com.tunefold.app;

/**
 * A playback or download failure, classified by what the app may do next (§19).
 *
 * <p>The class is derived from the message text the Rust transport and the
 * provider already produce, so failures gain a consistent, honest behaviour
 * across every surface without changing the Rust error vocabulary.
 */
final class PlaybackError {
    /** What the app can do about it. */
    enum Kind {
        /** Transient: offer Retry, and re-resolve the source when it is safe. */
        RECOVERABLE,
        /** The track itself is gone: no infinite retrying. */
        TRACK_UNAVAILABLE,
        /** Local disk / decode problem. */
        LOCAL_FAILURE
    }

    /** Where the failure happened, which decides the wording. */
    enum Stage {
        METADATA, SOURCE_RESOLUTION, DOWNLOAD, DECODE, OUTPUT, RECOMMENDATION, UNKNOWN
    }

    final Kind kind;
    final Stage stage;
    final String message;
    /** Whether offering Retry makes sense at all. */
    final boolean retryable;

    PlaybackError(Kind kind, Stage stage, String message, boolean retryable) {
        this.kind = kind;
        this.stage = stage;
        this.message = message == null ? "" : message;
        this.retryable = retryable;
    }

    /**
     * Classifies a failure message.
     *
     * <p>Substring matching is deliberate and narrow: it covers the exact
     * categories the Rust layer and the provider emit, and anything unrecognised
     * is treated as recoverable so the user is offered a way forward instead of
     * a dead end.
     */
    static PlaybackError classify(Stage stage, String raw) {
        String text = raw == null ? "" : raw;
        String lower = text.toLowerCase(java.util.Locale.ROOT);

        // Local codec/container problems are checked FIRST: their wording
        // ("unsupported codec") overlaps with the terminal bucket, and telling
        // the user a track is unavailable when the real problem is an
        // unsupported codec would be simply wrong.
        if (lower.contains("local media") || lower.contains("empty stream")
                || lower.contains("no audio track") || lower.contains("unsupported codec")
                || lower.contains("no soportado")
                || lower.contains("audio track has no sample rate")) {
            return new PlaybackError(Kind.LOCAL_FAILURE, stage, text, false);
        }
        // Terminal: the resource is simply not there, or is gone for good.
        if (lower.contains("404") || lower.contains("no existe")
                || lower.contains("not found")) {
            return new PlaybackError(Kind.TRACK_UNAVAILABLE, stage,
                    "This track is unavailable.", false);
        }
        // Recoverable: anything network- or lifetime-shaped.
        return new PlaybackError(Kind.RECOVERABLE, stage, text, true);
    }

    /** True when the failure should not become a user-visible error at all. */
    static boolean isSilent(Stage stage, String raw) {
        if (stage == Stage.RECOMMENDATION) return true;
        String lower = raw == null ? "" : raw.toLowerCase(java.util.Locale.ROOT);
        return lower.isEmpty();
    }

    @Override public String toString() { return kind + "/" + stage + ": " + message; }
}