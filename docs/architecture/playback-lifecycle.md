# Ciclo de vida de una reproducción

## TUI de escritorio

1. **Intención.** `BackendCommand::Play/Next/Prev/Autoplay` entra al backend.
   `begin_playback_epoch()` incrementa `playback_epoch`. Si el motor está en
   `Buffering` (consulta sin efectos laterales vía `is_buffering()`), se
   cancela para no bloquear la orden nueva con el prebuffer viejo.
2. **Resolución.** `StreamResolver::resolve` corre fuera de
   `playback_commit`. Puede tardar; otra orden puede adelantarla.
3. **Chequeo de época.** Al volver de red, si `epoch` ya no es el vigente se
   devuelve `"reproducción reemplazada por otra orden"` y no se toca nada.
4. **Commit.** Bajo `playback_commit`: se comprueba otra vez la época, se
   marca la cola, se fija `current`/`current_context`, se arma el presupuesto
   de recuperación y se llama `PlaybackRouter::play`.
5. **Arranque del motor.** El motor incrementa su `generation`, cancela el
   buffer anterior, emite `Buffering` y prebuffea. Al terminar, comprueba su
   generación bajo el lock de estado; si sigue vigente, hace
   `player.clear/append/(pause|play)`, emite `Playing` o `Paused` y lanza el
   monitor.
6. **Persistencia.** Solo tras el arranque vigente se guarda track, señal
   `Play`/`RecClick`, historial y estadísticas. Si la época cambió mientras
   `router.play` arrancaba, se para el motor antes de devolver: el stop no
   puede quedar revivido por un arranque tardío.
7. **Monitor.** Lee estado+generación bajo el mismo lock. Detecta EOF, `Cut`,
   errores del buffer, pausa por refill y resume. Un monitor de generación
   vieja sale sin publicar eventos.

### Pausa durante el prebuffer

`pause_requested` es un `AtomicBool` del motor. `pause()` lo pone a true;
`play()` lo resetea al empezar (nueva sesión) y el commit lo lee justo antes
de arrancar. Si el usuario pausó mientras se llenaba el buffer, el player
queda pausado y se emite `Paused`, no `Playing`.

### Stop

`BackendCommand::Stop` incrementa la época (invalida `Play`/recuperación en
vuelo) y llama `router.stop()`. El motor incrementa generación, cancela el
buffer, para el player, limpia track/duración y emite `Stopped`. Un `play`
anterior que arranque justo después comprueba la época al volver y para el
motor; no puede revivir el stop.

### Fallo y recuperación

- El monitor publica `PlaybackEvent::Error` solo si la generación sigue
  vigente.
- El backend aplica `RecoveryBudget` (un refresco por track).
- `recover_current` captura la época, re-resuelve, toma `playback_commit` y
  verifica época + que `current` siga siendo el mismo track antes de
  `router.play`.
- Si otra reproducción ganó, para cualquier motor que hubiera arrancado y
  devuelve `RecoveryOutcome::Superseded` sin mensaje. Si falla, conserva el aviso original. Si funciona, avisa que la
  reproducción **se reinició desde el inicio**; no finge conservar posición.

## Android

1. Kotlin llama `playAudio` con URL, headers y `tap_to_decoder_us`.
2. `tunefold_oboe_play_stream` toma el mutex `lifecycle`, cancela el hilo
   anterior, hace join, resetea ring/posición/flags, arranca un hilo decoder e
   instala su handle antes de soltar el mutex. `stop` usa el mismo mutex, así
   que no puede retirar un handle que aún no existía ni limpiar el ring de una
   reproducción nueva.
3. `AndroidDecoder::decode_stream` abre el transporte con token de
   cancelación, hace probe, crea decoder y alimenta `PcmRing` + `PcmTap`.
4. Si Symphonia cambia de `SignalSpec`, recrea `SampleBuffer`/resampler.
5. `EndOfMedia` publica `track_finished` (consumible una vez). `Cancelled` no.
6. `stop` pone `playing=false`, `cancel=true`, hace join fuera del mutex y
   limpia el ring **después** del join.
7. Un error de decode no se ignora indefinidamente: 3 fallos iniciales u 8
   consecutivos terminan la sesión con `STATE_ERROR`.

## Transiciones de estado

```
Idle ──Play──► Buffering ──buffer ok──► Playing ⇄ Paused
                  │                       │
                  ├─ stop ──► Stopped     ├─ stop ──► Stopped
                  ├─ error ─► Error       ├─ EOF ──► Finished/Stopped
                  └─ refill ► Buffering   └─ refill ► Buffering
```

`Finished` no existe como estado del motor de escritorio: EOF se publica como
evento y la cola decide. En Android, `track_finished` es la señal de fin real.
