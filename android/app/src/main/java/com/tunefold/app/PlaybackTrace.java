package com.tunefold.app;

import android.os.SystemClock;
import android.util.Log;

import java.util.ArrayList;
import java.util.Collections;
import java.util.List;

/**
 * Per-play monotonic timeline, anchored at the Play tap.
 *
 * <p>The timeline has a single origin: the instant the user asked to play. Java
 * and Rust do not share a clock, so the JNI boundary receives
 * {@code tap_to_decoder_us} (microseconds already elapsed since the tap) and the
 * Rust side anchors its own events to it. That makes every event in the log
 * directly comparable and lets us compute:
 *
 * <pre>
 *   TTFA = AUDIOTRACK_FIRST_POSITIVE_WRITE - PLAY_TAP
 * </pre>
 *
 * <p>Nothing logged here contains a signed stream URL or request headers: only
 * track ids, byte offsets, statuses and durations.
 *
 * <p>Event names are the canonical vocabulary documented in
 * {@code docs/android/playback-glossary.md}.
 */
final class PlaybackTrace {
    /** Instante en que el usuario solicita reproducir. Origen de la línea temporal. */
    static final String PLAY_TAP = "PLAY_TAP";
    /** El controlador Android recibió la orden de reproducir. */
    static final String CONTROLLER_RECEIVED_PLAY = "CONTROLLER_RECEIVED_PLAY";
    /** Hay metadata del track. */
    static final String METADATA_AVAILABLE = "METADATA_AVAILABLE";
    /** Empieza la resolución Track → fuente reproducible. */
    static final String SOURCE_RESOLUTION_START = "SOURCE_RESOLUTION_START";
    /** Ya hay PlayableSource (URL temporal o `file:`). */
    static final String PLAYABLE_SOURCE_AVAILABLE = "PLAYABLE_SOURCE_AVAILABLE";
    /** Empieza la apertura del stream HTTP. */
    static final String HTTP_OPEN_START = "HTTP_OPEN_START";
    /** Llegaron las cabeceras de la primera respuesta Range. */
    static final String HTTP_FIRST_RESPONSE = "HTTP_FIRST_RESPONSE";
    /** Empieza el probe del contenedor por Symphonia. */
    static final String SYMPHONIA_PROBE_START = "SYMPHONIA_PROBE_START";
    /** Termina el probe del contenedor. */
    static final String SYMPHONIA_PROBE_END = "SYMPHONIA_PROBE_END";
    /** El decoder produjo su primer paquete comprimido. */
    static final String DECODER_FIRST_PACKET = "DECODER_FIRST_PACKET";
    /** Primer PCM aceptado por el PCM ring. */
    static final String DECODER_FIRST_PCM = "DECODER_FIRST_PCM";
    /** AudioTrack existe y está en play. */
    static final String AUDIOTRACK_START = "AUDIOTRACK_START";
    /** Primer write positivo a AudioTrack: proxy mínimo de salida audible. */
    static final String AUDIOTRACK_FIRST_POSITIVE_WRITE = "AUDIOTRACK_FIRST_POSITIVE_WRITE";
    /** TTFA = primer output positivo − Play tap. */
    static final String TTFA = "TTFA";

    // ---- Queue and transport navigation -----------------------------------
    /** The queue advanced to its next item. */
    static final String QUEUE_NEXT = "QUEUE_NEXT";
    /** The queue moved back to the previous item. */
    static final String QUEUE_PREVIOUS = "QUEUE_PREVIOUS";
    static final String STALE_EOF = "STALE_EOF";
    /** A track finished and autoplay took over. */
    static final String AUTOPLAY_TRIGGER = "AUTOPLAY_TRIGGER";

    // ---- Recommendations ----------------------------------------------------
    /** A recommendation request started. */
    static final String RECOMMENDATIONS_START = "RECOMMENDATIONS_START";
    /** A recommendation request finished; carries kept/raw counts. */
    static final String RECOMMENDATIONS_END = "RECOMMENDATIONS_END";
    /** A candidate was chosen and queued. */
    static final String RECOMMENDATION_SELECTED = "RECOMMENDATION_SELECTED";

    // ---- Download lifecycle -------------------------------------------------
    /** An explicit download was requested. */
    static final String DOWNLOAD_START = "DOWNLOAD_START";
    /** Periodic progress sample. */
    static final String DOWNLOAD_PROGRESS = "DOWNLOAD_PROGRESS";
    /** The download committed atomically and was registered. */
    static final String DOWNLOAD_COMPLETE = "DOWNLOAD_COMPLETE";
    /** The user cancelled the download. */
    static final String DOWNLOAD_CANCELLED = "DOWNLOAD_CANCELLED";
    /** The download failed. */
    static final String DOWNLOAD_FAILED = "DOWNLOAD_FAILED";
    /** A download request was refused because the file already exists. */
    static final String DOWNLOAD_ALREADY_EXISTS = "DOWNLOAD_ALREADY_EXISTS";
    /** A download request was refused because one was already running. */
    static final String DOWNLOAD_ALREADY_RUNNING = "DOWNLOAD_ALREADY_RUNNING";
    /** A committed local file was found for the track being played. */
    static final String LOCAL_TRACK_FOUND = "LOCAL_TRACK_FOUND";
    /** No local file existed, so a remote source was resolved. */
    static final String LOCAL_TRACK_MISSING = "LOCAL_TRACK_MISSING";

