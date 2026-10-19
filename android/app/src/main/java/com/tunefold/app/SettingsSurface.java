package com.tunefold.app;

import android.content.Context;
import android.text.InputType;
import android.text.TextUtils;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.view.inputmethod.EditorInfo;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.TextView;

import java.util.ArrayList;
import java.util.List;

/**
 * Ajustes: el nombre, el tema y el estado del proveedor.
 *
 * <p>Tres secciones y nada más. Cada una existe porque hay algo real que cambiar
 * o que saber, y cada fila hace una de esas dos cosas: <b>o</b> cambia algo, <b>o</b>
 * dice la verdad sobre el estado de algo. Una fila que no hace ninguna de las dos
 * no está aquí.
 *
 * <ul>
 *   <li><b>Nombre</b>: cómo te llamamos. Sin cuenta, sin contraseña, sin sitio al
 *       que mandarlo — sólo el saludo de la portada, porque inventarle un uso
 *       sería inventar una función.</li>
 *   <li><b>Tema</b>: los seis, con sus cinco colores a la vista. Tocar uno lo pone;
 *       el que está puesto lo dice con un visto, no sólo con el color.</li>
 *   <li><b>Proveedor</b>: si YouTube está activo en esta compilación, y si no lo
 *       está, por qué. <b>No hay interruptor</b>, y esa ausencia es deliberada: la
 *       función es de compilación —los símbolos nativos existen o no existen— y un
 *       interruptor que no puede hacer nada es un interruptor que miente. La fila
 *       lo dice y dice cómo se activa.</li>
 * </ul>
 *
 * <p>El cambio de tema reconstruye la ventana en lugar de repintarla, y el motivo
 * está en {@link UiTheme}. El nombre no reconstruye nada: sólo cambia dos
 * cadenas, y la portada las vuelve a leer en su siguiente render.
 */
@android.annotation.SuppressLint("ViewConstructor")
final class SettingsSurface extends SectionSurface {

    /** Lo que la pantalla tiene que hacer por esta pantalla. */
    interface Host extends SectionSurface.Host {
        /** Un tema distinto al puesto. */
        void onThemeChosen(ThemeSpec theme);

        /** El nombre ha cambiado; la portada vuelve a leerlo. */
        void onDisplayNameChanged(String name);
    }

    private final SettingsSurface.Host host;
    private final EditText nameField;
    private final LinearLayout themes;
    private final List<ThemeRow> themeRows = new ArrayList<>();

