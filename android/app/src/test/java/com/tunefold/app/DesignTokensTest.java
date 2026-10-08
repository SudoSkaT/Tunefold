package com.tunefold.app;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertTrue;
import static org.junit.Assert.fail;

import org.junit.Test;

/**
 * Guards the design-token scale.
 *
 * <p>The scale is only worth having if it cannot rot. These assertions are the ones
 * that were actually violated by the surface this phase replaced — a 40&nbsp;dp
 * navigation slot and a 40&nbsp;dp error action both slipped through review — so they
 * are executable rather than a comment.
 *
 * <p>Reads only the raw {@code dp}/{@code sp} constants, so it needs no Android
 * framework.
 */
public class DesignTokensTest {

    @Test public void spacingScaleIsStrictlyIncreasing() {
        float[] scale = {DesignTokens.SPACE_1, DesignTokens.SPACE_2, DesignTokens.SPACE_3,
                DesignTokens.SPACE_4, DesignTokens.SPACE_5, DesignTokens.SPACE_6,
                DesignTokens.SPACE_7, DesignTokens.SPACE_8, DesignTokens.SPACE_9};
        for (int i = 1; i < scale.length; i++) {
            assertTrue("spacing step " + i + " must exceed its predecessor",
                    scale[i] > scale[i - 1]);
        }
    }

    @Test public void spacingScaleStartsAtTwoDp() {
        assertEquals(2f, DesignTokens.SPACE_1, 0f);
    }

    /** The hard accessibility floor. Any control dimension under it is a regression. */
    @Test public void noControlDimensionDropsUnderTheTouchTargetFloor() {
        float[] controls = {DesignTokens.CONTROL_TARGET,
                DesignTokens.CONTROL_TARGET_SECONDARY,
                DesignTokens.CONTROL_TARGET_PRIMARY,
                DesignTokens.CONTROL_TARGET_NAVIGATION,
                DesignTokens.ROW_HEIGHT,
                DesignTokens.TOP_BAR_HEIGHT,
                DesignTokens.SEARCH_FIELD_HEIGHT};
        for (float control : controls) {
            assertTrue("control " + control + "dp is under the "
                            + DesignTokens.TOUCH_TARGET_MIN + "dp touch target",
                    control >= DesignTokens.TOUCH_TARGET_MIN);
        }
    }

    @Test public void primaryControlIsLargerThanASecondaryOne() {
        assertTrue(DesignTokens.CONTROL_TARGET_PRIMARY > DesignTokens.CONTROL_TARGET);
    }

    @Test public void touchTargetFloorItselfIsTheAccessibilityMinimum() {
        assertEquals(48f, DesignTokens.TOUCH_TARGET_MIN, 0f);
    }

    @Test public void iconSizesAreStrictlyIncreasing() {
        float[] sizes = {DesignTokens.ICON_SIZE_EXTRA_SMALL, DesignTokens.ICON_SIZE_SMALL,
                DesignTokens.ICON_SIZE_MEDIUM, DesignTokens.ICON_SIZE_LARGE,
                DesignTokens.ICON_SIZE_EXTRA_LARGE};
        for (int i = 1; i < sizes.length; i++) {
            assertTrue("icon size " + sizes[i] + " must exceed " + sizes[i - 1],
                    sizes[i] > sizes[i - 1]);
        }
    }

    /**
     * Icons are authored on the grid and scaled, so a size may exceed it — but never
     * shrink below the point where the default weight would vanish.
     */
    @Test public void everyIconSizeStaysVisibleWhenScaled() {
        assertTrue(DesignTokens.ICON_GRID > 0f);
        assertTrue(DesignTokens.ICON_SIZE_EXTRA_SMALL > DesignTokens.ICON_STROKE);
        assertTrue(DesignTokens.ICON_SIZE_LARGE > DesignTokens.ICON_SIZE_MEDIUM);
        assertTrue(DesignTokens.ICON_SIZE_EXTRA_LARGE > DesignTokens.ICON_SIZE_LARGE);
    }

