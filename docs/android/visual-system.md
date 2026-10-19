# Tunefold Android — sistema visual

Este documento es la fuente canónica de **por qué** la interfaz de Android se ve
como se ve. Los valores viven en el código y sólo en un sitio:
`android/app/src/main/java/com/tunefold/app/DesignTokens.java` (dimensiones y
roles de color) y `ArtworkPalette.java` (extracción). Este documento explica las
decisiones; si el código y este documento discrepan, el código gana y hay que
arreglar el documento.

---

## 1. Los temas

No hay una paleta de marca. Hay **seis temas**, cada uno de cinco colores, y el
código resuelve a partir de esos cinco todo lo que la interfaz necesita. Los cinco
colores son la materia prima; los roles se calculan.

`ThemeSpec` es el catálogo y `ThemePalette.resolve` es el resolvedor. El tema por
defecto es `STAGE_SPOTLIGHT`.

| Tema | id | Colores, en el orden que declara el catálogo |
|---|---|---|
| Stage Spotlight | `stage_spotlight` | `#111827 #F59E0B #FDE68A #EF4444 #F9FAFB` |
| Jazz Loounge | `jazz_loounge` | `#2B1D1A #7C2D12 #D97706 #F3E8D4 #1C1917` |
| Drumline Concrete | `drumline_concrete` | `#111827 #374151 #9CA3AF #F97316 #F3F4F6` |
| Indie Dusk | `indie_dusk` | `#2D1B69 #4B5563 #FCA5A5 #F3F4F6 #111827` |
| Opera Violet | `opera_violet` | `#4C0519 #9F1239 #F1C40F #F5F3FF #1F2937` |
| Choir Paste | `choir_paste` | `#FCE7F3 #DDD6FE #C7F9CC #BAE6FD #1F2937` |

`ThemePaletteTest` falla la build si un tema deja de cumplir el contrato de
contraste de abajo. Los roles (`ACCENT`, `SURFACE_CARD`, `ON_SURFACE_FAINT`…) ya
no son constantes: son campos que `UiTheme.load` instala al arrancar, desde el
tema guardado.

### 1.1 Cómo se decide el campo

El campo es el color más luminoso o el más apagado, según si el tema es oscuro.
La regla no es «el primero de la lista»: es la que sobrevive a los seis temas sin
que ninguno salga con dos colores que separe un pelo.

### 1.2 La escalera de superficies

La escalera va **en las dos direcciones**, y en cada tema **en el sentido que le
corresponde**:

- **Elevada**: un paso hacia la tinta respecto del campo.
- **Pozo**: la tarjeta donde se dibujan las barras y la línea de posición. En un
  tema oscuro es una placa llevada hacia el negro; en un tema claro es **una
  placa levantada, no un pozo**.
- **Superpuesta**: un paso corto hacia la tinta.

El pozo importa más de lo que parece. Es donde se dibujan las barras en vivo y la
línea de posición, y esos tienen que sobrevivir a 3:1 contra él. Una tarjeta
meramente elevada no podía: un azul profundo o un carmesí saturados no llegaban a
3:1 en ningún valor, la extracción renunciaba y devolvía el acento de marca — tres
fundas distintas salían del mismo dorado.

Y el pozo **no se puede invertir sin romper la tinta**. La versión clara de la
tarjeta es una placa levantada por una razón concreta: sobre un campo pálido, una
tarjeta casi negra con la tinta del tema encima deja a las dos en el mismo negro,
que es un 1:1 exacto. En un tema claro la tinta es oscura, así que la tarjeta
tiene que ser clara; y no hace falta oscurecer las fundas, porque contra una
superficie clara un azul profundo o un carmesí saturados llegan a 3:1 sin tocar
nada.

### 1.3 El acento

El acento marca **lo que el usuario hace**. Dos garantías, y las dos están
comprobadas para los seis temas:

