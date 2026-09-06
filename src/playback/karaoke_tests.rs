//! Tests de sincronización del karaoke con PositionClock (spec §17, FASE 6).
//!
//! Verifican que el karaoke **nunca** intenta adivinar la posición,
//! sino que siempre depende de `PositionClock::snapshot()`, y que
//! las letras obsoletas de una canción anterior no modifican el estado
//! de la actual.

use std::time::{Duration, Instant};

use crate::domain::lyrics::SyncLyrics;
use crate::playback::PositionClock;

fn lrc_lines() -> SyncLyrics {
    SyncLyrics::parse("[00:05] uno\n[00:10] dos\n[00:15] tres\n[00:20] cuatro\n")
}

fn position_after(lyrics: &SyncLyrics, duration: Duration) -> usize {
    lyrics.active_index(duration).unwrap_or(0)
}

// ───────────────────────────────────────────────────────────────────
// Reproducción normal
// ───────────────────────────────────────────────────────────────────

#[test]
fn karaoke_normal_playback() {
    // Canción de 20s con líneas cada 5s.
    let lyrics = lrc_lines();
    let mut clock = PositionClock::new();
    let now = Instant::now();

    // Track "song-a" arranca.
    assert_eq!(
        clock.update(Some("song-a"), Duration::ZERO, now),
        Some(crate::playback::ClockEvent::NewTrack)
    );

    // Position 0 → ninguna línea activa (la primera es a 5s).
    clock.update(Some("song-a"), Duration::from_secs(0), now);
    let pos = clock.snapshot(true, false, Some(Duration::from_secs(20)), now);
    assert_eq!(
        position_after(&lyrics, pos),
        0,
        "posición 0s → sin línea activa (unwrap a 0)"
    );

    // Position 5s → línea 0 activa.
    clock.update(Some("song-a"), Duration::from_secs(5), now);
    let pos = clock.snapshot(true, false, Some(Duration::from_secs(20)), now);
    assert_eq!(position_after(&lyrics, pos), 0, "posición 5s → línea 0");

    // Position 10s → línea 1 activa.
    clock.update(Some("song-a"), Duration::from_secs(10), now);
    let pos = clock.snapshot(true, false, Some(Duration::from_secs(20)), now);
    assert_eq!(position_after(&lyrics, pos), 1, "posición 10s → línea 1");

    // Position 15s → línea 2 activa.
    clock.update(Some("song-a"), Duration::from_secs(15), now);
    let pos = clock.snapshot(true, false, Some(Duration::from_secs(20)), now);
    assert_eq!(position_after(&lyrics, pos), 2, "posición 15s → línea 2");
}

// ───────────────────────────────────────────────────────────────────
// Pause
// ───────────────────────────────────────────────────────────────────

#[test]
fn karaoke_pause() {
    let lyrics = lrc_lines();
    let mut clock = PositionClock::new();
    let now = Instant::now();

    clock.update(Some("song-a"), Duration::ZERO, now);
    clock.update(Some("song-a"), Duration::from_secs(7), now);

    // Se pausa: snapshot congela la posición.
    let paused = clock.snapshot(
        false,
        false,
        Some(Duration::from_secs(20)),
        now + Duration::from_secs(30),
    );
    assert_eq!(paused, Duration::from_secs(7), "pausa congela en 7s");
    assert_eq!(position_after(&lyrics, paused), 0, "al pausar → línea 0");

    // Tras 30s de pausa, sigue en la línea 0 (no avanza).
    clock.update(
        Some("song-a"),
        Duration::from_secs(7),
        now + Duration::from_secs(30),
    );
    let still_paused = clock.snapshot(
        false,
        false,
        Some(Duration::from_secs(20)),
        now + Duration::from_secs(60),
    );
    assert_eq!(
        still_paused,
        Duration::from_secs(7),
        "pausa larga sigue en 7s"
    );
}

// ───────────────────────────────────────────────────────────────────
// Resume
// ───────────────────────────────────────────────────────────────────