    @Test public void strokeWeightIsAPositiveFractionOfTheGrid() {
        assertTrue(DesignTokens.ICON_STROKE > 0f);
        assertTrue(DesignTokens.ICON_STROKE < DesignTokens.ICON_GRID);
    }

    @Test public void artworkBoundsAreOrdered() {
        assertTrue(DesignTokens.ARTWORK_MIN > 0f);
        assertTrue(DesignTokens.ARTWORK_MAX > DesignTokens.ARTWORK_MIN);
        assertTrue(DesignTokens.ARTWORK_THUMB < DesignTokens.ARTWORK_MIN);
    }

    @Test public void cornerRadiiAreOrdered() {
        assertTrue(DesignTokens.RADIUS_SMALL < DesignTokens.RADIUS_MEDIUM);
        assertTrue(DesignTokens.RADIUS_MEDIUM < DesignTokens.RADIUS_LARGE);
    }

    @Test public void elevationsAreOrdered() {
        assertTrue(DesignTokens.ELEVATION_CARD < DesignTokens.ELEVATION_RAISED);
        assertTrue(DesignTokens.ELEVATION_RAISED < DesignTokens.ELEVATION_OVERLAY);
    }

    @Test public void motionDurationsAreOrdered() {
        assertTrue(DesignTokens.DURATION_FAST < DesignTokens.DURATION_BASE);
        assertTrue(DesignTokens.DURATION_BASE < DesignTokens.DURATION_SLOW);
    }

    /** Typography must descend, or the hierarchy is decoration rather than a hierarchy. */
    @Test public void typeScaleDescends() {
        assertTrue(DesignTokens.TYPE_DISPLAY_SIZE >= DesignTokens.TYPE_TITLE_SIZE);
        assertTrue(DesignTokens.TYPE_TITLE_SIZE > DesignTokens.TYPE_BODY_SIZE);
        assertTrue(DesignTokens.TYPE_BODY_SIZE > DesignTokens.TYPE_LABEL_SIZE);
        assertTrue(DesignTokens.TYPE_LABEL_SIZE > DesignTokens.TYPE_CAPTION_SIZE);
    }

    @Test public void lineHeightsAccommodateTheirTypeSize() {
        float[] sizes = {DesignTokens.TYPE_DISPLAY_SIZE, DesignTokens.TYPE_TITLE_SIZE,
                DesignTokens.TYPE_BODY_SIZE, DesignTokens.TYPE_LABEL_SIZE,
                DesignTokens.TYPE_CAPTION_SIZE};
        float[] lines = {DesignTokens.TYPE_DISPLAY_LINE, DesignTokens.TYPE_TITLE_LINE,
                DesignTokens.TYPE_BODY_LINE, DesignTokens.TYPE_LABEL_LINE,
                DesignTokens.TYPE_CAPTION_LINE};
        for (int i = 0; i < sizes.length; i++) {
            assertTrue("line height " + lines[i] + " is under its type size " + sizes[i],
                    lines[i] > sizes[i]);
        }
    }

    /** The palette must never make the app read as white, grey or pure black chrome. */
    @Test public void baseSurfaceIsDarkAndNotPureBlack() {
        int surface = DesignTokens.Palette.SURFACE_BASE;
        assertTrue("surface must be dark", luminance(surface) < 0.15d);
        assertTrue("surface must not be pure black",
                (surface & 0x00FFFFFF) != 0x000000);
    }

    @Test public void primaryTextIsLightAndNotPureWhite() {
        int onSurface = DesignTokens.Palette.ON_SURFACE;
        assertTrue("primary text must be light", luminance(onSurface) > 0.6d);
        assertTrue("primary text must not be pure white",
                (onSurface & 0x00FFFFFF) != 0x00FFFFFF);
    }

    @Test public void primaryTextStandsOutFromItsSurface() {
        double surface = luminance(DesignTokens.Palette.SURFACE_BASE);
        double onSurface = luminance(DesignTokens.Palette.ON_SURFACE);
        assertTrue("contrast ratio is " + contrast(surface, onSurface),
                contrast(surface, onSurface) >= 4.5d);
    }

