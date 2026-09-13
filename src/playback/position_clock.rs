//! Reloj de posición de reproducción: el audio es la fuente de verdad
//! temporal (spec §17).
//!
//! El motor reporta la posición (`player.get_pos()`, derivada de muestras
//! consumidas) solo cada ~500 ms; este reloj mantiene una lectura continua:
//!
//! - **monótona por clasificación de drift**: la muestra del motor es la
//!   verdad; un retroceso se clasifica (blip espurio ≤ [`SPURIOUS_BLIP`],
//!   drift moderado, discontinuidad → reinicio del mismo track) y solo el
//!   blip se ignora — los retrocesos reales se re-anclan a la muestra;
//! - **extrapolada** mientras se reproduce (última muestra + tiempo real
//!   transcurrido desde ella, pero jamás más allá de [`MAX_EXTRAPOLATION`]),
//!   congelada en pausa/stall y cuando el ancla quedó demasiado vieja
//!   (p. ej. la UI saturada no recibió una muestra): no se inventa tiempo;
//! - **re-anclada en CADA muestra avance** (posición mayor que la anterior),
//!   aunque el avance sea pequeño: así no acumula el tiempo de una pausa larga;
//!   una muestra con la MISMA posición NO re-ancla (el ticker/flap de estado
//!   intermedio no debe reiniciar la extrapolación y producir retrocesos), salvo
//!   cuando el ancla quedó fría (> [`MAX_EXTRAPOLATION`]) — reanudar tras un
//!   stall largo debe volver a arrancar la rampa;
//! - **continua (monótona en reproducción)**: el horizonte de extrapolación
//!   supera la cadencia de muestreo del motor ([`MAX_EXTRAPOLATION`]), de modo
//!   que durante la reproducción normal el ancla nunca se enfría entre muestras
//!   y la lectura crece sin el "diente de sierra" (sube a cada muestra y se cae
//!   a media rampa) que hacía saltar la línea activa del karaoke. Como última
//!   red, además, toda lectura en reproducción se sujeta a la última entregada
//!   (piso de continuidad), que se descarta cuando la reproducción realmente
//!   retrocede o cambia de track;
//! - **con seek pendiente**: mientras el motor no confirma el salto, sigue el
//!   reloj REAL del audio (que sigue sonando desde donde estaba) y solo al
//!   confirmarse adopta el objetivo.
//!
//! Consumidores: letras sincronizadas/karaoke, análisis y visualización. La
//! lógica fue extraída VERBATIM del reloj del karaoke de `ui/app.rs`; los
//! tests originales cubren ahora este módulo directamente.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Retroceso máximo tolerable sin considerarlo real: un pequeño blip espurio
/// del motor (re-buffer que reinicia transitoriamente su contador) no debe
/// rebobinar las letras. Por debajo de esto la muestra se ignora.
const SPURIOUS_BLIP: Duration = Duration::from_millis(500);
/// Retroceso mayor que esto (o hasta ~0) es una discontinuidad: reinicio del
/// MISMO track (recuperación en caliente, replay, reset del backend) cuyo
/// evento dedicado llegó a destiempo. Se re-ancla a lo reportado y se avisa con
/// [`ClockEvent::Restarted`]: la letra sigue siendo válida, solo se rebobina.
const RESTART_THRESHOLD: Duration = Duration::from_secs(10);
/// Horizonte máximo de extrapolación.
///
/// El motor reporta cada ~500 ms (ticker del backend). El límite debe quedar
/// POR ENCIMA de esa cadencia (650 ms = 500 + ~30% de holgura): si fuera menor,
/// en cada intervalo el ancla se enfriaría ANTES de la siguiente muestra y, al
/// expirar antes de que llegara el reporte nuevo, la lectura se caería por
/// ~un intervalo y volvería a subir en el frame siguiente — un diente de
/// sierra que, sobre un LRC denso (líneas a decenas de ms), hacía que la
/// línea activa saltara entre líneas (retroceso o doble avance en un frame).
///
/// Con el ancla siempre "caliente" durante la reproducción continua, la
/// lectura crece monótona y el karaoke cruza cada límite de línea una sola
/// vez, en cascada suave. El papel de freno que tenía el horizonte corto
/// (no adelantarse a un audio congelado) lo cumplen ahora los estados REALES
/// que el backend reenvía de inmediato (`Buffering`/`Paused`/`Stopped`/`Error`,
/// ver `forwards_real_state`) y la bandera `stalled` del tick: un corte
/// congelado llega a la UI como un cambio de estado que apaga la extrapolación
/// al instante. El horizonte queda solo como límite de último recurso si todo
/// eso fallara (casi inalcanzable en la práctica).
const MAX_EXTRAPOLATION: Duration = Duration::from_millis(650);

/// Seek solicitado aún no confirmado por el motor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingSeek {
    pub target: Duration,
}

