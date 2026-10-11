package com.tunefold.app;

import android.content.Context;
import android.view.View;
import android.widget.LinearLayout;

/**
 * The surface behind one bottom-navigation destination.
 *
 * <p>Four of them exist (Home, Search, L1K3D, Downloads) and they are <b>views</b>,
 * not destinations in a back stack. That is the whole reason navigating costs no
 * Activity recreation and no state loss: a section switch is a {@code visibility}
 * change on a view that was built once and keeps everything it had.
 *
 * <p>A surface that was never opened builds nothing and shows nothing, and one that
 * was opened and left keeps its rows in the pool: switching Home → L1K3D → Home is
 * the same three views and the same covers, not three rebuilds.
 */
abstract class SectionSurface extends LinearLayout {

    /** What a surface needs from the screen around it. */
    interface Host extends TrackList.StateSource, TrackRow.OverflowHost {
        /** The screen's shared row pool. */
        TrackList.RowPool rowPool();

        /** The scroller the surface's lists window against. */
        ScrollHost scrollHost();
    }

    final Host host;

    SectionSurface(Context context, Host host) {
        super(context);
        DesignTokens.init(context);
        this.host = host;
        setOrientation(VERTICAL);
        setVisibility(View.GONE);
    }

    /** Called once, the first time this surface is shown. */
    abstract void onEnter();

    /**
     * Called every time this surface becomes the visible one.
     *
     * <p>Arrival is the one moment where re-deriving data is right: the session may
     * have learned things while this surface was on the other side of the screen.
     */
    void onShow() { }

    /**
     * Called when the session changed for a reason this screen did not cause.
     *
     * <p>Deliberately not {@link #onShow()}: the session notifies twice a second for
     * every reason at once, and a surface that rebuilt itself on each notification
     * was rebuilding the same rows to show the same picture while the user read it.
     */
    void onSessionChanged() { }

    /** Re-applies shared state (active track, likes, downloads) without rebuilding. */
    void refresh() { }

    /** Declares which track is playing, so the rows that show it can say so. */
    abstract void setActiveKey(TrackKey key);

    /** The artwork of the row this surface considers active, or {@code null}. */
    MediaTrack activeTrack() { return null; }
}