- **≥ 3:1 contra todas las superficies donde se pinta.** Es lo que necesita un
  glifo, un anillo o un control relleno.
- **El acento no lleva palabras** salvo en el campo de la página y lo que se
  apoya en él. Para texto existe `ACCENT_TEXT`, reparado a 4,5:1.

La reparación se hace **contra varias superficies a la vez y en varias vueltas**
(`ThemePalette.onAll`). Reparar en una sola pasada no vale, y no por falta de
iteraciones sino por el orden: subir el brillo para leerse sobre una superficie
clara puede quitárselo de la oscura que venía después, y el resultado cumple la
última de la lista y falla la primera.

### 1.4 La tinta

La tinta es uno de los cinco colores, reparado a 4,5:1 contra **todas** las
superficies donde aparece. No es negro: el glifo de reproducir vive sobre un disco
de acento y negro sobre acento es justo el emparejamiento que haría que esta
pantalla pareciera un diálogo del sistema.

### 1.5 Estados funcionales

`ERROR`, `POSITIVE` y el estado «trabajando» se toman del conjunto de colores del
tema cuando hay uno a menos de 45° del matiz de referencia; si no, se sintetizan
girando el acento. Se reparan a 4,5:1 (el «trabajando», a 3:1, porque no es texto
que se lea de reojo).

Una garantía más: **un estado nunca puede acabar siendo el acento**. La reparación
por contraste puede llevar dos colores que empezaron distintos al mismo sitio, y
entonces «descargado» y «sonando» salían pintados igual. Cuando eso ocurre
(`ThemePalette.apart`) el color se rehace girándolo lejos del acento.

### 1.6 La pulsación

`DesignTokens.pressedAccent(accent, onAccent)` no tiene dirección fija. La decide
el glifo que el disco lleva encima, porque el gesto tiene que **apartar** el disco
de ese glifo. Una regla «siempre aclara» leía bien en los temas oscuros y enterraba
el glifo en los claros, donde la tinta es casi negra y oscurecer el disco lo
acerca a él en vez de alejarlo. Se prueban las dos direcciones y se queda con la
que más separa las dos cosas, y además hay que reparar la pulsación contra el
propio `onAccent` para que una pulsación no tape lo único que el disco dice.

---

## 2. La capa dinámica: la portada manda el color

La cadena del producto es **PORTADA → PALETA EXTRAÍDA → COMPOSICIÓN →
VISUALIZACIÓN → REPRODUCCIÓN**, y el eslabón que la cierra es
`ArtworkPalette` + `ArtworkTheme`. El eslabón ya existía y se ha **ampliado**, no
sustituido.

### 2.1 Extracción

`ArtworkPalette.derive` reparte los píxeles de una reducción 32×32 en doce cubos
de matiz y elige los dos mejor ponderados por recuento y saturación. Reglas:

- un píxel transparente no vota;
- saturación < 0,40 no es color (esto es lo que echa el blanco y el gris claro);
- valor < 0,28 no tiene color que prestar;
- **no hay techo de valor.** Hubo uno a 0,96 y era un error: una naranja o un
  amarillo puros (valor 1,0) se descartaban píxel a píxel, la extracción se
  quedaba sin nada y devolvía la marca. El brillo de verdad ya se cae por la
  saturación;
- dos tonos a menos de 25° son un tono, y se dice así (`duoTone == false`).

### 2.2 Reparación por contraste

`readableOnCard` tiene que llevar el color a 3:1 sobre la tarjeta. Primero sube
el valor (devuelve el color con su croma intacta). Si eso no basta —y para un azul
o un carmesí muy saturados no basta, porque subir el valor no sube el canal del
medio lo suficiente— baja el croma con el matiz intacto y un suelo de 0,45.

Ese segundo paso es exactamente lo que faltaba: sin él, la combinación de croma y
valor caía al final de la función y devolvía el oro de marca. Tres portadas
azules, carmesí y púrpuras salían las tres doradas. Con el croma bajando
despiacio el matiz sobrevive, que es lo que `BrandPaletteTest` comprueba.