/// Limita un objetivo de seek relativo a los límites reales del audio
/// (spec §15): nunca negativo ni más allá de `duration` (si es conocida).
///
/// `base` es la posición actual y `delta` el desplazamiento en segundos
/// (negativo = retroceder). Único sitio que acota el salto, compartido por la
/// línea de tiempo (vista Metadata) y el keybinding `Left`/`Right`.
pub fn clamp_seek_target(base: Duration, delta: i64, duration: Option<Duration>) -> Duration {
    let target = (base.as_secs() as i64).saturating_add(delta).max(0) as u64;
    match duration {
        Some(total) if !total.is_zero() && Duration::from_secs(target) > total => total,
        _ => Duration::from_secs(target),
    }
}

/// Qué cambió al incorporar una muestra (para que los consumidores reaccionen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockEvent {
    /// Sin track activo: el reloj se apagó (canción terminada/detenida).
    Cleared,
    /// Track nuevo o primera muestra tras limpiar: los consumidores deben
    /// descartar estado dependiente del track anterior (p. ej. letras).
    NewTrack,
    /// Reinicio del MISMO track detectado por discontinuidad (retroceso grande
    /// o hasta ~0 con la misma identidad): la letra sigue siendo válida pero la
    /// ventana del karaoke debe rebobinar con la reproducción.
    Restarted,
}

/// Reloj maestro de posición.
#[derive(Debug, Default)]
pub struct PositionClock {
    /// Identificador estable del track al que pertenece `position`.
    track_key: Option<String>,
    position: Duration,
    seek: Option<PendingSeek>,
    synced_at: Option<Instant>,
    /// Piso de continuidad de las lecturas en reproducción: la última lectura
    /// extrapolada entregada. Impide que una muestra plana o un evento de
    /// estado intermedio hagan "caer" la lectura unos milisegundos (salto
    /// extraño de las letras cerca de los límites). Se descarta cuando la
    /// reproducción retrocede de verdad (seek, restart, drift, track nuevo).
    last_playing_read: Mutex<Option<Duration>>,
}

impl PositionClock {
    pub fn new() -> Self {
        Self::default()
    }

    /// Incorpora una muestra del motor.
    ///
    /// `key` es el identificador estable del track reproducido (`None` cuando
    /// el motor reporta paro/cancelación). Devuelve el evento correspondiente
    /// para que la UI limpie lo dependiente del track anterior.
    pub fn update(
        &mut self,
        key: Option<&str>,
        reported: Duration,
        now: Instant,
    ) -> Option<ClockEvent> {
        let Some(key) = key else {
            if self.track_key.is_some() || self.position != Duration::ZERO {
                *self = Self::new();
                return Some(ClockEvent::Cleared);
            }
            return None;
        };

        // Track nuevo: reloj arranca en la posición reportada y los
        // consumidores descartan la letra/estado anterior.
        if self.track_key.as_deref() != Some(key) {
            self.track_key = Some(key.to_string());
            self.position = reported;
            self.synced_at = Some(now);
            self.seek = None;
            *self.last_playing_read.lock().unwrap() = None;
            return Some(ClockEvent::NewTrack);
        }

        // Seek pendiente: el salto puede tardar algún evento (el motor
        // pre-descarga la región objetivo). Mientras tanto el audio sigue
        // sonando en la posición real reportada; el guard monótono quedaría
        // bloqueando un salto hacia atrás, así que aquí se le hace caso omiso.
        if let Some(seek) = self.seek {
            if reported.abs_diff(seek.target) <= Duration::from_secs(1) {
                // Confirmación del motor: se re-ancla en el OBJETIVO elegido,
                // no en una muestra anterior que llegara por carrera.
                self.position = seek.target;
                self.synced_at = Some(now);
                self.seek = None;
                *self.last_playing_read.lock().unwrap() = None;
            } else {
                self.position = reported;
                self.synced_at = Some(now);
            }
            return None;
        }

        // Misma canción: clasificar la dirección de la muestra frente a lo
        // que ya reportamos (la muestra del motor es la fuente de verdad; el
        // reloj no debe inventar ni ignorar retrocesos reales).
        let old_position = self.position;
        let (event, refresh_anchor) = if reported >= old_position {
            // Adelanto (o igual): el motor avanza y el reloj adopta lo
            // reportado como base. El ancla SOLO se refresca cuando la
            // posición AVANZA o cuando quedó fría (> MAX_EXTRAPOLATION): una
            // muestra plana a mitad de intervalo no debe reiniciar la rampa y
            // provocar un micro-retroceso entre muestras (eso lo mantiene
            // suave además el piso de continuidad de [`Self::snapshot`]); pero
            // al REANUDAR tras una pausa/stall largo, el ancla frío debe
            // refrescarse para que la extrapolación arranque de nuevo — si no,
            // la letra se quedaría clavada en la última posición hasta que
            // llegara un avance nuevo.
            self.position = reported;
            let cold = self
                .synced_at
                .is_none_or(|t| now.saturating_duration_since(t) > MAX_EXTRAPOLATION);
            (None, reported > old_position || cold)
        } else {
            // Retroceso respecto a lo reportado.
            let delta = old_position - reported;
            if delta > RESTART_THRESHOLD {
                // Discontinuidad: el mismo track volvió a un punto anterior
                // muy lejano (o a ~0). En vez de congelarnos en una posición
                // obsoleta (lo que NO era el bug reportado pero sí una
                // rigidez), re-anclamos a lo que el motor dice y rebobinamos
                // la ventana de letras con la reproducción.
                self.position = reported;
                *self.last_playing_read.lock().unwrap() = None;
                (Some(ClockEvent::Restarted), true)
            } else if delta > SPURIOUS_BLIP {
                // Drift moderado hacia atrás (re-anclaje suave, sin salto
                // visual: entre 0.5s y 10s). El piso se descarta: el motor es
                // la verdad y retrocede de verdad.
                self.position = reported;
                *self.last_playing_read.lock().unwrap() = None;
                (None, true)
            } else {
                // Blip espurio (re-buffer que reinicia transitoriamente su
                // contador): se ignora; no se toca la posición.
                (None, false)
            }
        };
        if refresh_anchor {
            self.synced_at = Some(now);
        }
        event
    }

