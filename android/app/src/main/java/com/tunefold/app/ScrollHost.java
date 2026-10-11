package com.tunefold.app;

import android.content.Context;
import android.graphics.Canvas;
import android.view.MotionEvent;
import android.view.VelocityTracker;
import android.view.View;
import android.view.ViewConfiguration;
import android.widget.FrameLayout;
import android.widget.OverScroller;

/**
 * A vertical scroller for one tall content view.
 *
 * <p>{@code ScrollView} is not usable here, and the reason is the whole reason this
 * class exists: {@code ScrollView} measures its child with an unbounded height, which
 * forces every row of every list to be laid out at once. A library of 500 tracks would
 * build 500 rows — 500 covers' worth of {@code View}s — to show the eight that fit.
 * This host measures the content once for its height and then moves it, so the
 * content decides for itself which rows exist.
 *
 * <p>It is deliberately small: a drag, a fling, a clamped range, and a callback for
 * "the window moved" that the lists use to bind and unbind rows. No overscroll glow,
 * no nested scrolling, no scrollbar: those are the framework's job and this is not a
 * replacement for {@code ScrollView}, it is the one thing {@code ScrollView} cannot do.
 *
 * <p>Every clamp decision lives in {@link #clampScroll}, which is a pure function and is
 * unit tested; the touch handling is the part only a device can check.
 */
@android.annotation.SuppressLint("ViewConstructor")
final class ScrollHost extends FrameLayout {

    /** Told when the visible window moved, so a list can re-bind what is on screen. */
    interface OnScrollListener {
        /**
         * @param scrollY    the new scroll offset, always within {@code [0, maxScroll]}
         * @param maxScroll  the largest legal offset, i.e. how much is off-screen below
         */
        void onScrolled(int scrollY, int maxScroll);
    }

    /** Height of the content child, learned from its own measurement. */
    private int contentHeight;
    private int scrollY;
    private int maxScroll;

    private final OverScroller scroller;
    private final int touchSlop;
    private final int minimumFlingVelocity;
    private final int maximumFlingVelocity;

    private VelocityTracker velocityTracker;
    private int lastTouchY;
    private int downTouchY;
    private boolean dragging;
    private boolean canScroll;

    private OnScrollListener listener;

    ScrollHost(Context context) {
        super(context);
        DesignTokens.init(context);
        scroller = new OverScroller(context);
        ViewConfiguration configuration = ViewConfiguration.get(context);
        touchSlop = configuration.getScaledTouchSlop();
        minimumFlingVelocity = configuration.getScaledMinimumFlingVelocity();
        maximumFlingVelocity = configuration.getScaledMaximumFlingVelocity();
        setWillNotDraw(true);
    }

    /** The window moved; the lists use it to decide which rows to bind. */
    void setOnScrollListener(OnScrollListener value) { listener = value; }

    /** Current scroll offset, in pixels. */
    int scrollY() { return scrollY; }

    /** Largest legal offset: how much content sits below the fold. */
    int maxScroll() { return maxScroll; }

    /** Whether the content is taller than the host, i.e. whether this can scroll. */
    boolean canScroll() { return canScroll; }

    private final java.util.ArrayList<TrackList> windows = new java.util.ArrayList<>();

    /** A list inside the content that has to know where the viewport is. */
    void registerWindow(TrackList list) {
        if (windows.contains(list)) return;
        windows.add(list);
        updateWindows();
    }

    /** A list that left the tree; its rows are nobody's business any more. */
    void unregisterWindow(TrackList list) { windows.remove(list); }

    @Override public void addView(View child) {
        // One content child, and it is never clipped away: the whole design is that
        // the content decides its own height and this decides where it is drawn.
        super.addView(child, new LayoutParams(LayoutParams.MATCH_PARENT,
                LayoutParams.WRAP_CONTENT));
    }

    /** Recomputes one list's window against the current viewport, right now. */
    void refreshWindow(TrackList list) {
        list.updateWindow(scrollY - list.contentOffset(this), getHeight());
    }

    /** Tells every list where the viewport now is; the lists decide what that means. */
    private void updateWindows() {
        int viewport = getHeight();
        for (int i = 0; i < windows.size(); i++) {
            TrackList list = windows.get(i);
            list.updateWindow(scrollY - list.contentOffset(this), viewport);
        }
    }

    @Override protected void onMeasure(int widthSpec, int heightSpec) {
        measureChildren(widthSpec, MeasureSpec.makeMeasureSpec(0, MeasureSpec.UNSPECIFIED));
        int width = resolveSize(getSuggestedMinimumWidth(), widthSpec);
        int height = resolveSize(getSuggestedMinimumHeight(), heightSpec);
        contentHeight = getChildCount() == 0 ? 0 : getChildAt(0).getMeasuredHeight();
        maxScroll = Math.max(0, contentHeight - height);
        canScroll = maxScroll > 0;
        // A fling that was in flight when the content shrank would land outside the
        // range; clamping here is what keeps that from showing a gap.
        if (scrollY > maxScroll) scrollTo(clampScroll(scrollY));
        setMeasuredDimension(width, height);
        updateWindows();
    }

