package com.tunefold.app;

import java.util.concurrent.CopyOnWriteArrayList;

/**
 * The accent colour, and who has to repaint when it changes.
 *
 * <p>Exists so the accent is a <i>value</i> rather than a constant baked into every
 * control. Today it is the fixed brand mint from {@link DesignTokens.Palette#ACCENT};
 * the intent of the identity (ARTWORK &rarr; COLOUR &rarr; WAVEFORM &rarr; PLAYBACK) is
 * that it will instead be derived from the current cover, so this class is the one
 * place that has to learn how to do that. Nothing else in the UI hard-codes an
 * accent, so a derived colour lands everywhere at once and cannot miss a control.
 *
 * <h3>Cost model</h3>
 * Registration happens once, at control construction. Repaints are explicit and
 * targeted: a listener is told the new accent and invalidates itself, which is a
 * handful of views once per track change, not a rebind of the whole tree. The
 * listener list is copy-on-write because the interesting thread (artwork decoding)
 * is not the thread that registers.
 */
final class ArtworkTheme {

    /** A view that paints with the accent and must repaint when it moves. */
    interface AccentListener {
        /** The accent changed to {@code accent}; repaint now, do not rebuild. */
        void onAccentChanged(int accent);
    }

    private static final CopyOnWriteArrayList<AccentListener> LISTENERS = new CopyOnWriteArrayList<>();
    private static volatile int accent = DesignTokens.Palette.ACCENT;

    private ArtworkTheme() { }

    /** The accent every accent-aware control paints with. */
    static int accent() { return accent; }

    /** The colour content drawn on top of {@link #accent()}. */
    static int onAccent() { return DesignTokens.Palette.ON_ACCENT; }

    /**
     * Registers {@code listener} and immediately syncs it to the current accent.
     *
     * <p>Called from control constructors, so a control can never start out tinted
     * from a different accent than the rest of the screen.
     */
    static void register(AccentListener listener) {
        if (listener == null) return;
        LISTENERS.addIfAbsent(listener);
        listener.onAccentChanged(accent);
    }

    /** Unregisters {@code listener}; always paired with {@link #register}. */
    static void unregister(AccentListener listener) {
        if (listener == null) return;
        LISTENERS.remove(listener);
    }

    /** How many listeners are attached; for the leak guard in the component tests. */
    static int listenerCount() { return LISTENERS.size(); }

    /**
     * Moves the accent and notifies every listener.
     *
     * <p>A no-op when the accent is unchanged, so a derived colour that happens to
     * match the default costs nothing.
     */
    static void setAccent(int value) {
        int previous = accent;
        if (previous == value) return;
        accent = value;
        for (AccentListener listener : LISTENERS) {
            listener.onAccentChanged(value);
        }
    }

    /** Restores the brand accent; the seam's reset used by recreation and tests. */
    static void reset() { setAccent(DesignTokens.Palette.ACCENT); }
}