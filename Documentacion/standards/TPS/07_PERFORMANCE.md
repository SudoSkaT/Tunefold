# 07 · Rendimiento y eficiencia

## Presupuestos

TPS no inventa límites universales de CPU, RAM, batería o latencia: dependen de dispositivo y subsistema y no hay baseline Android de dispositivo disponible en esta auditoría. Antes de fijar límites MUST registrar el baseline reproducible (modelo/API/Hz/build/escenario) y acordar presupuesto por función. Comparar misma máquina, fuente, configuración y duración.

Al menos medir en un cambio crítico: latencia acción→feedback, arranque/TTFA cuando corresponda, CPU, memoria/alloc/GC, frame pacing para UI, energía para trabajo continuo. Reportar distribución y no sólo promedio. Sin medición, describir optimización como hipótesis.

## Reglas obligatorias

- Ninguna operación de red/disco/DB en hilo principal.
- Ningún trabajo de UI bloquea audio; audio no espera locks contenciosos, UI, I/O ni red (contrato existente DSP/Android).
- Reutilizar buffers en ciclos frecuentes; limitar tamaño; cancelar work/listeners al dejar de necesitarse.
- Invalidar sólo ante cambio observable. Evitar polling en reposo y visibilidad nula.
- Cachear geometría o contenido sólo si evita coste repetido medido y se invalida correctamente.
- Mantener imágenes acotadas, listas grandes en ventana/virtualizadas, y liberar recursos con lifecycle.
- No introducir optimización prematura ni dependencia de profiling/runtime sin justificar política, coste y licencia.

## Presupuestos por ruta

| Ruta | Comportamiento de diseño | Señal para validar |
|---|---|---|
| Audio real-time | sin I/O/alloc inesperada/bloqueo | xrun/drop y contrato DSP; ver docs audio |
| Análisis | buffers reusados y publicación desacoplada | hops/tasa, alloc declaradas en código |
| Visual | trabajo proporcional a datos nuevos y pantalla visible | draw/read/frame metrics |
| Listas | ventana visible + prefetch acotado | scroll jank, número de filas construidas |
| Artwork | decode acotado y extracción pequeña | memoria/tiempo/cache hit |
| Playback/resolución | async/cancelable, guardas de generación | latencia, cancelación y stale callbacks |

## Validación disponible

La matriz oficial es `Documentacion/engineering/VERIFICATION.md`. Android tiene tests JVM y scripts de dispositivo descritos en `android/README.md`; métricas reales requieren emulator/device. Los reportes históricos de rendimiento no equivalen a baseline vigente. Capturar trazas antes y después; no declarar batería/fps si no se midió.
