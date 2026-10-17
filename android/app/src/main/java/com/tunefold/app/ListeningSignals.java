package com.tunefold.app;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.util.ArrayDeque;
import java.util.List;

/**
 * Feedback de escucha (plays, skips, completados y clics de recomendación)
 * persistido por la app y convertido en la petición del ranking compartido.
 *
 * <p>Fase 5 del plan de recomendaciones: Java es el dueño de los datos y el
 * estado, el {@code .so} es <b>stateless</b> — cada llamada a
 * {@link TunefoldBridge#rankRelated} lleva candidatos + señales + perfiles en
 * JSON y devuelve el orden. El formato es línea/tabulado (tipo
 * {@code PlaybackTrace}), nunca org.json: los tests unitarios corren contra el
 * stub de android.jar, donde {@code org.json} no funciona.
 *
 * <p>Una línea = {@code kind \t context \t atMs \t listenedMs \t rawJson};
 * {@code listenedMs} es {@code -1} cuando no se conoce (solo lo usa
 * {@code completed}). Se conservan como máximo {@link #MAX_SIGNALS} eventos
 * (los más viejos se descartan) y la escritura es atómica estilo
 * {@code Library}: temporal + rename, para que un crash no trunque el archivo.
 */
final class ListeningSignals {

    private static final int MAX_SIGNALS = 200;
    static final String KIND_PLAY = "play";
    static final String KIND_SKIP = "skip";
    static final String KIND_COMPLETED = "completed";
    static final String KIND_REC_CLICK = "rec_click";

    private final File file;
    private final ArrayDeque<String[]> entries = new ArrayDeque<>();

    ListeningSignals(File file) {
        this.file = file;
        load();
    }

    /**
     * Registra un evento y lo persiste. Best-effort: cualquier fallo de E/S
     * se ignora y el ranking simplemente degrada a fail-open con menos
     * señales.
     *
     * @param listenedMs milisegundos escuchados de verdad, o {@code -1} si no
     *                   se conoce (solo relevante para {@code completed})
     */
    synchronized void record(MediaTrack track, String kind, String context, long listenedMs) {
        if (track == null || !usableRaw(track.rawJson) || kind == null || context == null) {
            return;
        }
        long at = System.currentTimeMillis();
        entries.addLast(new String[] {
                kind, context, Long.toString(at), Long.toString(listenedMs), track.rawJson });
        while (entries.size() > MAX_SIGNALS) {
            entries.removeFirst();
        }
        save();
    }

    synchronized void record(MediaTrack track, String kind, String context) {
        record(track, kind, context, -1L);
    }

    /**
     * Petición JSON para {@link TunefoldBridge#rankRelated}, o {@code null}
     * cuando no hay con qué puntuar (candidatos vacíos): el llamador
     * conserva el orden del provider.
     *
     * <p>Los candidatos se emiten <b>sin filtrar</b> — los índices de la
     * respuesta deben referirse al mismo array que Java reordena; si un
     * candidato viniera con raw roto, el native falla el parseo y Java hace
     * fail-open al orden del provider, que es el comportamiento seguro.
     */
    synchronized String buildRankRequest(List<MediaTrack> candidates) {
        if (candidates == null || candidates.isEmpty()) {
            return null;
        }
        StringBuilder request = new StringBuilder(4096);
        request.append("{\"candidates\":[");
        boolean first = true;
        for (MediaTrack candidate : candidates) {
            if (candidate == null) {
                continue;
            }
            if (!first) {
                request.append(',');
            }
            request.append(candidate.rawJson == null ? "{}" : candidate.rawJson);
            first = false;
        }
        request.append("],\"signals\":[");
        first = true;
        for (String[] entry : entries) {
            if (!usableRaw(entry[4])) {
                continue;
            }
            if (!first) {
                request.append(',');
            }
            request.append("{\"kind\":\"").append(jsonEscape(entry[0]))
                    .append("\",\"context\":\"").append(jsonEscape(entry[1]))
                    .append("\",\"at\":").append(entry[2])
                    .append(",\"track\":").append(entry[4]);
            if (Long.parseLong(entry[3]) >= 0) {
                request.append(",\"duration_ms\":").append(entry[3]);
            }
            request.append('}');
            first = false;
        }
        request.append("]}");
        return request.toString();
    }

    /** Señales almacenadas, de más antigua a más reciente (para tests). */
    synchronized int signalCount() {
        return entries.size();
    }

    private boolean usableRaw(String rawJson) {
        return rawJson != null && rawJson.length() > 2 && !"{}".equals(rawJson.trim());
    }

    private void load() {
        byte[] bytes = readAll(file);
        if (bytes == null) {
            return;
        }
        String content = new String(bytes, StandardCharsets.UTF_8);
        String[] lines = content.split("\n");
        for (String line : lines) {
            if (line.isEmpty()) {
                continue;
            }
            String[] columns = line.split("\t", 5);
            if (columns.length < 5) {
                continue;
            }
            try {
                Long.parseLong(columns[2]);
                Long.parseLong(columns[3]);
            } catch (NumberFormatException corrupted) {
                continue;
            }
            entries.addLast(columns);
        }
        while (entries.size() > MAX_SIGNALS) {
            entries.removeFirst();
        }
    }

    /** Temporal + sync + rename: un crash no deja el archivo a medias. */
    private void save() {
        StringBuilder text = new StringBuilder(4096);
        for (String[] entry : entries) {
            text.append(entry[0]).append('\t')
                    .append(entry[1]).append('\t')
                    .append(entry[2]).append('\t')
                    .append(entry[3]).append('\t')
                    .append(entry[4]).append('\n');
        }
        File parent = file.getParentFile();
        if (parent != null && !parent.isDirectory() && !parent.mkdirs()) {
            return;
        }
        File temporary = new File(parent, file.getName() + ".part");
        try {
            try (FileOutputStream output = new FileOutputStream(temporary)) {
                output.write(text.toString().getBytes(StandardCharsets.UTF_8));
                output.getFD().sync();
            }
            if (!temporary.renameTo(file)) {
                // renameTo no sobrescribe en algunos filesystems.
                file.delete();
                if (!temporary.renameTo(file)) {
                    return;
                }
            }
        } catch (IOException failure) {
            temporary.delete();
        }
    }

    private static byte[] readAll(File source) {
        if (source == null || !source.isFile() || source.length() > 4L * 1024 * 1024) {
            return null;
        }
        byte[] bytes = new byte[(int) source.length()];
        try (InputStream input = new FileInputStream(source)) {
            int offset = 0;
            while (offset < bytes.length) {
                int read = input.read(bytes, offset, bytes.length - offset);
                if (read < 0) {
                    return null;
                }
                offset += read;
            }
            return bytes;
        } catch (IOException failure) {
            return null;
        }
    }

    private static String jsonEscape(String value) {
        StringBuilder escaped = new StringBuilder(value.length() + 8);
        for (int i = 0; i < value.length(); i++) {
            char c = value.charAt(i);
            switch (c) {
                case '"':
                    escaped.append("\\\"");
                    break;
                case '\\':
                    escaped.append("\\\\");
                    break;
                case '\n':
                    escaped.append("\\n");
                    break;
                case '\r':
                    escaped.append("\\r");
                    break;
                case '\t':
                    escaped.append("\\t");
                    break;
                default:
                    if (c < 0x20) {
                        escaped.append(String.format("\\u%04x", (int) c));
                    } else {
                        escaped.append(c);
                    }
                    break;
            }
        }
        return escaped.toString();
    }
}
