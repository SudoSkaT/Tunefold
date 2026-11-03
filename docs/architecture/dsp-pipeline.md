# Arquitectura del pipeline DSP

Estado: Fase 2 (procesadores) implementada y verificada. La Fase 3 integra la
UI y la persistencia. Android todavía no usa esta cadena.

## Cadena

```
Stream HTTP
  → StreamBuffer
  → Decoder (Symphonia / rodio)
  → TapSource (análisis PRE-DSP)
  → DspSource (bloques de 256 frames)
       → ProcessorChain
            ├─ gain / preamp              (implementado)
            ├─ ecualizador biquad         (implementado)
            ├─ estéreo opcional           (implementado)
            ├─ ganancia de normalización  (implementado)
            └─ limitador de picos         (implementado)
  → Player rodio → MixerDeviceSink → dispositivo
```

El análisis se mantiene antes del DSP. Las features acústicas alimentan
recomendaciones y visualización; no deben cambiar porque el usuario mueva la
ecualización. La señal que suena es la salida de la cadena; la que se analiza
es la del decodificador. Una futura vista podrá ofrecer un tap post-DSP
opcional, pero no se crea un segundo motor de análisis.

Android no usa todavía esta cadena. Su PCM se remuestrea a 48 kHz y entra en
`PcmRing`; llevar el DSP allí exige un contrato JNI nuevo y validación en
hardware. El módulo `dsp` no depende de rodio y está preparado para ese uso.

## Módulos

| Archivo | Responsabilidad |
|---|---|
| `src/dsp/params.rs` | Modelo canónico `Copy`, perfiles de bandas, saneado de NaN/infinitos y clasificación Flat. |
| `src/dsp/control.rs` | `DspControl`: publicación de parámetros con `try_lock`; el audio nunca espera. |
| `src/dsp/processor.rs` | Trait `AudioProcessor` + `StreamFormat`. |
| `src/dsp/chain.rs` | `ProcessorChain`: refresco de parámetros por bloque, orden fijo, estado entre bloques, reset y composición `standard()`. |
| `src/dsp/rodio_source.rs` | `DspSource`: adaptador de `rodio::Source`, preasigna el bloque y procesa in place. |
| `src/dsp/smooth.rs` | Suavizado de ganancia en dB y conversión dB/lineal. |
| `src/dsp/gain.rs` | Preamplificación con rampa por muestra. |
| `src/dsp/biquad.rs` | Ecualizador de 10 bandas con biquads RBJ y estado por canal. |
| `src/dsp/stereo.rs` | Balance, anchura estéreo y crossfeed con delay/lowpass. |
| `src/dsp/normalize.rs` | Ganancia de normalización ReplayGain/LUFS con protección de picos. |
| `src/dsp/limiter.rs` | Limitador de picos con lookahead, attack/release y techo. |
| `examples/bench_dsp.rs` | Coste por bloque/muestra y sonda de asignaciones. |

## Contrato de tiempo real

- La composición (`Vec<Box<dyn AudioProcessor>>`) y los buffers se crean en el
  hilo de control, antes de `Player::append`.
- `ProcessorChain::process`:
  1. intenta leer una versión nueva con `try_lock`;
  2. si el perfil es Flat, retorna sin tocar el bloque;
  3. si no, llama a los procesadores con el mismo bloque in place.
- Nunca hay `lock()` bloqueante, `await`, I/O, SQLite ni red en el camino de
  audio.
- Los procesadores deben ser `Send`, no asignar memoria, no bloquear y tratar
  cualquier bloque como continuación del anterior.
- `prepare(format)` se llama al construir la fuente y cuando cambia el formato.
- `reset()` se llama en `seek` y en discontinuidades; los filtros no arrastran
  estado de otra posición.

## Publicación de parámetros

`DspControl` es un `Arc<Mutex<Published>>` con `DspParams` `Copy`:

- El productor (control) usa `set()`: sanea, compara y, si cambió, incrementa
  la versión. El lock solo cubre un memcpy pequeño.
- El consumidor (audio) usa `try_snapshot()`. Si el lock está ocupado, conserva
  la versión anterior y reintenta en el bloque siguiente.
- `DspParams` no contiene `Vec` ni `String`: leerlo no asigna memoria.
- El cambio se aplica en el siguiente límite de bloque (≤256 frames). Las
  transiciones de ganancia, EQ, estéreo y limitador se aplican en el siguiente
  bloque; la preamp y el estéreo interpolan por muestra para evitar clics.

La versión evita que los procesadores recalculen coeficientes en cada bloque
si los parámetros no han cambiado.

## Ruta Flat

`DspParams::flat()` es la referencia: sin tono, sin normalización, sin estéreo,
sin limitador. `DspParams::is_flat()` también reconoce un perfil con secciones
desactivadas aunque conserven valores. La cadena retorna antes de tocar las
muestras, así que Flat es bit-exacto. `bypassed()` apaga el interruptor global.

