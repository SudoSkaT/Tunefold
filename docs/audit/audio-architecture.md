# Auditoría de la arquitectura de audio

Estado: 2026-10-30. Cubre la TUI de escritorio (rodio) y el pipeline Android
(Symphonia + PCM ring + AudioTrack/Oboe).

## Mapa del pipeline

```
TUI / JNI
   │  Play(track)
   ▼
Backend (ui/backend.rs)
   │  época de intención + commit serializado
   ▼
PlaybackRouter (app/playback.rs)
   │  elige motor por Source
   ▼
PlaybackEngine (app/audio.rs)
   ├─ RodioBackend ──► HttpRangeStream ──► StreamBuffer ──► Decoder(Symphonia) ──► rodio Player
   └─ AndroidDecoder ─► HttpRangeStream ──► MediaSourceStream ─► Symphonia ─► resampler 48 kHz
                                                                  └─► PcmRing ─► Oboe/AudioTrack
                                                                  └─► PcmTap ─► análisis
```

La resolución `Track → MediaSource` (`media::StreamResolver`) es común. Toda
la red pasa por `HttpRangeStream`; el decoder no conoce proveedores.

## Fronteras

- `src/api/mod.rs` compone providers. Fuera de `src/providers/` no se importa
  código de YouTube; la feature `youtube` es la única puerta.
- `src/media/transport/` encapsula Range, reintentos, cancelación y métricas.
  No conoce tracks ni UI.
- `src/infrastructure/playback/` implementa el motor de escritorio. No conoce
  SQLite, recomendaciones ni la cola.
- `src/platform/android_decoder.rs` implementa el motor Android; comparte el
  transporte, no el motor de escritorio.
- `src/ui/backend.rs` es el dueño de la sesión lógica: track actual, contexto,
  cola y presupuesto de recuperación.

## Sesiones y concurrencia

Una reproducción es una **sesión** con dos contadores independientes:

| Contador | Dueño | Se incrementa | Protege |
|---|---|---|---|
| `playback_epoch` | Backend | al aceptar Play/skip/autoplay/stop | que una resolución vieja no pise la canción nueva |
| `generation` | Motor (rodio) | al iniciar play/stop | que un decoder/monitor viejo no escriba estado ni eventos |

El commit del backend (mutar `current`, cola, presupuesto y arrancar motor) se
serializa con `playback_commit`. La resolución de red queda fuera del lock.

En Android el equivalente es `cancel: AtomicBool` + `playing` + el join del
hilo decoder. `DecodeOutcome::Cancelled` distingue cancelación de fin real de
media; solo `EndOfMedia` dispara cola/autoplay.

## Buffering

- `StreamBuffer` (escritorio) acumula bytes y aplica histéresis:
  `BufferPolicy::adaptive_start` para el arranque y umbrales de pausa/refill.
- El decoder de escritorio se inicializa en `spawn_blocking`; el player no
  consume hasta que el buffer inicial alcanza el objetivo o llega EOF.
- Android usa un `PcmRing` SPSC. El decoder espera si el anillo está lleno y
  el consumidor (Oboe) drena; nunca se bloquea el hilo de audio.
- Android remuestrea a 48 kHz estéreo porque algunos HAL no drenan tracks a
  44.1 kHz creados fuera del mixer.

## Fallos y clasificación

`FailureCategory` distingue red, timeout, restricción, formato,
decodificación y reproducción. `TransportFailure::Cancelled` se clasifica como
`PlaybackFailure` y **no** es transitorio: reintentarlo desobedecería un stop.

Reglas de reintento en transporte:

- Solo red/timeout/truncamiento/5xx son transitorios.
- 401/403/404/416 y respuestas inválidas no se reintentan.
- Backoff exponencial con techo de 8 s y jitter determinista.
- Un 200 con `Content-Length` completo pero cuerpo corto es truncamiento, no
  un archivo válido.

## Estado actual y límites conocidos

- El motor de escritorio soporta seek; el de Android no
  (`PlaybackController.supportsSeek() == false`) y el JNI rechaza seek.
- El decoder vendor AAC soporta AAC-LC ≤ 2 canales. No soporta CCE, LTP/Main,
  SBR/PS ni layouts > 2 canales.
- La recuperación en caliente reinicia el track desde el inicio y lo dice de
  forma explícita; no promete conservar posición.
- El mensaje `network_error: no se pudo resolver <<>> ...` ya no puede
  producirse: `Track::identifier()` ignora ids vacíos y el error usa un texto
  explícito si no hay identidad. Queda por confirmar qué proveedor emitió el
  id vacío originalmente.
- Las pruebas de Android no se ejecutaron en hardware en esta auditoría.
