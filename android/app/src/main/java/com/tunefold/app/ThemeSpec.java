package com.tunefold.app;

import java.util.LinkedHashMap;
import java.util.Locale;
import java.util.Map;

/**
 * Un tema: cinco colores y un nombre. Es todo lo que el usuario puede elegir.
 *
 * <p>Los cinco vienen dados y no se tocan. Lo que el tema <em>no</em> lleva es una
 * tabla de roles: nadie elige «SURFACE_CARD_ACTIVE» en un selector de color, y
 *escribir eso para un humano sería una lista de identificadores. El usuario elige cinco
 * colores que significan algo —un fondo, una luz, un acento, dos tintas— y
 * {@link ThemePalette} decide cuál es cuál y qué superficies salen de ellos.
 *
 * <p>Los cinco pueden venir en cualquier orden y el orden no significa nada: los
 * roles se deciden por cómo se ven los colores, no por su posición en la lista. Es
 * lo que permite que un tema con cuatro colores claros y uno oscuro
 * ({@link #CHOIR_PASTE}) y uno con dos oscuros y tres claros
 * ({@link #STAGE_SPOTLIGHT}) coexistir sin reglas especiales.
 */
final class ThemeSpec {

    /** Identidad estable; lo que se guarda en las preferencias. */
    final String id;
    /** Cómo se llama en la lista de ajustes. */
    final String name;
    /** Los cinco colores, exactamente como los eligió quien definió el tema. */
    final int[] colors;

    private ThemeSpec(String id, String name, int... colors) {
        this.id = id;
        this.name = name;
        this.colors = colors;
    }

    // ---------------------------------------------------------- the catalogue

    /** Fondo casi negro de sala, foco ámbar y un rojo de escenario. */
    static final ThemeSpec STAGE_SPOTLIGHT =
            new ThemeSpec("stage_spotlight", "Stage Spotlight",
                    0xFF111827, 0xFFF59E0B, 0xFFFDE68A, 0xFFEF4444, 0xFFF9FAFB);

    /** Madera, coñac y papel crema: un club con la luz baja con la luz baja. */
    static final ThemeSpec JAZZ_LOUNGE =
            new ThemeSpec("jazz_loounge", "Jazz Loounge",
                    0xFF2B1D1A, 0xFF7C2D12, 0xFFD97706, 0xFFF3E8D4, 0xFF1C1917);

    /** Hormigón, acero y un naranja de obra. */
    static final ThemeSpec DRUMLINE_CONCRETE =
            new ThemeSpec("drumline_concrete", "Drumline Concrete",
                    0xFF111827, 0xFF374151, 0xFF9CA3AF, 0xFFF97316, 0xFFF3F4F6);

    /** Violeta de cierre, rosa y una luz fría. */
    static final ThemeSpec INDIE_DUSK =
            new ThemeSpec("indie_dusk", "Indie Dusk",
                    0xFF2D1B69, 0xFF4B5563, 0xFFFCA5A5, 0xFFF3F4F6, 0xFF111827);

    /** Granate, carmesí y oro de telón. */
    static final ThemeSpec OPERA_VIOLET =
            new ThemeSpec("opera_violet", "Opera Violet",
                    0xFF4C0519, 0xFF9F1239, 0xFFF1C40F, 0xFFF5F3FF, 0xFF1F2937);

    /** Pasteles: el único tema de campo claro. */
    static final ThemeSpec CHOIR_PASTE =
            new ThemeSpec("choir_paste", "Choir Paste",
                    0xFFFCE7F3, 0xFFDDD6FE, 0xFFC7F9CC, 0xFFBAE6FD, 0xFF1F2937);

    /** El tema con el que arranca la app: el primero de la lista. */
    static final ThemeSpec DEFAULT = STAGE_SPOTLIGHT;

    private static final Map<String, ThemeSpec> BY_ID = new LinkedHashMap<>();

    static {
        for (ThemeSpec theme : all()) BY_ID.put(theme.id, theme);
    }

    /** Todos los temas, en el orden en que se ofrecen. */
    static java.util.List<ThemeSpec> all() {
        return java.util.Arrays.asList(STAGE_SPOTLIGHT, JAZZ_LOUNGE,
                DRUMLINE_CONCRETE, INDIE_DUSK, OPERA_VIOLET, CHOIR_PASTE);
    }

    /** El tema con este identificador, o {@link #DEFAULT} si no se reconoce. */
    static ThemeSpec byId(String id) {
        ThemeSpec theme = id == null ? null : BY_ID.get(id);
        return theme == null ? DEFAULT : theme;
    }

    /** El color en la posición {@code index}, o negro si el tema está incompleto. */
    int color(int index) {
        return index >= 0 && index < colors.length ? colors[index] : 0xFF000000;
    }

    @Override public String toString() {
        return name.toLowerCase(Locale.ROOT);
    }
}