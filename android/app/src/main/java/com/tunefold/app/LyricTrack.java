package com.tunefold.app;

import java.util.ArrayList;
import java.util.List;

/**
 * Synchronised lyrics: the lines of a song and when each of them is sung.
 *
 * <p>Parsed on the provider worker from what the bridge returns, and then only read:
 * picking the active line is arithmetic over the position, and it happens twenty times
 * a second, so it must not touch JSON or allocate.
 *
 * <p>The rule is the console's: the active line is the last one whose time has passed.
 * That is the whole of "karaoke" — there is no scoring, no lookahead and no
 * smoothing, because a line that is right a moment late still belongs to the moment
 * after the one before it.
 */
final class LyricTrack {

    /** One timed line. */
    static final class Line {
        final long timeMs;
        final String text;

        Line(long timeMs, String text) {
            this.timeMs = timeMs;
            this.text = text;
        }
    }

    /** How many lines are kept; a song has tens, and this is a display, not a file. */
    private static final int MAX_LINES = 400;

    private final String identity;
    private final List<Line> lines = new ArrayList<>();

    LyricTrack(String identity) { this.identity = identity; }

    String identity() { return identity; }

    List<Line> lines() { return lines; }

    boolean isEmpty() { return lines.isEmpty(); }

    void add(long timeMs, String text) {
        if (lines.size() >= MAX_LINES) return;
        lines.add(new Line(timeMs, text));
    }

    /**
     * The line being sung at {@code positionMs}, or {@code null}.
     *
     * <p>A line with no text — an instrumental — is still a line: it is where the song
     * is, and showing the previous one over it would be a lie about what is being sung.
     * Callers decide how to draw an empty line; this only says which one it is.
     */
    Line activeAt(long positionMs) {
        if (lines.isEmpty() || positionMs < 0L) return null;
        int low = 0;
        int high = lines.size() - 1;
        Line found = null;
        while (low <= high) {
            int middle = (low + high) / 2;
            Line candidate = lines.get(middle);
            if (candidate.timeMs <= positionMs) {
                found = candidate;
                low = middle + 1;
            } else {
                high = middle - 1;
            }
        }
        return found;
    }

    /**
     * Lee lo que devuelve el puente.
     *
     * <p>Un lector escrito a mano y no {@code org.json}, por dos razones que se
     * pisaron: el formato es uno solo y lo produce esta misma aplicación, y en las
     * pruebas de JVM {@code org.json} es un esqueleto que devuelve valores en blanco
     * sin decir nada —con él, una letra válida se leía como ausente y el fallo era
     * invisible salvo por una línea que no aparecía.
     *
     * <p>Cualquier cosa que no sea ese formato deja la pista vacía. Media letra es
     * peor que ninguna: muestra la mitad de una canción y nada avisa de que a la otra
     * mitad no se llegó.
     *
     * @param response el JSON de {@link TunefoldBridge#syncedLyrics}
     * @return la pista, nunca {@code null}, vacía cuando no hay nada que cantar
     */
    static LyricTrack parse(String identity, String response) {
        String who = identity == null ? "" : identity;
        if (response == null || response.isEmpty()) return new LyricTrack(who);
        Reader reader = new Reader(response);
        try {
            if (!reader.object()) return new LyricTrack(who);
            boolean found = false;
            while (reader.member()) {
                String key = reader.string();
                if (!reader.colon()) return new LyricTrack(who);
                if (!"lines".equals(key)) {
                    if (!reader.skipValue()) return new LyricTrack(who);
                    continue;
                }
                found = true;
                LyricTrack track = new LyricTrack(who);
                if (!reader.lines(track)) return new LyricTrack(who);
                // El objeto tiene que cerrar y no quedar nada detrás. Sin esta
                // comprobación una respuesta cortada a mitad se leía como letra
                // válida y el teléfono cantaba las primeras líneas de una canción
                // que se había perdido a la mitad.
                if (!reader.endOfObject()) return new LyricTrack(who);
                return track;
            }
            // Sin "lines" la respuesta fue un {"ok":false}: no hay letra, no hay fallo.
            return found ? new LyricTrack(who) : new LyricTrack(who);
        } catch (RuntimeException malformed) {
            return new LyricTrack(who);
        }
    }

    /**
     * Lector de lo justo que emite el puente: un objeto con {@code lines}, cada uno un
     * objeto con un número {@code t} y un texto {@code s}.
     *
     * <p>Cualquier desviación lanza. Es lo que hace que un formato cambiado se note en
     * una prueba y no en un teléfono donde la letra simplemente nunca aparece.
     */
    private static final class Reader {
        private final String source;
        private int at;

        Reader(String source) { this.source = source; }

        private void space() {
            while (at < source.length() && Character.isWhitespace(source.charAt(at))) at++;
        }

