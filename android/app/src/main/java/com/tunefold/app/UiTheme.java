package com.tunefold.app;

import android.content.Context;
import android.content.SharedPreferences;

/**
 * El tema que está puesto, y de quién es culpa que no sea otro.
 *
 * <p>Existe porque {@link DesignTokens.Palette} era una tabla de constantes
 * compiladas y el producto pide poder cambiar de tema en caliente. La tabla pasa a
 * ser mutable y este es el único que la escribe, de modo que «qué color hay ahora»
 * tiene exactamente una respuesta en toda la app.
 *
 * <h3>Por qué un cambio de tema reconstruye la pantalla</h3>
 * Hay dos maneras de cambiar el tema en vivo: repintar cada vista que cacheó un
 * color, o reconstruir la ventana. La primera es más fina en papel y en la
 * práctica son quince sitios que se pueden dejar sin refrescar —una cache de color
 * olvidada es un bug que sólo aparece en el tercer tema que se prueba—.
 * La segunda es una línea, siempre correcta, y el coste —una ventana— se paga
 * sobre una acción deliberada del usuario, no sobre un scroll ni sobre un cambio
 * de configuración. Se elige la segunda.
 *
 * <p>La reproducción no lo nota: la sesión vive en el servicio, y lo único que se
 * reconstruye es la vista que la refleja.
 */
final class UiTheme {

    private static final String PREFS = "tunefold.ui";
    private static final String KEY_THEME = "theme";

    private static volatile ThemeSpec current = ThemeSpec.DEFAULT;

    private UiTheme() { }

    /** El tema puesto ahora mismo. */
    static ThemeSpec current() {
        return current;
    }

    /** Cuántos temas hay; la lista de ajustes no inventa ninguno. */
    static int count() { return ThemeSpec.all().size(); }

    /**
     * Escribe un tema en la tabla de tokens.
     *
     * <p>No avisa a nadie y no persiste: hace exactamente una cosa, que es poner
     * los colores. Quien llama decide si hay que reconstruir y si hay que guardar.
     *
     * @return {@code true} si el tema era distinto del que ya estaba
     */
    static boolean write(ThemeSpec theme) {
        if (theme == null) return false;
        if (current == theme) return false;
        current = theme;
        DesignTokens.Palette.install(ThemePalette.resolve(theme));
        return true;
    }

    /** Escribe el tema por defecto sin tocar las preferencias. */
    static void writeDefault() {
        current = null;
        DesignTokens.Palette.install(ThemePalette.resolve(ThemeSpec.DEFAULT));
        current = ThemeSpec.DEFAULT;
    }

    /**
     * Lee el tema guardado y lo pone.
     *
     * <p>Se llama una vez, antes de construir nada: desde ese momento
     * {@link DesignTokens.Palette} vale para el tema del usuario y ninguna vista
     * nace con un color que haya que corregir después.
     *
     * @param context el contexto del que sacar las preferencias
     */
    static void load(Context context) {
        writeDefault();
        if (context == null) return;
        SharedPreferences prefs = context.getSharedPreferences(PREFS,
                Context.MODE_PRIVATE);
        String id = prefs.getString(KEY_THEME, null);
        write(ThemeSpec.byId(id));
    }

    /**
     * Guarda un tema para la próxima vez que arranque la app.
     *
     * <p>El valor se guarda siempre, aunque no haya cambiado: quien elige un tema
     * espera que se le quede, y el caso «ya era el elegido» no puede pasar nada
     * por escribirlo.
     */
    static void persist(Context context, ThemeSpec theme) {
        if (context == null || theme == null) return;
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
                .putString(KEY_THEME, theme.id).apply();
    }

    // ------------------------------------------------------- other settings

    private static final String KEY_NAME = "display_name";

    /**
     * El nombre que el usuario ha puesto, o la cadena vacía.
     *
     * <p>Un nombre, no una cuenta: no se pide contraseña, no se manda a ningún
     * sitio y no hay con quién sincronizarlo. Se usa para saludar y para nada
     * más, porque inventarle un uso sería inventar una función.
     */
    static String displayName(Context context) {
        if (context == null) return "";
        return context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
                .getString(KEY_NAME, "");
    }

    /** Guarda el nombre; una cadena en blanco lo borra. */
    static void persistDisplayName(Context context, String name) {
        if (context == null) return;
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
                .putString(KEY_NAME, name == null ? "" : name.trim()).apply();
    }
}