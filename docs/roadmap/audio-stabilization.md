# Roadmap de estabilización de audio

Estado: 2026-10-30.

## Hecho

- [x] Diagnóstico de transporte, sesión y AAC.
- [x] Cancelación de `HttpRangeStream` (open, lecturas, reintentos, cuerpo).
- [x] Backoff exponencial con techo/jitter y clasificación de `Cancelled`.
- [x] Validación de `200` truncado.
- [x] Generación de motor en rodio: commit bajo lock, monitor y
      `reset_after_failed_play` con recheck.
- [x] `pause_requested` para conservar pausa durante prebuffer.
- [x] Época de intención + commit serializado en `ui/backend.rs`.
- [x] Recuperación que no pisa una canción nueva y mensaje honesto.
- [x] PCE AAC (ASC e in-band), sample rate 24-bit, realineado multi-bloque,
      ADTS CRC y MPEG-2.
- [x] Android: token de cancelación, errores de decode acotados, cambio de
      `SignalSpec`, join fuera del mutex, seek rechazado.
- [x] Fixtures y tests reproducibles.
- [x] Cancelación durante el cuerpo lento y mensajes sin `<<>>`.
- [x] Fase 1 DSP: infraestructura de cadena, parámetros `Copy`, control no
      bloqueante, ruta Flat bit-exacta, adaptador rodio y benchmark con 0
      asignaciones en el camino caliente. Ver `docs/architecture/dsp-pipeline.md`.
- [x] Fase 2 DSP: preamp, EQ biquad de 10 bandas, estéreo, normalización y
      limitador con lookahead; cadena estándar integrada en `DspSource`,
      tests por procesador y benchmark de la cadena activa. Sin UI ni
      persistencia todavía.

## Siguiente

1. **Fase 3 DSP**: UI de controles, persistencia de perfiles y análisis de
   loudness/picos para normalización.
2. **Validar Android en hardware** con la matriz de
   `docs/testing/audio-validation.md`; registrar TTFA y cancelación real.
3. **Rastrear el `external_id` vacío**: el mensaje ya no muestra `<<>>`, pero
   falta instrumentar el proveedor (source, fase, presencia de id) sin URLs ni
   tokens para encontrar el origen.
4. **Medir el lock de commit**: si `history.stats()` bloquea nuevas órdenes,
   mover la persistencia fuera del lock con una comprobación de época antes
   de cada escritura de estado.
5. **Recuperación con posición**: opcional; si se implementa, hacer
   `play` + `seek` best-effort y solo prometer “seguimos donde estabas” si el
   seek tuvo éxito.
6. **AAC**: decidir si se soportan SBR/PS o se documenta el rechazo; añadir
   fixtures HE-AAC para que el límite sea explícito.
7. **Transporte**: añadir prueba de `Cut` en el monitor de rodio (la
   cancelación durante cuerpo lento ya está cubierta).

## Riesgos

- El lock de commit cubre la persistencia; bajo SQLite lento puede retrasar
  una orden nueva. Es una decisión deliberada de consistencia.
- El decoder Android depende de que el token de transporte sea suficiente
  para desbloquear todas las fases; Symphonia podría bloquearse fuera de él.
- El vendor AAC sigue siendo una superficie mantenida localmente; cada
  actualización de Symphonia requiere reaplicar el patch.
- La validación Android pendiente no permite afirmar que TTFA o estabilidad
  de AudioTrack estén resueltas en dispositivo.

## Criterio de cierre

- Todos los comandos obligatorios en verde.
- `cargo test --test audio_decode` en verde.
- Matriz Android ejecutada en al menos un dispositivo físico.
- Sin mensajes de error con identificadores vacíos (`<<>>`).
