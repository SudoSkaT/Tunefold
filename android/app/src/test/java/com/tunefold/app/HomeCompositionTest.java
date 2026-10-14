package com.tunefold.app;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;

/**
 * Home's composition and the player's column rule (§ Phase 6).
 *
 * <p>Both are decisions about what to show and what to leave out, which is the part of
 * a screen that is easiest to argue about and hardest to look at: a layout can be
 * correct on every measurement and still be the wrong screen.
 */
public class HomeCompositionTest {

    private static MediaTrack track(String id) { return TestTracks.of("YouTube", id); }

    private static List<MediaTrack> tracks(String... ids) {
        List<MediaTrack> list = new ArrayList<>();
        for (String id : ids) list.add(track(id));
        return list;
    }

    /** Identities in order, because a track has no identity beyond its key. */
    private static List<String> ids(List<MediaTrack> tracks) {
        List<String> out = new ArrayList<>();
        for (MediaTrack track : tracks) out.add(track.providerId);
        return out;
    }

    // ------------------------------------------------------------ the preview

    /** A list that fits is shown whole: a preview of everything is not a preview. */
    @Test public void aShortListIsShownWhole() {
        assertEquals(Arrays.asList("a", "b", "c"),
                ids(HomeSurface.preview(tracks("a", "b", "c"), 6)));
    }

    /** What a preview keeps is the newest, which is the part not already heard. */
    @Test public void thePreviewKeepsTheMostRecent() {
        assertEquals(Arrays.asList("c", "d", "e"),
                ids(HomeSurface.preview(tracks("a", "b", "c", "d", "e"), 3)));
    }

    /** The preview must not mutate the list it was given. */
    @Test public void thePreviewIsACopy() {
        List<MediaTrack> all = tracks("a", "b", "c");
        HomeSurface.preview(all, 1);
        assertEquals("the caller's list is untouched", 3, all.size());
    }

    @Test public void degeneratePreviewsAreEmptyRatherThanBroken() {
        assertTrue(HomeSurface.preview(null, 6).isEmpty());
        assertTrue(HomeSurface.preview(tracks("a"), 0).isEmpty());
        assertTrue("a negative limit shows nothing rather than throwing",
                HomeSurface.preview(tracks("a"), -1).isEmpty());
    }

    // ------------------------------------------------ the player's composition

    /**
     * A wide window is allowed two regions; that is not the same as deserving them.
     *
     * <p>The cover has to have lent a colour, not two: requiring two distinct hues
     * meant the split appeared almost never, because most sleeves are one hue with a
     * neighbour inside the palette's "same tone" margin.
     */
    @Test public void twoRegionsNeedBothAWideWindowAndAColouredCover() {
        assertTrue(NowPlayingView.twoColumnFor(true, true));
        assertFalse("a grey cover has no contrast to split on",
                NowPlayingView.twoColumnFor(true, false));
        assertFalse("a narrow window never splits, whatever the cover says",
                NowPlayingView.twoColumnFor(false, true));
        assertFalse(NowPlayingView.twoColumnFor(false, false));
    }

    /** A cover that lends one hue still lends colour: the split is still earned. */
    @Test public void oneHueStillCountsAsAColouredCover() {
        int[] pixels = new int[16 * 16];
        for (int index = 0; index < pixels.length; index++) pixels[index] = 0xFFE03C28;
        ArtworkPalette.Palette palette = ArtworkPalette.derive(pixels);
        assertTrue(palette.derived);
        assertFalse("the two bars channels share its tint", palette.offersMoreThanOneColour());
        assertTrue("and the card still splits, because it is not grey",
                NowPlayingView.twoColumnFor(true, palette.derived));
    }

    // ------------------------------------------------------------ the palette

    /** The brand palette is not a cover offering two colours. */
    @Test public void theBrandPaletteOffersOneColour() {
        ArtworkPalette.Palette brand =
                ArtworkPalette.Palette.brand(DesignTokens.Palette.ACCENT);
        assertFalse(brand.offersMoreThanOneColour());
    }

    /** A cover with real hue variety is exactly what the split composition is for. */
    @Test public void aTwoHueCoverOffersMoreThanOneColour() {
        int[] pixels = new int[16 * 16];
        for (int index = 0; index < pixels.length; index++) {
            // Two halves: a red-ish and a blue-ish field, which is what a sleeve with
            // two colours looks like reduced to a swatch.
            pixels[index] = (index % 2 == 0) ? 0xFFE03C28 : 0xFF2858E0;
        }
        ArtworkPalette.Palette palette = ArtworkPalette.derive(pixels);
        assertTrue("derived at all", palette.derived);
        assertTrue("and two tones for it", palette.offersMoreThanOneColour());
    }

    /** One hue repeated is one colour, however saturated it is. */
    @Test public void aMonochromeCoverOffersOneColour() {
        int[] pixels = new int[16 * 16];
        for (int index = 0; index < pixels.length; index++) {
            pixels[index] = 0xFFE03C28;
        }
        ArtworkPalette.Palette palette = ArtworkPalette.derive(pixels);
        assertTrue("the cover still lends its colour", palette.derived);
        assertFalse("but it is one colour, and one colour is not a split",
                palette.offersMoreThanOneColour());
    }

    /** Nothing colourful at all: the brand answers, and it is not a cover. */
    @Test public void aCoverWithNoColourIsNotACover() {
        ArtworkPalette.Palette palette = ArtworkPalette.derive(new int[] {0xFF101010});
        assertFalse(palette.derived);
        assertFalse(palette.offersMoreThanOneColour());
        assertEquals(ArtworkPalette.Palette.brand(DesignTokens.Palette.ACCENT).primary,
                palette.primary);
    }

    // ----------------------------------------------------------- the live theme

    @Test public void theThemeRemembersWhatTheCoverOffered() {
        try {
            ArtworkTheme.setPalette(ArtworkPalette.Palette.brand(DesignTokens.Palette.ACCENT));
            assertFalse(ArtworkTheme.duoTone());

            int[] pixels = new int[16 * 16];
            for (int index = 0; index < pixels.length; index++) {
                pixels[index] = (index % 2 == 0) ? 0xFFE03C28 : 0xFF2858E0;
            }
            ArtworkTheme.setPalette(ArtworkPalette.derive(pixels));
            assertTrue("the composition follows the cover", ArtworkTheme.duoTone());

            ArtworkTheme.setPalette(ArtworkPalette.Palette.brand(DesignTokens.Palette.ACCENT));
            assertFalse("and follows it back", ArtworkTheme.duoTone());
            assertFalse("a grey cover keeps the card in one column",
                    ArtworkTheme.paletteDerived());
        } finally {
            ArtworkTheme.reset();
        }
    }
}