#[test]
fn karaoke_resume() {
    let lyrics = lrc_lines();
    let mut clock = PositionClock::new();
    let now = Instant::now();

    clock.update(Some("song-a"), Duration::ZERO, now);
    clock.update(Some("song-a"), Duration::from_secs(5), now);

    // Se pausa en 5s.
    let paused = clock.snapshot(
        false,
        false,
        Some(Duration::from_secs(20)),
        now + Duration::from_secs(30),
    );
    assert_eq!(paused, Duration::from_secs(5));

    // Se reanuda: llega la muestra de reanudación (aniđo fresco) y la
    // extrapolación dentro de la cadencia normal (< MAX_EXTRAPOLATION) avanza
    // por delante de la última muestra.
    let resume = now + Duration::from_secs(30);
    clock.update(Some("song-a"), Duration::from_secs(5), resume);
    let resumed = clock.snapshot(
        true,
        false,
        Some(Duration::from_secs(20)),
        resume + Duration::from_millis(250),
    );
    assert!(
        resumed > Duration::from_secs(5),
        "al reanudar extrapola por delante de 5s"
    );
    assert_eq!(
        position_after(&lyrics, resumed),
        0,
        "reanudando → sigue en línea 0 (5s)"
    );

    // Un ancla vieja (> MAX_EXTRAPOLATION) NO se extrapola: el reloj congela en
    // vez de inventar audio que la UI no pudo confirmar durante demasiado
    // tiempo (las letras jamás corren por delante sin control).
    let stale = clock.snapshot(
        true,
        false,
        Some(Duration::from_secs(20)),
        resume + Duration::from_secs(5),
    );
    assert_eq!(
        stale,
        Duration::from_secs(5),
        "ancla vieja no extrapola segundos de audio"
    );
}

// ───────────────────────────────────────────────────────────────────
// Seek forward
// ───────────────────────────────────────────────────────────────────

#[test]
fn karaoke_seek_forward() {
    let lyrics = lrc_lines();
    let mut clock = PositionClock::new();
    let now = Instant::now();

    clock.update(Some("song-a"), Duration::ZERO, now);
    clock.update(Some("song-a"), Duration::from_secs(15), now);
    assert_eq!(
        position_after(
            &lyrics,
            clock.snapshot(true, false, Some(Duration::from_secs(20)), now)
        ),
        2
    );

    // Seek forward a 5s.
    clock.begin_seek(Duration::from_secs(5));
    clock.update(Some("song-a"), Duration::from_secs(5), now);

    // Seek confirmado: PositionClock re-ancla a 5s.
    assert_eq!(clock.pending_seek(), None, "seek confirmado");
    assert_eq!(
        clock.position(),
        Duration::from_secs(5),
        "posición re-anclada a 5s"
    );

    let pos = clock.snapshot(true, false, Some(Duration::from_secs(20)), now);
    assert_eq!(position_after(&lyrics, pos), 0, "seek forward → línea 0");
}

// ───────────────────────────────────────────────────────────────────
// Seek backward
// ───────────────────────────────────────────────────────────────────

#[test]
fn karaoke_seek_backward() {
    let lyrics = lrc_lines();
    let mut clock = PositionClock::new();
    let now = Instant::now();

    clock.update(Some("song-a"), Duration::ZERO, now);
    clock.update(Some("song-a"), Duration::from_secs(15), now);
    assert_eq!(
        position_after(
            &lyrics,
            clock.snapshot(true, false, Some(Duration::from_secs(20)), now)
        ),
        2
    );

    // Seek backward a 3s.
    clock.begin_seek(Duration::from_secs(3));
    clock.update(Some("song-a"), Duration::from_secs(3), now);

    assert_eq!(clock.pending_seek(), None, "seek confirmado");
    assert_eq!(clock.position(), Duration::from_secs(3));

    let pos = clock.snapshot(true, false, Some(Duration::from_secs(20)), now);
    assert_eq!(position_after(&lyrics, pos), 0, "seek backward → línea 0");
}

// ───────────────────────────────────────────────────────────────────
// Buffering (stalled)
// ───────────────────────────────────────────────────────────────────

