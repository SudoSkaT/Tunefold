# §23 — Informe final: sistema de renderizado visual

**Espec:** §19 (fases A–H) · **Estado:** completo · **Gates:** verdes
(`cargo fmt --check`, `cargo check`, `cargo test`: 382 ok / 1 ignored,
`cargo clippy --all-targets --all-features -- -D warnings` limpio salvo el
warning heredado de `vendor/rustypipe`, preexistente y no fatal).

## 1. Resumen

PlayFusion muestra ahora una **banda visual reactiva** entre la tarjeta del
disco y la barra de progreso en la vista *NowPlaying* (y en *Related*): un
espectro de 24 barras de graves (izquierda) a agudos (derecha), un punto de
pulso ● en el título que late con el beat, y debajo de ambas una **capa
ambiental de "lava"** (metaballs) cuyos glóbulos, energía, brillo y distorsión
reaccionan a las características extraídas del PCM y cuya **paleta de color se
deriva de la portada** de la canción en reproducción.

La letra sincronizada (karaoke, vista *Related*) ya no reemplaza al visual:
ambos conviven, con las letras sobreescritas sobre la lava aplacada para
mantener legibilidad.

El trabajo se ejecutó en fases (A: auditoría, B: paleta+parámetros, D: motor,
E: renderer, F: overlay+tecla, G: pruebas, H: cierre) y se entregó el banco de
caminos calientes actualizado.

## 2. Arquitectura (flujo de datos)

La auditoría de la Fase A confirmó que el corredor ya existía y era correcto;
no se eliminó ninguna restricción del sistema:

```
YouTube Music ─▶ RodioBackend ─▶ AnalysisRuntime (PCM→features, hilo propio)
        ─▶ FeatureBus (SPSC) ─▶ BackendEvent::Features (bus de eventos)
        ─▶ App.features / features_at (snapshot + planificación)
        ─▶ PositionClock (única fuente de posición; fase determinista)
        ─▶ VisualEngine::update(features, position, &palette) ─▶ VisualState
        ─▶ UI views (dashboard / related / karaoke)
        ─▶ visualization::render / render_ambient / render_bars
```

Decisiones de la Fase A:

- **Sin GPU/wgpu ni dependencias nuevas** (§11): el renderer sigue siendo
  ratatui/crossterm, dibujando por celdas (`u8` termial color).
- **Un solo `AudioFeatures`** como contrato con el análisis; el motor no vuelve
  a calcular ni duplica análisis.
- **`PositionClock` como única fuente de fase**: el motor recibe la posición
  (`secs`) y deriva fases de ella; no hay reloj propio, lo que garantiza
  determinismo por secuencia de entradas (test
  `phase_depends_only_on_playback_position_not_wall_clock`).
- Sin flags nuevos: `AUDIO_ANALYSIS_ENABLED` y `ADVANCED_VISUALIZATION_ENABLED`
  siguen siendo los únicos interruptores.

## 3. Motor visual (`src/visualization/engine.rs`)

`VisualEngine::update(features, position, palette) → VisualState`:

| Campo | Rol |
| --- | --- |
| `bars[24]`, `level`, `intensity`, `pulse` | Espectro + pulso de beat (como antes) |
| `phase` | Fase agregada (inercia con EMA `SCENE_SMOOTH=0.45`) |
| `active` | `false` sin audio/features válidas |
| `scene: SceneState` | Capa ambiental: `blobs[5]`, `energy`, `brightness`, `distortion`, `palette`, `active` |

**Parámetros** (`params.rs`): se añadieron al `VisualParameters`:

- `energy` = rms conformado (graves ponderados) con piso de ruido 0.02;
- `brightness` = agudos+medios-agudos+flux (región de "chispas");
- `distortion` = medios × 0.6 + flux × ganancia de turbulencia (brazos de lava).

**Glóbulos (lava):** 5 blobs con reposo en `Blob { x, y, r }`, posiciones
función pura de `(freq del bpm/fallback, energía, brillo, distorsión)` y de
fases `BLOB_PHASES` fijas. El bpm real (si las features lo traen) impulsa la
velocidad de rotación; sin él se usa el fallback constante — mismo resultado
determinista.

**Transición por seek:** un salto de posición no tele-transporta la escena: la
mezcla `blend` se reinicia a 1.0 y decae con `SCENE_BLEND_DECAY=0.60` por
update, interpolando desde la escena previa al nuevo objetivo (test
`seek_blends_scene_toward_target_instead_of_teleporting`).

**Fusión de paleta:** el motor recibe la paleta de la portada y la **mezcla
según `PALETTE_RATE=0.35` por frame** (jamás salta); la escena expone la paleta
ya fusionada al renderer (test `palette_blends_toward_target_track`). Así, un
cambio de canción provoca una transición de color continua, no un parpadeo.

**Sin audio / None:** `update(None, …)` resetea barras, blobs y EMA y expone
`scene.active=false` incluso sin features (test
`none_features_resets_to_inactive`).

**Invariantes:** `VisualState` sigue `Copy + PartialEq`; todos los campos en
[0,1]. Los tests de determinismo comparan `scene` además de la salida clásica.

## 4. Paleta (`src/visualization/palette.rs`, nuevo)

`VisualPalette { primary, secondary, accent, background }`:

