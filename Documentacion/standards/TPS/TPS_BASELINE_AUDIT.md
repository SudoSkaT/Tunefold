# TPS · Baseline audit

Fecha: 2026-11-26 · Rama: `main` · HEAD: `67d5808` (`0.19.6`) · Versión efectiva `Cargo.toml`: 0.19.6. Worktree ya estaba sucio en `Cargo.lock`; este trabajo no lo modifica. La rama reportaba `main...origin/main` antes de los cambios.

## Alcance y estructura real

La entrada de agente describe TuneFold como TUI Rust y correctamente indexa `src/ui`, `src/analysis`, `src/dsp`, `src/visualization`, `src/platform` y `android/`. El pedido de producto corresponde además a una aplicación Android Java real con playback JNI/Rust, Views nativas y servicio foreground. TPS cubre ambas plataformas, y especifica el sistema visual Android sin fingir paridad con TUI.

Las clases de referencia siguen presentes: `DesignTokens`, `LayoutRules`, `ArtworkTheme`, `ArtworkPalette`, `NowPlayingView`, `SoundBarsView`, `LiveAudioBars`, `ProgressControl`, `IconButton`, `TrackRow`, `TrackList`, `NavigationItem`, `PlaybackSession`, `PlaybackController`, `ForegroundPlaybackService`, `MainActivity`, `HomeSurface`, `SearchSurface`, `LibrarySurface`, `SettingsSurface`, `MiniPlayer`. Revisadas por inventario y lectura de código selectivo.

## Estado implementado (evidencia de código)

- Sistema visual Android central en `DesignTokens.java`, roles dinámicos `ThemePalette`/`UiTheme`, seis temas (`ThemeSpec`) y extracción desde arte (`ArtworkPalette`/`ArtworkTheme`). Tests: `DesignTokensTest`, `ThemePaletteTest`, `ArtworkPaletteTest`.
- `LayoutRules` pura con tests, composición adaptativa, listas por ventana/prefetch y componentes Java dedicados.
- Reproducción Android pertenece al servicio/controller/session, con decoder Rust y AudioTrack separado. Fuentes: `engineering/systems/android.md`, `playback-lifecycle.md`.
- Análisis usa FFT 2048/hop 512 y ventana min/max del bus; la tasa depende del sample rate (a 44.1 kHz ~86.1 publicaciones/s). Véase `analysis/engine.rs`, `analysis/waveform.rs`.
- Android lee barras cada 50 ms (20 Hz) sólo con player visible/PLAYING, mediante array persistente de hasta 96 barras; misma generación no invalida. Dibujo usa path/paint persistentes y reconstruye geometría por tamaño. `SoundBarsView.stats()` informa repaints, lecturas, muestras nuevas, pico y último `onDraw`.
- Android seek está deshabilitado/no disponible en pipeline actual. No hay DSP Android en cadena.

## Sistema documental vigente y deuda

Fuentes actuales: `Documentacion/docs/android/visual-system.md`, `DESIGN_SYSTEM.md`, `COLOR_SYSTEM.md`, `TYPOGRAPHY_SPEC.md`, `COMPONENT_LIBRARY.md`, `engineering/*`, `docs/architecture/*`. Auditorías `docs/audit/*`, roadmap y `Documentacion/android/*_REPORT.md` son explícitamente históricos. Ya existía documentación de diseño útil; TPS la integra como norma y evita reemplazarla.

Hallazgos verificables:

1. `android/README.md` describía barras de dos carriles aunque `SoundBarsView.LANES=1` y combina extremos de ambos canales en una envolvente. Esta contradicción se corrigió durante esta intervención; se añadió nota de versión por descripciones históricas restantes.
2. `COMPONENT_LIBRARY.md` lista overflow de 40 dp mientras `DesignTokens.TOUCH_TARGET_MIN` fija 48 dp y su comentario prohíbe bajar de ahí. `CONTROL_TARGET_OVERFLOW=40` confirma excepción efectiva; incumple el objetivo TPS para nuevas acciones y requiere corrección de implementación separada.
3. TPS/UX no tenía índice canónico ni checklist de PR; `AGENTS.md` no dirigía a una fuente de producto/diseño.
4. Instrucciones dicen TUI; no explican que referencias UI Android deben ir a Java Views, generando riesgo de inspeccionar módulo equivocado.

