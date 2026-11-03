# Arquitectura objetivo del motor de audio

## Principios

1. **Una sola fuente de verdad por sesión.** El backend decide qué track
   suena; el motor decide cómo suena. Ninguno adivina el estado del otro.
2. **La red no bloquea al usuario.** Resolver y prebuffear no retienen el
   lock de commit más de lo necesario.
3. **Toda espera es cancelable.** Stop, cambio de pista y cierre de la app
   deben interrumpir I/O sin esperar timeouts largos.
4. **Un fallo se clasifica antes de decidir.** No todo error se reintenta; no
   todo reintento es visible.
5. **El audio no se bloquea.** El hilo de salida nunca espera red, disco ni
   locks de UI.

## Capas

| Capa | Responsabilidad | No debe |
|---|---|---|
| `ui/backend.rs` | intención, cola, historial, recuperación | decodificar ni abrir sockets |
| `app/playback.rs` | elegir motor y reemitir eventos | conocer providers |
| `app/audio.rs` | contrato `PlaybackEngine` | conocer rodio/Symphonia |
| `infrastructure/playback` | motor de escritorio | conocer SQLite/UI |
| `platform/android_decoder.rs` | motor Android | conocer la cola de la TUI |
| `media/transport` | Range, reintentos, cancelación | conocer tracks |
| `media/stream` | resolución y verificación de fuentes | reproducir |

## Contrato `PlaybackEngine`

- `play(track, source)` inicia una **generación** nueva y devuelve el estado
  inicial.
- `pause`/`resume` son idempotentes respecto al estado observable.
- `stop` invalida la generación y libera el buffer; no puede ser revivido por
  un `play` anterior que termine después.
- `status` lee un snapshot consistente; no bloquea al hilo de audio.
- `is_buffering` consulta el prebuffer sin los efectos laterales de `status`
  (no publica `Finished`).
- Los eventos (`Playing`, `Paused`, `Buffering`, `Error`, `Cut`, `Stopped`)
  llevan la generación o se filtran antes de publicarse.

## Buffering de escritorio

`StreamBuffer` es el amortiguador entre descarga y decoder:

- El decoder se crea en `spawn_blocking` para no ocupar un worker de Tokio.
- El arranque espera `BufferPolicy::adaptive_start` (según `bps_ema`), con
  timeout de llenado inicial.
- Si el buffer cae por debajo del umbral, el monitor pausa el player y emite
  `Buffering`; al recuperarse, `Resume`.
- EOF y `Cut` son señales distintas: EOF termina; `Cut` indica que el
  servidor cortó y la política puede decidir siguiente/repetir.

## Procesamiento DSP (escritorio)

```
Decoder → TapSource (análisis PRE-DSP) → DspSource → ProcessorChain → Player
```

La cadena (preamp → EQ → estéreo → normalización → limitador) se construye
antes de `Player::append`; el hilo de audio solo copia parámetros `Copy` con
`try_lock` y procesa bloques preasignados in place. Flat es una salida temprana
bit-exacta. El detalle vive en
[`dsp-pipeline.md`](dsp-pipeline.md).

## Android

```
HTTP Range (cancelable) → MediaSourceStream → Symphonia → f32 interleaved
   → resampler estéreo 48 kHz → PcmRing (SPSC) → Oboe
                             └→ PcmTap → análisis
```

- El ring es SPSC: un solo productor (decoder) y un solo consumidor (audio).
  No se limpia desde JNI mientras el productor vive.
- Un cambio de `SignalSpec` recrea `SampleBuffer` y resampler; nunca se copia
  con el layout viejo.
- El decoder no hace seek: la fuente es forward-only. Kotlin lo declara y el
  JNI lo rechaza.
- `DecodeOutcome::Cancelled` no es fin de pista.

## Cancelación

Dos mecanismos, con propósitos distintos:

- `HttpRangeStream::cancel` (`Arc<AtomicBool>`): interrumpe `open`,
  `next_chunk`, reintentos y lecturas de cuerpo.
- Generación/época: impide que una sesión vieja escriba estado o eventos
  después de que otra tomó el control.

El token de transporte no sustituye a la generación: cancelar I/O no debe
publicar `Error` de una sesión que ya no es la vigente.

## Invariantes

- `total` del transporte solo cambia de 0 a un valor, o falla si el servidor
  reporta otro total.
- Un `200` solo es válido para un rango que empieza en 0 y con cuerpo completo.
- Un `206` debe tener `Content-Range` coherente con el byte pedido y el
  tamaño del cuerpo.
- Ningún evento de una generación vieja llega a la UI como si fuera actual.
- La posición Android avanza solo por PCM aceptado por el ring, nunca por
  bytes descargados.
