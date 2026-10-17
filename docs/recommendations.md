# Motor de recomendaciones

Cómo Tunefold decide qué suena después, en la TUI y en Android. Ambas caras
usan **el mismo scoring** (`src/recommendation/ranker.rs`): Android lo recibe
por JNI desde `src/platform/android_rank.rs`, de modo que los dos
reproductores no pueden divergir sobre lo que es "mejor para ti".

## Principio

El orden del proveedor ("relacionados con esto") es una sugerencia de catálogo,
no un ranking personal. Tunefold lo **reeordena** con similitud acústica,
armónica y estructural más el propio historial del usuario, y la popularidad
del proveedor solo pesa un 5 %.

## Peso por componente

Los pesos son fijos (`WEIGHTS`, suma 1.0) y un test los fija:

| Componente | Peso | Qué mide |
|---|---|---|
| `meta` | 0.10 | Metadatos: artista/álbum/título compartidos con el seed |
| `acoustic` | 0.30 | Tímbrico: cromas, espectro, niveles, rollo/tempo |
| `harmonic` | 0.20 | Distancia tonal de tonalidades y notas guía |
| `structure` | 0.10 | Forma: densidad de compases, patrones rítmicos |
| `affinity` | 0.20 | Historial del usuario sobre esa música |
| `recency` | 0.05 | Freshness del contenido (favoritos envejecen) |
| `popularity` | 0.05 | Popularidad del proveedor |

El total de contenido sonoro es **0.60** (acoustic + harmonic + structure);
el resto combina gusto, frescura y una pizca de popularidad.

Además actúan dos moduladores:

- **`affinity`** = `0.6 · engagement + 0.4 · afinidad acústica`
  (`src/recommendation/scoring/affinity.rs`): lo que el usuario hace con la
  música pesa más que lo que se parece por análisis.
- **`negative_penalty`** = `1 / (1 + 3 · rate)` con
  `rate = skips_significativos / (escuchas + skips_significativos)`: un
  historial de abandonos hunde al candidato. Solo cuentan skips reales del
  usuario, nunca del presupuesto de autoplay (`src/recommendation/scoring/negative.rs`).

## Señales de escucha

`src/recommendation/signals.rs` define la semántica (los pesos exactos viven
ahí, fijados por tests):

- **Kinds**: `play`, `completed`, `skip`, `replay`, `like`, `unlike`,
  `playlist_add`, `rec_click`, `rec_impression`.
- **Contexts**: `manual`, `queue`, `autoplay`, `recommendation`.
- **Impresiones** (`rec_impression`, TUI): se registran cuando la lista se
  entrega — miden el catálogo ofrecido, **nunca puntúan** (peso 0.0): una
  lista vista entera no dice nada sobre la afinidad del usuario.
- **`rec_click`** colapsa con `play` sobre recomendaciones: un clic y una
  escucha posterior del mismo candidato son **una intención**, no dos, para
  que la recomendación no se autopremíe dos veces.

Las señales llevan duración total de la pista y (en `completed`) los
milisegundos escuchados de verdad, lo que separa un final genuino de un
abandono temprano (`is_completion`/`is_short` en `signals.rs`). Android envía
el escuchado en `completed` y el native deriva la duración total del track
embebido en la señal.

## Pre-análisis de candidatos (TUI)

El proveedor no devuelve perfiles acústicos; Tunefold los calcula. Para no
esperar a analizar toda la lista:

- `src/ui/preanalysis.rs` toma el prefijo más prometedor
  (`PREANALYZE_TOP_K = 10`), descarga los primeros
  (`PREANALYZE_SECS = 45`, `PREANALYZE_COVERAGE = 0.3`) con HTTP Range y un
  `StreamBuffer` sin abrir dispositivo de audio, y calcula croma/clave/perfil.
- Cada término nuevo dispara `LoadRelated` → `maybe_spawn_preanalysis`;
  los perfiles resultantes reordena la lista vía `BackendEvent::RelatedReranked`
  **solo si sigue vigente** la generación de la recomendación
  (`RecommendationSession::accepts_update`): un re-rank tardío sobre otra
  lista sería peor que no reordenar.

## Ranking compartido con Android (Fase 5)

Java conserva los filtros anti-bucle (`Recommendations.filter*` — la lista
nunca repite lo que suena, ya está encolado o recién escuchado) y delega
**solo el reorden** en el native:

- **Contrato estadoless**: `TunefoldBridge.rankRelated(requestJson)` recibe
  `{"candidates":[<Track>…],"signals":[…],"profiles":[…],"seed_profile":…}`
  y devuelve `{"ok":true,"order":[índices…]}` (índices sobre el array de
  entrada, para que Java reordene sus propios `MediaTrack` sin re-serializar)
  o `{"ok":false,…}`. El `.so` no guarda estado: Java es dueño de los datos.
- **Fail-open en cuatro capas**: sin datos útiles → `order:[]`; parseo
  inválido → `ok:false`; cualquier `Throwable` en Java (incluido
  `UnsatisfiedLinkError` de un binario antiguo) → orden del proveedor; y un
  orden que no sea permutación exacta se descarta. La calidad puede degradarse
  a orden de proveedor, pero la reproducción nunca se rompe.
- **Señales**: `PlaybackSession` registra `play` al arrancar cada pista,
  `skip` al abandonarla por mano propia (no en EOF), `completed` en el fin de
  medios y `rec_click` al empezar una pista arrancada desde recomendaciones.
  `ListeningSignals` las persiste en
  `filesDir/listening-signals.txt` (una línea por evento,
  `kind \t context \t atMs \t listenedMs \t trackJson`, últimas 200,
  escritura atómica tipo `Library`). **Local y reversible**: `rm` del archivo
  solo borra feedback, nunca biblioteca ni reproducción.
- **Ids**: los candidatos de Android no traen id interno, así que reciben ids
  pseudo-negativos por posición (mismo truco que la TUI) y las señales se
  casan por identificador estable (`external_id` o `título|artista`).
- **Qué se rankea**: solo `forTrack` (Home, autoplay, radio).
  `forDisplay` (la estantería) **no** se reordena a propósito: es descubrimiento,
  y su contrato es mostrar asociados sin aplicar la política de autoplay.
- **Eje sonoro dormido en v1**: el contrato ya admite `profiles` (perfiles
  acústicos de candidatos) y `seed_profile` (perfil de la canción semilla), y
  el scoring de Rust ya los usa cuando existen; pero Android todavía no
  analiza audio, así que v1 no los envía. Android puntúa hoy con
  meta + historial/afinidad + señales; el día que genere perfiles los aportará
  en la misma llamada, sin cambiar el contrato ni el native.

## Verificación

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --lib
cargo test --lib --features youtube
cargo test --lib --features android        # incluye src/platform/android_rank.rs
cargo check --offline
cargo check --offline --features youtube
cd android && ANDROID_SDK_ROOT=... ./gradlew --offline test
```

Los pesos, la semántica de señales y el contrato JNI tienen tests en ambos
lados: `src/recommendation/*`, `src/platform/android_rank.rs`,
`android/app/src/test/.../ListeningSignalsTest.java` y
`RecommendationsTest.java`.
