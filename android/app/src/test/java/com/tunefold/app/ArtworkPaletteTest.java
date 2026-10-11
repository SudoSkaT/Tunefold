package com.tunefold.app;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNotEquals;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

/**
 * What the cover has to promise before any colour reaches the screen.
 *
 * <p>The regression this file exists for is not hypothetical: the first extraction
 * blended the derived colour 55% towards the brand mint, and on a real device three
 * different covers came out as three almost identical dusty roses. Everything the
 * code did was correct; the output was the problem. So these tests assert the
 * <em>output</em>, not the algorithm: covers that differ on screen must differ here.
 */
public class ArtworkPaletteTest {

    private static final int OPAQUE = 0xFF000000;

    /** A cover painted with one colour, as the decoder would deliver it. */
    private static int[] cover(int color, int pixels) {
        int[] out = new int[pixels];
        java.util.Arrays.fill(out, color);
        return out;
    }

    /** Half the pixels in one colour and half in another. */
    private static int[] twoTone(int left, int right) {
        int[] out = new int[128];
        java.util.Arrays.fill(out, 0, 64, left);
        java.util.Arrays.fill(out, 64, 128, right);
        return out;
    }

    private static float saturation(int color) {
        float[] hsv = new float[3];
        ArtworkPalette.toHsv(color, hsv);
        return hsv[1];
    }

    private static float hue(int color) {
        float[] hsv = new float[3];
        ArtworkPalette.toHsv(color, hsv);
        return hsv[0];
    }

    private static float hueGap(int left, int right) {
        float difference = Math.abs(hue(left) - hue(right));
        return difference > 180f ? 360f - difference : difference;
    }

    /** A cover with no colour at all must leave the brand alone, not invent one. */
    @Test public void aGreyCoverLeavesTheBrandInPlace() {
        ArtworkPalette.Palette palette = ArtworkPalette.derive(cover(0xFF9A9A9A, 64));
        assertFalse("a grey cover has nothing to say about colour", palette.derived);
        assertEquals(DesignTokens.Palette.ACCENT, palette.primary);
        assertEquals(DesignTokens.Palette.ACCENT, palette.secondary);
    }

    @Test public void aBlackCoverLeavesTheBrandInPlace() {
        ArtworkPalette.Palette palette = ArtworkPalette.derive(cover(0xFF000000, 64));
        assertFalse(palette.derived);
        assertEquals(DesignTokens.Palette.ACCENT, palette.primary);
    }

    @Test public void aWhiteCoverLeavesTheBrandInPlace() {
        // White is a highlight, not a colour: treating it as one produced pale bars
        // on every bright cover.
        ArtworkPalette.Palette palette = ArtworkPalette.derive(cover(0xFFFFFFFF, 64));
        assertFalse(palette.derived);
        assertEquals(DesignTokens.Palette.ACCENT, palette.primary);
    }

    @Test public void noCoverAtAllLeavesTheBrandInPlace() {
        assertFalse(ArtworkPalette.derive(null).derived);
        assertFalse(ArtworkPalette.derive(new int[0]).derived);
    }

    /** Transparent pixels are not colour, however saturated the RGB channels look. */
    @Test public void transparentPixelsNeverVote() {
        ArtworkPalette.Palette palette = ArtworkPalette.derive(
                new int[] {0x00FF0000, 0x00FF0000, 0x00FF0000});
        assertFalse(palette.derived);
    }

    /** The whole point: a cover's colour comes out alive. */
    @Test public void aSaturatedCoverKeepsItsColourAlive() {
        ArtworkPalette.Palette palette = ArtworkPalette.derive(cover(0xFF1E6FD9, 64));
        assertTrue(palette.derived);
        assertTrue("the bar colour must not come back dusty: " + palette.primary,
                saturation(palette.primary) >= 0.45f);
        assertEquals("a blue cover has to give blue bars, not a brand-tinted something",
                210f, hue(palette.primary), 25f);
    }

    /**
     * The failure that shipped: three covers, three dusty roses.
     *
     * <p>Blending towards the brand is what made them identical, so the guarantee is
     * that covers far apart on screen stay far apart here.
     */
    @Test public void differentCoversStayVisiblyDifferent() {
        int blue = ArtworkPalette.derive(cover(0xFF1E6FD9, 64)).primary;
        int red = ArtworkPalette.derive(cover(0xFFD93025, 64)).primary;
        int amber = ArtworkPalette.derive(cover(0xFFE8A317, 64)).primary;
        assertTrue("blue and red must not converge", hueGap(blue, red) > 90f);
        assertTrue("blue and amber must not converge", hueGap(blue, amber) > 60f);
        assertTrue("red and amber must not converge", hueGap(red, amber) > 30f);
        assertNotEquals(blue, red);
        assertNotEquals(blue, amber);
    }