### 2.3 Fallback

Sin portada, con portada gris, negra o blanca, o si el color no llega al
contraste: **el acento del tema en uso**. `ArtworkPalette.Palette.brand`. Nunca un
color inventado.

Ese acento se lee en el momento de usarlo y no en un `static final`: como
constante congelaba lo que la paleta tuviera al inicializarse la clase, que es el
tema por defecto, y los otros cinco caían en el acento de un tema que no era el
suyo. Lo mismo pasaba con `DesignTokens.BOTTOM_NAV_SURFACE`, que se quedó con el
campo del primer tema: una página pálida apoyada en una barra negra, con los pies
de navegación ilegibles encima.

---

## 3. Tipografía y composición

La escala es `DISPLAY 26 / TITLE 20 / BODY 15 / LABEL 13 / CAPTION 11` (sp). El
nivel de display existí­a como token y no se usaba en ninguna parte: todas las
pantallas empezaban con el mismo cuerpo de texto y la jerarquía se fingía con el
peso. Ahora lo usa el **masthead** y el tipo de marca del reproductor.

- El tipo de marca se pone en **mayúsculas espaciadas hacia fuera**
  (`TYPE_DISPLAY_TRACKING = 0.16`), no cerradas: es el nombre de una funda, no el
  título de una barra del sistema.
- Los encabezados de bloque son versalitas con `TYPE_LABEL_TRACKING` y una regla
  fina debajo: la página impresa separa sus secciones con una línea, y sin ella
  cuatro bloques apilados son cuatro listas y no una página.
- Los pies de navegación y las pestañas de repisa se encogen dentro de su propio
  rango para caber en su hueco. `setAutoSizeTextTypeUniform…` no sirve aquí porque
  una fila ponderada mide a sus hijos con `EXACTLY`; `ShelfTabBar.fitCaptions`
  hace la regla a mano, donde el número sí se conoce.

### 3.1 Reglas

`RULE_THICKNESS = 2dp` para todas las reglas impresas: la del masthead, la del
encabezado, la de la pestaña seleccionada, la de la pestaña seleccionada del
reproductor, la del mini reproductor. Una regla más fina desaparece contra una
portada; más gruesa deja de ser una regla y pasa a ser un borde.

---

## 4. Las tres marcas de «estás aquí»

El color no puede ser el único indicador de estado. Cada superficiereutiliza tres:

| Superficie | Marca de posición | Marca de color |
|---|---|---|
| Navegación inferior | regla de oro sobre el glifo | glifo y pie en `ACCENT_TEXT` |
| Repisas del reproductor | regla de acento sobre la pestaña | pie en `ON_SURFACE` |
| Fila de lista en reproducción | el peso del título | la acción de la fila toma el acento |

Las tres reglas usan el mismo grosor y la misma forma, así que «estás aquí» es
una idea y no tres.

---

## 5. Adaptación

`LayoutRules` es aritmética pura y comprobable sin dispositivo
(`LayoutRulesTest`).

- **La portada manda en vertical**: 84 % del ancho, con un techo del 42 % del alto.
- **Dos regiones sólo si caben las dos**: `twoColumnPlayer()` exige
  `heightDp >= 600` además de ancho. Medido en un dispositivo en horizontal, con
  la partición activa la tarjeta quedaba mucho más alta que la pantalla con la
  portada centrada dentro: los datos arriba, la portada media pantalla más abajo,
  y el espacio al lado de los datos desperdiciado. Una columna con una portada
  que cede alto es la disposición que sí se lee ahí.
- **Una ventana corta** cede la letra (`NowPlayingView.lyricVisible`), que es la
  única línea de esa pantalla que es contexto y no un valor ni una acción. El
  reproductor, la posición, el transporte y las acciones no se tocan nunca.
