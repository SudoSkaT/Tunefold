package com.tunefold.app;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertNull;
import static org.junit.Assert.assertTrue;

import java.util.List;
import org.junit.Test;

/**
 * Qué línea se está cantando, y qué se hace con una respuesta que no es letra.
 *
 * <p>La regla probada es la de la consola: la activa es la última cuyo momento ya
 * pasó. No hay puntuación ni suavizado, y probarlo aquí es la forma de que el
 * reproductor y la consola no puedan discrepar sobre la misma canción.
 */
public class LyricTrackTest {

    private static final String RESPONSE = "{\"ok\":true,\"lines\":["
            + "{\"t\":1000,\"s\":\"First line\"},"
            + "{\"t\":4000,\"s\":\"Second line\"},"
            + "{\"t\":9000,\"s\":\"\"}]}";

    @Test public void parsesTimedLines() {
        LyricTrack track = LyricTrack.parse("YouTube/abc", RESPONSE);
        assertFalse(track.isEmpty());
        List<LyricTrack.Line> lines = track.lines();
        assertEquals(3, lines.size());
        assertEquals("First line", lines.get(0).text);
        assertEquals(9000L, lines.get(2).timeMs);
    }

    @Test public void activeLineIsTheLastOneThatHasPassed() {
        LyricTrack track = LyricTrack.parse("id", RESPONSE);
        assertEquals("First line", text(track.activeAt(1000L)));
        assertEquals("First line", text(track.activeAt(3999L)));
        assertEquals("Second line", text(track.activeAt(4000L)));
        assertEquals("Second line", text(track.activeAt(8999L)));
    }

    @Test public void nothingIsSungBeforeTheFirstLine() {
        LyricTrack track = LyricTrack.parse("id", RESPONSE);
        assertNull(track.activeAt(0L));
        assertNull(track.activeAt(999L));
        assertNull(track.activeAt(-1L));
    }

    @Test public void anInstrumentalBreaksIsStillALine() {
        LyricTrack track = LyricTrack.parse("id", RESPONSE);
        LyricTrack.Line line = track.activeAt(12000L);
        assertNotNull("the song is somewhere, even where nobody sings", line);
        assertEquals("", line.text);
    }

    @Test public void aTrackWithoutLyricsIsEmptyNotBroken() {
        assertTrue(LyricTrack.parse("id", null).isEmpty());
        assertTrue(LyricTrack.parse("id", "").isEmpty());
        assertTrue(LyricTrack.parse("id", "{\"ok\":false,\"error\":\"lyrics_unavailable\"}").isEmpty());
        assertTrue(LyricTrack.parse("id", "{\"ok\":true}").isEmpty());
        assertTrue(LyricTrack.parse("id", "{\"ok\":true,\"lines\":[]}").isEmpty());
    }

    @Test public void malformedJsonLeavesNothingHalfParsed() {
        // A truncated answer is a failure, not a lyric with the first lines intact:
        // showing half a song is worse than showing none.
        assertTrue(LyricTrack.parse("id", "{\"ok\":true,\"lines\":[{\"t\":1,\"s\":\"a\"}]").isEmpty());
        assertTrue(LyricTrack.parse("id", "not json at all").isEmpty());
    }

    @Test public void linesWithoutTextDoNotThrow() {
        LyricTrack track = LyricTrack.parse("id", "{\"ok\":true,\"lines\":[{\"t\":10}]}");
        assertFalse(track.isEmpty());
        assertEquals("", track.activeAt(10L).text);
    }

    /**
     * Comillas y barras invertidas: lo que escape el puente tiene que volver entero.
     *
     * <p>Una letra cantada dice "don't" y las comillas son el signo de puntuación de
     * media canción. Si el lector las comía como cierre, la línea se cortaba por el
     * primer signo de puntuación de la canción y no se notaba hasta cantarla.
     */
    @Test public void punctuationAndBackslashesSurvive() {
        LyricTrack track = LyricTrack.parse("id", "{\"ok\":true,\"lines\":["
                + "{\"t\":1,\"s\":\"quote \\\" inside\"},"
                + "{\"t\":2,\"s\":\"back\\\\slash\"},"
                + "{\"t\":3,\"s\":\"tab\\there\"}]}");
        assertFalse(track.isEmpty());
        assertEquals(3, track.lines().size());
        assertEquals("quote \" inside", track.lines().get(0).text);
        assertEquals("back\\slash", track.lines().get(1).text);
        assertEquals("tab\there", track.lines().get(2).text);
    }

    /** Una respuesta cortada no es media canción: no es canción. */
    @Test public void aTruncatedAnswerIsNotHalfASong() {
        assertTrue(LyricTrack.parse("id", "{\"ok\":true,\"lines\":[{\"t\":1,\"s\":\"a\"}]").isEmpty());
        assertTrue(LyricTrack.parse("id", "{\"ok\":true,\"lines\":[{\"t\":1,\"s\":\"a\"}").isEmpty());
        assertTrue(LyricTrack.parse("id", "{\"ok\":true,\"lines\":[{\"t\":1,\"s\":\"a\"}]}{\"x\":1}").isEmpty());
    }

    /**
     * El orden con el que llega de verdad.
     *
     * <p>El puente serializa con claves ordenadas, así que {@code "ok"} va detrás de
     * {@code "lines"} y cada línea trae la letra antes que el tiempo. Un lector escrito
     * para el orden que uno se imagine descarta la letra entera sin decir nada, y en
     * el teléfono eso se ve como una canción que nunca muestra su letra.
     */
    @Test public void keyOrderDoesNotMatter() {
        String asItArrives = "{\"lines\":[{\"s\":\"One more time\",\"t\":30750},"
                + "{\"s\":\"\",\"t\":33180}],\"ok\":true}";
        LyricTrack track = LyricTrack.parse("id", asItArrives);
        assertFalse(track.isEmpty());
        assertEquals(2, track.lines().size());
        assertEquals(30_750L, track.lines().get(0).timeMs);
        assertEquals("One more time", track.lines().get(0).text);
        assertEquals("One more time", text(track.activeAt(31_000L)));
    }

    /** Miembros que sobran detrás no rompen la lectura, pero una cola cortada sí. */
    @Test public void trailingMembersAreAllowedOnlyIfTheObjectCloses() {
        assertFalse(LyricTrack.parse("id",
                "{\"lines\":[{\"t\":1,\"s\":\"a\"}],\"ok\":true}").isEmpty());
        assertTrue(LyricTrack.parse("id",
                "{\"lines\":[{\"t\":1,\"s\":\"a\"}],\"ok\":true").isEmpty());
        assertTrue(LyricTrack.parse("id",
                "{\"lines\":[{\"t\":1,\"s\":\"a\"}],\"ok\":true,}").isEmpty());
    }

    private static String text(LyricTrack.Line line) {
        return line == null ? null : line.text;
    }
}