    /// Registra un seek del usuario aún sin confirmar.
    pub fn begin_seek(&mut self, target: Duration) {
        self.seek = Some(PendingSeek { target });
    }

    /// Confirma el seek EXTERNAMENTE (el backend reportó éxito).
    ///
    /// A diferencia de la confirmación heurística de [`Self::update`], esta
    /// re-ancla el reloj al objetivo elegido de inmediato porque el backend lo
    /// confirmó como salto REAL del audio. No depende de que una muestra
    /// posterior del motor coincida con el objetivo. También se usa para
    /// confirmar el seek cuando el motor reporta el estado tras el salto.
    ///
    /// Clasifica el destino frente a la posición actual sin tocarlo: un salto
    /// hacia atrás dentro del rango normal es un seek real (se re-ancla); un
    /// salto hacia atrás enorme entra como reinicio (autoplay que vuelve, o un
    /// evento de seek de una sesión confusa) pero siempre adopta el objetivo.
    pub fn confirm_seek(&mut self, now: Instant) {
        if let Some(seek) = self.seek.take() {
            self.position = seek.target;
            self.synced_at = Some(now);
            *self.last_playing_read.lock().unwrap() = None;
        }
    }

    /// Cancela un seek pendiente (p. ej. llegó otra orden antes de confirmar).
    pub fn cancel_pending_seek(&mut self) {
        self.seek = None;
    }

    /// Replay del MISMO track (autoplay que vuelve a su inicio): rebobina el
    /// reloj a cero SIN cambiar de track — la letra sigue siendo válida.
    pub fn restart_same_track(&mut self) {
        self.position = Duration::ZERO;
        self.synced_at = None;
        self.seek = None;
        *self.last_playing_read.lock().unwrap() = None;
    }

    /// Apaga el reloj por completo (cambio de canción pedido por el usuario:
    /// nada de la anterior debe sobrevivir).
    pub fn clear(&mut self) {
        *self = Self::new();
    }

    /// Establece EXPLÍCITAMENTE el track en curso y su posición de arranque.
    ///
    /// Fuerza un nuevo arranque (como si llegara una primera muestra de un
    /// track distinto) y avisa a los consumidores para que descarten el estado
    /// dependiente del track anterior (letras del karaoke). Lo usa la UI cuando
    /// recibe `PlaybackStarted`/`Playback` de una canción nueva: hace la
    /// transición determinista sin depender de que llegue una muestra con un
    /// `identifier` distinto (p. ej. un autoplay que repite el MISMO track.
    pub fn start_track(&mut self, key: &str, at: Duration, now: Instant) -> ClockEvent {
        self.position = at;
        self.synced_at = Some(now);
        self.seek = None;
        self.track_key = Some(key.to_string());
        *self.last_playing_read.lock().unwrap() = None;
        ClockEvent::NewTrack
    }