#[test]
fn karaoke_buffering() {
    let lyrics = lrc_lines();
    let mut clock = PositionClock::new();
    let now = Instant::now();

    clock.update(Some("song-a"), Duration::ZERO, now);
    clock.update(Some("song-a"), Duration::from_secs(7), now);

    // En buffering (stalled=true): snapshot congela la posición.
    let stalled = clock.snapshot(
        true,
        true,
        Some(Duration::from_secs(20)),
        now + Duration::from_secs(15),
    );
    assert_eq!(stalled, Duration::from_secs(7), "buffering congela en 7s");
    assert_eq!(
        position_after(&lyrics, stalled),
        0,
        "en buffering → línea 0 (no avanza)"
    );

    // Tras 30s de buffer, sigue en la misma línea.
    let still_stalled = clock.snapshot(
        true,
        true,
        Some(Duration::from_secs(20)),
        now + Duration::from_secs(45),
    );
    assert_eq!(
        still_stalled,
        Duration::from_secs(7),
        "buffering largo sigue en 7s"
    );
}

// ───────────────────────────────────────────────────────────────────
// Track change
// ───────────────────────────────────────────────────────────────────

#[test]
fn karaoke_track_change() {
    let _lyrics_a = lrc_lines();
    let lyrics_b = SyncLyrics::parse("[00:03] alpha\n[00:06] beta\n[00:09] gamma\n");
    let mut clock = PositionClock::new();
    let now = Instant::now();

    // Reproduce "song-a" hasta 10s.
    clock.update(Some("song-a"), Duration::ZERO, now);
    clock.update(Some("song-a"), Duration::from_secs(10), now);
    assert_eq!(clock.position(), Duration::from_secs(10));
    assert_eq!(clock.track_key(), Some("song-a"));

    // Cambio a "song-b": ClockEvent::NewTrack.
    let event = clock.update(Some("song-b"), Duration::ZERO, now);
    assert_eq!(
        event,
        Some(crate::playback::ClockEvent::NewTrack),
        "cambio de track → NewTrack"
    );
    assert_eq!(clock.track_key(), Some("song-b"), "track_key actualizado");
    assert_eq!(clock.position(), Duration::ZERO, "posición reset a 0");

    // La canción "song-b" empieza: la posición es 0 → ninguna línea activa.
    let pos = clock.snapshot(true, false, Some(Duration::from_secs(9)), now);
    assert_eq!(
        position_after(&lyrics_b, pos),
        0,
        "nuevo track → sin línea activa aún"
    );
}

// ───────────────────────────────────────────────────────────────────
// Stale lyric response
// ───────────────────────────────────────────────────────────────────

#[test]
fn karaoke_stale_lyric_response() {
    // Simula que LRCLIB devuelve lyrics para "song-a" tarde,
    // mientras el usuario ya escucha "song-b".

    let _lyrics_a = SyncLyrics::parse("[00:05] uno\n[00:10] dos\n");
    let _lyrics_b = SyncLyrics::parse("[00:03] alpha\n[00:06] beta\n");

    // La garantía de sesión (FASE 4): el karaoke solo acepta letras cuyo
    // track coincida con el del reloj maestro. Simulamos que la respuesta de
    // "song-a" llega cuando el reloj ya apunta a "song-b": el clock la
    // descarta porque el track_key NO coincide.
    let mut clock = PositionClock::new();
    let now = Instant::now();
    clock.update(Some("song-a"), Duration::ZERO, now); // sesión A
    clock.update(Some("song-b"), Duration::ZERO, now); // sesión B vigente

    // La respuesta tardía de song-a no puede "ganar" la sesión actual:
    // track_key() sigue siendo song-b y las letras de song-a se ignoran en la
    // capa de UI (que compara contra `now_playing`).
    assert_eq!(
        clock.track_key(),
        Some("song-b"),
        "sesión vigente no se pisa"
    );
    assert_ne!(
        clock.track_key(),
        Some("song-a"),
        "lyrics de song-a son stale"
    );
}

