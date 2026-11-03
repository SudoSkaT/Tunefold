# Auditoría del motor de audio y del camino PCM

Estado: 2026-11-02. Auditoría previa a la infraestructura DSP. Cubre el motor
de escritorio (rodio), el motor Android (Symphonia + PcmRing + Oboe) y el
análisis/visualización. No se declara resuelto ningún fallo sin prueba.

## 1. Arquitectura real

### Escritorio

```
UI / BackendCommand
  └─ ui/backend.rs: intención, época, cola, historial
       └─ app/playback.rs: PlaybackRouter elige motor
            └─ infrastructure/playback/rodio_backend.rs
                 ├─ HttpRangeStream (media/transport)
                 │    └─ StreamBuffer (bytes + Condvar + métricas)
                 ├─ Decoder de rodio/Symphonia (hilo de bloqueo aparte para init)
                 ├─ TapSource → anillo SPSC → hilo de análisis → FeatureBus
                 └─ Player de rodio → MixerDeviceSink (cpal) → dispositivo
```

- El PCM interno es `f32` intercalado. El decodificador de rodio entrega
  `f32`; no hay una representación PCM entera propia.
- La frecuencia y los canales son los del contenedor. rodio/cpal realizan la
  conversión final al formato del dispositivo.
- El stream se descarga por ventanas Range y se acumula completo en
  `StreamBuffer`; el decodificador consume un `Read + Seek` continuo.
- El prebuffer adaptativo pausa/reanuda el `Player` según
  `BufferPolicy` (arranque 1.5–4.5 s, crítico 1.5 s, recuperación 3.5 s).
- Cada `play`/`stop` incrementa una generación. El commit del player ocurre
  bajo el mutex de estado; el monitor comprueba generación antes de publicar.
- `seek` precarga la región objetivo antes de llamar a `try_seek`, para no
  bloquear el hilo de audio con la red.
- Volumen: `PlaybackEngine::set_volume(0..100)` → `Player::set_volume(v/100)`.
  No se persiste y no existe preamplificación ni protección de picos.
- Persistencia: SQLite para catálogo/historial/playlists; `.env` canónico para
  `PLAYBACK_POLICY` y feature flags. Las preferencias de audio no existen aún.

### Android

```
HTTP Range cancelable → MediaSourceStream → Symphonia
  → f32 intercalado → downmix/remuestreo estéreo 48 kHz
  → PcmRing SPSC → Oboe/AudioTrack
  → PcmTap → análisis
```

- El decodificador corre en un hilo dedicado, no en el callback de audio.
- El resampler estéreo conserva continuidad entre paquetes.
- `DecodeOutcome::{EndOfMedia, Cancelled}` distingue fin real de cancelación.
- El seek no está soportado: la fuente es forward-only y el JNI lo rechaza.
- El motor Android no implementa el trait `PlaybackEngine`; su contrato es
  JNI con Kotlin. Comparte transporte, no el backend de escritorio.

### Análisis y visualización

- `TapSource` envuelve la fuente **antes** del player y copia cada muestra a un
  anillo SPSC acotado. Si el anillo se llena, descarta lo nuevo; nunca bloquea
  el audio.
- Un hilo dedicado hace ventana/Hann + FFT (`rustfft`) por hop de 512 muestras,
  bandas, flujo, onset, BPM, suavizado y publica snapshots `Arc` por
  `FeatureBus`. El coste medido de un frame completo de análisis es ≈17 µs
  frente a 11.6 ms de presupuesto por hop.
- La visualización consume el bus y `PositionClock`; no toca el callback.
- Las features también alimentan el agregador acústico de recomendaciones.
  Por eso el análisis debe seguir siendo PRE-DSP.

## 2. Estado de los fallos mencionados

Comprobado contra el árbol actual, no contra informes anteriores:

| Problema | Estado | Evidencia |
|---|---|---|
| AAC Program Config Element | Corregido en el vendor | `tests/audio_decode.rs::adts_with_program_config_decodes`, `mp4_with_program_config_decodes` |
| Paquetes ADTS con varios bloques | Corregido | `adts_multi_block_packet_realigns` |
| ADTS CRC / MPEG-2 | Corregido | `adts_crc_and_mpeg2_headers_decode` |
| Sample rate ASC de 24 bits | Corregido | vendor + auditoría previa |
| Headers ADTS reservados | Rechazo correcto; no es un fallo | `adts.rs` valida índices 13/14 |
| Timeouts de resolución de streams | No reproducidos en esta sesión | clientes con timeout 10/15 s, verificación 4 s, `INFLIGHT_TIMEOUT` 30 s; no se ejecutó red real |

`cargo test --test audio_decode`: 6/6 en verde. No se mezclan estos fallos con
el DSP: los tres primeros son de decodificación, el último es de red.

## 3. Problemas detectados

1. No existe una cadena DSP: no hay ecualización, preamp, limitador,
   normalización ni estéreo. El único control es el volumen del player.