    /// Lectura "ahora mismo".
    ///
    /// Mientras reproduce (sin stall ni ancla vieja) extrapola con el tiempo
    /// transcurrido desde la última muestra, pero nunca más allá de
    /// [`MAX_EXTRAPOLATION`]: un ticker perdido no debe inventar segundos de
    /// audio. En pausa/stall queda congelada. Nunca supera `duration` si esta
    /// es conocida: la letra no debe "terminar" antes de tiempo porque el motor
    /// dejara de reportar.
    ///
    /// En reproducción ACTIVA (extrapolando) la lectura es MONÓTONA (piso de
    /// continuidad): nunca entrega menos que la anterior. Es la defensa contra
    /// los micro-retrocesos de la extrapolación entre muestras (una muestra
    /// plana o un evento de estado intermedio re-ancla la base sin mover la
    /// posición; sin el piso, la letra "saltaría hacia atrás" un instante). El
    /// piso solo vive mientras la extrapolación está SANO: en pausa/stall o
    /// con el ancla fría se entrega la posición exacta (y se descarta el piso),
    /// para no sostener un valor que la verdad del motor ya no respalda.
    pub fn snapshot(
        &self,
        playing: bool,
        stalled: bool,
        duration: Option<Duration>,
        now: Instant,
    ) -> Duration {
        let extrapolate = playing
            && !stalled
            && self
                .synced_at
                .is_some_and(|t| now.saturating_duration_since(t) <= MAX_EXTRAPOLATION);
        let value = if extrapolate {
            let t = self.synced_at.expect("extrapolate ⇒ synced_at");
            self.position + now.saturating_duration_since(t)
        } else {
            self.position
        };
        let value = match duration {
            Some(total) if !total.is_zero() && value > total => total,
            _ => value,
        };
        let mut floor = self.last_playing_read.lock().unwrap();
        if extrapolate {
            let out = value.max(floor.unwrap_or(Duration::ZERO));
            *floor = Some(out);
            out
        } else {
            // Extrapolación no respaldada (pausa, stall o ancla fría): el piso
            // expira junto con la confianza en la rampa.
            *floor = None;
            value
        }
    }

    /// Posición base (sin extrapolación): útil para aserciones y depuración.
    pub fn position(&self) -> Duration {
        self.position
    }

    pub fn track_key(&self) -> Option<&str> {
        self.track_key.as_deref()
    }

    pub fn pending_seek(&self) -> Option<PendingSeek> {
        self.seek
    }