    @Override protected void onLayout(boolean changed, int left, int top, int right, int bottom) {
        if (getChildCount() == 0) return;
        int childHeight = getChildAt(0).getMeasuredHeight();
        int childWidth = getChildAt(0).getMeasuredWidth();
        getChildAt(0).layout(0, -scrollY, childWidth, childHeight - scrollY);
        updateWindows();
    }

    @Override public void computeScroll() {
        if (!scroller.computeScrollOffset()) return;
        scrollTo(clampScroll(scroller.getCurrY()));
        postInvalidateOnAnimation();
    }

    @Override protected void onDraw(Canvas canvas) {
        // The content is a child; nothing is painted here.
    }

    /** Moves the window, clamped, and tells the listener. */
    void scrollTo(int value) {
        int next = clampScroll(value);
        if (next == scrollY) return;
        scrollY = next;
        scrollTo(0, -scrollY);
        updateWindows();
        if (listener != null) listener.onScrolled(scrollY, maxScroll);
    }

    /** Scrolls to the very top; used when a destination is opened. */
    void scrollToTop() { scrollTo(0); }

    /**
     * The clamp, as a pure function so the rule can be tested without a device.
     *
     * <p>An offset outside the range is not clamped to an edge and remembered: it is
     * clamped here, every time, because a scroll offset that can be wrong is a scroll
     * offset that will be.
     */
    static int clampScroll(int value, int maxScroll) {
        if (value < 0) return 0;
        return value > maxScroll ? Math.max(0, maxScroll) : value;
    }

    private int clampScroll(int value) { return clampScroll(value, maxScroll); }

    // ------------------------------------------------------------------ input

    /**
     * Intercepts a gesture only once it is a scroll.
     *
     * <p>Claiming the {@code DOWN} is what makes a whole screen unclickable: the
     * content holds buttons, and a host that takes every touch before the child sees
     * it turns them all into dead pixels. So the host watches first and decides at the
     * first move that passes the slop.
     */
    @Override public boolean onInterceptTouchEvent(MotionEvent event) {
        if (!canScroll) return false;
        switch (event.getActionMasked()) {
            case MotionEvent.ACTION_DOWN:
                scroller.abortAnimation();
                downTouchY = lastTouchY = (int) event.getY();
                dragging = false;
                return false;
            case MotionEvent.ACTION_MOVE:
                int delta = (int) event.getY() - downTouchY;
                if (!dragging && Math.abs(delta) > touchSlop) dragging = true;
                // The list only takes the gesture once it is a drag, so a tap on a
                // row is a tap and not the first pixel of a scroll.
                return dragging;
            case MotionEvent.ACTION_UP:
            case MotionEvent.ACTION_CANCEL:
                dragging = false;
                return false;
            default:
                return dragging;
        }
    }

    @Override public boolean onTouchEvent(MotionEvent event) {
        if (!canScroll) return false;
        switch (event.getActionMasked()) {
            case MotionEvent.ACTION_DOWN:
                scroller.abortAnimation();
                downTouchY = lastTouchY = (int) event.getY();
                if (velocityTracker == null) {
                    velocityTracker = VelocityTracker.obtain();
                } else {
                    velocityTracker.clear();
                }
                velocityTracker.addMovement(event);
                dragging = true;
                return true;
            case MotionEvent.ACTION_MOVE: {
                if (velocityTracker != null) velocityTracker.addMovement(event);
                int y = (int) event.getY();
                scrollTo(clampScroll(scrollY + lastTouchY - y));
                lastTouchY = y;
                return true;
            }
            case MotionEvent.ACTION_UP: {
                if (velocityTracker != null) {
                    velocityTracker.addMovement(event);
                    velocityTracker.computeCurrentVelocity(1000, maximumFlingVelocity);
                    int velocity = (int) velocityTracker.getYVelocity();
                    velocityTracker.recycle();
                    velocityTracker = null;
                    if (Math.abs(velocity) > minimumFlingVelocity) {
                        scroller.fling(0, scrollY, 0, -velocity, 0, 0, 0, maxScroll);
                        postInvalidateOnAnimation();
                    }
                }
                dragging = false;
                return true;
            }
            case MotionEvent.ACTION_CANCEL:
                dragging = false;
                return true;
            default:
                return dragging;
        }
    }

    @Override public boolean performClick() {
        return super.performClick();
    }

    /** Exposed for the drag state, which the tests and the caller both read. */
    boolean isDragging() { return dragging; }
}