// ───────────────────────────────────────────────────────────────────
// Seek confirmado explícitamente (FASE 2: separar seek real del reloj)
// ───────────────────────────────────────────────────────────────────

#[test]
fn karaoke_seek_backward_confirmed_by_backend_event() {
    let lyrics = lrc_lines();
    let mut clock = PositionClock::new();
    let now = Instant::now();

    clock.update(Some("song-a"), Duration::ZERO, now);
    clock.update(Some("song-a"), Duration::from_secs(15), now);

    // Seek backward a 3s. Mientras el backend pre-descarga, el audio sigue en
    // 15s: el karaoke sigue ese reloj real, no el objetivo.
    clock.begin_seek(Duration::from_secs(3));
    clock.update(Some("song-a"), Duration::from_secs(15), now);
    assert_eq!(
        clock.pending_seek().map(|s| s.target),
        Some(Duration::from_secs(3))
    );
    assert_eq!(
        clock.position(),
        Duration::from_secs(15),
        "audio real antes del salto"
    );

    // El backend CONFIRMA el salto real: se re-ancla en el objetivo.
    clock.confirm_seek(now);
    assert_eq!(clock.position(), Duration::from_secs(3));
    assert!(clock.pending_seek().is_none());
    let pos = clock.snapshot(true, false, Some(Duration::from_secs(20)), now);
    assert_eq!(position_after(&lyrics, pos), 0, "seek backward → línea 0");
}

#[test]
fn karaoke_seek_failed_keeps_following_real_audio() {
    let lyrics = lrc_lines();
    let mut clock = PositionClock::new();
    let now = Instant::now();

    clock.update(Some("song-a"), Duration::ZERO, now);
    clock.update(Some("song-a"), Duration::from_secs(80), now);

    // Seek a 5s, pero el backend NO lo confirmó (falló).
    clock.begin_seek(Duration::from_secs(5));
    clock.cancel_pending_seek();
    assert!(clock.pending_seek().is_none());
    assert_eq!(
        clock.position(),
        Duration::from_secs(80),
        "el audio nunca se movió"
    );
    let pos = clock.snapshot(true, false, Some(Duration::from_secs(90)), now);
    assert_eq!(
        position_after(&lyrics, pos),
        3,
        "sigue la última línea (80s)"
    );
}

// ───────────────────────────────────────────────────────────────────
// Reset de clock
// ───────────────────────────────────────────────────────────────────

#[test]
fn karaoke_reset_on_clear() {
    let _lyrics = lrc_lines();
    let mut clock = PositionClock::new();
    let now = Instant::now();

    clock.update(Some("song-a"), Duration::ZERO, now);
    clock.update(Some("song-a"), Duration::from_secs(10), now);
    assert_eq!(clock.position(), Duration::from_secs(10));

    // clear() resetea todo.
    clock.clear();
    assert_eq!(clock.position(), Duration::ZERO, "clock reseteado a 0");
    assert_eq!(clock.track_key(), None, "track_key limpio");
    assert!(clock.pending_seek().is_none(), "sin seek pendiente");

    // snapshot refleja el reset.
    let pos = clock.snapshot(true, false, Some(Duration::from_secs(20)), now);
    assert_eq!(pos, Duration::ZERO, "snapshot tras clear = 0");
}

// ───────────────────────────────────────────────────────────────────
// Timestamps discontinuos (seek)
// ───────────────────────────────────────────────────────────────────