- `fallback()`: paleta fija de reposo (const, sin alocaciones).
- `from_cover(Option<[[u8;3];3]>)`: sin portada ⇒ fallback exacto; con
  dominantes de color ⇒ `primary/secondary/accent` mapean a los matices y el
  **fondo se oscurece** (multiplicador ~0.22) para que la lava resalte.
- `mix(&b, t)`: interpolación por canal `u8` saturada a `t∈[0,1]`.
- Es pura y determinista (tests: extremos exactos, determinismo, convergencia).

La UI ya no fabrica colores en el render: la paleta viaja engine→renderer.

## 5. Renderer compositado (`src/visualization/render.rs`)

`render(frame, area, state, position_secs)` dibuja, en una sola pasada:

1. Caja con título y el pulso ● (color según intensidad).
2. **Capa ambiental** (`render_ambient`): por cada celda evalúa un kernel de
   metaball (densidad sobre blobs suavizada por `SCENE_SMOOTH`) ⇒ fondo teñido
   de lava; **solo toca el fondo**, nunca el símbolo (test
   `ambient_only_sets_background_not_symbols`).
3. **Barras** (`render_bars`) sobre el interior: color desde la paleta de la
   escena por niveles de intensidad (dominante/acento/fondo, test
   `palette_colors_the_bars`), con `Cell::set_style` (fusión: respeta el fondo
   ambiental).

`render_ambient(frame, area, state, subdued)` admite un modo *subdued* que
oscurece la lava (test `subdued_dims_the_lava_for_legibility`).

**Rendimiento:** render 80×5 ~32 µs (presupuesto 66 ms/tick, margen ×2000);
sin allocations por frame salvo el snapshot `Arc` compartido del análisis.

## 6. Coexistencia letras + lava (§7/§15/§19)

El karaoke usaba un `Paragraph` que *limpiaba el fondo*, rompiendo la capa
ambiental. `karaoke::render_over_scene` reescribe las letras **celda a celda**
(`Buffer::set_stringn`, solo fg) preservando el fondo: la lava aplacada queda
detrás del texto (test `overlay_preserves_background_below_text`). El deslizamiento
en cascada del `KaraokeScroller` se conserva, y `BandContent::resolve` decide
qué pinta la banda en *Related*:

- **Auto** (default): letras si hay pista sincronizada; visual en caso contrario;
  estados especiales `Unavailable`/`Waiting`.
- **Letras**: fuerza karaoke sobre lava.
- **Visual**: fuerza lava+barras ignorando letras.

La tecla **`v`** (vistas de solo lectura) cicla Auto → Letras → Visual, con
aviso en la barra de estado. Los colores de lectura/actual/sin leer del karaoke
derivan de la misma paleta (`karaoke_colors`).

## 7. Integración

- `app.rs`: campo `visual_mode`, manejo de `v`, y en `render_view` se calcula
  `VisualPalette::from_cover(cover_palette())` una vez y se pasa a
  `visual.update` en *NowPlaying* y *Related*.
- `dashboard/mod.rs`: render 4-arg de `visualizer::render` sobre la banda;
  eliminado el helper `track_palette` que ya no correspondía.
- `related.rs`: `render` recibe el modo visual y el `VisualState`; usa
  `render_ambient(subdued)` + `render_over_scene` o `render` puro.
- `examples/bench_hotpaths.rs`: actualizado a `engine.update(…,&palette)` y
  `render(f, area, &state, pos)`.

## 8. Pruebas

382 pasan (1 ignorado), **+20** respecto a la base 362, todas en el dominio
visual salvo errores de compilación resueltos:

| Módulo | Nuevos tests |
| --- | --- |
| `palette` | 5 (fallback, from_cover, extremos de mix, determinismo/acotación, convergencia) |
| `params` | 5 (energía vs RMS, brillo agudos, distorsión medios+flux, silencio nulo, sanidad RMS) |
| `engine` | 4 (seek blend, viewport bajo extremos, bass infla/deriva fase, fusión de paleta) |
| `render` | 7 (activo/inactivo, áreas diminutas, más relleno, colores por paleta, brillo vs energía, subdued, solo-fondo) |
| `karaoke` overlay | 1 (fondo preservado bajo el texto) |
| `related` | 3 (modo explícito, resolución de banda, mapeo de colores) |

## 9. Cambios por archivo

`src/visualization/{palette.rs (nuevo), mod.rs, params.rs, engine.rs, render.rs}`,
`src/ui/{mod.rs, app.rs, related.rs, dashboard/mod.rs, widgets/karaoke.rs}`,
`examples/bench_hotpaths.rs`.

## 10. Riesgos y limitaciones

- **Rejilla de terminal (80×5)**: la lava es más convincente en paneles altos;
  por diseño se degrada con gracia a manchas de 1-2 celdas (test de áreas
  diminutas) pero la baja densidad vertical limita el detalle.
- **Paleta por portada**: si la portada es monocroma, la lava puede carecer de
  contraste; el modo *subdued* de las letras y el oscurecido del fondo lo
  mitigan.
- **Determinismo vs. suavizado**: el EMA en fase introduce estado interno;
  sigue siendo determinista por secuencia de entradas (misma entrada ⇒ misma
  salida), tal como pide la spec.
- **Tecla `v`**: sólo en vistas de solo lectura; si en el futuro se permite
  en reproducción, habrá que decidir persistencia del modo.
- El warning de `vendor/rustypipe` (lifetime elidido) permanece sin tocar.