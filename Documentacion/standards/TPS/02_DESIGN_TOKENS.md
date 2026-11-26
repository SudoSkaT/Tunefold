# 02 · Design tokens

## Fuente de verdad

Android: `android/app/src/main/java/com/tunefold/app/DesignTokens.java`; colores resueltos por `ThemePalette` y actualizados por `UiTheme`. Este catálogo registra los valores observados en 0.19.6. Al cambiar código, actualizar la referencia y los tests. dp/sp son unidades lógicas; valores no son automáticamente recomendación para nuevas superficies.

| Identificador | Valor vigente | Propósito/contexto | Restricción |
|---|---:|---|---|
| `SPACE_1..9` | 2, 4, 6, 8, 12, 16, 20, 24, 32 dp | espaciado | usar escala o alias semántico |
| `GAP_TIGHT/GAP/BLOCK_GAP/SECTION_GAP/REGION_GAP` | 6/8/12/16/20 dp | separación semántica | preferir alias por relación |
| `CONTENT_PADDING_HORIZONTAL/TOP/BOTTOM` | 20/8/32 dp | margen página | considerar ventana compacta |
| `CARD_PADDING/ROW_PADDING_HORIZONTAL/ROW_GAP` | 16/6/8 dp | interior tarjeta/fila | no duplicar padding del padre |
| `RADIUS_SMALL/MEDIUM/LARGE/PILL/ARTWORK_THUMB` | 8/14/22/999/10 dp | chips, superficies, arte | pill sólo para forma cápsula |
| `RULE_THICKNESS/NAV_INDICATOR_WIDTH` | 2/18 dp | regla editorial/selección | misma regla para indicadores afines |
| `ELEVATION_CARD/RAISED/OVERLAY` | 6/10/18 dp | capas | usar sólo con jerarquía real |
| `ICON_GRID/ICON_STROKE` | 24/2 | autoría de iconos | stroke consistente |
| `ICON_SIZE_EXTRA_SMALL/SMALL/MEDIUM/LARGE/EXTRA_LARGE` | 16/20/24/32/40 dp | glifos | no confundir con target táctil |
| `TOUCH_TARGET_MIN` | 48 dp | interacción | suelo obligatorio |
| `CONTROL_TARGET/SECONDARY/PRIMARY/NAVIGATION/OVERFLOW` | 48/48/64/56/40 dp | controles | Overflow de 40 dp es excepción existente; objetivo TPS futuro ≥48 dp y debe revisarse en implementación |
| `ROW_HEIGHT/SEARCH_FIELD_HEIGHT/TOP_BAR_HEIGHT` | 68/52/48 dp | fila/búsqueda/cabecera | validar con texto ampliado |
| `TIME_LABEL_WIDTH/ROW_DURATION_WIDTH/ROW_STATUS_HEIGHT` | 52 dp/44 dp/20 dp | datos alineados | no truncar datos sin alternativa accesible |
| `BOTTOM_NAV_HEIGHT` | 60 dp | navegación | no invadir contenido |
| `MINI_PLAYER_HEIGHT/ARTWORK/GAP` | 60/44/6 dp | mini player | conservar control táctil accesible |
| `LIST_PREFETCH_ROWS` | 3 filas | ventana de lista | revisar sólo con perfilado |
| `PROGRESS_BAR_HEIGHT` | 6 dp | progreso | no implica target táctil; control padre ≥48 dp |
| `SOUNDBARS_HEIGHT/COMPACT` | 36/26 dp | visualización | el bloque cede espacio antes que controles |
| `SOUNDBARS_BAR_PITCH/GAP/HEAD_SIZE` | 4/2/10 dp | geometría | geometría sólo al cambiar tamaño |
| `PROGRESS_LINE_THICKNESS` | 2 dp | línea de progreso | conservar separación visual |
| `ARTWORK_MIN/MAX/THUMB` | 160/420/56 dp | tamaño de portada | adaptable al perfil |
| `ARTWORK_GUTTER_LANDSCAPE/PORTRAIT` | 16/8 dp | separación portada/detalle | layout responsivo |
| `ARTWORK_*_WIDTH_RATIO` | portrait .84; landscape .42 | composición | limitar también por altura |
| `ARTWORK_*_HEIGHT_RATIO` | portrait .42; landscape .88 | composición | no ocultar transporte |
| `ARTWORK_DECODE_MAX_PX/SAMPLE_PX` | 640/32 px | bitmap/extracción | no elevar sin evidencia de memoria |
| `TYPE_DISPLAY/TITLE/BODY/LABEL/CAPTION/MONOSPACE_SIZE` | 26/20/15/13/11/11 sp | jerarquía tipográfica | respetar escalado de fuente |
| `TYPE_*_LINE` display/title/body/label/caption | 32/26/20/16/14 dp | caja de línea | comprobar multi-línea |
| `TYPE_DISPLAY_TRACKING/LABEL_TRACKING` | .16/.06 em | voz editorial | sólo niveles semánticos definidos |
| `STATE_DOT_SIZE` | 14 dp | estado | siempre acompañar con estado accesible |
| `DURATION_FAST/BASE/SLOW` | 90/150/260 ms | feedback/estado/superficie | respetar reducción de movimiento |
| `VISUAL_INTERVAL_MS/POSITION_INTERVAL_MS` | 50/200 ms | lectura visual/posición | cadencias independientes, no fps medido |
| `SESSION_REFRESH_INTERVAL_MS/STATE_SEGMENT_INTERVAL_MS` | 500/400 ms | snapshot de sesión/indicador indeterminado | sólo con surface lifecycle activo |
| `LYRIC_LEAD_MS` | 475 ms | compensación sólo de letra | no aplicarla a posición/datos del motor |
| `NAV_CAPTION_SIZE/NAV_CAPTION_MIN_SIZE_SP/SHELF_TAB_MIN_SIZE_SP` | 11/8/8 sp | pies de navegación/pestaña | fitting dentro del slot; no truncar |
| `ACCENT_PRESS_LIFT` | 0.25 | magnitud de estado pressed | el resolvedor elige dirección que conserva contraste |
| `ALPHA_DISABLED/ALPHA_HAIRLINE/ALPHA_ARTWORK_EDGE/ALPHA_MUTED_SHAPE` | .38/.12/.55/.45 | opacidad de glifo, regla, borde de placeholder, forma silenciada | aplicar a rol semántico; no ocultar texto esencial |
| `Palette.SURFACE_BASE/RAISED/CARD/CARD_ACTIVE/OVERLAY/ARTWORK/ARTWORK_EDGE` | dinámicos por tema | campo, elevación, pozo, acción, menú, placeholder | se resuelven en `ThemePalette`; no congelar |
| `Palette.ON_SURFACE/ON_SURFACE_MUTED/ON_SURFACE_FAINT` | dinámicos por tema | tinta principal/secundaria/terciaria | faint no es texto esencial; revisar contraste final |
| `Palette.ACCENT/ACCENT_TEXT/ACCENT_PRESSED/ON_ACCENT` | dinámicos por tema/portada | acción, texto de acento, pressed, contenido sobre acento | accent shape ≥3:1; texto ≥4.5:1 |
| `Palette.ERROR/ERROR_PRESSED/POSITIVE/WARNING/STATE_WORKING` | dinámicos por tema | estados funcionales | distinto del acento; nunca depender sólo de color |
| `Palette.HAIRLINE/RIPPLE/RIPPLE_ON_ACCENT/SCRIM` | dinámicos por tema/superficie | separación, feedback, oscurecimiento | sólo en contexto semántico correspondiente |

Los roles de color son dinámicos y se enumeran en `DesignTokens.Palette`; no se congelan como constantes derivadas de un tema inicial. Opacidad y colores exactos se consultan allí. El código aún contiene alguna excepción histórica (por ejemplo target overflow 40 dp); no copiarla en componentes nuevos.

## Reglas de uso

- MUST reutilizar token existente; si falta, añadir uno con propósito y test cuando sea estable.
- MUST convertir dp/sp por helpers que respetan configuración; no usar px fijos para UI.
- MUST evitar números mágicos repetidos en vistas.
- SHOULD derivar tokens compuestos de escala existente; desviación justificada por layout o accesibilidad.
- MAY crear alias semánticos para mejorar lectura sin crear una segunda fuente.

## Validación

`DesignTokensTest`, `ThemePaletteTest`, `LayoutRulesTest`, `ProgressScaleTest`, `IconGeometryTest`; ver sección Android de [`VERIFICATION.md`](../../engineering/VERIFICATION.md). Estos tests no reemplazan revisión en tamaños de ventana reales.
