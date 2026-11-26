# 06 · Visualización de audio

## Subsistema y pipeline observado

Android y TUI son pipelines distintos. Android: decoder Rust → PCM f32 estéreo 48 kHz/ring y tap de análisis → análisis/waveform bus → JNI `readLiveWaveform` → `LiveAudioBars` buffer preasignado → `SoundBarsView` geometría/path → Canvas/VSync del sistema. Fuentes: `src/platform/android_decoder.rs`, `src/analysis/{engine,waveform}.rs`, `TunefoldBridge.java`, `LiveAudioBars.java`, `SoundBarsView.java`. No cambiar contratos JNI ni tap en una intervención visual.

El análisis observado usa FFT 2048 y hop 512; a 44.1 kHz equivale a ventana ~46.4 ms y publicación ~86.1 Hz; tasa cambia con sample rate. UI solicita lectura cada 50 ms (20 Hz) sólo con pantalla visible, reproductor abierto y estado PLAYING. En pausa/buffering mantiene frame; si generación no cambia evita `invalidate`. La vista tiene un carril envolvente combinado de L/R, máximo 96 barras, un `Path` y `Paint` persistentes; geometría se recalcula al tamaño. Estos valores son evidencia de código, no prueba de FPS de pantalla.

`android/README.md` tenía descripciones de dos carriles; se corrigieron durante esta intervención. Quedan secciones del README con instrucciones de diagnóstico heredadas que deben contrastarse con los scripts/código antes de usarlas. `Documentacion/docs/android/visual-system.md` documenta identidad, paleta y layout, mientras que los detalles actuales de render se confirman en `SoundBarsView`.

## Semántica y requisitos

- MUST conservar min/max medidos por canal y la semántica documentada del bus. Una envolvente combinada puede ocultar separación estéreo; no llamarla representación estéreo separada.
- MUST no fabricar ni etiquetar valores interpolados como nuevas mediciones.
- SHOULD presentar a VSync si se busca mayor fluidez. Interpolación visual puede estimar posición entre dos mediciones válidas, con timestamp y límites explícitos; los extremos medidos y amplitud real deben conservarse, picos no deben desaparecer. Exige experimento/revisión y no se da por implementado.
- Attack/release/smoothing configurable sólo si se separa medición de render, mantiene picos con política testeada y no introduce retraso engañoso. Ahora `SoundBarsView` dibuja medida con expansión de raíz, sin interpolación temporal ni attack/release.
- En silencio debe comunicarse reposo sin ruido visual; pausa conserva frame, cambio/parada limpia según estado explícito, buffering no inventa muestras.
- Transición de pista invalida generación previa; seek Android no soportado actualmente.
- Reducir movimiento puede sustituir visual por estado estático; la reproducción nunca depende de la vista.
- MAY conservar estrategia actual de 20 Hz mientras satisface requisitos y mediciones; 60/90/120 Hz no es meta automática.

## Relojes separados

1. **Medición/publicación**: frecuencia definida por hop/sample rate y datos que efectivamente existen.
2. **Presentación**: oportunidad de UI/VSync. Puede repetirse la medida más reciente; sólo interpolar visualmente con política futura, jamás ocultar timestamps/realidad de origen.

El lector no debe aumentar trabajo de audio para perseguir refresco de pantalla. Nada de polling activo cuando surface no visible o audio no produce datos.

## Instrumentación y brecha actual

Hoy `SoundBarsView.stats()` expone barras pintadas, repaints, lecturas, nuevas generaciones, pico y último tiempo de `onDraw`; `LiveAudioBars` mantiene conteos de poll/update. No encontramos fps efectivo, frame-time percentiles, jank/frame misses, invalidaciones acumuladas formales, alloc/GC correlacionado ni consumo CPU/batería del visualizador. No hay mediciones numéricas reclamadas en TPS.

Cuando se instrumente, recopilar ventana y dispositivo/build: cadencia de muestras; lecturas vs muestras nuevas; invalidaciones/draws; frame-time p50/p95/p99 y missed frames por FrameMetrics/JankStats o herramienta ya disponible; alloc/GC en perfil separado; CPU y energía por experimento controlado. Informar resolución, Hz, estado, duración y baseline. Evitar logs en hot path.

## Casos de validación

Señal constante/senoidal/silencio; picos cortos; estéreo con canales distintos; mono; pausa/resume; buffering; track change rápido; stop/EOF; tamaño/rotación; Activity stop/start; surface fuera de pantalla. Comprobar fidelidad al bus, stale data, trabajo en reposo y coste.