## Bloque y latencia

- `DSP_BLOCK_FRAMES = 256`: 5.8 ms a 44.1 kHz, 5.3 ms a 48 kHz.
- El prebuffer es un `Vec<f32>` único por fuente, dimensionado para 8 canales
  (5.1/7.1). Por encima de 8 canales se hace passthrough para no asignar en el
  hilo de audio.
- El adaptador añade un bloque de buffering; no hay buffering adicional entre
  bloques aparte del lookahead del limitador.
- `DspSource::current_span_len()` devuelve `None` porque lee por delante. El
  decodificador mantiene un formato constante durante la pista; si cambia, el
  adaptador reinicia los procesadores al inicio del bloque siguiente.
- El limitador añade un lookahead de 5 ms (hasta 192 kHz; se limita a 960
  frames por encima de esa tasa). La latencia total añadida es, por tanto,
  aproximadamente un bloque + 5 ms: ~10.8 ms a 44.1 kHz.
- El limitador no emite un tail de silencio al terminar la fuente: las últimas
  ~5 ms de audio pueden quedar en su buffer de delay. Es una limitación
  conocida hasta que el adaptador soporte flush de cola.

## Orden de procesadores implementado

1. **Ganancia/preamplificación**: ajusta el nivel de entrada; rampa por muestra.
2. **Ecualizador**: biquads en cascada con estado por canal y coeficientes
   precalculados; solo procesa bandas con ganancia distinta de cero.
3. **Estéreo**: balance, anchura y crossfeed; solo para 2 canales.
4. **Normalización**: ganancia de sonoridad ReplayGain/LUFS con protección de
   picos; no hay compresión dinámica.
5. **Limitador**: último antes del volumen del player; techo y release
   configurables, sin limitar cuando no hace falta.

Este orden evita que un boost de EQ eluda la protección y que la normalización
se aplique sobre una señal ya recortada. El volumen de rodio queda después y
solo puede atenuar (0..1).

## Pruebas

- `cargo test --lib dsp::`: parámetros, saneado, versiones, no bloqueo del
  snapshot, Flat bit-exacto, estado entre bloques, reset, adaptador rodio y
  cada procesador (ganancia, EQ, estéreo, normalización y limitador).
- `cargo run --release --example bench_dsp`: coste por bloque/muestra y
  verificación de 0 asignaciones en 20 000 bloques.
- `cargo test --test audio_decode`: AAC/ADTS sigue en verde; el DSP no toca el
  decodificador.

## Resultados medidos (Fase 2)

Una ejecución de `bench_dsp` en esta máquina, build `--release`, bloque estéreo
de 256 frames a 44.1 kHz (presupuesto 5.8 ms), con preamp +3 dB, dos bandas de
EQ activas, width 1.2 y limitador:

| Medición | Resultado |
|---|---|
| Bloque en crudo (referencia) | 0.42 µs |
| Cadena Flat, 0 procesadores | 44 ns |
| Cadena estándar activa (5 procesadores) | 17.4 µs |
| `DspSource` Flat por muestra | 5 ns |
| `Iterator` crudo por muestra | 1 ns |
| Asignaciones en 20 000 bloques | 0 |

La cadena estándar activa consume ~0.3 % de un núcleo a 44.1 kHz; la Flat,
~0.0008 %. El adaptador añade ~4 ns por muestra, ≈0.035 % de CPU a 88.2 k
muestras/s. Las cifras varían entre ejecuciones y equipos; la comparación
válida es contra el mismo hardware y build.

La suite completa queda en 710 tests por defecto, 733 con `youtube` y 738 con
`android` (1 ignorado), más 6 tests de `audio_decode`. `bench_hotpaths`
mantiene el frame de análisis en ~17 µs.

## Limitaciones conocidas

- No hay UI de ecualización ni persistencia de perfiles hasta Fase 3.
- La normalización no mide loudness ni picos todavía: consume `gain_db`,
  `source_lufs` y `source_peak` ya calculados/persistidos. Si `source_peak` es
  0 (desconocido), la protección de picos no puede actuar.
- El limitador es de un solo polo, sin oversampling; el techo se aplica también
  como clamp final por muestra.
- El limitador no hace flush de cola: se pierden hasta ~5 ms al final.
- Android no comparte todavía la cadena; su PCM sigue yendo a `PcmRing`.
- No se ha validado el DSP en hardware Android.
- La visualización sigue siendo pre-DSP; una representación post-DSP requerirá
  un tap adicional sobre el mismo runtime de análisis, no un segundo sistema.
- Una fuente con más de 8 canales queda en passthrough mientras mantenga ese
  formato; si cambia a ≤8 canales, el adaptador vuelve a preparar la cadena
  usando los buffers ya reservados, sin asignar en el hilo de audio.