        private char take() {
            if (at >= source.length()) throw new IllegalStateException("unexpected end");
            return source.charAt(at++);
        }

        private void expect(char wanted) {
            if (take() != wanted) throw new IllegalStateException("expected " + wanted);
        }

        /** Abre el objeto de la respuesta; si está vacío, {@link #member()} lo dirá. */
        private boolean object() {
            space();
            expect('{');
            return true;
        }

        /**
         * ¿Hay otro miembro? Come la coma si la hay.
         *
         * <p>Devuelve {@code false} sólo al encontrar la llave de cierre, y al
         * acabarse el texto lanza. La diferencia importa: devolver {@code false} en los
         * dos casos hacía que una respuesta cortada pasara por una respuesta cerrada.
         */
        private boolean member() {
            space();
            char next = take();
            if (next == '}') return false;
            if (next == ',') {
                space();
                next = take();
            }
            if (next != '"') throw new IllegalStateException("expected a member, got " + next);
            // La comilla de apertura se le devuelve a quien lee el nombre: leerla
            // aquí y otra vez en string() era un byte de más y un texto de menos.
            at--;
            return true;
        }

        private boolean colon() {
            space();
            expect(':');
            return true;
        }

        /**
         * Un texto completo, comillas incluidas.
         *
         * <p>Se come la comilla de apertura, porque quien la encuentra ya sabe que
         * empieza un texto: leerla de nuevo era el error de leer dos veces la misma
         * comilla y devolver una cadena vacía donde había una letra entera.
         */
        private String string() {
            space();
            expect('"');
            StringBuilder out = new StringBuilder();
            while (true) {
                char next = take();
                if (next == '"') return out.toString();
                if (next != '\\') { out.append(next); continue; }
                char escaped = take();
                switch (escaped) {
                    case 'n': out.append('\n'); break;
                    case 't': out.append('\t'); break;
                    case 'r': out.append('\r'); break;
                    case 'b': out.append('\b'); break;
                    case 'f': out.append('\f'); break;
                    case 'u': {
                        String digits = source.substring(at, at + 4);
                        at += 4;
                        try {
                            out.append((char) Integer.parseInt(digits, 16));
                        } catch (NumberFormatException broken) {
                            throw new IllegalStateException("bad escape");
                        }
                        break;
                    }
                    default: out.append(escaped); break;
                }
            }
        }

        /** Salta un valor que no interesa, con su profundidad. */
        private boolean skipValue() {
            space();
            char next = source.charAt(at);
            if (next == '"') { string(); return true; }
            if (next != '{' && next != '[') {
                while (at < source.length() && ",}]".indexOf(source.charAt(at)) < 0) at++;
                return true;
            }
            int depth = 0;
            while (at < source.length()) {
                char step = source.charAt(at++);
                if (step == '"') { at--; string(); continue; }
                if (step == '{' || step == '[') depth++;
                if (step == '}' || step == ']') {
                    depth--;
                    if (depth == 0) return true;
                }
            }
            throw new IllegalStateException("unbalanced");
        }

        /**
         * Los miembros que van detrás de {@code lines}, y el cierre del objeto.
         *
         * <p>Hacen falta porque el puente serializa con claves ordenadas: en la
         * respuesta real {@code "ok"} va <em>después</em> de {@code "lines"}, y exigir
         * que no quedara nada detrás descartaba letras perfectamente buenas. Lo que sí
         * se exige es que el objeto cierre de verdad y no quede cola: una respuesta
         * cortada a la mitad no es una canción, es media canción.
         */
        private boolean endOfObject() {
            while (member()) {
                string();
                colon();
                if (!skipValue()) return false;
            }
            space();
            return at >= source.length();
        }

        /** El arreglo de {@code lines}, como lo emite el puente. */
        private boolean lines(LyricTrack track) {
            space();
            expect('[');
            space();
            if (source.charAt(at) == ']') { at++; return true; }
            while (true) {
                space();
                expect('{');
                long time = 0L;
                String text = "";
                while (true) {
                    space();
                    if (source.charAt(at) == '}') { at++; break; }
                    if (source.charAt(at) == ',') { at++; continue; }
                    String key = string();
                    colon();
                    space();
                    if ("t".equals(key)) {
                        time = Long.parseLong(number());
                    } else if ("s".equals(key)) {
                        text = string();
                    } else if (!skipValue()) {
                        throw new IllegalStateException("bad line");
                    }
                }
                track.add(time, text);
                space();
                char next = source.charAt(at);
                at++;
                if (next == ']') return true;
                if (next != ',') throw new IllegalStateException("bad lines");
            }
        }

        private String number() {
            int start = at;
            while (at < source.length() && "-+.eE0123456789".indexOf(source.charAt(at)) >= 0) at++;
            if (at == start) throw new IllegalStateException("no number");
            return source.substring(start, at);
        }
    }
}