## Problemas funcionales y riesgos

- No se reporta una falla reproducida en reproducción o funcionalidad durante esta auditoría de sólo lectura; no se ejecutaron tests ni dispositivo. No inventar defecto funcional.
- `ProgressControl` no debe asumirse como seek funcional: docs Android especifican streaming forward-only/seek off.
- Cambios visuales en estado deben respetar generación/cancelación y no hacer que UI replique PlaybackSession.
- Excepción de target overflow 40 dp es riesgo de acceso tangible en código.
- Discrepancia de documentación de carriles puede llevar a rediseño accidental o pruebas mal planteadas.

## Rendimiento: evidencia y límites

Hay decisiones implementadas explícitas: array reutilizado, `Path`/`Paint` persistentes, geometría al cambiar tamaño, lectura condicionada por estado/visibilidad, invalidación sólo cuando cambia generación, bitmap de artwork acotado. Éstas son propiedades de código, no resultados medidos en este turno.

No hay evidencia actual recolectada aquí de FPS efectivo, frame time p50/p95/p99, missed frames, alloc/GC correlacionados, CPU/batería, o mediciones device comparables. El `last_draw_us` individual no sustituye distribución ni presenta frame pacing. No declarar problema de fluidez demostrado ni mejora de rendimiento.

## Accesibilidad: evidencia y límites

Existen tests de contraste/token/layout, targets habituales 48 dp, descriptions en algunas acciones, escalado de tokens sp. Pero overflow es 40 dp. `visual-system.md` deja pendientes TalkBack, fuente grande real, tablet y notificación. La vista de barras se excluye del árbol: verificar equivalente accesible para estado esencial en producto. No existe validación general de accesibilidad de dispositivo en la evidencia revisada.

## Tests y medición

Tests Android JVM observados: `DesignTokensTest`, `ThemePaletteTest`, `LayoutRulesTest`, `LiveAudioBarsTest`, `ArtworkPaletteTest`, `NavigationRulesTest`, más tests de playback, búsqueda, listas, etc. Rust tests y CI descritos en `engineering/VERIFICATION.md`/`.github/workflows/ci.yml`. Validación física de audio y layout usa scripts descritos en `android/README.md`, hardware requerido.

Durante esta intervención no se añadieron ni ejecutaron tests. Se verificaron rutas, versión, rama, commit y docs/código por lectura. La validación documental se realiza por índice/enlaces y git diff.

## Oportunidades

- Usar DesignTokens/ThemePalette actuales; no reemplazarlos.
- Corregir inconsistencia overflow en un cambio funcional específico y ampliar test.
- Reconciliar README/visual-system al carril actual.
- Añadir FrameMetrics/JankStats o captura Perfetto sólo al hacer baseline de dispositivo; documentar cadencia real del bus y UI.
- Evaluar presentación VSync/interpolación sólo como investigación independiente, preservando extremos reales y midiendo coste.
- Ampliar comprobación de fuente escalada, TalkBack, reducida de movimiento y notificaciones.
- Separar documentos actuales de reportes históricos mediante enlaces claros, sin borrar historial.

## Clasificación

| Resultado | Estado |
|---|---|
| Auditoría de versiones, ramas, estructura y fuentes | VALIDATED por comandos/lectura |
| Inventario de design system/componentes Android | VALIDATED por código y tests existentes identificados |
| Identidad, tokens, UX, componentes, motion, accesibilidad y gobernanza TPS | DOCUMENTED |
| Integración de descubrimiento en instrucciones/índices | DOCUMENTED e integrada |
| Mejoras en código de UI/DSP/playback | PENDING, fuera del alcance de establecimiento documental |
| Rendimiento y accesibilidad de dispositivo | PENDING |
| Corrección de overflow/documentación contradictoria | PENDING |
