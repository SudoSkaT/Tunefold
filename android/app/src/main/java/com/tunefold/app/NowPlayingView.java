package com.tunefold.app;

import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.drawable.ColorDrawable;
import android.text.TextUtils;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.widget.LinearLayout;
import android.widget.TextView;

import java.util.ArrayList;
import java.util.List;

/**
 * The now-playing surface: artwork, then identity, then state, then progress, then the
 * transport, then the secondary actions.
 *
 * <p>Reflects whatever {@link PlaybackSession} reports and holds no state of its own
 * beyond the last value each part rendered, which is what lets a recreated Activity
 * look identical to the one it replaced.
 *
 * <h3>One entry point per kind of change</h3>
 * This surface used to have a single {@code render(snapshot)} that re-applied
 * everything: metadata, state, progress, download and error, on every notification. A
 * position tick therefore re-set six labels and a progress bar, and a state change
 * re-read the track title. Each of those is a {@code setText} or {@code setProgress},
 * and each requests layout and then a draw.
 *
 * <p>So the update is split into {@link #applyTrack}, {@link #applyPlaybackState},
 * {@link #applyLibraryState}, {@link #applyDownloadState}, {@link #applyError} and
 * {@link #setPosition}, and every one of them compares against what it last rendered
 * before it writes. A notification that changes one thing now touches one view, and a
 * tick that cannot change anything visible costs a handful of comparisons.
 *
 * <h3>Built in code, from tokens</h3>
 * Every dimension and colour comes from {@link DesignTokens}, so nothing here can drift
 * from the rest of the surface. There are no fixed sizes: the cover is derived from the
 * window by the Activity and clamped here.
 *
 * <p>Technical diagnostics never appear here; they belong to {@link PlaybackDebugPanel}.
 */
final class NowPlayingView implements ArtworkTheme.AccentListener {

    /** Callbacks to the Activity. All invoked on the UI thread. */
    interface Actions {
        void onPlayPause();
        void onNext();
        void onPrevious();
        void onLike();
        void onDownload();
        void onCancelDownload();
        void onRetryDownload();
        void onRetryPlayback();
        void onSkipToNext();
        void onToggleAutoplay();
        /** Appends a recommended track to the queue without interrupting playback. */
        void onEnqueueRecommended(MediaTrack track);
        /** Starts a radio station from the current track. */
        void onCreateRadio();
        /** Starts a radio station from the current artist. */
        void onCreateArtistRadio();
        /** Stops the radio station; the queue stays as it stands. */
        void onStopRadio();
    }

    /** Which shelf the player shows below the transport. */
    enum Tab { QUEUE, RECOMMENDED, RADIO }

    /** What the error action offers; decides its glyph, its description and its effect. */
    private enum ErrorAction { NONE, RETRY, SKIP }

    /** Told when a row of "next" is tapped. */
    interface QueuedTrackChosen {
        void onQueuedTrackChosen(MediaTrack track);
        /** Takes a queued track out of the queue. */
        void onQueuedTrackRemoved(MediaTrack track);
    }

    /** How many tracks the player shows as coming next. */
    private static final int UP_NEXT_ROWS = 4;
    /** La letra se apaga, no desaparece, mientras la canción no tiene qué cantar. */
    private static final float LYRICS_SILENCE_ALPHA = 0.45f;

    private static final CharSequence NOTHING_PLAYING = "Nothing playing";

    private final Actions actions;
    private final Surface root;
    private final LinearLayout stage;
    private final ArtworkView artwork;
    private final LinearLayout details;
    private final TextView title;
    private final TextView subtitle;
    private final StateDotView stateDot;
    private final TextView stateLabel;
    private final TextView errorLabel;
    private final IconButton errorAction;
    private final ProgressControl progress;
    private final TextView lyricLine;
    private final LinearLayout upNext;
    private LyricTrack lyricTrack;
    private long lyricPositionMs = -1L;
    /** Qué lista de "sigue" está construida; el mismo contenido no se reconstruye. */
    private String queuedKey;
    /** What the "next" rows do; the screen owns the queue, this owns the buttons. */
    private QueuedTrackChosen queuedTrackChosen;
    /** Row holders of the current Sigue window, for cover delivery. */
    private final java.util.List<QueueRow> queueRows = new ArrayList<>();

    /** One queue row: cover and track, rebuilt with the window. */
    private static final class QueueRow {
        final MediaTrack track;
        final ArtworkView cover;
        String shownCoverId = "";

        QueueRow(MediaTrack track, ArtworkView cover) {
            this.track = track;
            this.cover = cover;
        }
    }
    /**
     * Lo recomendado para lo que suena, ya filtrado por la pantalla.
     *
     * <p>Su propio espacio al final, con filas de lista —portada, título,
     * artista y un añadir— como los seleccionables de una playlist: la fila
     * entera mete la pista en la cola y la fila sale de la oferta.
     */
    private final LinearLayout recommended;
    /** How many recommended rows the player offers; Sigue shows the same count. */
    private static final int RECOMMENDED_ROWS = 4;
    private final java.util.List<RecommendedRow> recommendedRows = new ArrayList<>();
    /** Identity of the recommendations on screen; rebuilt only on change. */
    private String recommendedKey;
    /**
     * Las dos repisas —cola y descubrimiento— bajo un selector.
     *
     * <p>La vista no decide qué suena, solo qué lista se mira: la cola que va
     * a sonar, los recomendados para elegir o la radio. La radio es un modo de
     * la sesión con segmento propio siempre visible: apagada ofrece armar una
     * estación; encendida enseña la cola que está armando.
     */
    private Tab selectedTab = Tab.QUEUE;
    private boolean radioOn;
    private String radioLabel = "";
    private LinearLayout tabBar;
    private TextView queueTab;
    private TextView recommendedTab;
    private TextView radioTab;
    /** Radio apagada: armar estación por canción o por artista. */
    private LinearLayout radioPane;
    /** What an empty shelf says, instead of blank space. */
    private TextView emptyHint;
    private final IconButton previous;
    private final PrimaryPlaybackButton playPause;
    private final IconButton next;
    private final SecondaryActionButton autoplay;
    private final SecondaryActionButton like;
    private final SecondaryActionButton download;

    // ---- last rendered values; every apply* method compares before it writes ----

    private TrackKey activeTrackKey;
    private boolean metadataShown;
    /** Session state the status word was last written for. */
    private PlaybackState renderedPhaseState;
    /** Phase the indicator was last given, dot and word together. */
    private PlaybackStatus.Phase renderedPhase;
    private boolean renderedPlaying;
    private boolean renderedAutoplay;
    private boolean renderedLiked;
    private DownloadState.Phase renderedDownloadPhase;
    private int renderedDownloadPercent = Integer.MIN_VALUE;
    private ErrorAction renderedErrorAction = ErrorAction.NONE;
    /** Whether audio can be reaching the bars: only then is polling worth it (§7). */
    private boolean soundPlayable;
    /** Whether the primary control last drew its working ring. */
    private boolean renderedBusy;
    private CharSequence renderedErrorText = "";
    /** The window this surface was last laid out for; {@code null} before the first. */
    private LayoutRules.Profile window;
    /** Identity of the current window's layout, so a repeated call costs one compare. */
    private String windowKey = "";
    /**
     * Identity of the composition itself: window and cover together.
     *
     * <p>Separate from {@link #windowKey} because the cover can change the composition
     * on its own — a track change is not a configuration change.
     */
    private String compositionKey = "";
    private int artworkSidePx;
    private String artworkSideKey = "";
    /**
     * El lado que la ventana pide para la portada, antes de que los datos la aprieten.
     *
     * <p>Es el techo al que puede volver a crecer: sin él el único número disponible
     * era el lado actual, ya recortado, y una portada encogida por una cola larga se
     * quedaba pequeña para siempre.
     */
    private int preferredArtworkSidePx;

    /** El contexto de la pantalla, para lo que se construye más tarde. */
    private final Context context;