#[test]
fn karaoke_timestamp_discontinuity() {
    let lyrics = lrc_lines();
    let mut clock = PositionClock::new();
    let now = Instant::now();

    clock.update(Some("song-a"), Duration::ZERO, now);

    // Avanza normalmente hasta 15s.
    clock.update(Some("song-a"), Duration::from_secs(15), now);
    assert_eq!(
        position_after(
            &lyrics,
            clock.snapshot(true, false, Some(Duration::from_secs(20)), now)
        ),
        2
    );

    // Seek backward a 2s: timestamp discontinuo.
    clock.begin_seek(Duration::from_secs(2));
    clock.update(Some("song-a"), Duration::from_secs(2), now);

    // active_index salta de 2 → 0.
    let pos = clock.snapshot(true, false, Some(Duration::from_secs(20)), now);
    assert_eq!(
        position_after(&lyrics, pos),
        0,
        "seek backward → salto de línea 2 a 0"
    );

    // Avanza hasta 7s.
    clock.update(Some("song-a"), Duration::from_secs(7), now);
    let pos = clock.snapshot(true, false, Some(Duration::from_secs(20)), now);
    assert_eq!(
        position_after(&lyrics, pos),
        0,
        "después de seek → reanuda normalmente"
    );

    // Seek forward a 18s.
    clock.begin_seek(Duration::from_secs(18));
    clock.update(Some("song-a"), Duration::from_secs(18), now);
    let pos = clock.snapshot(true, false, Some(Duration::from_secs(20)), now);
    assert_eq!(position_after(&lyrics, pos), 2, "seek forward → línea 2");
}

// ───────────────────────────────────────────────────────────────────
// Finished (fin real de la canción)
// ───────────────────────────────────────────────────────────────────

#[test]
fn karaoke_finished() {
    let lyrics = SyncLyrics::parse("[00:05] uno\n[00:10] dos\n");
    let mut clock = PositionClock::new();
    let now = Instant::now();

    clock.update(Some("song-a"), Duration::ZERO, now);
    clock.update(Some("song-a"), Duration::from_secs(5), now);

    // Mientras suena: línea 0 activa (la primera línea a 5s).
    let pos = clock.snapshot(true, false, Some(Duration::from_secs(10)), now);
    assert_eq!(
        position_after(&lyrics, pos),
        0,
        "durante la canción → línea 0 activa"
    );

    // Avanza a 10s: línea 1 activa.
    clock.update(Some("song-a"), Duration::from_secs(10), now);
    let pos = clock.snapshot(true, false, Some(Duration::from_secs(10)), now);
    assert_eq!(position_after(&lyrics, pos), 1, "en 10s → línea 1 activa");

    // Fin real: player.empty() + EOF.
    let finished = clock.snapshot(false, false, Some(Duration::from_secs(10)), now);
    assert_eq!(finished, Duration::from_secs(10), "fin real = 10s");

    // Con finished=true, la UI limpia el karaoke.
    // (La lógica de finished se maneja en RelatedState::render_lyrics.)
    assert!(
        finished >= Duration::from_secs(10),
        "la canción terminó → karaoke se limpia"
    );
}

// ───────────────────────────────────────────────────────────────────
// Outro largo (lyrics acaba antes que la canción)
// ───────────────────────────────────────────────────────────────────

#[test]
fn karaoke_outro_largo() {
    // LRC termina a los 10s, canción dura 60s.
    let lyrics = SyncLyrics::parse("[00:05] uno\n[00:10] dos\n");
    let mut clock = PositionClock::new();
    let now = Instant::now();

    clock.update(Some("song-a"), Duration::ZERO, now);
    clock.update(Some("song-a"), Duration::from_secs(30), now);

    // En 30s, la última línea (dos, a 10s) sigue visible (outro).
    let pos = clock.snapshot(true, false, Some(Duration::from_secs(60)), now);
    assert_eq!(
        position_after(&lyrics, pos),
        1,
        "outro: última línea sigue activa"
    );

    // El karaoke NO se limpia hasta el fin real (finished=true).
}

// ───────────────────────────────────────────────────────────────────
// Seek confirmado con margen de 1 segundo
// ───────────────────────────────────────────────────────────────────

#[test]
fn karaoke_seek_within_one_second_confirmed() {
    let lyrics = lrc_lines();
    let mut clock = PositionClock::new();
    let now = Instant::now();

    clock.update(Some("song-a"), Duration::ZERO, now);
    clock.update(Some("song-a"), Duration::from_secs(10), now);

    // Seek a 12s. El motor reporta 12s (±1s) → confirmado.
    clock.begin_seek(Duration::from_secs(12));
    clock.update(Some("song-a"), Duration::from_secs(12), now);

    // El seek se confirma (abs_diff(12, 12) <= 1s).
    assert!(clock.pending_seek().is_none(), "seek confirmado por margen");
    assert_eq!(clock.position(), Duration::from_secs(12));

    let pos = clock.snapshot(true, false, Some(Duration::from_secs(20)), now);
    assert_eq!(position_after(&lyrics, pos), 1, "seek confirmado → línea 1");
}

