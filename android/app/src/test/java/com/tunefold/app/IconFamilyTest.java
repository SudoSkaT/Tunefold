package com.tunefold.app;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

/**
 * Guards the icon family.
 *
 * <p>Path data is hand-authored, so the failure mode is silent: a typo produces a
 * button that draws nothing, and the user sees a dead control with no explanation.
 * These assertions turn that into a failing build.
 */
public class IconFamilyTest {

    @Test public void everyIconParsesToGeometry() {
        for (Icon icon : Icon.values()) {
            assertFalse(icon + " has no geometry", icon.path().isEmpty());
        }
    }

    /** The stroke family must stay inside its box, or a glyph is clipped by its target. */
    @Test public void everyIconFitsTheAuthoringGrid() {
        float low = -DesignTokens.ICON_STROKE;
        float high = DesignTokens.ICON_GRID + DesignTokens.ICON_STROKE;
        for (Icon icon : Icon.values()) {
            IconPath path = icon.path();
            assertTrue(icon + " starts left of the grid at " + path.minX(),
                    path.minX() >= low);
            assertTrue(icon + " starts above the grid at " + path.minY(),
                    path.minY() >= low);
            assertTrue(icon + " ends right of the grid at " + path.maxX(),
                    path.maxX() <= high);
            assertTrue(icon + " ends below the grid at " + path.maxY(),
                    path.maxY() <= high);
        }
    }

    /** A glyph that occupies no area at all would draw nothing. */
    @Test public void everyIconHasPositiveExtent() {
        for (Icon icon : Icon.values()) {
            IconPath path = icon.path();
            boolean wide = path.maxX() > path.minX();
            boolean tall = path.maxY() > path.minY();
            assertTrue(icon + " has neither width nor height", wide || tall);
        }
    }

    /** A glyph must be big enough to read, not a sub-pixel detail. */
    @Test public void everyIconOccupiesARealPortionOfTheGrid() {
        for (Icon icon : Icon.values()) {
            IconPath path = icon.path();
            float longer = Math.max(path.maxX() - path.minX(), path.maxY() - path.minY());
            assertTrue(icon + " spans only " + longer + " units", longer >= 8f);
        }
    }

    @Test public void everyIconStrokesWithTheFamilyWeight() {
        for (Icon icon : Icon.values()) {
            if (icon.isFilled()) continue;
            assertEquals(DesignTokens.ICON_STROKE, icon.strokeWidth(), 0f);
        }
    }

    @Test public void onlyTheTransportFamilyIsFilled() {
        for (Icon icon : Icon.values()) {
            if (!icon.isFilled()) continue;
            String name = icon.name();
            assertTrue(name + " is filled but is not transport",
                    "PLAY".equals(name) || "PAUSE".equals(name) || "PREVIOUS".equals(name)
                            || "NEXT".equals(name) || "STOP".equals(name)
                            || "HEART_FILLED".equals(name));
        }
    }

    /**
     * A filled twin must share its outline exactly, otherwise selecting an option
     * changes its silhouette and the set stops looking like one family.
     */
    @Test public void selectedAndUnselectedTwinsShareTheirOutline() {
        assertEquals(Icon.HEART.geometry(), Icon.HEART_FILLED.geometry());
        assertFalse(Icon.HEART.isFilled());
        assertTrue(Icon.HEART_FILLED.isFilled());
        assertFalse(Icon.AUTOPLAY.geometry().equals(Icon.AUTOPLAY_OFF.geometry()));
    }

    @Test public void everyIconTheSurfaceUsesExists() {
        Icon[] required = {Icon.PLAY, Icon.PAUSE, Icon.PREVIOUS, Icon.NEXT, Icon.STOP,
                Icon.SEARCH, Icon.HEART, Icon.HEART_FILLED, Icon.DOWNLOAD,
                Icon.DOWNLOADED, Icon.DOWNLOADS, Icon.MORE, Icon.QUEUE, Icon.AUTOPLAY,
                Icon.AUTOPLAY_OFF, Icon.BACK, Icon.HOME, Icon.LIBRARY, Icon.SETTINGS,
                Icon.RETRY, Icon.DIAGNOSTICS, Icon.WARNING, Icon.CLOSE, Icon.CHEVRON_RIGHT};
        for (Icon icon : required) {
            assertFalse(icon + " is unparseable", icon.path().isEmpty());
        }
    }

    // ------------------------------------------------------- the path dialect

    @Test public void parserReadsEveryCommand() {
        IconPath path = IconPath.parse("M 1 2 L 3 4 C 5 6 7 8 9 10 Q 11 12 13 14 Z");
        assertEquals(5, path.size());
        assertEquals(IconPath.MOVE, path.opAt(0));
        assertEquals(IconPath.LINE, path.opAt(1));
        assertEquals(IconPath.CUBIC, path.opAt(2));
        assertEquals(IconPath.QUAD, path.opAt(3));
        assertEquals(IconPath.CLOSE, path.opAt(4));
        assertEquals(1f, path.valueAt(path.offsetOf(0)), 0f);
        assertEquals(2f, path.valueAt(path.offsetOf(0) + 1), 0f);
        assertEquals(13f, path.valueAt(path.offsetOf(3) + 2), 0f);
    }

    @Test public void parserAcceptsCommasAndSignedNumbers() {
        IconPath commas = IconPath.parse("M1,2L-3,-4.5");
        IconPath spaces = IconPath.parse("M 1 2 L -3 -4.5");
        assertEquals(commas.size(), spaces.size());
        assertEquals(-3f, commas.valueAt(2), 0f);
        assertEquals(-4.5f, commas.valueAt(3), 0f);
    }

    @Test public void parserTreatsASignAsTheNextNumber() {
        IconPath path = IconPath.parse("M 1 2 L 3-4");
        assertEquals(2, path.size());
        assertEquals(3f, path.valueAt(2), 0f);
        assertEquals(-4f, path.valueAt(3), 0f);
    }

    @Test public void parserTracksBounds() {
        IconPath path = IconPath.parse("M -2 -3 L 20 18");
        assertEquals(-2f, path.minX(), 0f);
        assertEquals(-3f, path.minY(), 0f);
        assertEquals(20f, path.maxX(), 0f);
        assertEquals(18f, path.maxY(), 0f);
    }

    /** A bad definition must degrade to nothing drawn, never to a crash on startup. */
    @Test public void malformedDefinitionsDegradeToEmpty() {
        assertTrue(IconPath.parse(null).isEmpty());
        assertTrue(IconPath.parse("").isEmpty());
        assertTrue(IconPath.parse("   ").isEmpty());
        assertTrue(IconPath.parse("Z").isEmpty());
        assertTrue(IconPath.parse("X 1 2").isEmpty());
        assertTrue(IconPath.parse("M 1").isEmpty());
        assertTrue(IconPath.parse("M 1 2 L").isEmpty());
        assertTrue(IconPath.parse("M 1 2 L a b").isEmpty());
    }

    @Test public void lowercaseCloseIsAccepted() {
        IconPath path = IconPath.parse("M 1 2 z");
        assertEquals(2, path.size());
        assertEquals(IconPath.CLOSE, path.opAt(1));
    }

    @Test public void valueCountsMatchTheDialect() {
        assertEquals(2, IconPath.valueCount(IconPath.MOVE));
        assertEquals(2, IconPath.valueCount(IconPath.LINE));
        assertEquals(6, IconPath.valueCount(IconPath.CUBIC));
        assertEquals(4, IconPath.valueCount(IconPath.QUAD));
        assertEquals(0, IconPath.valueCount(IconPath.CLOSE));
    }
}