    NowPlayingView(Context context, Actions actions) {
        this.context = context;
        DesignTokens.init(context);
        this.actions = actions;
        int pad = DesignTokens.dp(DesignTokens.CARD_PADDING);
        root = new Surface(context, this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setPadding(pad, pad, pad, pad);
        root.setBackground(new ColorDrawable(DesignTokens.Palette.SURFACE_CARD));
        root.setElevation(DesignTokens.dp(DesignTokens.ELEVATION_CARD));

        // `stage` holds artwork and details as SIBLINGS from the start, so a layout
        // change only re-weights them: no view is ever re-parented and nothing can
        // end up hidden in the wrong mode.
        stage = new LinearLayout(context);
        stage.setOrientation(LinearLayout.VERTICAL);
        // La portada es el ancla de la pantalla: centrada en horizontal, con los
        // datos debajo alineados al mismo centro. Sin esto la columna de texto se
        // pegaba al borde y la composición parecía rota.
        stage.setGravity(Gravity.CENTER_HORIZONTAL);
        root.addView(stage, new LinearLayout.LayoutParams(-1, -2));

        artwork = new ArtworkView(context);
        stage.addView(artwork, new LinearLayout.LayoutParams(-1, -2));

        details = new LinearLayout(context);
        details.setOrientation(LinearLayout.VERTICAL);
        details.setGravity(Gravity.CENTER_HORIZONTAL);
        stage.addView(details, new LinearLayout.LayoutParams(-1, -2));

        title = label(context, DesignTokens.TYPE_TITLE_SIZE, DesignTokens.Palette.ON_SURFACE);
        title.setMaxLines(2);
        title.setEllipsize(TextUtils.TruncateAt.END);
        title.setText(NOTHING_PLAYING);
        addDetails(title, DesignTokens.BLOCK_GAP);

        subtitle = label(context, DesignTokens.TYPE_BODY_SIZE,
                DesignTokens.Palette.ON_SURFACE_MUTED);
        subtitle.setMaxLines(1);
        subtitle.setEllipsize(TextUtils.TruncateAt.END);
        addDetails(subtitle, 0f);

        // The state is a shape plus a word: neither alone tells the user whether
        // Tunefold is working, moving, waiting or broken (§7).
        LinearLayout statusRow = row(context, Gravity.CENTER | Gravity.CENTER_VERTICAL);
        stateDot = new StateDotView(context);
        statusRow.addView(stateDot, new LinearLayout.LayoutParams(
                DesignTokens.dp(DesignTokens.STATE_DOT_SIZE),
                DesignTokens.dp(DesignTokens.STATE_DOT_SIZE)));
        stateLabel = label(context, DesignTokens.TYPE_LABEL_SIZE,
                DesignTokens.Palette.ON_SURFACE_MUTED);
        stateLabel.setLetterSpacing(DesignTokens.TYPE_LABEL_TRACKING);
        stateLabel.setAllCaps(true);
        LinearLayout.LayoutParams stateParams = new LinearLayout.LayoutParams(-2, -2);
        stateParams.leftMargin = DesignTokens.dp(DesignTokens.GAP_TIGHT);
        statusRow.addView(stateLabel, stateParams);
        addDetails(statusRow, DesignTokens.GAP);

        // A failure gets its own line plus an action, never a silent dead end (§19).
        errorLabel = label(context, DesignTokens.TYPE_BODY_SIZE, DesignTokens.Palette.ERROR);
        errorLabel.setVisibility(View.GONE);
        addDetails(errorLabel, 0f);

        errorAction = new IconButton(context);
        errorAction.setVisibility(View.GONE);
        addDetails(errorAction, DesignTokens.GAP_TIGHT, -2);

        progress = new ProgressControl(context);
        addDetails(progress, DesignTokens.BLOCK_GAP);

        // La línea que se está cantando. Va donde estaba la mitad de las barras que
        // ya no hay, y es la misma regla que la consola: la última línea cuyo
        // momento ya pasó.
        lyricLine = new TextView(context);
        lyricLine.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_BODY_SIZE);
        lyricLine.setTextColor(DesignTokens.Palette.ON_SURFACE);
        lyricLine.setGravity(Gravity.CENTER);
        lyricLine.setMaxLines(3);
        lyricLine.setEllipsize(TextUtils.TruncateAt.END);
        lyricLine.setVisibility(View.GONE);
        addDetails(lyricLine, DesignTokens.GAP);

        // ---- transport: previous, the one control that matters, next ----
        LinearLayout transport = row(context, Gravity.CENTER);
        int target = DesignTokens.dp(DesignTokens.CONTROL_TARGET);
        previous = new IconButton(context);
        previous.setAction(Icon.PREVIOUS, "Previous track");
        previous.setOnClickListener(ignored -> actions.onPrevious());
        transport.addView(previous, new LinearLayout.LayoutParams(target, target));

        playPause = new PrimaryPlaybackButton(context);
        playPause.setAction(Icon.PLAY, "Start playback");
        playPause.setOnClickListener(ignored -> actions.onPlayPause());
        LinearLayout.LayoutParams playParams = new LinearLayout.LayoutParams(
                DesignTokens.dp(DesignTokens.CONTROL_TARGET_PRIMARY),
                DesignTokens.dp(DesignTokens.CONTROL_TARGET_PRIMARY));
        playParams.leftMargin = DesignTokens.dp(DesignTokens.SECTION_GAP);
        transport.addView(playPause, playParams);

        next = new IconButton(context);
        next.setAction(Icon.NEXT, "Next track");
        next.setOnClickListener(ignored -> actions.onNext());
        LinearLayout.LayoutParams nextParams = new LinearLayout.LayoutParams(target, target);
        nextParams.leftMargin = DesignTokens.dp(DesignTokens.SECTION_GAP);
        transport.addView(next, nextParams);
        addDetails(transport, DesignTokens.SECTION_GAP);

        // ---- acciones secundarias: una sola fila, en el orden en que se usan
        // (guardar, repetir, bajar). Parar se fue: parar es lo que hace el
        // servicio al terminar la cola, no una acción que el usuario reasonably
        // busca en la pantalla de una canción.
        LinearLayout actionRow = row(context, Gravity.CENTER);
        like = new SecondaryActionButton(context);
        like.setAction(Icon.HEART, "Not liked. Double tap to add to L1K3D.");
        like.setOnClickListener(ignored -> actions.onLike());
        actionRow.addView(like, new LinearLayout.LayoutParams(target, target));

        autoplay = new SecondaryActionButton(context);
        autoplay.setAction(Icon.AUTOPLAY, "Autoplay");
        autoplay.setOnClickListener(ignored -> actions.onToggleAutoplay());
        LinearLayout.LayoutParams autoplayParams = new LinearLayout.LayoutParams(target, target);
        autoplayParams.leftMargin = DesignTokens.dp(DesignTokens.GAP);
        actionRow.addView(autoplay, autoplayParams);

        download = new SecondaryActionButton(context);
        download.setAction(Icon.DOWNLOAD, "Download. Double tap to start.");
        // Dispatch is on the last rendered phase, never on wording: matching strings
        // made this control's action depend on its label and silently routed Cancel to
        // a repaint, which made cancellation look broken (§8, §26).
        download.setOnClickListener(ignored -> {
            switch (renderedDownloadPhase) {
                case RESOLVING_SOURCE:
                case DOWNLOADING:
                    actions.onCancelDownload();
                    break;
                case FAILED:
                case CANCELLED:
                    actions.onRetryDownload();
                    break;
                default:
                    actions.onDownload();
                    break;
            }
        });
        LinearLayout.LayoutParams downloadParams = new LinearLayout.LayoutParams(target, target);
        downloadParams.leftMargin = DesignTokens.dp(DesignTokens.GAP);
        actionRow.addView(download, downloadParams);
        addDetails(actionRow, 0f);

        // "A continuación": la cola que el reproductor va a soltar. Sólo cuando hay
        // algo detrás — una lista puesta desde el reparto o una cola vaciada no
        // merecen un encabezado con nada debajo.
        upNext = new LinearLayout(context);
        upNext.setOrientation(LinearLayout.VERTICAL);
        upNext.setVisibility(View.GONE);
        addDetails(upNext, DesignTokens.BLOCK_GAP);

        // Recomendados: su propio espacio al final, con filas de lista —
        // portada, título, artista y un añadir — como los seleccionables de
        // una playlist: la fila entera mete la pista en la cola y la fila sale
        // de la oferta.
        recommended = new LinearLayout(context);
        recommended.setOrientation(LinearLayout.VERTICAL);
        recommended.setVisibility(View.GONE);
        addDetails(recommended, DesignTokens.BLOCK_GAP);

        // El selector de repisas, delante de las listas: Siguiente,
        // Recomendados y siempre Radio — apagada para armarla, encendida para
        // mirar la cola que está armando.
        tabBar = new LinearLayout(context);
        tabBar.setOrientation(LinearLayout.HORIZONTAL);
        tabBar.setGravity(Gravity.CENTER);
        queueTab = tabButton("Siguiente");
        queueTab.setOnClickListener(ignored -> selectTab(Tab.QUEUE));
        tabBar.addView(queueTab, tabParams());
        recommendedTab = tabButton("Recomendados");
        recommendedTab.setOnClickListener(ignored -> selectTab(Tab.RECOMMENDED));
        tabBar.addView(recommendedTab, tabParams());
        radioTab = tabButton("Radio");
        radioTab.setOnClickListener(ignored -> selectTab(Tab.RADIO));
        tabBar.addView(radioTab, tabParams());
        details.addView(tabBar, details.indexOfChild(upNext),
                new LinearLayout.LayoutParams(-1, -2));

        // Armar radio: por la canción que suena o por su artista, por separado.
        radioPane = new LinearLayout(context);
        radioPane.setOrientation(LinearLayout.VERTICAL);
        radioPane.setVisibility(View.GONE);
        radioPane.addView(radioStarter("Radio de esta canción",
                "Create a radio station from the current track",
                () -> actions.onCreateRadio()));
        radioPane.addView(radioStarter("Radio del artista",
                "Create a radio station from the current artist",
                () -> actions.onCreateArtistRadio()));
        details.addView(radioPane, details.indexOfChild(upNext),
                new LinearLayout.LayoutParams(-1, -2));

        // Lo que dice una repisa vacía, en vez de un hueco mudo.
        emptyHint = new TextView(context);
        emptyHint.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_CAPTION_SIZE);
        emptyHint.setTextColor(DesignTokens.Palette.ON_SURFACE_MUTED);
        emptyHint.setGravity(Gravity.CENTER);
        emptyHint.setVisibility(View.GONE);
        details.addView(emptyHint, details.indexOfChild(upNext),
                new LinearLayout.LayoutParams(-1, -2));

        // Deslizar sobre las listas cambia de repisa: el gesto horizontal manda
        // y el vertical sigue siendo del scroll. Solo observa, nunca consume,
        // así que los toques y el desplazamiento no cambian.
        upNext.setOnTouchListener(tabSwipe);
        recommended.setOnTouchListener(tabSwipe);
        paintTabs();

