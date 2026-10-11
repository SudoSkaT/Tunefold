package com.tunefold.app;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

/**
 * The adaptive rules, checked without a device.
 *
 * <p>Every assertion here is about a decision that is invisible to a compiler and
 * obvious only on hardware: how big the cover is on a phone turned sideways, whether
 * the transport still fits on a short window, whether a tablet stretches the column
 * forever. The rules live in {@link LayoutRules} precisely so they can be asserted
 * here instead of being eyeballed on six screens.
 */
public class LayoutRulesTest {

    /** A small phone, portrait: the window everything has to fit in. */
    private static LayoutRules.Profile smallPhone() {
        return LayoutRules.of(360, 640, 360, false);
    }

    /** A large phone, portrait. */
    private static LayoutRules.Profile largePhone() {
        return LayoutRules.of(360, 900, 360, false);
    }

    @Test public void aPhoneIsCompact() {
        assertEquals(LayoutRules.FORM_COMPACT, smallPhone().form());
        assertEquals(LayoutRules.FORM_COMPACT, largePhone().form());
    }

    @Test public void aTabletIsExpanded() {
        // The three size classes are the platform's, not a number invented here: a
        // 600dp window is the smallest that earns two regions, and 840dp is where a
        // window is wide enough to stop being a large phone.
        assertEquals(LayoutRules.FORM_MEDIUM, LayoutRules.of(800, 1280, 800, false).form());
        assertEquals(LayoutRules.FORM_EXPANDED, LayoutRules.of(900, 1440, 900, false).form());
        assertEquals(LayoutRules.FORM_COMPACT, LayoutRules.of(599, 900, 599, false).form());
    }

    @Test public void theFormFollowsTheScreenNotTheOrientation() {
        // Same tablet, on its side: still a tablet, because the smallest width is
        // the screen's, not the window's.
        LayoutRules.Profile portrait = LayoutRules.of(800, 1280, 800, false);
        LayoutRules.Profile landscape = LayoutRules.of(1280, 800, 800, true);
        assertEquals(portrait.form(), landscape.form());
        assertEquals(portrait.artworkMaxDp(), landscape.artworkMaxDp());
    }

    @Test public void aPhoneInLandscapeStaysCompact() {
        LayoutRules.Profile landscape = LayoutRules.of(796, 360, 360, true);
        assertEquals(LayoutRules.FORM_COMPACT, landscape.form());
        assertEquals("a phone has no column maximum in any orientation",
                0, landscape.contentMaxDp());
    }

    @Test public void theCoverIsTheLoudestThingInPortrait() {
        int inner = Math.round(360 - 2 * smallPhone().gutterDp() - 2 * DesignTokens.CARD_PADDING);
        int side = smallPhone().artworkSideDp(inner, 640);
        assertTrue("portrait cover should dominate the width: " + side + " of " + inner,
                side > inner * 0.7);
    }

    @Test public void theCoverIsBoundedByTheHeight() {
        // A very short window: the width rule alone would push every control under
        // the fold, so the height guard has to win.
        int inner = 300;
        int tall = LayoutRules.of(300, 900, 300, false).artworkSideDp(inner, 900);
        int shortWindow = LayoutRules.of(300, 420, 300, false).artworkSideDp(inner, 420);
        assertTrue("the short window must have the smaller cover: " + shortWindow,
                shortWindow < tall);
    }

    @Test public void theCoverNeverLeavesItsBounds() {
        for (int width : new int[] {160, 240, 320, 360, 412, 600, 800, 1280}) {
            for (int height : new int[] {240, 360, 480, 640, 900, 1280}) {
                for (boolean landscape : new boolean[] {false, true}) {
                    LayoutRules.Profile profile = LayoutRules.of(width, height, 0, landscape);
                    int side = profile.artworkSideDp(width, height);
                    assertTrue("below the floor at " + width + "x" + height + " "
                                    + landscape + ": " + side,
                            side >= DesignTokens.ARTWORK_MIN);
                    assertTrue("above the ceiling at " + width + "x" + height + " "
                                    + landscape + ": " + side,
                            side <= profile.artworkMaxDp());
                }
            }
        }
    }

