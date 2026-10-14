package com.tunefold.app;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

/**
 * The two rules a scroll is made of: how far the window may move, and where the drag
 * begins once the gesture is recognised as a scroll.
 *
 * <p>Both are pure functions on purpose. The touch handling around them is the part
 * only a device can check, which is exactly why the arithmetic underneath it should
 * not be left to a device.
 *
 * <p><b>What a device found, and what it cannot be tested for.</b> These tests passed
 * while the content was being drawn the wrong way up: the offset was handed to
 * {@code View.scrollTo} negated, and the framework draws a child at
 * {@code childTop - mScrollY}, so a finger travelling up moved the list down by the
 * same distance — measured on the panel as a 400&nbsp;px drag displacing the drawn
 * content 400&nbsp;px in the wrong direction, and a saturated offset putting the whole
 * screen 1408&nbsp;px below its place. Pure arithmetic over the same numbers looks
 * identical either way, so the sign is asserted here against the rule it obeys rather
 * than against a number, and the measured pixels are recorded in
 * {@code ScrollHost#scrollTo}.
 */
public class ScrollHostTest {

    private static final int SLOP = 16;   // 8dp at density 2.0
    private static final int DOWN_Y = 500;

    /**
     * How far the window moves for one touch at {@code fingerY}, given a drag that
     * began at {@link #DOWN_Y}.
     *
     * <p>The gesture model, written out: the slop has to be crossed before anything
     * moves at all, and once it is, the window advances by what the finger travelled
     * <em>beyond</em> the slop. {@link ScrollHost#anchorAfterSlop} is what makes that
     * true, so this is the only place that has to know how a drag is applied.
     */
    private static int scrolledBy(int fingerY, int slop) {
        int delta = fingerY - DOWN_Y;
        if (Math.abs(delta) <= slop) return 0;
        return ScrollHost.anchorAfterSlop(DOWN_Y, delta, slop) - fingerY;
    }

    // ---------------------------------------------------------------- the clamp

    /** The window never moves above the top of the content. */
    @Test public void theOffsetCannotGoNegative() {
        assertEquals(0, ScrollHost.clampScroll(-1, 1000));
        assertEquals(0, ScrollHost.clampScroll(-100000, 1000));
        assertEquals("no overscroll: pulling past the top does nothing",
                0, ScrollHost.clampScroll(0, 1000));
    }

    /** Nor past the end: the offset is how much is off-screen below, not more. */
    @Test public void theOffsetCannotPassTheEnd() {
        assertEquals(1000, ScrollHost.clampScroll(1000, 1000));
        assertEquals(1000, ScrollHost.clampScroll(99999, 1000));
    }

    /** Anything inside the range is untouched, so a drag maps one to one. */
    @Test public void anOffsetInsideTheRangeIsKept() {
        for (int value : new int[] {0, 1, 137, 999, 1000}) {
            assertEquals(value, ScrollHost.clampScroll(value, 1000));
        }
    }

    /** Content shorter than the viewport has no range at all, not a negative one. */
    @Test public void contentThatFitsHasNoRange() {
        assertEquals(0, ScrollHost.clampScroll(500, 0));
        assertTrue("a clamp must never produce a negative offset",
                ScrollHost.clampScroll(500, -50) >= 0);
    }

    // ------------------------------------------------------------ the drag rule

    /**
     * A drag is the finger's own movement, once the slop has been spent.
     *
     * <p>Twenty pixels past the slop moves the content twenty pixels, in the direction
     * the finger went. This is the whole contract a scroller has with the person
     * using it, and it is what "the scroll is inverted" is a violation of.
     */
    @Test public void theContentFollowsTheFingerByExactlyWhatItTravelled() {
        assertEquals("up", 20, scrolledBy(DOWN_Y - (SLOP + 20), SLOP));
        assertEquals("down", -20, scrolledBy(DOWN_Y + (SLOP + 20), SLOP));
        assertEquals("further up, further", 400, scrolledBy(DOWN_Y - (SLOP + 400), SLOP));
    }

    /** A tap that never passes the slop is a tap: it moves nothing at all. */
    @Test public void aGestureInsideTheSlopNeverScrolls() {
        for (int travel = -SLOP; travel <= SLOP; travel++) {
            assertEquals("a tap inside the slop must not scroll", 0,
                    scrolledBy(DOWN_Y + travel, SLOP));
        }
    }

    /**
     * The first movement of a scroll must not replay the slop.
     *
     * <p>The regression this guards: the anchor used to be the point the finger went
     * down, so the moment a drag was recognised the content jumped by everything the
     * finger had travelled to get there — the slop — in whichever direction. Every
     * gesture therefore began with a lurch, which is what a list that "pega saltos"
     * feels like.
     */
    @Test public void startingADragDoesNotJumpByTheSlop() {
        for (int travel : new int[] {SLOP + 1, SLOP + 9, SLOP + 120}) {
            int up = scrolledBy(DOWN_Y - travel, SLOP);
            assertEquals("the slop is subtracted, not replayed", travel - SLOP, up);
            // What the old anchor did with the very same gesture.
            assertEquals("the old anchor applied the whole travel", travel, DOWN_Y - (DOWN_Y - travel));
            assertEquals("so it over-shot by exactly the slop", SLOP, travel - up);

            int down = scrolledBy(DOWN_Y + travel, SLOP);
            assertEquals("the slop is subtracted, not replayed", -(travel - SLOP), down);
            assertEquals("the old anchor applied the whole travel", -travel, DOWN_Y - (DOWN_Y + travel));
            assertEquals("so it over-shot by exactly the slop", SLOP, travel + down);
        }
    }

    /** The anchor is a point, and it must be inside the slop of where the finger is. */
    @Test public void theAnchorIsWhereTheSlopEnds() {
        assertEquals(DOWN_Y - SLOP, ScrollHost.anchorAfterSlop(DOWN_Y, -(SLOP + 20), SLOP));
        assertEquals(DOWN_Y + SLOP, ScrollHost.anchorAfterSlop(DOWN_Y, SLOP + 20, SLOP));
        // Exactly at the boundary the sign is not yet decided, so it goes forward; the
        // caller only asks once the gesture is past it.
        assertEquals(DOWN_Y + SLOP, ScrollHost.anchorAfterSlop(DOWN_Y, 0, SLOP));
    }

    /** A zero slop degrades to the raw finger movement: the anchor is where it started. */
    @Test public void withoutASlopTheAnchorIsTheFinger() {
        assertEquals(400, ScrollHost.anchorAfterSlop(400, 50, 0));
        assertEquals(400, ScrollHost.anchorAfterSlop(400, -50, 0));
        assertEquals("and the drag is then the finger's own movement",
                -50, scrolledBy(DOWN_Y + 50, 0));
        assertEquals(50, scrolledBy(DOWN_Y - 50, 0));
    }

    /** Whatever the geometry, the window stays inside its range while it is dragged. */
    @Test public void noDragCanLeaveTheRange() {
        for (int travel = -3000; travel <= 3000; travel += 37) {
            int offset = ScrollHost.clampScroll(0 + scrolledBy(DOWN_Y + travel, SLOP), 1000);
            assertTrue("offset " + offset, offset >= 0 && offset <= 1000);
        }
    }
}