    @Test public void mutedTextStaysLegibleOnTheBaseSurface() {
        double ratio = contrast(luminance(DesignTokens.Palette.SURFACE_BASE),
                luminance(DesignTokens.Palette.ON_SURFACE_MUTED));
        assertTrue("muted contrast ratio is " + ratio, ratio >= 3.0d);
    }

    @Test public void accentStandsOutFromTheBaseSurface() {
        double ratio = contrast(luminance(DesignTokens.Palette.SURFACE_BASE),
                luminance(DesignTokens.Palette.ACCENT));
        assertTrue("accent contrast ratio is " + ratio, ratio >= 3.0d);
    }

    /** Content on the accent must be readable, so the accent cannot be pale. */
    @Test public void accentIsDarkEnoughToCarryLightContent() {
        assertTrue(contrast(luminance(DesignTokens.Palette.ACCENT),
                luminance(DesignTokens.Palette.ON_ACCENT)) >= 4.5d);
    }

    @Test public void disabledColourIsFadedRatherThanHidden() {
        int disabled = DesignTokens.controlColor(false, false, false,
                DesignTokens.Palette.ACCENT);
        int enabled = DesignTokens.controlColor(true, false, false,
                DesignTokens.Palette.ACCENT);
        assertTrue("a disabled control must still be visible",
                luminance(disabled) > 0d && luminance(disabled) < luminance(enabled));
    }

    @Test public void selectedControlUsesTheAccent() {
        assertEquals(DesignTokens.Palette.ACCENT,
                DesignTokens.controlColor(true, true, false, DesignTokens.Palette.ACCENT));
        assertEquals(DesignTokens.Palette.ACCENT,
                DesignTokens.controlColor(true, false, true, DesignTokens.Palette.ACCENT));
    }

    @Test public void controlColorHonoursAnInjectedAccent() {
        int injected = 0xFFFF8800;
        assertEquals(injected,
                DesignTokens.controlColor(true, true, false, injected));
    }

    @Test public void alphaScalingKeepsTheColourAndClamps() {
        int faded = DesignTokens.scaleAlpha(0xFF112233, 0.5f);
        assertEquals(0x80, (faded >>> 24) & 0xFF);
        assertEquals(0x112233, faded & 0x00FFFFFF);
        assertEquals(0xFF, (DesignTokens.scaleAlpha(0xFF112233, 9f) >>> 24) & 0xFF);
        assertEquals(0x00, (DesignTokens.scaleAlpha(0xFF112233, -1f) >>> 24) & 0xFF);
    }

    @Test public void aFullyTransparentDisabledGlyphIsInvisible() {
        int disabled = DesignTokens.controlColor(false, false, false, 0xFF3DD6C0);
        assertTrue("a disabled control must not be fully transparent",
                ((disabled >>> 24) & 0xFF) > 0);
    }

    @Test public void mixingInterpolatesAndClamps() {
        assertEquals(0x00000000, DesignTokens.mix(0x00000000, 0x00000000, 5f));
        int middle = DesignTokens.mix(0xFF000000, 0xFFFFFFFF, 0.5f);
        assertEquals(0xFF, (middle >>> 24) & 0xFF);
        assertEquals(0xFF808080, middle);
        assertEquals(0xFF000000, DesignTokens.mix(0xFF000000, 0xFFFFFFFF, -2f));
        assertEquals(0xFFFFFFFF, DesignTokens.mix(0xFF000000, 0xFFFFFFFF, 7f));
    }

    // ------------------------------------------------------------- colour math

    private static double luminance(int color) {
        double r = channel((color >> 16) & 0xFF);
        double g = channel((color >> 8) & 0xFF);
        double b = channel(color & 0xFF);
        return 0.2126d * r + 0.7152d * g + 0.0722d * b;
    }

    private static double channel(int value) {
        double srgb = value / 255.0d;
        return srgb <= 0.03928d ? srgb / 12.92d
                : Math.pow((srgb + 0.055d) / 1.055d, 2.4d);
    }

    private static double contrast(double from, double to) {
        double lighter = Math.max(from, to);
        double darker = Math.min(from, to);
        return (lighter + 0.05d) / (darker + 0.05d);
    }
}