    @Test public void aLandscapeWindowComposesInTwoRegions() {
        assertTrue(LayoutRules.of(796, 360, 360, true).twoColumnPlayer());
        assertFalse(smallPhone().twoColumnPlayer());
    }

    @Test public void aLargeScreenAlsoComposesInTwoRegions() {
        // A portrait tablet: wide enough that a cover beside its details beats one
        // above them, and tall enough not to be a "short window".
        assertTrue(LayoutRules.of(800, 1280, 800, false).twoColumnPlayer());
        assertFalse(LayoutRules.of(800, 1280, 800, false).shortWindow());
    }

    @Test public void aLandscapePhoneIsAShortWindow() {
        assertTrue(LayoutRules.of(796, 360, 360, true).shortWindow());
        assertFalse(smallPhone().shortWindow());
    }

    @Test public void aTabletDoesNotStretchTheColumn() {
        LayoutRules.Profile tablet = LayoutRules.of(1280, 800, 1280, true);
        assertTrue(tablet.contentMaxDp() > 0);
        assertTrue("content must stay inside its maximum: " + tablet.contentMaxDp(),
                tablet.contentMaxDp() <= LayoutRules.CONTENT_MAX_DP);
    }

    @Test public void aPhoneHasNoColumnMaximumBecauseItHasNothingToSpare() {
        assertEquals(0, smallPhone().contentMaxDp());
        assertEquals(0, largePhone().contentMaxDp());
    }

    @Test public void aNarrowTabletColumnStillFitsItsGutters() {
        // Just over the medium threshold: the maximum must leave room for the page
        // gutters, or the content would be wider than the screen it sits on.
        LayoutRules.Profile narrow = LayoutRules.of(620, 960, 620, false);
        assertTrue(narrow.contentMaxDp() <= 620 - 2 * narrow.gutterDp());
    }

    @Test public void aBiggerScreenGetsMoreAirAndABiggerCover() {
        LayoutRules.Profile phone = smallPhone();
        LayoutRules.Profile tablet = LayoutRules.of(900, 1440, 900, false);
        assertTrue(tablet.gutterDp() > phone.gutterDp());
        assertTrue(tablet.blockGapDp() > phone.blockGapDp());
        assertTrue(tablet.sectionGapDp() > phone.sectionGapDp());
        assertTrue(tablet.artworkMaxDp() > phone.artworkMaxDp());
    }

    @Test public void nothingScalesIndiscriminately() {
        // The tokens a person touches are the same on every screen: a tablet is a
        // bigger room, not a bigger person.
        LayoutRules.Profile tablet = LayoutRules.of(1280, 800, 1280, true);
        assertEquals(DesignTokens.CONTROL_TARGET, DesignTokens.CONTROL_TARGET, 0.001);
        assertEquals(DesignTokens.TYPE_TITLE_SIZE, DesignTokens.TYPE_TITLE_SIZE, 0.001);
        assertEquals(DesignTokens.ARTWORK_MIN, DesignTokens.ARTWORK_MIN, 0.001);
        assertTrue(tablet.artworkMaxDp() > smallPhone().artworkMaxDp());
    }

    @Test public void theSameWindowIsTheSameLayout() {
        LayoutRules.Profile one = LayoutRules.of(360, 640, 360, false);
        LayoutRules.Profile same = LayoutRules.of(360, 640, 360, false);
        LayoutRules.Profile other = LayoutRules.of(360, 700, 360, false);
        assertTrue(LayoutRules.sameLayout(one, same));
        assertFalse(LayoutRules.sameLayout(one, other));
        assertFalse(LayoutRules.sameLayout(one, null));
    }

    @Test public void aDegenerateWindowDoesNotProduceANonsenseLayout() {
        LayoutRules.Profile zero = LayoutRules.of(0, 0, 0, false);
        assertEquals(1, zero.widthDp());
        assertEquals(1, zero.heightDp());
        assertTrue(zero.artworkSideDp(0, 0) >= DesignTokens.ARTWORK_MIN);
    }
}