    /** A single-tone cover paints both lanes the same, and says so. */
    @Test public void oneTonePaintsBothLanesTheSame() {
        ArtworkPalette.Palette palette = ArtworkPalette.derive(cover(0xFF7A3FD1, 64));
        assertEquals(palette.primary, palette.secondary);
    }

    /** A cover with two real tones gives each lane its own, or says it could not. */
    @Test public void twoTonesGiveTwoLanes() {
        ArtworkPalette.Palette palette = ArtworkPalette.derive(twoTone(0xFF1E6FD9, 0xFFE8A317));
        assertTrue(palette.derived);
        // Equal pixel counts: the winner is the more saturated of the two, and the
        // other must still be a different colour rather than a copy.
        assertTrue("two tones, two lanes: " + Integer.toHexString(palette.primary) + " / "
                + Integer.toHexString(palette.secondary),
                hueGap(palette.primary, palette.secondary) > 25f);
    }

    /** Near-identical tones are one tone: pretending otherwise reads as a bug. */
    @Test public void nearIdenticalTonesCollapseToOne() {
        ArtworkPalette.Palette palette = ArtworkPalette.derive(twoTone(0xFF1E6FD9, 0xFF2A63C8));
        assertEquals(palette.primary, palette.secondary);
    }

    /** Bars live on the card, so the card is the contrast they have to survive. */
    @Test public void barColoursStayReadableOnTheCard() {
        int[] covers = {0xFF1E6FD9, 0xFFD93025, 0xFFE8A317, 0xFF7A3FD1, 0xFF00A882,
                0xFFFF6D00, 0xFF00838F, 0xFFB00020};
        for (int cover : covers) {
            ArtworkPalette.Palette palette = ArtworkPalette.derive(cover(cover, 64));
            double contrast = ArtworkPalette.contrast(palette.primary,
                    DesignTokens.Palette.SURFACE_CARD);
            assertTrue("bars on the card need 3:1: " + Integer.toHexString(cover)
                    + " gives " + contrast, contrast >= 3.0d);
            assertEquals("every derived colour is opaque", OPAQUE, palette.primary & 0xFF000000);
        }
    }

    /** The control colour carries dark content on top, so it is lifted until it can. */
    @Test public void aTooDarkColourIsLiftedUntilItCarriesItsContent() {
        int dark = ArtworkPalette.vivid(0xFF101010);
        int lifted = ArtworkPalette.lift(dark);
        assertTrue("lifting must actually lift",
                ArtworkPalette.luminance(lifted) >= ArtworkPalette.MIN_ACCENT_LUMINANCE);
        assertTrue("the content on the disc has to be readable: "
                + ArtworkPalette.contrast(lifted, DesignTokens.Palette.ON_ACCENT),
                ArtworkPalette.contrast(lifted, DesignTokens.Palette.ON_ACCENT) >= 4.5d);
    }

    /** A colour that already carries content is left exactly as it is. */
    @Test public void liftingLeavesAReadableColourAlone() {
        int bright = ArtworkPalette.vivid(0xFFE8A317);
        assertEquals(bright, ArtworkPalette.lift(bright));
    }

    /** Saturation is restored after averaging, which is what averaging takes away. */
    @Test public void averagingCannotReturnADustyColour() {
        // Two blues 40 levels apart average into something desaturated on their own.
        ArtworkPalette.Palette mixed = ArtworkPalette.derive(twoTone(0xFF1E6FD9, 0xFF5A9BE8));
        assertTrue("averaged blues must not come back grey: "
                + Integer.toHexString(mixed.primary), saturation(mixed.primary) >= 0.45f);
    }

    /** The conversion round-trips, or every guarantee above is measuring noise. */
    @Test public void theHsvConversionRoundTrips() {
        int[] samples = {0xFF1E6FD9, 0xFFD93025, 0xFFE8A317, 0xFF00A882, 0xFF7A3FD1};
        float[] hsv = new float[3];
        float[] rgb = new float[3];
        for (int sample : samples) {
            ArtworkPalette.toHsv(sample, hsv);
            ArtworkPalette.fromHsv(hsv, rgb);
            assertEquals((sample >> 16) & 0xFF, Math.round(rgb[0]), 2);
            assertEquals((sample >> 8) & 0xFF, Math.round(rgb[1]), 2);
            assertEquals(sample & 0xFF, Math.round(rgb[2]), 2);
        }
    }
}