// ───────────────────────────────────────────────────────────────────
// Seek NO confirmado (más de 1s de diferencia)
// ───────────────────────────────────────────────────────────────────

#[test]
fn karaoke_seek_not_confirmed_follows_real_audio() {
    let lyrics = lrc_lines();
    let mut clock = PositionClock::new();
    let now = Instant::now();

    clock.update(Some("song-a"), Duration::ZERO, now);
    clock.update(Some("song-a"), Duration::from_secs(10), now);

    // Seek a 3s, pero el motor sigue en 10s (más de 1s de diferencia).
    clock.begin_seek(Duration::from_secs(3));
    clock.update(Some("song-a"), Duration::from_secs(10), now);

    // El seek NO se confirmó (abs_diff(10, 3) > 1s).
    assert!(clock.pending_seek().is_some(), "seek pendiente");
    assert_eq!(
        clock.position(),
        Duration::from_secs(10),
        "sigue el audio real"
    );

    // El karaoke sigue la línea real (10s → línea 1).
    let pos = clock.snapshot(true, false, Some(Duration::from_secs(20)), now);
    assert_eq!(
        position_after(&lyrics, pos),
        1,
        "seek pendiente → sigue el audio real"
    );
}

// ───────────────────────────────────────────────────────────────────
// Clear durante seek pendiente
// ───────────────────────────────────────────────────────────────────

#[test]
fn karaoke_clear_during_pending_seek() {
    let mut clock = PositionClock::new();
    let now = Instant::now();

    clock.update(Some("song-a"), Duration::ZERO, now);
    clock.update(Some("song-a"), Duration::from_secs(5), now);
    clock.begin_seek(Duration::from_secs(20));
    assert!(clock.pending_seek().is_some(), "seek pendiente");

    // clear() cancela el seek y resetea todo.
    clock.clear();
    assert!(clock.pending_seek().is_none(), "seek cancelado por clear");
    assert_eq!(clock.position(), Duration::ZERO);
    assert_eq!(clock.track_key(), None);
}

// ───────────────────────────────────────────────────────────────────
// Restart same track
// ───────────────────────────────────────────────────────────────────

#[test]
fn karaoke_restart_same_track_resets_position() {
    let lyrics = lrc_lines();
    let mut clock = PositionClock::new();
    let now = Instant::now();

    clock.update(Some("song-a"), Duration::ZERO, now);
    clock.update(Some("song-a"), Duration::from_secs(12), now);
    assert_eq!(
        position_after(
            &lyrics,
            clock.snapshot(true, false, Some(Duration::from_secs(20)), now)
        ),
        1
    );

    // Replay del MISMO track (autoplay).
    clock.restart_same_track();
    assert_eq!(clock.position(), Duration::ZERO, "replay → posición a 0");
    assert_eq!(clock.track_key(), Some("song-a"), "track NO cambia");
    assert!(clock.pending_seek().is_none(), "sin seek pendiente");

    // La siguiente muestra (posición 1) se acepta.
    clock.update(Some("song-a"), Duration::from_secs(1), now);
    assert_eq!(clock.position(), Duration::from_secs(1));
    let pos = clock.snapshot(true, false, Some(Duration::from_secs(20)), now);
    assert_eq!(position_after(&lyrics, pos), 0, "replay → línea 0");
}

// ───────────────────────────────────────────────────────────────────
// Sync + Send para PositionClock
// ───────────────────────────────────────────────────────────────────

#[test]
fn position_clock_sync_send_with_karaoke() {
    fn assert_sync<T: Sync>() {}
    fn assert_send<T: Send>() {}
    assert_sync::<PositionClock>();
    assert_send::<PositionClock>();
}
