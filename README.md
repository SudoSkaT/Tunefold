# Tunefold (PlayFusion)

Reproductor de música en TUI con motor de análisis y recomendación. Busca
canciones, resuelve streams de audio, reproduce con rodio/symphonia (análisis
PCM en tiempo real + visualizador reactivo) y sincroniza letras (LRCLIB) con
karaoke.

El proyecto se distribuía antes bajo el nombre **PlayFusion**; fue renombrado
por conflicto de marca. Los datos locales del usuario se migran de forma no
destructiva (ver [Datos locales](#datos-locales)).

## Licencia

**GPL-3.0** — ver `LICENSE`. Todo el código propio del repositorio está bajo
GNU General Public License v3.0.

El proveedor de YouTube/YouTube Music reutiliza **rustypipe** (GPL-3.0),
distribuido con un parche local (el fuente modificado vive en `vendor/` y se
ofrece como fuente bajo los términos de la GPL). Ver `NOTICE` y
`THIRD_PARTY.md` para las obligaciones de redistribución.

### Proveedor de YouTube — build separado

Por los términos de servicio de YouTube, el **binario oficial se compila sin
el proveedor de YouTube** (feature `youtube` desactivada por defecto). Ese
binario no resuelve streams ni reproduce audio de YouTube.

Para obtener el reproductor completo:

```sh
cargo build --release --features youtube
```

Quien compile la feature acepta las restricciones de los términos de servicio
de YouTube (acceso programático, cliente interno de Innertube, PO tokens,
descarga de streams por rangos). No se distribuye ningún binario con esa
feature habilitada.

## Instalación

```sh
cargo install --path .            # binario oficial sin proveedor YouTube
cargo install --path . --features youtube   # reproductor completo
```

Requiere una salida de audio real funcionando (rodio/cpal).

## CLI

```sh
tunefold [--tui]              # lanza la interfaz de terminal
tunefold --search <consulta>  # busca y guarda en la BD local
tunefold --sources            # lista las fuentes activas
tunefold --history            # últimas reproducciones
tunefold --version            # versión (semver desde Cargo.toml)
tunefold --update             # actualiza a la última release oficial
tunefold --update-check       # solo comprueba si hay release más nueva
```

`tunefold update` descarga el binario de tu plataforma, lo sustituye en sitio
y purga solo la caché (`~/.cache/tunefold`); los datos de usuario y la
configuración nunca se tocan.

> El karaoke funciona en el **binario oficial** (sin la feature `youtube`):
> las letras sincronizadas vienen de LRCLIB, un servicio legítimo e
> independiente, identificado con el User-Agent `Tunefold/<versión>`. Solo la
> **búsqueda y reproducción** requieren el build con la feature.

## Datos locales

Los datos siguen la especificación XDG en Linux/macOS:

| Tipo | Ruta | ¿Se toca al actualizar? |
| --- | --- | --- |
| Datos de usuario (`music.db`: playlists, historial, señales, perfiles) | `~/.local/share/tunefold/` | **Nunca** |
| Configuración (`.env`) | `~/.config/tunefold/` | **Nunca** |
| Caché (thumbnails, rustypipe) | `~/.cache/tunefold/` | Reciclable |
| Logs | `~/.local/state/tunefold/` | Reciclable |

La primera ejecución detecta una base antigua `data/music.db` heredada y la
**copia** (nunca borra) a `~/.local/share/tunefold/`.

## Configuración

La configuración se lee de variables de entorno (o un `.env` en el directorio
de configuración):

| Variable            | Descripción                                                       |
| ------------------- | ----------------------------------------------------------------- |
| `PLAYBACK_POLICY`   | `auto` (por fuente) o `rodio` (forzar el motor local)             |
| `HTTP_PROXY`        | Proxy para todas las peticiones a YouTube (ver sección Proxy)     |
| `HTTPS_PROXY`       | Igual que `HTTP_PROXY` para conexiones TLS                        |
| `ALL_PROXY`         | Igual que `HTTP_PROXY` para cualquier protocolo                   |
| `NO_PROXY`          | Lista de hosts que se conectan sin proxy                          |
| `TUNEFOLD_RANGE_WINDOW_KIB` | Tamaño de ventana de descarga por rangos (32–4096)        |

Las variables legacy `PLAYFUSION_*` se aceptan como alias.

## Límites de streaming (diagnóstico 2026-08)

El síntoma «la reproducción se corta alrededor de 1 MiB (~65 s)» fue
diagnosticado con evidencia HTTP (probes en `examples/probe_*.rs`):

- googlevideo exige `Range` cerrado: un GET completo responde **403**.
- Las URLs resueltas con clientes directos capados (ANDROID_VR/iOS) tienen un
  **techo posicional por URL** (~1,02–1,07 MiB): cualquier rango cuyo fin lo
  supere responde **403**; las repeticiones dentro del techo se sirven
  al instante a plena velocidad. **No es cuota por IP** ni expiración.
- Las URLs del cliente **VISIONOS** (usado como primario desde el parche del
  vendored rustypipe) no tienen ese techo y sirven el archivo completo.
- El provider verifica cada stream con una sonda más allá de 1 MiB antes de
  aceptarlo (`stream_url_ok`); si VISIONOS fallara, Android/iOS quedan de
  respaldo (con prefijo servible + aviso honesto `Cut`, nunca autoplay ciego).
- La descarga usa ventanas Range encadenadas con validación estricta
  ([`HttpRangeStream`], capa Media): retries acotados solo para fallos
  transitorios; 403/416/200-forzado/Content-Range inválido se clasifican sin
  reintentos ciegos.

Herramientas de diagnóstico:

```sh
cargo run --release --example probe_range      # GET vs rangos A–F sobre 1 resolución
cargo run --release --example probe_boundary   # frontera exacta del techo
cargo run --release --example probe_frontier   # ¿el techo avanza? (no)
cargo run --release --example probe_clients    # techo por contexto de cliente
cargo run --release --example probe_seek       # E2E: pista COMPLETA hasta Finished
```

El proxy del entorno sigue respetándose (`PROXY_ENABLED`), pero ya no es parte
de la solución: la causa nunca fue la IP.

> Los `probe_*` y `probe_youtube` requieren la feature `youtube`.

## Visualizador de audio

Al reproducir una canción aparece una banda **Visual** entre la tarjeta del
disco y la barra de progreso: espectro de barras reactivo (graves a la
izquierda), punto de pulso ● en el título que late con el beat y color según
la intensidad de agudos. Debajo, una capa ambiental de **lava** (metaballs)
reacciona a energía/brillo/distorsión del PCM, y su **paleta se deriva de la
portada** de la canción. La fase deriva SIEMPRE de la posición real de
reproducción (sin relojes visuales propios), y la transición al hacer seek se
suaviza en vez de tele-transportar. En la vista *Related*, la tecla **`v`**
cicla entre Auto / Letras / Visual, y las letras karaoke se superponen sobre la
lava aplacada sin borrarle el fondo.

Se controla desde el entorno sin recompilar:

| Variable                        | Default | Descripción                                  |
| ------------------------------- | ------- | -------------------------------------------- |
| `AUDIO_ANALYSIS_ENABLED`        | `1`     | Análisis PCM→features en hilo dedicado       |
| `ADVANCED_VISUALIZATION_ENABLED`| `1`     | Renderizado del visualizador en la TUI       |

Si el análisis está apagado, la banda se muestra gris e inactiva.

## Rendimiento (medido, spec §34)

Banco de caminos calientes: `cargo run --release --example bench_hotpaths`.

| Métrica | Valor | Presupuesto | Margen |
| --- | --- | --- | --- |
| Frame de análisis completo (FFT 2048 + bandas + flux) | ~15 µs | 11.6 ms/hop | ×780 |
| Render visual TUI (80×5) | ~32 µs | 66 ms/tick | ×2000 |
| Anillo SPSC audio→análisis | ~630 M muestras/s | 96 k/s | ×6500 |
| Hilo de análisis end-to-end | ~23× tiempo real | 1× | ✓ |

El camino caliente del DSP y el render no hacen allocations por frame
(buffers reutilizados; el único alloc por frame es el snapshot `Arc` que se
publica al bus). El análisis corre en hilo propio y jamás bloquea el audio:
si se atrasa, descarta muestras nuevas (drop-newest) en vez de cortar la
reproducción.