    SettingsSurface(Context context, SettingsSurface.Host host) {
        super(context, host);
        this.host = host;
        setOrientation(VERTICAL);

        // ---- nombre -----------------------------------------------------------
        addView(header("Nombre"), spaced(DesignTokens.SECTION_GAP));

        nameField = new EditText(context);
        nameField.setSingleLine(true);
        nameField.setHint("Cómo te llamamos");
        nameField.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_BODY_SIZE);
        nameField.setTextColor(DesignTokens.Palette.ON_SURFACE);
        nameField.setHintTextColor(DesignTokens.Palette.ON_SURFACE_FAINT);
        nameField.setInputType(InputType.TYPE_CLASS_TEXT
                | InputType.TYPE_TEXT_FLAG_CAP_WORDS);
        nameField.setImeOptions(EditorInfo.IME_ACTION_DONE);
        nameField.setBackground(MainActivity.roundDrawable(
                DesignTokens.Palette.SURFACE_RAISED, DesignTokens.RADIUS_MEDIUM));
        // Se guarda al terminar y al perder el foco, y en ningún otro momento: un
        // guardado por pulsación es un guardado que se pierde, y uno por cambio es
        // un guardado que escribe mientras se teclea.
        nameField.setOnEditorActionListener((view, actionId, event) -> {
            if (actionId == EditorInfo.IME_ACTION_DONE) {
                commitName();
                return true;
            }
            return false;
        });
        nameField.setOnFocusChangeListener((view, focused) -> {
            if (!focused) commitName();
        });
        nameField.setContentDescription("Tu nombre");
        LinearLayout.LayoutParams nameParams = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                DesignTokens.dp(DesignTokens.SEARCH_FIELD_HEIGHT));
        nameParams.topMargin = DesignTokens.dp(DesignTokens.SPACE_3);
        addView(nameField, nameParams);

        TextView nameHint = caption(context,
                "Se guarda en este dispositivo. Pulsa Intro para confirmar.");
        LinearLayout.LayoutParams hintParams = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT);
        hintParams.topMargin = DesignTokens.dp(DesignTokens.SPACE_2);
        addView(nameHint, hintParams);

        // ---- tema --------------------------------------------------------------
        addView(header("Tema"), spaced(DesignTokens.SECTION_GAP));

        themes = new LinearLayout(context);
        themes.setOrientation(VERTICAL);
        addView(themes, spaced(DesignTokens.SPACE_3));
        for (ThemeSpec theme : ThemeSpec.all()) {
            ThemeRow row = new ThemeRow(context, theme);
            themeRows.add(row);
            themes.addView(row.view,
                    new LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT,
                            LinearLayout.LayoutParams.WRAP_CONTENT));
        }

        // ---- proveedor ----------------------------------------------------------
        addView(header("Proveedor"), spaced(DesignTokens.SECTION_GAP));

        boolean youtube = BuildConfig.YOUTUBE_ENABLED;
        addView(statusRow(context, "YouTube", youtube ? "Activa" : "Inactiva", youtube),
                spaced(DesignTokens.SPACE_3));
        if (!youtube) {
            // La explicación va con la fila, no aparte: un estado que necesita un
            // párrafo para entenderse está mal explicado, y un párrafo suelto al
            // lado de la fila se lee como otra cosa.
            TextView why = caption(context,
                    "Es una función de compilación: los símbolos que reproduce el "
                            + "proveedor se compilan dentro de la app, así que no hay "
                            + "ningún interruptor que pueda activarla en marcha. Se "
                            + "activa compilando con la opción de Gradle "
                            + "tunefoldYoutube.");
            LinearLayout.LayoutParams whyParams = new LinearLayout.LayoutParams(
                    LinearLayout.LayoutParams.MATCH_PARENT,
                    LinearLayout.LayoutParams.WRAP_CONTENT);
            whyParams.topMargin = DesignTokens.dp(DesignTokens.SPACE_2);
            addView(why, whyParams);
        }
    }

    @Override void onEnter() { applySelection(); }

    /** Esta pantalla no muestra pistas, así que no hay fila que pueda estar sonando. */
    @Override void setActiveKey(TrackKey key) { }

    @Override void onShow() {
        nameField.setText(UiTheme.displayName(getContext()));
        applySelection();
    }

    /** El campo de texto, para que la pantalla pueda pedirle el foco al llegar. */
    EditText nameField() { return nameField; }

    private void commitName() {
        String value = nameField.getText().toString().trim();
        if (value.equals(UiTheme.displayName(getContext()))) return;
        UiTheme.persistDisplayName(getContext(), value);
        host.onDisplayNameChanged(value);
    }

    /** Pinta el tema que está puesto, que es el único que lleva el visto. */
    private void applySelection() {
        ThemeSpec chosen = UiTheme.current();
        for (ThemeRow row : themeRows) {
            row.setChosen(row.theme == chosen);
        }
    }

    // ------------------------------------------------------------- row pieces

    private SectionHeader header(String title) {
        return new SectionHeader(getContext(), title);
    }

    private static TextView caption(Context context, String text) {
        TextView view = new TextView(context);
        view.setText(text);
        view.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_CAPTION_SIZE);
        view.setTextColor(DesignTokens.Palette.ON_SURFACE_MUTED);
        view.setLineSpacing(0f, 1.15f);
        return view;
    }

    /** Una fila que dice qué cosa es y en qué estado está. */
    private View statusRow(Context context, String title, String value, boolean live) {
        LinearLayout row = row(context);
        TextView label = label(context, title);
        row.addView(label, new LinearLayout.LayoutParams(0,
                LinearLayout.LayoutParams.WRAP_CONTENT, 1f));
        TextView state = label(context, value);
        state.setTextColor(live ? DesignTokens.Palette.POSITIVE
                : DesignTokens.Palette.ON_SURFACE_MUTED);
        state.setLetterSpacing(DesignTokens.TYPE_LABEL_TRACKING);
        row.addView(state, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT,
                LinearLayout.LayoutParams.WRAP_CONTENT));
        row.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_YES);
        row.setContentDescription(title + ": " + value);
        return row;
    }

    private static LinearLayout row(Context context) {
        LinearLayout view = new LinearLayout(context);
        view.setOrientation(HORIZONTAL);
        view.setGravity(Gravity.CENTER_VERTICAL);
        return view;
    }

    private static TextView label(Context context, String text) {
        TextView view = new TextView(context);
        view.setText(text);
        view.setTextSize(TypedValue.COMPLEX_UNIT_SP, DesignTokens.TYPE_BODY_SIZE);
        view.setTextColor(DesignTokens.Palette.ON_SURFACE);
        view.setMaxLines(2);
        view.setEllipsize(TextUtils.TruncateAt.END);
        view.setSingleLine();
        view.setIncludeFontPadding(false);
        return view;
    }

    private LinearLayout.LayoutParams spaced(float gapDp) {
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT);
        if (gapDp > 0f) params.topMargin = DesignTokens.dp(gapDp);
        return params;
    }

    /**
     * Un tema en una fila: sus colores, su nombre y el visto del que está puesto.
     *
     * <p>El visto no es decoración: el color es el acento —y con la portada de
     * encima puede ser cualquier cosa—, así que la fila que está elegida tiene que
     * decirlo por otra vía. Y la fila entera es el objetivo, no el texto ni las
     * muestras, porque es lo que apunta el dedo.
     */
    private final class ThemeRow {
        final ThemeSpec theme;
        final LinearLayout view;
        private final ThemeSwatchView swatch;
        private final IconButton check;

        ThemeRow(Context context, ThemeSpec theme) {
            this.theme = theme;
            view = row(context);
            view.setMinimumHeight(DesignTokens.dp(DesignTokens.TOUCH_TARGET_MIN));
            view.setClickable(true);
            view.setFocusable(true);
            view.setPadding(0, DesignTokens.dp(DesignTokens.SPACE_2), 0,
                    DesignTokens.dp(DesignTokens.SPACE_2));
            view.setBackground(IconButton.ripple(DesignTokens.Palette.RIPPLE, false));

            int sampleWidth = DesignTokens.dp(DesignTokens.ICON_SIZE_EXTRA_LARGE * 2f);
            swatch = new ThemeSwatchView(context, theme);
            LinearLayout.LayoutParams swatchParams = new LinearLayout.LayoutParams(
                    sampleWidth, DesignTokens.dp(DesignTokens.ICON_SIZE_MEDIUM));
            swatchParams.rightMargin = DesignTokens.dp(DesignTokens.GAP);
            view.addView(swatch, swatchParams);

            TextView name = label(context, theme.name);
            view.addView(name, new LinearLayout.LayoutParams(0,
                    LinearLayout.LayoutParams.WRAP_CONTENT, 1f));

            // En dp, como los dos argumentos del constructor: dp() se aplica dentro.
            // Pasarlo ya convertido hace un inset tres veces mayor que el control, y
            // el resultado es un botón presente en la jerarquía y vacío en la
            // pantalla, que es la forma más difícil de ver de un fallo de pintado.
            check = new IconButton(context, DesignTokens.ICON_SIZE_EXTRA_LARGE,
                    DesignTokens.ICON_SIZE_SMALL);
            check.setAction(Icon.CHECK, theme.name);
            view.addView(check, new LinearLayout.LayoutParams(
                    DesignTokens.dp(DesignTokens.ICON_SIZE_EXTRA_LARGE),
                    DesignTokens.dp(DesignTokens.ICON_SIZE_EXTRA_LARGE)));

            view.setOnClickListener(ignored -> choose());
            setChosen(theme == UiTheme.current());
        }

        void choose() {
            if (!UiTheme.write(theme)) return;
            UiTheme.persist(getContext(), theme);
            SettingsSurface.this.host.onThemeChosen(theme);
        }

        /** Pone o quita el visto, y lo dice también con la descripción. */
        void setChosen(boolean chosen) {
            check.setEnabled(chosen);
            check.setVisibility(chosen ? View.VISIBLE : View.INVISIBLE);
            view.setContentDescription(chosen
                    ? theme.name + ", puesto" : "Poner el tema " + theme.name);
        }
    }
}