        // The cover decides whether the card splits, so the card listens to the cover
        // — LAST, because `register` hands the listener the current palette straight
        // away, and this card has to be able to answer that before it has built
        // anything to compose.
        ArtworkTheme.register(this);
    }

    View view() { return root; }

    /** Quién oye un toque en una fila de "sigue". */
    void setQueuedTrackChosen(QueuedTrackChosen chosen) { queuedTrackChosen = chosen; }

    /**
     * Lo que viene detrás de lo que suena, ya sin la pista actual.
     *
     * <p>Se quita la que está sonando en el reproductor y no aquí: el reproductor es
     * quien sabe cuál es, y hacer las dos cosas aquí obligaría a esta clase a volver a
     * preguntar por la posición del cursor en cada fila.
     */
    void setUpNextFrom(PlaybackQueue queue, TrackKey current) {
        List<MediaTrack> next = new ArrayList<>();
        int from = current == null ? 0 : queue.indexOfKey(current);
        if (from < 0) from = 0;
        for (int index = from + 1; index < queue.size() && next.size() < UP_NEXT_ROWS + 1; index++) {
            MediaTrack track = queue.at(index);
            if (track != null) next.add(track);
        }
        setUpNext(next);
    }

    /** The cover's side, already clamped to what is left of the window. */
    void fitArtworkTo(int side) { applyArtworkSide(side); }
    /**
     * The card, as a {@code LinearLayout} that lets the cover give up height.
     *
     * <p>The cover is the biggest thing on the surface and the only one that can be
     * made smaller without losing meaning: a title, the state, the bars and the
     * transport are what the user came for, and a {@code LinearLayout} that runs out
     * of height squeezes its LAST children — which was exactly the transport and the
     * actions, cut clean off the bottom of the screen.
     *
     * <p>The details are measured first, at their natural height, and the cover takes
     * whatever is left of the window. Both numbers are known before the cover is laid
     * out, so this converges in one pass and cannot loop.
     *
     * <h3>Why the measurement is remembered</h3>
     * Measuring the details subtree is the expensive half of this pass, and it was
     * repeated on <em>every</em> measure — every scroll-driven layout, every keyboard,
     * every insets change — to arrive at the same number. The memo below keys on the
     * two inputs the answer depends on (the inner width and the available height), so
     * a second pass over the same window costs two comparisons instead of a subtree
     * walk.
     */
    // ---- the card ----

    private static final class Surface extends LinearLayout {
        private final NowPlayingView owner;
        /** Inner width the details were last measured at; -1 when never. */
        private int fitWidth = -1;
        /** Available height they were last measured against; -1 when never. */
        private int fitHeight = -1;
        /** Measured height of the details subtree, reused while nothing moved. */
        private int detailsHeight;
        /** El relleno de la tarjeta, sin el aire repartido que se le añade luego. */
        private int basePaddingTop;
        private int basePaddingBottom;
        private boolean paddingRemembered;

        Surface(Context context, NowPlayingView owner) {
            super(context);
            this.owner = owner;
        }

        @Override protected void onMeasure(int widthSpec, int heightSpec) {
            int width = MeasureSpec.getSize(widthSpec);
            int available = MeasureSpec.getSize(heightSpec);
            // Two regions are fitted by width, not by height: there the cover is
            // beside its details, so it gives up width, and the height guard is
            // the window's own business.
            if (!owner.twoColumn() && width > 0 && available > 0) {
                if (!paddingRemembered) {
                    basePaddingTop = getPaddingTop();
                    basePaddingBottom = getPaddingBottom();
                    paddingRemembered = true;
                }
                int inner = Math.max(0, width - getPaddingLeft() - getPaddingRight());
                if (inner != fitWidth || available != fitHeight) {
                    View details = owner.details;
                    details.measure(MeasureSpec.makeMeasureSpec(inner, MeasureSpec.EXACTLY),
                            MeasureSpec.makeMeasureSpec(0, MeasureSpec.UNSPECIFIED));
                    fitWidth = inner;
                    fitHeight = available;
                    detailsHeight = details.getMeasuredHeight();
                }
                LinearLayout.LayoutParams art =
                        (LinearLayout.LayoutParams) owner.artwork.getLayoutParams();
                LinearLayout.LayoutParams text =
                        (LinearLayout.LayoutParams) owner.details.getLayoutParams();
                int gaps = art.bottomMargin + text.topMargin;
                int spare = available - getPaddingTop() - getPaddingBottom()
                        - detailsHeight - gaps;
                // El suelo es el mínimo del token: por debajo, una portada ya no es
                // una portada y el problema pasa a ser de ventana, no de proporción.
                // El techo es el lado que la ventana pidió, para que la portada no
                // se coma la pantalla cuando los datos son pocos.
                int wanted = Math.min(owner.preferredArtworkSidePx(),
                        Math.max(DesignTokens.dp(DesignTokens.ARTWORK_MIN), spare));
                // Crecer Y encogerse. Sólo encogerse dejaba la portada pegada a su
                // mínimo para siempre: un momento con la cola larga —«Sigue» con sus
                // filas, o una letra de tres líneas— la encogía, y al vaciarse la
                // cola ya no volvía a su tamaño. La pantalla se quedaba con un hueco
                // de tres dedos abajo y la portada pequeña en un rincón.
                if (wanted != art.height) owner.fitArtworkTo(wanted);
                // El aire que sobra ni con la portada a su tamaño se reparte a mitades
                // arriba y abajo. Amontonado abajo, el reproductor parecía no estar
                // centrado; era el mismo aire, pero todo en un solo lado.
                //
                // Reparto óptico 60/40 en vez de 50/50: la mitad matemática deja la
                // composición visualmente alta (la portada pesa arriba), así que el
                // héroe —portada, datos, estado, barras y transporte— se asienta un
                // poco más abajo y respira como centro de la vista.
                int air = spare - wanted;
                int free = Math.max(0, air);
                int top = basePaddingTop + free * 3 / 5;
                int bottom = basePaddingBottom + free - free * 3 / 5;
                if (getPaddingTop() != top || getPaddingBottom() != bottom) {
                    setPadding(getPaddingLeft(), top, getPaddingRight(), bottom);
                }
            }
            super.onMeasure(widthSpec, heightSpec);
        }
    }



    /** The artwork sub-view, for window-derived sizing and cover identity. */
    ArtworkView artwork() { return artwork; }

    /**
     * Binds the live audio source behind the bars.
     *
     * <p>The surface never reads the engine itself: it asks the source, and the
     * source is what knows how to ask once and how often.
     */
    void attachSound(LiveAudioBars source) {
        progress.attach(source);
    }

    /**
     * The UI tick, the one periodic thing this surface does.
     *
     * <p>It advances the working indicator and nothing else: the bars read their
     * own data through {@link #refreshSound}, because a read is worth doing
     * exactly when something might have changed.
     */
    void tick() {
        stateDot.advance();
    }

    /**
     * The visual tick (20 Hz), which exists only to move the bars.
     *
     * <p>Separate from {@link #tick} on purpose: the working indicator needs a
     * human pace and the bars need to answer to the sound, and a shared timer can
     * only be as fast as the slowest of the two or as smooth as the fastest.
     *
     * It asks nothing while nothing can be heard: a surface with no audio rests
     * instead of polling a source that will only answer silence.
     */
    void refreshSound() {
        progress.refreshSound(soundPlayable);
    }

    /** Counters for the diagnostics panel. */
    String progressStats() { return progress.stats(); }

    /** Side the cover is currently laid out at, in pixels; the decode target. */
    int artworkSidePx() { return artworkSidePx; }


    // ---------------------------------------------------------------- artwork

    /**
     * Declares which cover the view will accept.
     *
     * <p>Separate from {@link #setArtwork} because the cover arrives asynchronously and
     * must never land on a track it was not requested for.
     */
    void expectArtwork(String identity) { artwork.expectCover(identity); }

    /** Shows a decoded cover, for the identity the view is currently waiting for. */
    void setArtwork(String identity, Bitmap bitmap) { artwork.setCover(identity, bitmap); }

    // ----------------------------------------------------------------- render

    /**
     * Metadata: title, attribution and cover identity.
     *
     * <p>Guarded by track identity, so a notification that changed only the position
     * never re-read or re-set the title.
     */
    void applyTrack(PlaybackSession.Snapshot snapshot) {
        MediaTrack track = snapshot.currentTrack;
        if (track == null) {
            if (!metadataShown) return;
            metadataShown = false;
            activeTrackKey = null;
            setText(title, NOTHING_PLAYING);
            setText(subtitle, "");
            progress.clear();
            progress.clearSound();
            // No track means no accent: leaving the last cover's colour behind would
            // mean the surface keeps advertising a song that is no longer there.
            ArtworkTheme.reset();
            artwork.expectCover(null);
            artwork.clearCover();
            return;
        }
        TrackKey key = track.key();
        boolean identityChanged = !equal(activeTrackKey, key);
        if (identityChanged) {
            // New track: the previous envelope must go now, not when the new
            // audio first arrives. Otherwise the old track's bars sit frozen
            // behind the new title for the whole resolution.
            progress.clearSound();
        }
        // Identity guards the cover, but not the words: the same track can be resolved
        // again with corrected metadata, and a title that never refreshes after that is
        // worse than the extra comparison it costs.
        if (metadataShown && equal(activeTrackKey, key)
                && TextUtils.equals(track.title, title.getText())
                && TextUtils.equals(attributionOf(track), subtitle.getText())) {
            return;
        }
        metadataShown = true;
        activeTrackKey = key;
        setText(title, track.title);
        setText(subtitle, attributionOf(track));
        artwork.expectCover(key == null ? track.providerId : key.toString());
    }

    /** Playback state: status line, state indicator, transport, autoplay. */
    void applyPlaybackState(PlaybackSession.Snapshot snapshot) {
        PlaybackState state = snapshot.state;
        boolean hasTrack = snapshot.currentTrack != null;
        // §26: an action must state what it will do, so the primary control reads
        // Pause while audio is coming out. Mirroring the state instead once left a
        // playing track showing "Play".
        boolean playing = state == PlaybackState.PLAYING;
        if (renderedPlaying != playing) {
            renderedPlaying = playing;
            playPause.setAction(playing ? Icon.PAUSE : Icon.PLAY,
                    playing ? "Pause playback" : "Start playback");
        }
        playPause.setEnabled(hasTrack);
        // Next is never disabled by an empty queue: with autoplay on it still acts, and
        // the session decides between the next item, a recommendation and stopping (§13).
        next.setEnabled(hasTrack || snapshot.autoplayEnabled);
        previous.setEnabled(hasTrack);
        applyAutoplay(snapshot.autoplayEnabled);
        applyStatus(snapshot);
        // The bars only make sense while the engine is engaged with this track: a
        // source still resolving belongs to no measurement yet (§7).
        soundPlayable = state == PlaybackState.BUFFERING
                || state == PlaybackState.PLAYING
                || state == PlaybackState.PAUSED;
        if (!soundPlayable && (state == PlaybackState.STOPPED
                || state == PlaybackState.IDLE || state == PlaybackState.ERROR)) {
            // Terminal states rest empty: a frozen envelope after stop reads as
            // activity where there is none. Pauses and resolutions keep their
            // frame (frozen by refresh, cleared by applyTrack on change).
            progress.clearSound();
        }
        // El control principal también dice en qué estado está: un aro mientras
        // espera, como el punto de estado y sin palabras nuevas.
        applyBusy(state == PlaybackState.BUFFERING
                || state == PlaybackState.RESOLVING_SOURCE
                || state == PlaybackState.RESOLVING_METADATA
                || (snapshot.download != null && snapshot.download.isActive()));
    }

    /**
     * The status line and the indicator, decided together.
     *
     * <p>Called from the download render as well as the state render, because a
     * running download is a state this surface has to show even while playback is
     * busy: {@link PlaybackStatus} decides which of the two wins, and both the word
     * and the dot follow that one decision so they can never disagree.
     */
    private void applyStatus(PlaybackSession.Snapshot snapshot) {
        DownloadState download = snapshot.download;
        DownloadState.Phase downloadPhase = download == null
                ? DownloadState.Phase.IDLE : download.phase;
        boolean transferring = downloadPhase == DownloadState.Phase.RESOLVING_SOURCE
                || downloadPhase == DownloadState.Phase.DOWNLOADING;
        PlaybackStatus.Phase phase = PlaybackStatus.phaseOf(snapshot.state, transferring);
        // Both are compared: two different waiting states share a phase but not a
        // word, and "Searching" turning into "Resolving metadata" is information.
        if (phase != renderedPhase || snapshot.state != renderedPhaseState) {
            renderedPhase = phase;
            renderedPhaseState = snapshot.state;
            setText(stateLabel, PlaybackStatus.labelOf(snapshot.state, transferring));
        }
        // Always, and not only when something changed: two things move without the
        // phase changing — a download's percentage, and the artwork-derived accent
        // (the dot's own colour comes from it). The control compares three integers
        // and does nothing when they all match.
        int percent = downloadPhase == DownloadState.Phase.DOWNLOADING && download != null
                ? download.progressPercent() : -1;
        stateDot.apply(renderedPhase, ArtworkTheme.accent(), percent);
    }

    private void applyAutoplay(boolean enabled) {
        if (renderedAutoplay == enabled) return;
        renderedAutoplay = enabled;
        autoplay.setToggleState(enabled);
        autoplay.setPhase(enabled ? Icon.AUTOPLAY : Icon.AUTOPLAY_OFF,
                enabled ? SecondaryActionButton.Phase.IDLE : SecondaryActionButton.Phase.OFF,
                -1);
        autoplay.applyDescription(enabled
                ? "Autoplay is on. Double tap to turn it off."
                : "Autoplay is off. Double tap to turn it on.");
    }

    /** Library membership: the like control. */
    void applyLibraryState(PlaybackSession.Snapshot snapshot) {
        like.setEnabled(snapshot.currentKey != null);
        if (renderedLiked == snapshot.liked) return;
        renderedLiked = snapshot.liked;
        // §26: the control always shows the result of the last action.
        like.setToggleState(snapshot.liked);
        like.setPhase(snapshot.liked ? Icon.HEART_FILLED : Icon.HEART,
                snapshot.liked ? SecondaryActionButton.Phase.DONE
                        : SecondaryActionButton.Phase.IDLE,
                -1);
        like.applyDescription(snapshot.liked
                ? "Liked. Double tap to remove from L1K3D."
                : "Not liked. Double tap to add to L1K3D.");
    }

    /** Download state: glyph, tint and progress ring. */
    void applyDownloadState(PlaybackSession.Snapshot snapshot) {
        DownloadState state = snapshot.download;
        DownloadState.Phase phase = state == null ? DownloadState.Phase.IDLE : state.phase;
        int percent = phase == DownloadState.Phase.DOWNLOADING && state != null
                ? state.progressPercent() : -1;
        download.setEnabled(snapshot.currentTrack != null);
        if (renderedDownloadPhase == phase && renderedDownloadPercent == percent) return;
        renderedDownloadPhase = phase;
        renderedDownloadPercent = percent;
        download.setPhase(downloadIcon(phase), downloadPhaseOf(phase), percent);
        download.applyDescription(downloadDescription(phase, percent));
        applyStatus(snapshot);
    }

    /** The working ring on the primary control, when only the ring would change. */
    private void applyBusy(boolean value) {
        if (renderedBusy == value) return;
        renderedBusy = value;
        playPause.setBusy(value);
    }

    /** Failure: message and the one action that can move it forward. */
    void applyError(PlaybackSession.Snapshot snapshot) {
        PlaybackError error = snapshot.error;
        boolean failing = error != null && snapshot.state == PlaybackState.ERROR;
        ErrorAction offer = ErrorAction.NONE;
        CharSequence message = "";
        if (failing) {
            // Recoverable failures offer Retry; an unavailable track offers Next,
            // because retrying a 404 forever would be dishonest (§19).
            offer = error.retryable ? ErrorAction.RETRY : ErrorAction.SKIP;
            message = error.message;
        }
        if (offer == renderedErrorAction && TextUtils.equals(message, renderedErrorText)) return;
        renderedErrorAction = offer;
        renderedErrorText = message;
        if (offer == ErrorAction.NONE) {
            setText(errorLabel, "");
            errorLabel.setVisibility(View.GONE);
            errorAction.setVisibility(View.GONE);
            return;
        }
        setText(errorLabel, message);
        errorLabel.setVisibility(View.VISIBLE);
        errorAction.setVisibility(View.VISIBLE);
        errorAction.setAction(offer == ErrorAction.RETRY ? Icon.RETRY : Icon.NEXT,
                offer == ErrorAction.RETRY ? "Retry playback" : "Skip to the next track");
    }

    /**
     * Applies a position tick.
     *
     * <p>Guarded by identity inside {@link ProgressControl}: a tick computed for a
     * track that is no longer the active one is dropped, so a late update can never
     * repaint the elapsed time of the previous song over the new one. This is the last
     * line of defence behind the session's own generation check.
     */
    void setPosition(TrackKey key, long positionMs, long durationMs) {
        if (key == null) return;
        progress.setPosition(key, positionMs, durationMs);
        showLyricAt(positionMs);
    }

    /**
     * The lines of what is playing, or {@code null} when there is none.
     *
     * <p>Comes late: the lyric arrives after the song started, and re-setting it moves
     * the caret straight to the line being sung right now instead of replaying the
     * song from its first line.
     */
    void setLyrics(LyricTrack track) {
        lyricTrack = track;
        lyricPositionMs = -1L;
        if (track == null || track.isEmpty()) {
            lyricLine.setVisibility(View.GONE);
            return;
        }
        lyricLine.setVisibility(View.VISIBLE);
        lyricLine.setText(DesignTokens.LYRICS_SILENCE);
        lyricLine.setAlpha(LYRICS_SILENCE_ALPHA);
        showLyricAt(lyricPositionMs >= 0L ? lyricPositionMs : 0L);
    }

    /**
     * Which "next" rows to offer, in order, already starting after what is playing.
     *
     * <p>Tapping one is the whole point: the list is a queue you can act on, not a
     * label. Rows are rebuilt only when the identity of the list changes, so the
     * twenty-hertz tick never touches this.
     */
    void setUpNext(List<MediaTrack> tracks) {
        java.util.List<MediaTrack> wanted = tracks == null
                ? java.util.Collections.<MediaTrack>emptyList() : tracks;
        if (wanted.isEmpty()) {
            if (queuedKey != null) {
                upNext.removeAllViews();
                queueRows.clear();
                queuedKey = null;
            }
            refreshTabContent();
            return;
        }
        StringBuilder identity = new StringBuilder();
        for (int index = 0; index < wanted.size(); index++) {
            TrackKey key = wanted.get(index).key();
            identity.append(key == null ? "" : key.toString()).append('\u0001');
        }
        String stamp = identity.toString();
        if (stamp.equals(queuedKey)) return;
        queuedKey = stamp;
        upNext.removeAllViews();
        queueRows.clear();
        TextView header = queueHeader();
        header.setLayoutParams(new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT));
        upNext.addView(header);
        int shown = Math.min(wanted.size(), UP_NEXT_ROWS);
        for (int index = 0; index < shown; index++) {
            MediaTrack track = wanted.get(index);
            QueueRow row = new QueueRow(track, new ArtworkView(context));
            queueRows.add(row);
            upNext.addView(buildQueueRow(row, index));
        }
        if (wanted.size() > shown) {
            TextView more = moreRow(wanted.size() - shown);
            more.setLayoutParams(new LinearLayout.LayoutParams(
                    LinearLayout.LayoutParams.MATCH_PARENT,
                    LinearLayout.LayoutParams.WRAP_CONTENT));
            upNext.addView(more);
        }
        refreshTabContent();
    }

    /**
     * Tracks currently shown in Sigue, in order.
     *
     * <p>Lets the screen load their covers once per window instead of once per
     * render: the view owns the window, the screen owns the loading.
     */
    List<MediaTrack> queuedShown() {
        List<MediaTrack> shown = new ArrayList<>(queueRows.size());
        for (QueueRow row : queueRows) shown.add(row.track);
        return shown;
    }

    private TextView queueHeader() {
        TextView header = new TextView(context);
        header.setText("SIGUE");
        header.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_CAPTION_SIZE);
        header.setTextColor(DesignTokens.Palette.ON_SURFACE_MUTED);
        header.setLetterSpacing(0.12f);
        header.setGravity(Gravity.CENTER);
        header.setFocusable(false);
        return header;
    }

    /**
     * One queue row, playlist style: cover, title, artist and a remove control.
     *
     * <p>Same visual language as the recommended rows: left-aligned,
     * informative at a glance, one tap away from its action. Tapping the row
     * plays it now; the cross takes it out of the queue. The cover arrives
     * through {@link #setQueueArtwork} with the identity it was requested for.
     */
    private View buildQueueRow(QueueRow row, int index) {
        MediaTrack track = row.track;
        TrackKey key = track.key();
        String identity = key == null ? "" : key.toString();

        LinearLayout item = new LinearLayout(context);
        item.setOrientation(LinearLayout.HORIZONTAL);
        item.setGravity(Gravity.CENTER_VERTICAL);
        item.setMinimumHeight(DesignTokens.dp(DesignTokens.ROW_HEIGHT));
        item.setClickable(true);
        item.setFocusable(true);
        int rowPadding = DesignTokens.dp(DesignTokens.ROW_PADDING_HORIZONTAL);
        item.setPadding(rowPadding, 0, rowPadding, 0);

        ArtworkView cover = row.cover;
        cover.setCornerRadiusDp(DesignTokens.RADIUS_ARTWORK_THUMB);
        int side = DesignTokens.dp(DesignTokens.ARTWORK_THUMB);
        LinearLayout.LayoutParams coverParams =
                new LinearLayout.LayoutParams(side, side);
        coverParams.rightMargin = DesignTokens.dp(DesignTokens.ROW_GAP);
        item.addView(cover, coverParams);
        cover.expectCover(identity);
        row.shownCoverId = identity;

        LinearLayout text = new LinearLayout(context);
        text.setOrientation(LinearLayout.VERTICAL);
        text.setGravity(Gravity.CENTER_VERTICAL);
        LinearLayout.LayoutParams textParams = new LinearLayout.LayoutParams(
                0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f);
        textParams.rightMargin = DesignTokens.dp(DesignTokens.GAP);
        item.addView(text, textParams);

        TextView title = new TextView(context);
        title.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_BODY_SIZE);
        title.setTextColor(DesignTokens.Palette.ON_SURFACE);
        title.setMaxLines(1);
        title.setEllipsize(TextUtils.TruncateAt.END);
        title.setIncludeFontPadding(false);
        title.setText(track.title);
        text.addView(title, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT));

        TextView subtitle = new TextView(context);
        subtitle.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_CAPTION_SIZE);
        subtitle.setTextColor(DesignTokens.Palette.ON_SURFACE_MUTED);
        subtitle.setMaxLines(1);
        subtitle.setEllipsize(TextUtils.TruncateAt.END);
        subtitle.setIncludeFontPadding(false);
        subtitle.setText(recommendedInfo(track));
        text.addView(subtitle, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT));

        int target = DesignTokens.dp(DesignTokens.CONTROL_TARGET);
        IconButton remove = new IconButton(context);
        remove.setAction(Icon.CLOSE, "Remove from queue");
        remove.setOnClickListener(ignored -> {
            if (queuedTrackChosen != null) queuedTrackChosen.onQueuedTrackRemoved(track);
        });
        item.addView(remove, new LinearLayout.LayoutParams(target, target));

        String who = track.artist.isEmpty() ? track.channel : track.artist;
        item.setContentDescription((index + 1) + ". " + track.title
                + (who.isEmpty() ? "" : ", " + who)
                + ". Double tap to play this track now.");
        item.setOnTouchListener(tabSwipe);
        item.setOnClickListener(ignored -> {
            if (queuedTrackChosen != null) queuedTrackChosen.onQueuedTrackChosen(track);
        });
        return item;
    }

    /**
     * Shows a decoded cover on the queue row waiting for {@code identity}.
     *
     * <p>Refuses any other identity, so a decode that finishes after the
     * window moved on cannot paint the wrong cover.
     */
    void setQueueArtwork(String identity, android.graphics.Bitmap bitmap) {
        if (identity == null || identity.isEmpty() || bitmap == null) return;
        for (QueueRow row : queueRows) {
            if (identity.equals(row.shownCoverId)) row.cover.setCover(identity, bitmap);
        }
    }

    private TextView moreRow(int hidden) {
        TextView row = new TextView(context);
        row.setText("+" + hidden + " más");
        row.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_CAPTION_SIZE);
        row.setTextColor(DesignTokens.Palette.ON_SURFACE_MUTED);
        row.setGravity(Gravity.CENTER);
        row.setFocusable(false);
        return row;
    }

    /** One tab segment: full-width third, 48dp target, selected in accent. */
    private TextView tabButton(String text) {
        TextView tab = new TextView(context);
        tab.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_LABEL_SIZE);
        tab.setTextColor(DesignTokens.Palette.ON_SURFACE_MUTED);
        tab.setGravity(Gravity.CENTER);
        tab.setMinHeight(DesignTokens.dp(DesignTokens.CONTROL_TARGET));
        tab.setText(text);
        tab.setContentDescription(text);
        tab.setClickable(true);
        tab.setFocusable(true);
        return tab;
    }

    private LinearLayout.LayoutParams tabParams() {
        return new LinearLayout.LayoutParams(0,
                LinearLayout.LayoutParams.WRAP_CONTENT, 1f);
    }

    /**
     * Shows one shelf. Tapping Cola while the radio plays stops the station:
     * choosing the manual queue is choosing it over the generated one.
     */
    private void selectTab(Tab tab) {
        if (tab == null) return;
        if (tab == Tab.QUEUE && radioOn) {
            actions.onStopRadio();
            return;
        }
        if (tab == selectedTab) return;
        selectedTab = tab;
        paintTabs();
        refreshTabContent();
    }

    /**
     * Radio state from the session.
     *
     * <p>Turning on jumps to the radio shelf; turning off falls back to the
     * queue shelf when the radio one was showing. The segment itself never
     * hides: with no station it offers to build one.
     */
    void applyRadio(boolean on, String label) {
        boolean moved = on != radioOn;
        radioOn = on;
        radioLabel = label == null ? "" : label;
        if (moved) {
            selectedTab = on ? Tab.RADIO : Tab.QUEUE;
            paintTabs();
            refreshTabContent();
        } else {
            paintTabs();
        }
    }

    /** The selected tab, for the swipe handler. */
    Tab selectedTab() { return selectedTab; }

    /** Paints the three segments; only the radio label changes over time. */
    private void paintTabs() {
        paintTab(queueTab, "Siguiente", selectedTab == Tab.QUEUE);
        paintTab(recommendedTab, "Recomendados", selectedTab == Tab.RECOMMENDED);
        paintTab(radioTab, radioOn && !radioLabel.isEmpty() ? radioLabel : "Radio",
                selectedTab == Tab.RADIO);
    }

    private static void paintTab(TextView tab, String text, boolean selected) {
        if (!TextUtils.equals(tab.getText(), text)) tab.setText(text);
        int color = selected ? DesignTokens.Palette.ACCENT
                : DesignTokens.Palette.ON_SURFACE_MUTED;
        if (tab.getCurrentTextColor() != color) tab.setTextColor(color);
        tab.setContentDescription(text + (selected ? ", selected" : ""));
    }

    /**
     * Shows exactly one shelf plus the empty hint when it has nothing.
     *
     * <p>The single place visibility is decided: builders fill containers and
     * this decides which one is on screen, so a repeated notification costs
     * comparisons instead of layout passes. The radio shelf shows the queue
     * while a station plays and the station builders while it does not.
     */
    private void refreshTabContent() {
        boolean showRecs = selectedTab == Tab.RECOMMENDED;
        boolean showRadioPane = selectedTab == Tab.RADIO && !radioOn;
        boolean showQueue = !showRecs && !showRadioPane;
        boolean queueEmpty = upNext.getChildCount() == 0;
        boolean recsEmpty = recommended.getChildCount() == 0;
        setVisible(upNext, showQueue && !queueEmpty);
        setVisible(recommended, showRecs && !recsEmpty);
        setVisible(radioPane, showRadioPane);
        if ((showQueue && queueEmpty) || (showRecs && recsEmpty)) {
            setText(emptyHint, showRecs
                    ? "Nada recomendado por aquí todavía."
                    : "La cola está vacía.");
            setVisible(emptyHint, true);
        } else {
            setVisible(emptyHint, false);
        }
        paintTabs();
    }

    /** One station builder: full-width, 48dp target, accent action. */
    private TextView radioStarter(String text, String description, Runnable action) {
        TextView starter = new TextView(context);
        starter.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_BODY_SIZE);
        starter.setTextColor(DesignTokens.Palette.ACCENT);
        starter.setGravity(Gravity.CENTER);
        starter.setMinHeight(DesignTokens.dp(DesignTokens.CONTROL_TARGET));
        starter.setText(text);
        starter.setContentDescription(description);
        starter.setClickable(true);
        starter.setFocusable(true);
        starter.setOnTouchListener(tabSwipe);
        starter.setOnClickListener(ignored -> action.run());
        return starter;
    }

    private static void setVisible(View view, boolean visible) {
        int wanted = visible ? View.VISIBLE : View.GONE;
        if (view.getVisibility() != wanted) view.setVisibility(wanted);
    }

    /**
     * Which tab a horizontal fling on the shelves moves to.
     *
     * <p>Pure so the gesture is a unit test instead of a device rehearsal:
     * left goes forward (Siguiente → Recomendados → Radio) and back again,
     * and anything vertical or short stays where it is.
     */
    static Tab swipeTab(Tab current, float dx, float dy) {
        if (current == null) return Tab.QUEUE;
        if (Math.abs(dx) < DesignTokens.dp(96f) || Math.abs(dx) < 2f * Math.abs(dy)) {
            return current;
        }
        boolean forward = dx < 0f;
        switch (current) {
            case QUEUE: return forward ? Tab.RECOMMENDED : Tab.QUEUE;
            case RECOMMENDED: return forward ? Tab.RADIO : Tab.QUEUE;
            default: return forward ? Tab.RADIO : Tab.RECOMMENDED;
        }
    }

    /**
     * Observes flings over the shelves without ever consuming touches.
     *
     * <p>One instance shared by the shelf containers and every row: a
     * container only sees gestures that land on no clickable child, so rows
     * need it fitted directly or swipes starting on a row never arrive.
     */
    private final View.OnTouchListener tabSwipe = new TabSwipe();

    /** Observes flings over the shelves without ever consuming touches. */
    private final class TabSwipe implements View.OnTouchListener {
        private float downX;
        private float downY;
        private float lastX;
        private float lastY;

        @Override public boolean onTouch(View view, android.view.MotionEvent event) {
            switch (event.getActionMasked()) {
                case android.view.MotionEvent.ACTION_DOWN:
                    downX = event.getX();
                    downY = event.getY();
                    lastX = downX;
                    lastY = downY;
                    break;
                case android.view.MotionEvent.ACTION_MOVE:
                    lastX = event.getX();
                    lastY = event.getY();
                    break;
                case android.view.MotionEvent.ACTION_UP:
                    selectTab(swipeTab(selectedTab, event.getX() - downX,
                            event.getY() - downY));
                    break;
                case android.view.MotionEvent.ACTION_CANCEL:
                    // The scroll container stole the gesture mid-way (a mostly
                    // horizontal swipe with enough vertical noise still scrolls
                    // a little): judge it by where it got to, not by the
                    // missing UP that never arrives.
                    selectTab(swipeTab(selectedTab, lastX - downX,
                            lastY - downY));
                    break;
                default:
                    break;
            }
            return false;
        }
    }

    /** One recommended row: cover, text and add control for one track. */
    private static final class RecommendedRow {
        final MediaTrack track;
        final ArtworkView cover;
        String shownCoverId = "";

        RecommendedRow(MediaTrack track, ArtworkView cover) {
            this.track = track;
            this.cover = cover;
        }
    }

    /**
     * Offers recommended tracks for {@code forIdentity}, already filtered.
     *
     * <p>Rebuilds only when the identity or the content moved: the tick never
     * touches this. Rows are playlist selectables — cover, title, artist and
     * an add control, all visible — and tapping anywhere on one puts it in the
     * queue, where Sigue picks it up.
     */
    void setRecommendations(String forIdentity, List<MediaTrack> tracks) {
        java.util.List<MediaTrack> wanted = tracks == null
                ? java.util.Collections.<MediaTrack>emptyList() : tracks;
        StringBuilder identity = new StringBuilder(forIdentity == null ? "" : forIdentity);
        identity.append('\u0002');
        int shown = Math.min(wanted.size(), RECOMMENDED_ROWS);
        for (int index = 0; index < shown; index++) {
            TrackKey key = wanted.get(index).key();
            identity.append(key == null ? "" : key.toString()).append('\u0001');
        }
        String stamp = identity.toString();
        if (stamp.equals(recommendedKey)) return;
        recommendedKey = stamp;
        recommendedRows.clear();
        recommended.removeAllViews();
        if (shown == 0) {
            refreshTabContent();
            return;
        }
        recommended.addView(recommendedHeader());
        for (int index = 0; index < shown; index++) {
            MediaTrack track = wanted.get(index);
            RecommendedRow row = new RecommendedRow(track, new ArtworkView(context));
            recommendedRows.add(row);
            recommended.addView(buildRecommendedRow(row));
        }
        refreshTabContent();
    }

    /** The section header, shared by fresh builds and removals. */
    private View recommendedHeader() {
        LinearLayout header = new LinearLayout(context);
        header.setOrientation(LinearLayout.HORIZONTAL);
        header.setGravity(Gravity.CENTER_VERTICAL);
        TextView caption = queueHeader();
        caption.setText("RECOMENDADOS");
        caption.setGravity(Gravity.START | Gravity.CENTER_VERTICAL);
        caption.setTextAlignment(View.TEXT_ALIGNMENT_VIEW_START);
        header.addView(caption, new LinearLayout.LayoutParams(
                0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f));
        TextView create = new TextView(context);
        create.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_LABEL_SIZE);
        create.setTextColor(DesignTokens.Palette.ACCENT);
        create.setGravity(Gravity.END | Gravity.CENTER_VERTICAL);
        create.setMinHeight(DesignTokens.dp(DesignTokens.CONTROL_TARGET));
        create.setText("Crear radio");
        create.setContentDescription("Create a radio station from the current track");
        create.setClickable(true);
        create.setFocusable(true);
        create.setOnClickListener(ignored -> actions.onCreateRadio());
        header.addView(create, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT,
                LinearLayout.LayoutParams.WRAP_CONTENT));
        header.setLayoutParams(new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT));
        return header;
    }

    /** Drops the recommendations, e.g. when the track changed. */
    void clearRecommendations() {
        recommendedKey = null;
        recommendedRows.clear();
        if (recommended.getChildCount() > 0) recommended.removeAllViews();
        refreshTabContent();
    }

    /**
     * Takes one track out of the recommendations, after it joined the queue.
     *
     * <p>What is in Sigue is not offered again: the row goes and the rest stay
     * exactly where they were.
     */
    void removeRecommended(TrackKey key) {
        if (key == null || recommendedRows.isEmpty()) return;
        boolean removed = false;
        for (int index = recommendedRows.size() - 1; index >= 0; index--) {
            if (key.equals(recommendedRows.get(index).track.key())) {
                recommendedRows.remove(index);
                removed = true;
            }
        }
        if (!removed) return;
        recommendedKey = null;
        recommended.removeAllViews();
        if (recommendedRows.isEmpty()) {
            refreshTabContent();
            return;
        }
        recommended.addView(recommendedHeader());
        StringBuilder identity = new StringBuilder();
        for (RecommendedRow row : recommendedRows) {
            recommended.addView(buildRecommendedRow(row));
            TrackKey rowKey = row.track.key();
            identity.append(rowKey == null ? "" : rowKey.toString()).append('\u0001');
        }
        recommendedKey = identity.toString();
        refreshTabContent();
    }

    /**
     * One playlist-style selectable: cover, title, artist and an add control.
     *
     * <p>Left-aligned and fully informative, like the songs inside a playlist:
     * tapping anywhere on the row puts the track in the queue. The row's cover
     * arrives through {@link #setRecommendedArtwork} with the identity it was
     * requested for, so a late decode can never paint the wrong track.
     */
    private View buildRecommendedRow(RecommendedRow row) {
        MediaTrack track = row.track;
        TrackKey key = track.key();
        String identity = key == null ? "" : key.toString();

        LinearLayout item = new LinearLayout(context);
        item.setOrientation(LinearLayout.HORIZONTAL);
        item.setGravity(Gravity.CENTER_VERTICAL);
        // The list standard height; compactness comes from the single-line
        // content (no status lines, no inline actions) rather than from
        // shrinking the touch target below what the rest of the app uses.
        item.setMinimumHeight(DesignTokens.dp(DesignTokens.ROW_HEIGHT));
        item.setClickable(true);
        item.setFocusable(true);
        int rowPadding = DesignTokens.dp(DesignTokens.ROW_PADDING_HORIZONTAL);
        item.setPadding(rowPadding, 0, rowPadding, 0);

        ArtworkView cover = row.cover;
        // Detached from whatever item held it before: rebuilds reuse the row
        // (and its already-decoded cover) instead of decoding again, but a
        // View with a parent cannot be added to another one — removeAllViews
        // on the section only detaches the items, not what is inside them, and
        // re-adding the cover blindly died here with IllegalStateException.
        android.view.ViewParent previousParent = cover.getParent();
        if (previousParent instanceof android.view.ViewGroup) {
            ((android.view.ViewGroup) previousParent).removeView(cover);
        }
        cover.setCornerRadiusDp(DesignTokens.RADIUS_ARTWORK_THUMB);
        int side = DesignTokens.dp(DesignTokens.ARTWORK_THUMB);
        LinearLayout.LayoutParams coverParams =
                new LinearLayout.LayoutParams(side, side);
        coverParams.rightMargin = DesignTokens.dp(DesignTokens.ROW_GAP);
        item.addView(cover, coverParams);
        cover.expectCover(identity);
        row.shownCoverId = identity;

        LinearLayout text = new LinearLayout(context);
        text.setOrientation(LinearLayout.VERTICAL);
        text.setGravity(Gravity.CENTER_VERTICAL);
        LinearLayout.LayoutParams textParams = new LinearLayout.LayoutParams(
                0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f);
        textParams.rightMargin = DesignTokens.dp(DesignTokens.GAP);
        item.addView(text, textParams);

        TextView title = new TextView(context);
        title.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_BODY_SIZE);
        title.setTextColor(DesignTokens.Palette.ON_SURFACE);
        title.setMaxLines(1);
        title.setEllipsize(TextUtils.TruncateAt.END);
        title.setIncludeFontPadding(false);
        title.setText(track.title);
        text.addView(title, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT));

        TextView subtitle = new TextView(context);
        subtitle.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_CAPTION_SIZE);
        subtitle.setTextColor(DesignTokens.Palette.ON_SURFACE_MUTED);
        subtitle.setMaxLines(1);
        subtitle.setEllipsize(TextUtils.TruncateAt.END);
        subtitle.setIncludeFontPadding(false);
        subtitle.setText(recommendedInfo(track));
        text.addView(subtitle, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT));

        int target = DesignTokens.dp(DesignTokens.CONTROL_TARGET);
        IconButton add = new IconButton(context);
        add.setAction(Icon.ADD, "Add to queue");
        add.setOnClickListener(ignored -> actions.onEnqueueRecommended(track));
        item.addView(add, new LinearLayout.LayoutParams(target, target));

        String who = track.artist.isEmpty() ? track.channel : track.artist;
        item.setContentDescription(track.title
                + (who.isEmpty() ? "" : ", " + who)
                + ". Double tap to add to the queue.");
        item.setOnTouchListener(tabSwipe);
        item.setOnClickListener(ignored -> actions.onEnqueueRecommended(track));
        return item;
    }

    /**
     * Shows a decoded cover on the recommended row waiting for {@code identity}.
     *
     * <p>Refuses any other identity, so a decode that finishes after the list
     * moved on cannot paint the wrong cover.
     */
    void setRecommendedArtwork(String identity, android.graphics.Bitmap bitmap) {
        if (identity == null || identity.isEmpty() || bitmap == null) return;
        for (RecommendedRow row : recommendedRows) {
            if (identity.equals(row.shownCoverId)) row.cover.setCover(identity, bitmap);
        }
    }

    /** Attribution and duration for a recommended row, or "" when unknown. */
    private static String recommendedInfo(MediaTrack track) {
        String who = track.artist.isEmpty() ? track.channel : track.artist;
        if (track.durationMs > 0L) {
            String duration = TimeFormat.time(track.durationMs);
            return who.isEmpty() ? duration : who + " • " + duration;
        }
        return who;
    }

    /**
     * Paints the line being sung, if it changed.
     *
     * <p>Twenty times a second, so the comparison is a long against a long: with the
     * same line still in the view there is no {@code setText}, no measure and no
     * layout. Silence keeps whatever was last written instead of blanking the space
     * on every gap between phrases.
     */
    private void showLyricAt(long positionMs) {
        LyricTrack current = lyricTrack;
        if (current == null || current.isEmpty()) return;
        lyricPositionMs = positionMs;
        LyricTrack.Line line = current.activeAt(positionMs);
        if (line == null) {
            lyricLine.setVisibility(View.GONE);
            return;
        }
        lyricLine.setVisibility(View.VISIBLE);
        String text = line.text.trim();
        boolean singing = !text.isEmpty();
        String shown = singing ? text : DesignTokens.LYRICS_SILENCE;
        if (shown.equals(lyricLine.getText().toString())
                && lyricLine.getAlpha() == (singing ? 1f : LYRICS_SILENCE_ALPHA)) {
            return;
        }
        lyricLine.setText(shown);
        lyricLine.setAlpha(singing ? 1f : LYRICS_SILENCE_ALPHA);
    }

    // -------------------------------------------------------------- geometry

    /**
     * Adapts the whole surface to one window, in one call.
     *
     * <p>This is the only way a configuration change reaches this class, and it is one
     * call on purpose. The surface used to be told twice — "be landscape" and then "the
     * cover is N pixels" — and each of those ended in a {@code setLayoutParams}, so
     * one rotation was two layout passes over a card that holds the largest bitmap in
     * the app. Orientation, cover, gutters and column limits are one decision here,
     * applied together, and a window whose layout key did not move costs one string
     * comparison.
     *
     * <h3>What actually changes</h3>
     * <ul>
     *   <li><b>One column</b> (portrait phone): cover above, details below, both
     *       centred — the cover is the loudest thing and everything else is what is
     *       left.</li>
     *   <li><b>Two regions</b> (wide window, <em>and</em> a cover with more than one
     *       colour): cover on the left owning its share of the width, details on the
     *       right taking the rest, and the two sharing one vertical centre so they
     *       read as one composition instead of two blocks that happen to sit on the
     *       same row.</li>
     *   <li><b>Maximum width</b> (tablet): the column stops growing at
     *       {@link LayoutRules#CONTENT_MAX_DP} and centres, so the composition is
     *       comfortable instead of merely wide.</li>
     *   <li><b>Short window</b>: the sound bars give up the height they can spare, so
     *       the transport is never the thing that falls off the bottom.</li>
     * </ul>
     *
     * <p><b>Why the cover decides.</b> A two-region composition is a composition
     * about contrast: a cover here, the words there. A cover that is one colour — a
     * monochrome sleeve, a very dark frame — offers no contrast to build that on, and
     * the split only makes the screen look like two unrelated halves. So the split is
     * taken only when {@link ArtworkTheme#duoTone()} says the cover really brought two
     * tones; otherwise everything composes in one column, which is the calmer and
     * more legible arrangement for a single-colour cover. The cover decides, because
     * the cover is the loudest thing on the screen.
     *
     * @param profile     the window, already read from the configuration
     * @param innerWidthDp  width the card has inside the page gutter, in dp
     * @param innerHeightDp height the card has, in dp, or {@code 0} when unknown
     */
    void setWindow(LayoutRules.Profile profile, int innerWidthDp, int innerHeightDp) {
        if (profile == null) return;
        String key = profile.key() + "/" + innerWidthDp + "x" + innerHeightDp;
        if (key.equals(windowKey)) return;
        windowKey = key;
        window = profile;
        applySpacing(profile);
        // A short window is a landscape phone: the bars are the one block that can
        // give up height without losing meaning, and they are what pushes the
        // transport off the bottom.
        progress.setCompact(profile.shortWindow());

        // The cover keeps its share of the window and never leaves its bounds: a
        // window too small for the composition is a window problem, not a deformed
        // cover. Both numbers are decided here, once, and every listener downstream
        // reads the result instead of recomputing it.
        preferredArtworkSidePx = clampArtworkSide(
                DesignTokens.dp(profile.artworkSideDp(innerWidthDp, innerHeightDp)));
        setArtworkSidePx(preferredArtworkSidePx);
        applyComposition();
    }

    /**
     * One column or two, for the window this card was given.
     *
     * <p>Wide enough is necessary and not sufficient: the cover has to bring a second
     * tone for the split to mean anything. See {@link #setWindow}.
     */
    private boolean twoColumn() {
        return twoColumnFor(window != null && window.twoColumnPlayer(),
                ArtworkTheme.paletteDerived());
    }

    /**
     * The composition rule, as a pure function of the two facts it reads.
     *
     * <p>Wide enough is necessary and not sufficient: the cover has to have lent its
     * colour. A grey, black or white cover offers no contrast for a split composition
     * to be about, so that one composes in a single column — calmer and more legible
     * than two halves carrying the same ink.
     *
     * <p>It does <em>not</em> require two distinct hues. That was the first rule and
     * it was wrong: most sleeves are dominated by one hue with a neighbour inside the
     * 25° the palette calls "the same tone", so requiring two hues meant the split
     * almost never appeared — the feature existed and nobody ever saw it. What makes
     * the two regions worth having is a cover that is not grey.
     *
     * @param windowAllows whether the window is wide enough for two regions
     * @param coverHasColour whether the cover lent a palette of its own
     */
    static boolean twoColumnFor(boolean windowAllows, boolean coverHasColour) {
        return windowAllows && coverHasColour;
    }

    /**
     * Lays the two regions out, in whichever composition the window and the cover
     * between them ask for.
     *
     * <p>Separate from {@link #setWindow} because the cover can change its mind
     * without the window moving: the palette arrives with the artwork, and a track
     * change is not a configuration change. Idempotent on the pair, so a repeated
     * notification costs one string comparison.
     */
    private void applyComposition() {
        // A listener is registered at the very end of this constructor precisely so
        // this is unreachable, and it stays a guard rather than a fix: a card asked to
        // compose before it has built anything to compose with has nothing to say, and
        // a half-composed card is worse than a card that has not composed yet.
        if (stage == null || details == null || artwork == null) return;
        boolean columns = twoColumn();
        String key = (window == null ? "-" : window.key()) + "/" + columns
                + "/" + ArtworkTheme.duoTone();
        if (key.equals(compositionKey)) return;
        compositionKey = key;
        stage.setOrientation(columns ? LinearLayout.HORIZONTAL : LinearLayout.VERTICAL);
        // One vertical centre for the whole composition: in two regions the cover and
        // the words start from the same height and are centred against each other, so
        // neither can look like something that fell into place beside the other.
        stage.setGravity(columns
                ? Gravity.CENTER_HORIZONTAL | Gravity.CENTER_VERTICAL
                : Gravity.CENTER_HORIZONTAL);
        // The details column is the region that changes weight, not the cover: the
        // cover's width is a fact about the window and the details take the rest.
        LinearLayout.LayoutParams detailsParams =
                (LinearLayout.LayoutParams) details.getLayoutParams();
        detailsParams.width = columns ? 0 : -1;
        // Always wrap content, in one region and in two. A details column told to
        // fill the row is measured at the row's height — the cover's height — and a
        // LinearLayout squeezed to that height drops its LAST children, which are the
        // transport and the secondary actions: the two things the user came for. The
        // column grows to what it needs and the cover centres against it instead.
        detailsParams.height = -2;
        detailsParams.weight = columns ? 1f : 0f;
        detailsParams.leftMargin = columns
                ? DesignTokens.dp(DesignTokens.ARTWORK_GUTTER_LANDSCAPE) : 0;
        details.setGravity(Gravity.CENTER_HORIZONTAL
                | (columns ? Gravity.CENTER_VERTICAL : 0));
        details.setLayoutParams(detailsParams);
        // The cover's own margin depends on which composition it is in, so the
        // memoised side has to be re-applied rather than trusted.
        artworkSideKey = "";
        setArtworkSidePx(artworkSidePx);
    }

    @Override public void onPaletteChanged(int barsPrimary, int barsSecondary) {
        // The cover decided the composition; it has just answered.
        applyComposition();
    }

    /**
     * The accent colour moved.
     *
     * <p>Nothing here: the card does not paint itself with the accent, its controls
     * do, and each of them is already listening. This card only cares about whether
     * the cover brought two tones, which is {@link #onPaletteChanged}.
     */
    @Override public void onAccentChanged(int accent) { }

    /** Unhooks this card from the palette it listens to; called with the screen. */
    void release() { ArtworkTheme.unregister(this); }

    /** The window this surface is laid out for, or {@code null} before the first one. */
    LayoutRules.Profile window() { return window; }

    /**
     * Artwork side in PIXELS.
     *
     * <p>Public to the caller that measured the window, and to the card itself when a
     * short window has to give height back: the cover is the one thing on this
     * surface that may shrink, and shrinking it is not the same as recomposing.
     * Repeated calls with the same side and the same composition cost one comparison.
     */
    void setArtworkSidePx(int requestedPx) {
        int side = clampArtworkSide(requestedPx);
        String key = side + "/" + twoColumn();
        if (key.equals(artworkSideKey)) return;
        applyArtworkSide(side);
    }

    /** El lado pedido, dentro de los límites de la ventana y del mínimo del token. */
    private int clampArtworkSide(int requestedPx) {
        return Math.max(DesignTokens.dp(DesignTokens.ARTWORK_MIN),
                Math.min(DesignTokens.dp(window == null
                                ? DesignTokens.ARTWORK_MAX : window.artworkMaxDp()),
                        requestedPx));
    }

    /** El lado que la ventana pidió, para el reparto del aire que sobra. */
    int preferredArtworkSidePx() {
        return preferredArtworkSidePx > 0
                ? preferredArtworkSidePx : clampArtworkSide(artworkSidePx);
    }

    /** The cover's side, applied and remembered; {@code side} is already clamped. */
    private void applyArtworkSide(int side) {
        boolean columns = twoColumn();
        artworkSideKey = side + "/" + columns;
        artworkSidePx = side;
        LinearLayout.LayoutParams params = (LinearLayout.LayoutParams) artwork.getLayoutParams();
        params.width = side;
        params.height = side;
        params.bottomMargin = columns ? 0
                : DesignTokens.dp(DesignTokens.ARTWORK_GUTTER_PORTRAIT);
        artwork.setLayoutParams(params);
    }

    private TextView label(Context context, float sizeSp, int color) {
        TextView view = new TextView(context);
        view.setTextSize(TypedValue.COMPLEX_UNIT_SP, sizeSp);
        view.setTextColor(color);
        view.setIncludeFontPadding(false);
        // El texto se centra dentro de su propia caja: los hijos ocupan todo el
        // ancho, así que el centrado del padre no alinea las palabras, sólo la
        // caja, y la composición seguía leyéndose descentrada.
        view.setGravity(Gravity.CENTER_HORIZONTAL);
        view.setTextAlignment(View.TEXT_ALIGNMENT_CENTER);
        return view;
    }

    private LinearLayout row(Context context, int gravity) {
        LinearLayout view = new LinearLayout(context);
        view.setOrientation(LinearLayout.HORIZONTAL);
        view.setGravity(gravity);
        return view;
    }

    private void addDetails(View view, float gapDp) {
        addDetails(view, gapDp, -1);
    }

    /**
     * Adds a child to the details column.
     *
     * <p>The width is a parameter because a control must not be stretched by a
     * MATCH_PARENT default: a 48dp icon button given the full width became a 360dp
     * target whose glyph floated in the middle of it.
     */
    private void addDetails(View view, float gapDp, int width) {
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(width, -2);
        if (gapDp > 0f) {
            params.topMargin = DesignTokens.dp(gapDp);
            // Remembered so a wider window can open the gaps up in one pass. Air is
            // the cheapest thing to add on a big screen and the first thing that
            // looks cramped without it.
            spacedChildren.add(view);
            spacedGaps.add(gapDp);
        }
        details.addView(view, params);
    }

    /** Detail children that carry a top margin, and the token each one was built with. */
    private final ArrayList<View> spacedChildren = new ArrayList<>();
    private final ArrayList<Float> spacedGaps = new ArrayList<>();

    /**
     * Re-opens the gaps for a window with more room in it.
     *
     * <p>Only the air moves: the controls, the type and the cover keep their token
     * sizes, because a tablet is a bigger room and not a bigger person.
     */
    private void applySpacing(LayoutRules.Profile profile) {
        int extra = Math.round(profile.spacingStep() * LayoutRules.SPACING_STEP_DP);
        for (int i = 0; i < spacedChildren.size(); i++) {
            View view = spacedChildren.get(i);
            LinearLayout.LayoutParams params = (LinearLayout.LayoutParams) view.getLayoutParams();
            int wanted = DesignTokens.dp(spacedGaps.get(i)) + extra;
            if (params.topMargin == wanted) continue;
            params.topMargin = wanted;
            view.setLayoutParams(params);
        }
    }

    /**
     * Writes a label only when it would actually change.
     *
     * <p>{@code setText} on a TextView re-measures and re-lays-out; skipping it is the
     * difference between a notification that costs a comparison and one that costs a
     * layout pass.
     */
    private static void setText(TextView view, CharSequence value) {
        if (TextUtils.equals(view.getText(), value)) return;
        view.setText(value);
    }

    private static boolean equal(TrackKey left, TrackKey right) {
        return left == null ? right == null : left.equals(right);
    }

    private static boolean isWaiting(PlaybackState state) {
        return state == PlaybackState.BUFFERING
                || state == PlaybackState.RESOLVING_METADATA
                || state == PlaybackState.RESOLVING_SOURCE
                || state == PlaybackState.SEARCHING;
    }

    private static CharSequence attributionOf(MediaTrack track) {
        String who = track.artist.isEmpty() ? track.channel : track.artist;
        return who;
    }

    private static Icon downloadIcon(DownloadState.Phase phase) {
        switch (phase) {
            case COMPLETED:
            case ALREADY_DOWNLOADED: return Icon.DOWNLOADED;
            case FAILED:
            case CANCELLED: return Icon.RETRY;
            default: return Icon.DOWNLOAD;
        }
    }

    private static SecondaryActionButton.Phase downloadPhaseOf(DownloadState.Phase phase) {
        switch (phase) {
            case RESOLVING_SOURCE: return SecondaryActionButton.Phase.PENDING;
            case DOWNLOADING: return SecondaryActionButton.Phase.RUNNING;
            case COMPLETED:
            case ALREADY_DOWNLOADED: return SecondaryActionButton.Phase.DONE;
            case FAILED:
            case CANCELLED: return SecondaryActionButton.Phase.FAILED;
            default: return SecondaryActionButton.Phase.IDLE;
        }
    }

    /**
     * What the download action currently offers.
     *
     * <p>The glyph is {@link Icon#DOWNLOADED} once a file exists, and the action then
     * removes it: an icon-only control has to keep saying what its tap will do, or
     * cancelling and removing become indistinguishable (§8, §26).
     */
    private static CharSequence downloadDescription(DownloadState.Phase phase, int percent) {
        switch (phase) {
            case RESOLVING_SOURCE: return "Preparing download. Double tap to cancel.";
            case DOWNLOADING:
                return percent >= 0
                        ? "Downloading " + percent + " percent. Double tap to cancel."
                        : "Downloading. Double tap to cancel.";
            case COMPLETED:
            case ALREADY_DOWNLOADED: return "Downloaded. Double tap to remove the download.";
            case FAILED:
            case CANCELLED: return "Download failed. Double tap to try again.";
            default: return "Download. Double tap to start.";
        }
    }
}