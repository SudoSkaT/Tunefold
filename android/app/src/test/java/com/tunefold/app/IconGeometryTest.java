package com.tunefold.app;

import static org.junit.Assert.assertTrue;

import org.junit.Test;

public class IconGeometryTest {

    /**
     * Cada icono de la familia tiene que.parsear a algo que se pueda pintar.
     *
     * <p>Un contorno mal formado no lanza: {@link IconPath} degrada a
     * {@link IconPath#empty()}, y el resultado es un control invisible sin ningún
     * error en ninguna parte. Eso es exactamente lo que pasó con el visto del tema
     * puesto, que salía en la jerarquía y no se veía en la pantalla.
     */
    @Test public void everyIconParsesToInkInsideTheGrid() {
        StringBuilder blind = new StringBuilder();
        for (Icon icon : Icon.values()) {
            IconPath path = icon.path();
            if (path.isEmpty()) {
                blind.append(icon).append("(vacío) ");
                continue;
            }
            if (path.minX() < 0f || path.minY() < 0f
                    || path.maxX() > DesignTokens.ICON_GRID
                    || path.maxY() > DesignTokens.ICON_GRID) {
                blind.append(icon).append("(fuera de la rejilla ")
                        .append(path.minX()).append(' ').append(path.minY()).append(' ')
                        .append(path.maxX()).append(' ').append(path.maxY()).append(") ");
            }
        }
        assertTrue("Iconos que no se pueden pintar: " + blind, blind.length() == 0);
    }
}
