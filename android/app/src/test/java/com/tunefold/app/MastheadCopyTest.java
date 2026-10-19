package com.tunefold.app;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

/**
 * The masthead's words: what Home says about itself and about the library.
 *
 * <p>A caption on the first screen is a claim about the app, and a claim that can
 * be wrong is worse than no caption. So the two functions that compose it are pure
 * and asserted here: they can only say what the session passed in, and the empty
 * case has to say something the user can act on rather than nothing at all.
 */
public class MastheadCopyTest {

    // -------------------------------------------------------------- greeting

    /** Three words a day, and each one belongs to its part of the day. */
    @Test public void theGreetingFollowsTheHour() {
        assertEquals("Buenas noches", MastheadView.greetingForHour(2));
        assertEquals("Buenos días", MastheadView.greetingForHour(7));
        assertEquals("Buenos días", MastheadView.greetingForHour(12));
        assertEquals("Buenas tardes", MastheadView.greetingForHour(13));
        assertEquals("Buenas tardes", MastheadView.greetingForHour(20));
        assertEquals("Buenas noches", MastheadView.greetingForHour(23));
    }

    /**
 * The greeting only moves at the hours it claims to.
 *
 * <p>It is written on every render and changes three times a day, so the value of
 * this test is that an hour either side of a boundary lands on the right side of
 * it — and that no hour in the day produces a word the view has never shown.
 */
@Test public void theGreetingMovesOnlyAtItsBoundaries() {
        int[][] bands = {{0, 5}, {6, 12}, {13, 20}, {21, 23}};
        for (int[] band : bands) {
            String word = MastheadView.greetingForHour(band[0]);
            for (int hour = band[0]; hour <= band[1]; hour++) {
                assertEquals("hour " + hour + " left its band",
                        word, MastheadView.greetingForHour(hour));
            }
        }
        // Six and twenty-one are the first hours of their bands, and they must
        // differ from the hour before them: a boundary that does not change
        // anything is a boundary that was never there.
        assertFalse(MastheadView.greetingForHour(6).equals(MastheadView.greetingForHour(5)));
        assertFalse(MastheadView.greetingForHour(13).equals(MastheadView.greetingForHour(12)));
        assertFalse(MastheadView.greetingForHour(21).equals(MastheadView.greetingForHour(20)));
    }

    /** An impossible hour gets a word, not a crash: the calendar is not trusted. */
    @Test public void anImpossibleHourStillProducesAWord() {
        assertEquals("Hola", MastheadView.greetingForHour(-1));
        assertEquals("Hola", MastheadView.greetingForHour(24));
        for (int hour = 0; hour < 24; hour++) {
            assertTrue(MastheadView.greetingForHour(hour).length() > 0);
        }
    }

    // --------------------------------------------------------- library line

    /** The counts the user can see are the counts the session knows. */
    @Test public void theLineCountsWhatTheSessionHas() {
        String line = MastheadView.libraryLine(12, 3, false);
        assertTrue(line, line.contains("12"));
        assertTrue(line, line.contains("L1K3D"));
        assertTrue(line, line.contains("3"));
        assertTrue(line, line.contains("descarga"));
    }

    /** Something playing is said plainly, without pretending nothing is loaded. */
    @Test public void playingIsStatedWhenSomethingIsPlaying() {
        String line = MastheadView.libraryLine(4, 1, true);
        assertTrue(line, line.toLowerCase(java.util.Locale.ROOT).contains("sonando"));
        assertFalse("an empty library is not hidden behind 'sonando'",
                line.contains("nada guardado"));
    }

    /**
     * An empty library gets an instruction, not a silence.
     *
     * <p>An empty home screen with no sentence on it reads as a broken screen; this
     * is the one place where the app is allowed to ask the user for something,
     * because there is genuinely nothing to show them.
     */
    @Test public void anEmptyLibrarySaysWhatToDo() {
        String idle = MastheadView.libraryLine(0, 0, false);
        assertTrue(idle, idle.length() > 0);
        assertFalse("an empty library has counts to report", idle.contains("L1K3D"));
        String playing = MastheadView.libraryLine(0, 0, true);
        assertFalse("and it is not a dead end while something plays",
                playing.contains("nada guardado"));
    }

    /** One saved track and none on disk is one sentence, not two empty ones. */
    @Test public void aSingleTrackDoesNotProduceEmptyPhrases() {
        String line = MastheadView.libraryLine(1, 0, false);
        assertEquals("1 en L1K3D", line);
        String downloads = MastheadView.libraryLine(0, 1, false);
        assertEquals("1 descarga", downloads);
        assertFalse("no dangling separator", downloads.contains("·"));
    }

    /** Negative counts are nonsense and must not reach the screen as "-3". */
    @Test public void impossibleCountsDoNotReachTheCaption() {
        assertEquals(MastheadView.libraryLine(0, 0, false),
                MastheadView.libraryLine(-4, -9, false));
        assertFalse(MastheadView.libraryLine(-4, -9, false).contains("-"));
    }

    /** The same facts produce the same sentence, so the view can skip the write. */
    @Test public void theSameFactsProduceTheSameSentence() {
        assertEquals(MastheadView.libraryLine(7, 2, true),
                MastheadView.libraryLine(7, 2, true));
    }
}