2. No hay un punto único de procesamiento PCM común a escritorio y Android.
3. El análisis está correctamente desacoplado, pero no hay forma de comparar
   PCM pre-DSP y post-DSP.
4. No hay medición de asignaciones en la ruta de audio; el bench existente mide
   análisis y visualización, no un pipeline DSP.
5. La configuración de audio no se persiste porque aún no existe.
6. Android no tiene paridad de controles de audio; añadir DSP allí exige un
   contrato JNI nuevo y no puede darse por validado sin hardware.
7. `TapSource` no reenviaba `try_seek` (el seek de escritorio fallaba con el
   análisis activo) ni declaraba fuente nueva (dos pistas consecutivas del
   mismo formato no reiniciaban el reloj de análisis). Corregido en Fase 1
   porque el adaptador DSP depende de ese reenvío para resetear filtros.

## 4. Componentes reutilizables

- `analysis::TapSource` y `analysis::SpScRing`: tap no bloqueante y análisis
  desacoplado.
- `analysis::AnalysisRuntime` y `FeatureBus`: reutilizables sin crear un
  segundo sistema de análisis.
- `media::transport::HttpRangeStream` y `StreamBuffer`: la red queda fuera del
  DSP.
- `PlaybackEngine`/`PlaybackRouter`: punto correcto para publicar parámetros
  sin que la UI conozca rodio.
- `infrastructure::dirs` y `infrastructure::config`: persistencia futura.
- `rodio::Source`: el adaptador DSP puede envolver cualquier fuente sin tocar
  el decodificador.

## 5. Cuellos de botella y riesgos

- El coste dominante sigue siendo red + decodificación; el DSP no debe añadir
  I/O ni esperas.
- El hilo de audio es el hilo interno de rodio/cpal: no admite locks
  bloqueantes, asignaciones ni recálculos costosos.
- `seek` y cambio de pista deben resetear el estado de filtros para no
  arrastrar historia.
- El `current_span_len` de una fuente con prebuffer no se puede reenviar con
  exactitud; el adaptador lo oculta y detecta cambios reales de formato al
  inicio de bloque.
- Si el análisis se colocase después del DSP, la EQ alteraría las features
  acústicas y las recomendaciones. Se mantiene antes.
- El limitador debe ir al final de la cadena, antes del volumen del player; el
  volumen de rodio (0..1) no puede recortar la protección.
- Android exige un trabajo separado: el PCM se remuestrea antes del ring y el
  callback Oboe no puede compartir el mutex de control de escritorio.

## 6. Mejoras propuestas

1. Crear un módulo `dsp` neutral con parámetros `Copy`, control no bloqueante,
   trait de procesador y cadena por bloques.
2. Insertar `DspSource` entre el tap de análisis y el player de rodio.
3. Mantener Flat como salida temprana bit-exacta.
4. Medir el coste de la infraestructura y demostrar cero asignaciones en el
   camino caliente.
5. En fases posteriores: gain/preamp, biquads, limitador, normalización con
   análisis persistido y estéreo opcional, en ese orden de señal.
6. Extender el contrato de parámetros a Android mediante JNI cuando exista una
   prueba en hardware.

## 7. Archivos que requieren intervención

Nuevos en Fase 1:

- `src/dsp/mod.rs`, `params.rs`, `control.rs`, `processor.rs`, `chain.rs`,
  `rodio_source.rs`.
- `examples/bench_dsp.rs`.
- `docs/architecture/dsp-pipeline.md`.
- `docs/audit/dsp-audio-audit.md` (este documento).

Modificados en Fase 1:

- `src/lib.rs` (módulo público).
- `src/infrastructure/playback/rodio_backend.rs` (crear el control y envolver
  la fuente después del tap).
- `src/analysis/tap.rs` (reenviar seek y declarar fuente nueva; necesario para
  que el seek llegue al DSP y para no arrastrar el reloj entre pistas).

Fases posteriores:

- `src/dsp/` (procesadores), `src/app/audio.rs`, `src/app/playback.rs`,
  `src/ui/backend.rs`, `src/ui/settings.rs` o vista nueva, `src/ui/app.rs`,
  `src/infrastructure/config.rs`, `src/platform/android_decoder.rs`,
  `src/platform/android.rs` y la app Kotlin si se lleva DSP a Android.

## 8. Línea base medida

Antes de añadir DSP:

- `cargo test --lib`: 654 tests, 0 fallos, 1 ignorado, 2.02 s.
- `cargo test --test audio_decode`: 6 tests, 0 fallos.
- `cargo run --release --example bench_hotpaths`:
  - FFT + magnitudes: 12.3 µs (presupuesto 11.6 ms).
  - Frame de análisis completo: 17.0 µs.
  - Ring SPSC: 247 M muestras/s (≈2574× tiempo real estéreo 48 k).
  - Hilo de análisis: 20.87× tiempo real.
  - Render TUI completo 80×5: 49.2 µs.

Estas cifras son de esta máquina y este build `--release`; no son una promesa
para otros equipos.