    // ---- Library -------------------------------------------------------------
    /** L1K3D membership changed; carries liked=true/false. */
    static final String LIKE_CHANGED = "LIKE_CHANGED";

    // ---- Failures and recovery ----------------------------------------------
    /** A user-visible failure; carries the classified kind. */
    static final String PLAYBACK_ERROR = "PLAYBACK_ERROR";
    /** An automatic recovery attempt was made. */
    static final String PLAYBACK_RETRY = "PLAYBACK_RETRY";

    private static final String TAG = "TunefoldPerf";

    /**
     * The trace of the play currently in progress, if any.
     *
     * <p>Playback is owned by the service, but downloads, queue navigation and
     * recommendations can originate from any surface. Routing their events
     * through one holder keeps a single timeline per playback instead of making
     * every caller thread a trace through the playback session. Explicitly
     * {@link #clearCurrent()} when a playback ends.
     */
    private static volatile PlaybackTrace current;

    /** Installs the trace that later events should join, or {@code null}. */
    static void setCurrent(PlaybackTrace trace) { current = trace; }

    /** Removes the current trace; called when playback stops or is replaced. */
    static void clearCurrent() { current = null; }

    /** The current trace, or {@code null} when nothing is playing. */
    static PlaybackTrace current() { return current; }

    /** Marks an event on the current playback trace, if there is one. */
    static void markCurrent(String event, String detail) {
        PlaybackTrace trace = current;
        if (trace != null) trace.mark(event, detail);
    }

    private final long startedNs = SystemClock.elapsedRealtimeNanos();
    private final String trackId;
    private final List<String> events = Collections.synchronizedList(new ArrayList<>());
    private volatile long engine;
    private volatile boolean ttfaReported;

    PlaybackTrace(String trackId) {
        this.trackId = trackId == null || trackId.isEmpty() ? "unknown" : trackId;
        mark(PLAY_TAP);
    }

    /**
     * Registra un evento de la línea temporal Java.
     *
     * <p>Se escribe en logcat con su desplazamiento respecto al Play tap, de
     * modo que un {@code adb logcat -s TunefoldPerf} basta para reconstruir una
     * reproducción completa.
     */
    void mark(String event) {
        mark(event, "");
    }

    void mark(String event, String detail) {
        long elapsedUs = elapsedUs();
        String line = event
                + (detail == null || detail.isEmpty() ? "" : " " + detail)
                + " @" + (elapsedUs / 1000L) + "ms";
        events.add(line);
        Log.i(TAG, "track=" + trackId + " " + line);
    }

    /**
     * Microsegundos transcurridos desde el Play tap.
     *
     * <p>Es el valor que se pasa a Rust para anclar su línea temporal al mismo
     * origen, y el que define TTFA.
     */
    long elapsedUs() {
        return (SystemClock.elapsedRealtimeNanos() - startedNs) / 1_000L;
    }

    long elapsedMs() {
        return elapsedUs() / 1000L;
    }

    /** Asocia el handle del engine Rust para poder drenar su traza. */
    void attachEngine(long engineHandle) {
        this.engine = engineHandle;
    }

    /**
     * Registra el primer write positivo a AudioTrack y calcula TTFA.
     *
     * <p>Es el proxy mínimo aceptable de salida audible: el buffer de
     * AudioTrack aún puede no haber sonado, así que no mideacoustic onset.
     */
    void markFirstPositiveWrite(int frames) {
        mark(AUDIOTRACK_FIRST_POSITIVE_WRITE, "frames=" + frames);
        if (ttfaReported) return;
        ttfaReported = true;
        Log.i(TAG, "track=" + trackId + " " + TTFA + " " + elapsedMs() + "ms");
        drainRustTrace();
    }

    /**
     * Publica la línea temporal completa (Java + Rust) en logcat.
     *
     * <p>Se llama en el primer write positivo, cuando ya están todos los datos:
     * así el volcado ordenado de Rust no depende del ritmo de sondeo de la UI.
     */
    void drainRustTrace() {
        long handle = engine;
        if (handle == 0) return;
        String raw;
        try {
            raw = TunefoldBridge.drainPlaybackTrace(handle);
        } catch (Throwable failure) {
            Log.i(TAG, "track=" + trackId + " trace_unavailable " + failure.getClass().getSimpleName());
            return;
        }
        if (raw == null || raw.isEmpty()) return;
        for (String line : raw.split("\n")) {
            if (line.isEmpty()) continue;
            // Rust emite `at_us<TAB>evento<TAB>detalle`; reemitimos alineado al tap.
            String[] parts = line.split("\t", 3);
            if (parts.length < 2) continue;
            long atMs;
            try {
                atMs = Long.parseLong(parts[0]) / 1000L;
            } catch (NumberFormatException malformed) {
                continue;
            }
            String tail = parts.length > 2 && !parts[2].isEmpty() ? " " + parts[2] : "";
            events.add(parts[1] + tail + " @" + atMs + "ms");
            Log.i(TAG, "track=" + trackId + " " + parts[1] + tail + " @" + atMs + "ms");
        }
    }

    /** Copia ordenada de la línea temporal Java de esta reproducción. */
    List<String> events() {
        synchronized (events) {
            return new ArrayList<>(events);
        }
    }

    /** Resumen compacto de las etapas para el panel de diagnóstico. */
    String summary() {
        StringBuilder text = new StringBuilder();
        for (String line : events()) {
            if (text.length() > 0) text.append('\n');
            text.append(line);
        }
        return text.toString();
    }
}