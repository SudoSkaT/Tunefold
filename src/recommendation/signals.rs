//! Modelo de señales de interacción del usuario (FASE 11).
//!
//! Cada `PlaySignal` es UN evento de interacción con UN track, con su contexto.
//! Distingue señales que el historial antiguo mezclaba (play ≠ completed ≠ skip)
//! y permite que el perfil (FASE 10) no trate `play` como `like` ni `skip` como
//! `dislike` sin analizar el contexto.
//!
//! El modelo es puramente local y autónomo por fila, pensado para poder migrar
//! a un sistema remoto sin modificar el dominio: no hay estado agregado, solo
//! eventos con marca de tiempo e identificador opcional de recomendación.

/// Tipo de interacción. Cada variante es una señal distinta con un peso
/// distinto al construir el perfil.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignalKind {
    /// Se inició una reproducción (punto de partida, sin juicio de valor).
    Play,
    /// La canción llegó al final (se escuchó completa) ≈ señal positiva fuerte.
    Completed,
    /// El usuario saltó la canción antes de terminarla (neutra hasta ver
    /// contexto: saltar por aburrimiento ≠ saltar el autoplay).
    Skip,
    /// El usuario repitió la canción de inmediato (señal positiva fuerte).
    Replay,
    /// El usuario marcó "me gusta" explícitamente (señal positiva máxima).
    Like,
    /// El usuario quitó el "me gusta" (señal negativa).
    Unlike,
    /// El usuario añadió la canción a una playlist (señal positiva).
    PlaylistAdd,
    /// El usuario eligió reproducir una recomendación (conversión).
    RecClick,
    /// La recomendación se mostró (impresión: solo para medir CTR por
    /// semilla con `recomm_id`; su peso en el perfil es 0 — ver
    /// [`signal_weight`] — así que nunca influye en el ranking por sí sola).
    RecImpression,
}

impl SignalKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SignalKind::Play => "play",
            SignalKind::Completed => "completed",
            SignalKind::Skip => "skip",
            SignalKind::Replay => "replay",
            SignalKind::Like => "like",
            SignalKind::Unlike => "unlike",
            SignalKind::PlaylistAdd => "playlist_add",
            SignalKind::RecClick => "rec_click",
            SignalKind::RecImpression => "rec_impression",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "play" => SignalKind::Play,
            "completed" => SignalKind::Completed,
            "skip" => SignalKind::Skip,
            "replay" => SignalKind::Replay,
            "like" => SignalKind::Like,
            "unlike" => SignalKind::Unlike,
            "playlist_add" => SignalKind::PlaylistAdd,
            "rec_click" => SignalKind::RecClick,
            "rec_impression" => SignalKind::RecImpression,
            _ => return None,
        })
    }
}

/// Contexto en que ocurrió la interacción. Distinto contexto ⇒ distinta
/// semántica: un `play` por autoplay no implica gusto, mientras un `play`
/// manual (o peor, un `rec_click`) sí. El perfil ajusta el peso por contexto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlayContext {
    /// El usuario eligió la canción explícitamente (búsqueda, playlist, cola).
    Manual,
    /// Reproducción en cola encolada manualmente.
    Queue,
    /// Reproducción automática tras terminar otra canción (menor señal).
    Autoplay,
    /// Reproducción elegida desde una recomendación (señal fuerte).
    Recommendation,
}

impl PlayContext {
    pub fn as_str(self) -> &'static str {
        match self {
            PlayContext::Manual => "manual",
            PlayContext::Queue => "queue",
            PlayContext::Autoplay => "autoplay",
            PlayContext::Recommendation => "recommendation",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "manual" => PlayContext::Manual,
            "queue" => PlayContext::Queue,
            "autoplay" => PlayContext::Autoplay,
            "recommendation" => PlayContext::Recommendation,
            _ => return None,
        })
    }
}