    /// SOLO TESTS: fuerza el instante de anclaje para simular muestras viejas
    /// (p. ej. una pausa larga) sin depender del reloj real.
    #[cfg(test)]
    pub fn force_anchor(&mut self, at: Instant) {
        self.synced_at = Some(at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> Instant {
        Instant::now()
    }

    #[test]
    fn classifies_blips_reanchors_drift_and_restarts() {
        let mut c = PositionClock::new();
        let now = t0();
        assert_eq!(
            c.update(Some("a"), Duration::from_secs(10), now),
            Some(ClockEvent::NewTrack)
        );
        // Blip espurio pequeño (< 500 ms): se ignora, la letra no parpadea.
        assert_eq!(c.update(Some("a"), Duration::from_millis(9_600), now), None);
        assert_eq!(c.position(), Duration::from_secs(10));
        // Drift moderado hacia atrás (0.5s..10s): re-anclaje suave a la muestra
        // (el motor es la fuente de verdad), sin avisar como reinicio.
        assert_eq!(c.update(Some("a"), Duration::from_secs(8), now), None);
        assert_eq!(c.position(), Duration::from_secs(8));
        // Y avanza normal después.
        c.update(Some("a"), Duration::from_secs(15), now);
        assert_eq!(c.position(), Duration::from_secs(15));
    }

    #[test]
    fn large_backward_jump_is_a_restart_of_same_track() {
        let mut c = PositionClock::new();
        let now = t0();
        c.update(Some("a"), Duration::from_secs(120), now);
        // El mismo track reinicia muy atrás (o a ~0): re-ancla a lo reportado y
        // avisa para rebobinar la ventana de letras. La identidad se conserva:
        // la letra SÍ sigue siendo la del track en curso.
        assert_eq!(
            c.update(Some("a"), Duration::from_secs(3), now),
            Some(ClockEvent::Restarted)
        );
        assert_eq!(c.position(), Duration::from_secs(3));
        assert_eq!(c.track_key(), Some("a"));
    }

    #[test]
    fn new_track_event_and_reset_from_reported_position() {
        let mut c = PositionClock::new();
        let now = t0();
        c.update(Some("a"), Duration::from_secs(90), now);
        assert_eq!(
            c.update(Some("b"), Duration::from_secs(3), now),
            Some(ClockEvent::NewTrack),
            "cambiar de track avisa a los consumidores"
        );
        assert_eq!(c.position(), Duration::from_secs(3));
        assert_eq!(c.track_key(), Some("b"));
    }

    #[test]
    fn cleared_when_motor_reports_no_track() {
        let mut c = PositionClock::new();
        let now = t0();
        c.update(Some("a"), Duration::from_secs(5), now);
        assert_eq!(
            c.update(None, Duration::ZERO, now),
            Some(ClockEvent::Cleared)
        );
        assert_eq!(c.position(), Duration::ZERO);
        assert_eq!(c.track_key(), None);
        // Un segundo vacío consecutivo no repite el evento.
        assert_eq!(c.update(None, Duration::ZERO, now), None);
    }

    #[test]
    fn pending_seek_follows_real_audio_until_confirmed() {
        let mut c = PositionClock::new();
        let now = t0();
        c.update(Some("a"), Duration::from_secs(100), now);

        // El usuario pide 50s: mientras el motor pre-descarga, el audio sigue
        // en ~100s y el karaoke debe seguirlo (no congelarse en el objetivo).
        c.begin_seek(Duration::from_secs(50));
        c.update(Some("a"), Duration::from_secs(100), now);
        assert_eq!(c.position(), Duration::from_secs(100));
        assert!(c.pending_seek().is_some());

        // El motor llega al objetivo: el seek termina anclado al ELEGIDO.
        c.update(Some("a"), Duration::from_secs(50), now);
        assert_eq!(c.position(), Duration::from_secs(50));
        assert!(c.pending_seek().is_none());

        // Tras resolver, un blip espurio pequeño se ignora (no rebobina).
        c.update(Some("a"), Duration::from_millis(49_800), now);
        assert_eq!(c.position(), Duration::from_secs(50));
        // Un reinicio grande del MISMO track (a ~0) se re-ancla y avisa.
        assert_eq!(
            c.update(Some("a"), Duration::ZERO, now),
            Some(ClockEvent::Restarted)
        );
        assert_eq!(c.position(), Duration::ZERO);
    }

    #[test]
    fn extrapolates_while_playing_freezes_paused_and_clamps_to_duration() {
        let mut c = PositionClock::new();
        let now = t0();
        c.update(Some("a"), Duration::from_secs(42), now);

        // Extrapolación hacia adelante mientras reproduce...
        std::thread::sleep(Duration::from_millis(15));
        let s1 = c.snapshot(true, false, Some(Duration::from_secs(200)), Instant::now());
        assert!(s1 > Duration::from_secs(42), "extrapola entre muestras");

        // ...se congela en pausa...
        assert_eq!(
            c.snapshot(false, false, Some(Duration::from_secs(200)), Instant::now()),
            Duration::from_secs(42)
        );
        // ...y en stall también.
        assert_eq!(
            c.snapshot(true, true, Some(Duration::from_secs(200)), Instant::now()),
            Duration::from_secs(42)
        );

        // Nunca supera la duración conocida: la base YA está en el límite y
        // la extrapolación lo supera → clamp.
        c.update(Some("a"), Duration::from_secs(200), now);
        std::thread::sleep(Duration::from_millis(15));
        assert_eq!(
            c.snapshot(true, false, Some(Duration::from_secs(200)), Instant::now()),
            Duration::from_secs(200),
            "clamp a duración"
        );
    }

    #[test]
    fn long_pause_does_not_leak_into_extrapolation() {
        let mut c = PositionClock::new();
        c.update(Some("a"), Duration::from_secs(50), t0());
        // Simula una muestra llegada mucho después CON la misma posición
        // (ticker durante pausa): el re-anclaje evita saltar 10 minutos.
        let later = Instant::now();
        c.update(
            Some("a"),
            Duration::from_secs(50),
            later + Duration::from_secs(600),
        );
        let snap = c.snapshot(false, false, Some(Duration::from_secs(200)), Instant::now());
        assert_eq!(snap, Duration::from_secs(50), "pausa larga no contamina");
    }

    #[test]
    fn restart_same_track_rewinds_but_keeps_identity() {
        let mut c = PositionClock::new();
        let now = t0();
        c.update(Some("a"), Duration::from_secs(120), now);
        c.begin_seek(Duration::from_secs(10));

        c.restart_same_track();
        assert_eq!(c.position(), Duration::ZERO);
        assert!(c.pending_seek().is_none());
        assert_eq!(c.track_key(), Some("a"), "el track NO cambia");

        // La siguiente muestra (posición 1 del replay) se acepta: partía de 0.
        c.update(Some("a"), Duration::from_secs(1), now);
        assert_eq!(c.position(), Duration::from_secs(1));
    }

    #[test]
    fn clear_drops_everything() {
        let mut c = PositionClock::new();
        c.update(Some("a"), Duration::from_secs(9), t0());
        c.begin_seek(Duration::from_secs(2));
        c.clear();
        assert_eq!(c.track_key(), None);
        assert_eq!(c.position(), Duration::ZERO);
        assert!(c.pending_seek().is_none());
    }

    #[test]
    fn confirm_seek_anchors_to_target_without_waiting_for_a_matching_sample() {
        let mut c = PositionClock::new();
        let now = t0();
        c.update(Some("a"), Duration::from_secs(100), now);
        // El audio real está en 100s; el usuario pide 20s.
        c.begin_seek(Duration::from_secs(20));
        // El backend confirma el salto: re-ancla en 20s aunque el audio real
        // (que ya se movió) aún no haya mandado una muestra con esa posición.
        c.confirm_seek(now);
        assert_eq!(c.position(), Duration::from_secs(20));
        assert!(c.pending_seek().is_none());
    }

    #[test]
    fn cancel_pending_seek_returns_to_real_audio() {
        let mut c = PositionClock::new();
        let now = t0();
        c.update(Some("a"), Duration::from_secs(80), now);
        c.begin_seek(Duration::from_secs(5));
        c.cancel_pending_seek();
        assert!(c.pending_seek().is_none());
        assert_eq!(
            c.position(),
            Duration::from_secs(80),
            "el audio nunca se movió"
        );
    }

    #[test]
    fn clamp_seek_target_never_goes_below_zero() {
        assert_eq!(
            clamp_seek_target(Duration::from_secs(5), -10, None),
            Duration::ZERO,
            "retroceder desde 5s diez segundos siempre produce 0, nunca underflow"
        );
        assert_eq!(clamp_seek_target(Duration::ZERO, -1, None), Duration::ZERO);
    }

    #[test]
    fn clamp_seek_target_clamps_to_known_duration() {
        assert_eq!(
            clamp_seek_target(Duration::from_secs(190), 10, Some(Duration::from_secs(200))),
            Duration::from_secs(200),
            "no salta más allá de la duración"
        );
        // Sin duración conocida se entrega el objetivo tal cual.
        assert_eq!(
            clamp_seek_target(Duration::from_secs(190), 10, None),
            Duration::from_secs(200)
        );
    }

    #[test]
    fn clamp_seek_target_keeps_deltas_inside_the_range() {
        assert_eq!(
            clamp_seek_target(Duration::from_secs(100), 10, Some(Duration::from_secs(200))),
            Duration::from_secs(110)
        );
        assert_eq!(
            clamp_seek_target(
                Duration::from_secs(100),
                -10,
                Some(Duration::from_secs(200))
            ),
            Duration::from_secs(90)
        );
    }

    // ───────────────────────────────────────────────────────────────────
    // Drift / extrapolación acotada (FASE de sincronización rítmica)
    // ───────────────────────────────────────────────────────────────────

    #[test]
    fn snapshot_caps_extrapolation_when_anchor_stales() {
        let mut c = PositionClock::new();
        let now = t0();
        c.update(Some("a"), Duration::from_secs(30), now);

        // El motor deja de reportar (ticker perdido, UI saturada) pero el
        // estado sigue "Playing": la extrapolación NUNCA se dispara más allá
        // de MAX_EXTRAPOLATION. No se inventan segundos de audio.
        let stale_read = c.snapshot(
            true,
            false,
            Some(Duration::from_secs(200)),
            now + Duration::from_secs(5),
        );
        assert_eq!(
            stale_read,
            Duration::from_secs(30),
            "ancla vieja: congela, no inventa audio"
        );

        // Dentro de la cadencia normal (p. ej. 400 ms) sí extrapola.
        let fresh = c.snapshot(
            true,
            false,
            Some(Duration::from_secs(200)),
            now + Duration::from_millis(400),
        );
        assert!(
            fresh > Duration::from_secs(30) && fresh <= Duration::from_secs(31),
            "extrapolación acotada dentro de la cadencia: {fresh:?}"
        );
    }

    #[test]
    fn out_of_order_samples_reanchor_to_the_newer_report() {
        // Muestras que llegan en desorden (un tick atascado se vacía tarde):
        // el reloj debe seguir la ÚLTIMA reportada, nunca quedarse clavado en
        // una lectura antigua que llegó después por retraso.
        let mut c = PositionClock::new();
        let now = t0();
        c.update(Some("a"), Duration::from_secs(40), now);
        // Reportes nuevos que llegan "antes" que una lectura vieja no deben
        // hacer retroceder al clock a la lectura vieja si esta es un blip.
        c.update(Some("a"), Duration::from_secs(45), now);
        // Lectura vieja por retraso, moderada (dentro del rango re-anclable):
        // re-ancla suavemente a lo reportado.
        c.update(Some("a"), Duration::from_secs(41), now);
        assert_eq!(c.position(), Duration::from_secs(41));
    }

    #[test]
    fn extrapolation_never_exceeds_duration() {
        let mut c = PositionClock::new();
        let now = t0();
        c.update(Some("a"), Duration::from_secs(199), now);
        let read = c.snapshot(
            true,
            false,
            Some(Duration::from_secs(200)),
            now + Duration::from_millis(900),
        );
        assert!(read <= Duration::from_secs(200), "nunca supera la duración");
    }

    #[test]
    fn long_session_with_stalls_pauses_seeks_and_restarts_never_ends_ahead() {
        // El síntoma reportado: tras una sesión larga con pausas, stalls,
        // seeks y una recuperación/replay, las letras terminan PROGRESIVAMENTE
        // ADELANTADAS. Aquí se simula esa sesión y se verifica que, en cada
        // punto tras una muestra real del motor, la posición nunca queda por
        // delante de lo que el audio reporta (salvo la extrapolación acotada
        // de la cadencia normal).
        let mut c = PositionClock::new();
        let mut now = t0();

        // Reproducción normal, 0s → 30s (muestras cada ~0.5s).
        c.update(Some("song"), Duration::ZERO, now);
        now += Duration::from_millis(500);
        c.update(Some("song"), Duration::from_millis(500), now);
        now += Duration::from_millis(500);
        c.update(Some("song"), Duration::from_secs(1), now);

        // Stall largo en 1s: el motor deja de avanzar (buffer vacío).
        now += Duration::from_secs(20);
        // Stall: la lectura no extrapola.
        assert_eq!(
            c.snapshot(true, true, Some(Duration::from_secs(300)), now),
            Duration::from_secs(1),
            "stall: congelado"
        );

        // Se reanuda el flujo: el reloj se re-ancla a la muestra nueva.
        now += Duration::from_millis(500);
        c.update(Some("song"), Duration::from_secs(1), now);
        let after_resume = c.snapshot(
            true,
            false,
            Some(Duration::from_secs(300)),
            now + Duration::from_millis(400),
        );
        assert!(
            after_resume <= Duration::from_secs(2),
            "tras reanudar no se aleja del audio: {after_resume:?}"
        );

        // Pausa en 100s.
        for s in (2..=100u64).step_by(2) {
            now += Duration::from_millis(500);
            c.update(Some("song"), Duration::from_secs(s), now);
        }
        let paused_at = c.snapshot(false, false, Some(Duration::from_secs(300)), now);
        assert_eq!(paused_at, Duration::from_secs(100));
        now += Duration::from_secs(45); // pausa larga
        let still = c.snapshot(false, false, Some(Duration::from_secs(300)), now);
        assert_eq!(still, Duration::from_secs(100), "pausa larga no avanza");

        // Seek hacia atrás 100s → 40s (con su periodo de confirmación).
        c.begin_seek(Duration::from_secs(40));
        now += Duration::from_millis(500);
        c.update(Some("song"), Duration::from_secs(100), now); // aún pre-descarga
        assert_eq!(c.position(), Duration::from_secs(100));
        c.update(Some("song"), Duration::from_secs(40), now); // confirma
        assert_eq!(c.position(), Duration::from_secs(40));
        assert!(c.pending_seek().is_none());

        // Recuperación en caliente: replay del MISMO track desde 0.
        c.restart_same_track();
        assert_eq!(c.position(), Duration::ZERO);
        now += Duration::from_millis(500);
        c.update(Some("song"), Duration::ZERO, now);
        assert_eq!(c.position(), Duration::ZERO);

        // Sigue reproduciendo hasta 180s: en cada re-anclaje el reloj está
        // al menos tan atrás como el audio reportado (nunca por delante una
        // cantidad acumulada).
        for s in (1u64..=180).step_by(3) {
            now += Duration::from_millis(500);
            c.update(Some("song"), Duration::from_secs(s), now);
            let read = c.snapshot(
                true,
                false,
                Some(Duration::from_secs(300)),
                now + Duration::from_millis(500),
            );
            // La extrapolación de la cadencia puede superar la muestra en un
            // intervalo, pero NUNCA en más de MAX_EXTRAPOLATION ni acumulando.
            assert!(
                read <= Duration::from_secs(s) + MAX_EXTRAPOLATION,
                "las letras no se adelantan progresivamente (s={s}, read={read:?})"
            );
        }

        // Un fallo (error no recuperable): el motor queda Stopped, pero la UI
        // recibe el estado real y detiene el reloj → en cada muestra posterior
        // (o congelada) no puede estar delante.
        c.update(Some("song"), Duration::from_secs(180), now);
        now += Duration::from_secs(30);
        // El motor reporta Stopped congelado a 180s (como en `PlaybackEvent::Error`).
        let frozen = c.snapshot(false, false, Some(Duration::from_secs(300)), now);
        assert_eq!(
            frozen,
            Duration::from_secs(180),
            "tras el error la UI congela el reloj, no lo extrapola"
        );
    }

    #[test]
    fn extrapolation_never_reverts_within_normal_sample_cadence() {
        // El horizonte de extrapolación debe superar la cadencia del motor
        // (ticker de 500 ms). Si fuera menor, a mitad de intervalo el ancla se
        // enfriaría y la lectura se CAERÍA de vuelta a la posición confirmada
        // (diente de sierra): sobre un LRC denso eso hacía saltar la línea
        // activa entre líneas. Dentro de la cadencia la lectura crece estricta
        // y monotonamente, sin retroceder jamás.
        let mut c = PositionClock::new();
        let t0 = Instant::now();
        c.update(Some("a"), Duration::from_secs(45), t0);
        assert!(MAX_EXTRAPOLATION > Duration::from_millis(500));

        let mut prev = Duration::from_secs(45);
        let mut readings = Vec::new();
        for ms in (50..=480).step_by(30) {
            let read = c.snapshot(
                true,
                false,
                Some(Duration::from_secs(200)),
                t0 + Duration::from_millis(ms),
            );
            assert!(
                read >= prev,
                "la lectura nunca cae dentro de la cadencia ({ms} ms): {read:?} < {prev:?}"
            );
            assert!(
                read <= Duration::from_millis(45_000 + ms),
                "no se adelanta más que el tiempo real ({ms} ms): {read:?}"
            );
            prev = read;
            readings.push(ms);
        }
        assert!(
            !readings.is_empty() && prev > Duration::from_secs(45),
            "extrapoló a lo largo de toda la cadencia"
        );
    }
    #[test]
    fn equal_position_samples_do_not_reanchor_the_extrapolation() {
        // Una muestra con la MISMA posición (p. ej. el ticker reenviando el
        // estado, o un flap Buffering/Playing) a mitad de intervalo no debe
        // reiniciar la extrapolación: si re-anclara y reiniciara la rampa,
        // la lectura se caería a la posición exacta y volvería a subir — el
        // "salto extraño" de las letras entre muestras.
        let mut c = PositionClock::new();
        let t0 = Instant::now();
        c.update(Some("a"), Duration::from_secs(42), t0);
        let read1 = c.snapshot(
            true,
            false,
            Some(Duration::from_secs(200)),
            t0 + Duration::from_millis(300),
        );
        assert_eq!(read1, Duration::from_millis(42_300));

        // Muestra plana llegada a mitad del intervalo...
        c.update(
            Some("a"),
            Duration::from_secs(42),
            t0 + Duration::from_millis(300),
        );
        // ...la extrapolación CONTINÚA desde el ancla original (400 ms tras la
        // muestra, no "cae" a 42 + 100 ms).
        let read2 = c.snapshot(
            true,
            false,
            Some(Duration::from_secs(200)),
            t0 + Duration::from_millis(400),
        );
        assert_eq!(
            read2,
            Duration::from_millis(42_400),
            "una muestra plana no reinicia la rampa de extrapolación"
        );
    }

    #[test]
    fn playing_reads_never_step_backwards_across_flat_intermediate_events() {
        // Incluso si un evento intermedio consigue tocar el ancla sin mover la
        // posición, la lectura entregada es MONÓTONA (piso de continuidad): un
        // flap de estado no puede hacer "saltar hacia atrás" la letra aunque el
        // valor subyacente caiga.
        let mut c = PositionClock::new();
        let t0 = Instant::now();
        c.update(Some("a"), Duration::from_secs(42), t0);
        let r1 = c.snapshot(
            true,
            false,
            Some(Duration::from_secs(200)),
            t0 + Duration::from_millis(300),
        );
        assert!(r1 > Duration::from_secs(42));

        // El backend re-ancla con la misma posición 250 ms después (peor caso:
        // el valor subyacente cae de 42.30 a 42.05).
        c.force_anchor(t0 + Duration::from_millis(300));
        let r2 = c.snapshot(
            true,
            false,
            Some(Duration::from_secs(200)),
            t0 + Duration::from_millis(350),
        );
        assert!(r2 >= r1, "la lectura nunca retrocede: {r1:?} → {r2:?}");
        assert_eq!(
            r2,
            Duration::from_millis(42_300),
            "el piso sostiene la rampa"
        );

        // Y cuando el valor subyacente vuelve a superar el piso, sigue subiendo.
        let r3 = c.snapshot(
            true,
            false,
            Some(Duration::from_secs(200)),
            t0 + Duration::from_millis(700),
        );
        assert!(r3 > r2, "la rampa se reanuda al superar el piso: {r3:?}");
    }

    #[test]
    fn continuity_floor_drops_on_real_backward_moves() {
        let mut c = PositionClock::new();
        let t0 = Instant::now();
        c.update(Some("a"), Duration::from_secs(120), t0);
        let read = c.snapshot(
            true,
            false,
            Some(Duration::from_secs(300)),
            t0 + Duration::from_millis(300),
        );
        assert!(read > Duration::from_secs(120));

        // Un seek real hacia atrás DESCARTÓ el piso: la lectura vuelve a servir
        // la posición verdadera (baja) sin quedarse pegada al valor extrapolado.
        c.begin_seek(Duration::from_secs(50));
        c.confirm_seek(t0);
        let after = c.snapshot(
            true,
            false,
            Some(Duration::from_secs(300)),
            t0 + Duration::from_millis(50),
        );
        assert_eq!(
            after,
            Duration::from_millis(50_050),
            "piso descartado al buscar atrás"
        );

        // Replay del mismo track: parte de cero sin piso heredado y, al no
        // haber ancla aún, sirve la posición exacta (0) — nunca un valor
        // extrapolado de la canción anterior.
        c.restart_same_track();
        let replay = c.snapshot(
            true,
            false,
            Some(Duration::from_secs(300)),
            t0 + Duration::from_millis(100),
        );
        assert_eq!(replay, Duration::ZERO, "replay desde 0 sin piso ni ancla");
    }
}