- El ancho máximo de columna (560dp) es lo que impide que una tableta convierta
  la tarjeta en una banda panorámica.

---

## 6. Reglas de rendimiento que se tocaron

Nada aquí es una hipótesis; cada punto se cambió por una razón medible o por un
defecto observado.

- **La extracción de color ocurre en el hilo de la portada**, contra una
  reducción 32×32, una vez por funda. `ProviderRegistry.applyDerivedAccent`.
- **`ArtworkTheme.setPalette` es un no-op si nada se movió**, así que una funda
  monocroma no repinta nada.
- **Las barras sólo redibujan si hay audio nuevo** (`LiveAudioBars.poll`
  compara generaciones). En reposo el coste es cero.
- **`SoundBarsView.build` no reserva memoria**: dos `Path` y un `float[2]` son
  campos, no locales.
- **La geometría de las barras se mide una vez por tamaño**, en
  `onSizeChanged`, no por fotograma.
- **Sin elevación en el mini reproductor.** Su regla superior hace el trabajo de
  la sombra, que además se derramaba sobre la barra de navegación y ensuciaba la
  marca de la pestaña activa. Eliminar la sombra quita también el pase de render
  extra que cuesta una vista elevada.
- **`ShelfTabBar.fitCaptions` está memorizado** por ancho de hueco: un pase de
  medida corre en cada layout por scroll y la respuesta sólo se mueve con la
  ventana.

---

## 7. Los ajustes

`SettingsSurface` es la quinta sección, con `UiTheme` detrás.

- **Nombre**: un campo, sin cuenta ni contraseña, que sólo se usa para saludar en
  el masthead. El nombre es un final, no una frase nueva: se añade a la palabra que
  dio la hora, así que cambiar el nombre no puede cambiar el momento del día y uno
  vacío deja el saludo exactamente como estaba. No se manda a ningún sitio.
- **Tema**: seis filas, cada una con sus cinco colores y su nombre. El que está
  puesto lo dice con un **visto**, no sólo con el color. Cambiarlo reinicia la
  Activity, porque casi todas las vistas leen su color al construirse; por eso la
  sección activa se guarda en el estado de la instancia, para no devolver al
  usuario a Home justo después de mirar un tema.
- **Proveedor**: el estado real de la feature YouTube
  (`BuildConfig.YOUTUBE_ENABLED`). **No hay interruptor**, y no es una.carencia:
  los símbolos del proveedor se compilan dentro de la app, así que ningún
  interruptor en marcha podría activarlo. La pantalla lo dice y explica cómo se
  activa, en lugar de enseñar un interruptor que no hace nada.

### 7.1 Las barras del sistema

Un tema claro pone el campo pálido detrás del reloj, y la barra de navegación del
sistema también. `MainActivity.paintSystemBarIcons` decide el color de los iconos
con la misma regla que el resto de la paleta: un campo pálido quiere iconos
oscuros. En API 35 los colores dejan de valer (borde a borde) pero los iconos no,
así que la llamada a los iconos no depende del nivel. Sin esto, cinco de los seis
temas mostraban un reloj blanco sobre el campo claro.

## 8. Lo que queda por verificar en un dispositivo

Ninguna de estas comprobaciones se puede hacer sin hardware y **no se han
hecho**:

- Escalado de fuente grande (1,3× / 1,5× / 2×): los pies y las pestañas se
  encogen dentro de su rango, pero el comportamiento exacto del reproductor con
  letra de tres líneas no está verificado.
- Lectores de pantalla: las etiquetas están escritas y las unidades de
  navegación son un nodo cada una, pero no se ha pasado TalkBack por la app.
- Un tablet real (forma `FORM_EXPANDED`): las reglas están comprobadas en pruebas
  y la partición de dos regiones sigue activa por altura, pero no se ha visto en
  una pantalla de ese tamaño.
- Notificación y pantalla de bloqueo, que usan el mismo acento pero no se han
  revisado en este trabajo.