/// Un evento de interacción persistido.
#[derive(Debug, Clone)]
pub struct PlaySignal {
    pub id: i64,
    pub track_id: i64,
    pub signal: SignalKind,
    pub context: PlayContext,
    pub at: String,
    /// Ms realmente escuchados en este evento (para completed/skip real).
    pub duration_ms: Option<i64>,
    /// Identificador de la recomendación cuando aplica.
    pub recomm_id: Option<i64>,
    /// Duración total del track, para normalizar completed/skip.
    pub track_duration_ms: Option<i64>,
}

impl PlaySignal {
    /// ¿Este evento cuenta como "escuchado completo"? (señal positiva)
    pub fn is_completion(&self) -> bool {
        match (self.duration_ms, self.track_duration_ms) {
            (Some(d), Some(td)) => td > 0 && d >= td,
            _ => false,
        }
    }

    /// ¿Este evento cuenta como "saltado"? (depende del contexto, no es fijo)
    pub fn is_short(&self) -> bool {
        match (self.duration_ms, self.track_duration_ms) {
            (Some(d), Some(td)) => td > 0 && d * 5 < td,
            _ => false,
        }
    }
}

/// Peso de una señal para el perfil (FASE 10), según orden/basado en contexto.
pub fn signal_weight(signal: SignalKind, context: PlayContext, completion: bool) -> f32 {
    // Intentos de reproducir en contexto automático valen poco: el usuario no
    // eligió conscientemente la canción.
    let context_weight = match context {
        PlayContext::Manual => 1.0,
        PlayContext::Queue => 0.8,
        PlayContext::Recommendation => 1.0,
        PlayContext::Autoplay => 0.4,
    };
    // La adición/señal base según el tipo de interacción.
    let kind_weight = match signal {
        SignalKind::Like => 3.0,
        SignalKind::Replay => 2.5,
        SignalKind::PlaylistAdd => 2.0,
        SignalKind::RecClick => 2.0,
        SignalKind::Completed => 1.5,
        SignalKind::Play => 1.0,
        // Impresión = solo métrica (CTR por semilla vía `recomm_id`): pesa 0
        // para que `from_signals` no confunda "se mostró" con "le gustó".
        SignalKind::RecImpression => 0.0,
        SignalKind::Skip | SignalKind::Unlike => -1.0,
    };
    // Una canción "escuchada completa" refuerza la señal; un evento corto en
    // contexto automático (autoplay) NO debe convertirse en aversión.
    let completion_bonus = if completion { 1.2 } else { 1.0 };
    context_weight * kind_weight * completion_bonus
}

/// Filtra señales de "play" espurias (autoplay en curso) que no deberían
/// alimentar afinidad negativa: no todo `skip` es disgusto.
pub fn is_meaningful_negative(signal: SignalKind, context: PlayContext) -> bool {
    matches!(signal, SignalKind::Skip | SignalKind::Unlike)
        && !matches!(context, PlayContext::Autoplay)
}

/// Agrega señales por track en `plays` / `negative` para la penalización
/// negativa del ranking (FASE 9/11). Los intentos son plays + rec_clicks; las
/// negativas son solo skips/unlikes contextualmente significativos.
pub fn aggregate_signals(
    signals: &[PlaySignal],
) -> std::collections::HashMap<i64, crate::recommendation::types::TrackSignals> {
    let mut map: std::collections::HashMap<i64, crate::recommendation::types::TrackSignals> =
        std::collections::HashMap::new();
    for s in signals {
        let e = map.entry(s.track_id).or_default();
        match s.signal {
            SignalKind::Play | SignalKind::RecClick => e.plays += 1,
            _ => {}
        }
        if is_meaningful_negative(s.signal, s.context) {
            e.negative += 1;
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_is_completion() {
        let s = PlaySignal {
            id: 1,
            track_id: 1,
            signal: SignalKind::Completed,
            context: PlayContext::Manual,
            at: "now".into(),
            duration_ms: Some(200_000),
            recomm_id: None,
            track_duration_ms: Some(200_000),
        };
        assert!(s.is_completion());
        assert!(!s.is_short());
    }

    #[test]
    fn short_play_is_not_completion() {
        let s = PlaySignal {
            id: 2,
            track_id: 1,
            signal: SignalKind::Play,
            context: PlayContext::Manual,
            at: "now".into(),
            duration_ms: Some(10_000),
            recomm_id: None,
            track_duration_ms: Some(200_000),
        };
        assert!(!s.is_completion());
        assert!(s.is_short());
    }

    #[test]
    fn like_weights_more_than_plain_play() {
        assert!(
            signal_weight(SignalKind::Like, PlayContext::Manual, true)
                > signal_weight(SignalKind::Play, PlayContext::Manual, false)
        );
    }

    #[test]
    fn skip_in_autoplay_is_not_meaningful_negative() {
        assert!(!is_meaningful_negative(
            SignalKind::Skip,
            PlayContext::Autoplay
        ));
        assert!(is_meaningful_negative(
            SignalKind::Skip,
            PlayContext::Manual
        ));
        assert!(is_meaningful_negative(
            SignalKind::Unlike,
            PlayContext::Manual
        ));
    }

    #[test]
    fn roundtrip_kind_and_context() {
        for s in [
            SignalKind::Play,
            SignalKind::Completed,
            SignalKind::Skip,
            SignalKind::Replay,
            SignalKind::Like,
            SignalKind::Unlike,
            SignalKind::PlaylistAdd,
            SignalKind::RecClick,
            SignalKind::RecImpression,
        ] {
            assert_eq!(SignalKind::parse(s.as_str()), Some(s));
        }
        for c in [
            PlayContext::Manual,
            PlayContext::Queue,
            PlayContext::Autoplay,
            PlayContext::Recommendation,
        ] {
            assert_eq!(PlayContext::parse(c.as_str()), Some(c));
        }
    }

    fn mks(track_id: i64, kind: SignalKind, ctx: PlayContext) -> PlaySignal {
        PlaySignal {
            id: track_id,
            track_id,
            signal: kind,
            context: ctx,
            at: "now".into(),
            duration_ms: Some(1000),
            recomm_id: None,
            track_duration_ms: Some(200_000),
        }
    }

    #[test]
    fn aggregate_signals_counts_plays_and_meaningful_negatives() {
        use crate::recommendation::types::TrackSignals;
        let sigs = vec![
            mks(1, SignalKind::Play, PlayContext::Manual),
            mks(1, SignalKind::RecClick, PlayContext::Recommendation),
            mks(1, SignalKind::Skip, PlayContext::Manual), // cuenta negativa
            mks(1, SignalKind::Skip, PlayContext::Autoplay), // NO cuenta
            mks(2, SignalKind::Play, PlayContext::Manual),
            mks(2, SignalKind::Unlike, PlayContext::Manual), // cuenta negativa
            mks(3, SignalKind::Completed, PlayContext::Manual), // no intento
        ];
        let map = aggregate_signals(&sigs);
        assert_eq!(
            map[&1],
            TrackSignals {
                plays: 2,
                negative: 1
            }
        );
        assert_eq!(
            map[&2],
            TrackSignals {
                plays: 1,
                negative: 1
            }
        );
        assert_eq!(
            map[&3],
            TrackSignals {
                plays: 0,
                negative: 0
            }
        );
    }

    #[test]
    fn impression_is_measured_but_never_scores() {
        // La impresión alimenta el CTR (recomm_id), no el gusto: peso 0.
        assert_eq!(
            signal_weight(
                SignalKind::RecImpression,
                PlayContext::Recommendation,
                false
            ),
            0.0,
            "se mostró ≠ le gusta"
        );
        assert_eq!(
            signal_weight(SignalKind::Play, PlayContext::Recommendation, false),
            1.0
        );
        assert_eq!(
            signal_weight(SignalKind::RecClick, PlayContext::Recommendation, false),
            2.0,
            "la conversión sí puntúa fuerte"
        );

        // Y el agregado la ignora: ni intento ni negativa.
        let sigs = vec![mks(
            1,
            SignalKind::RecImpression,
            PlayContext::Recommendation,
        )];
        let map = aggregate_signals(&sigs);
        assert_eq!(
            map.get(&1).copied(),
            Some(crate::recommendation::types::TrackSignals::default()),
            "una impresión no mueve plays ni negatives"
        );
    }
}
