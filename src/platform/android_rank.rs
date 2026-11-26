//! Ranking de recomendaciones compartido por JNI (v0.20, Fase 5).
//!
//! Android usa el MISMO scoring que la TUI: `recommendation::rank_sync` con
//! los pesos canónicos (`ScoringWeights`). Java conserva los filtros
//! anti-bucle (`Recommendations.filter*`) y este módulo solo REORDENA.
//!
//! Contrato JSON (estadoless: el `.so` no guarda nada; Java inyecta todo en
//! cada llamada — señales persistidas en `filesDir/listening-signals.txt`):
//!
//! ```text
//! petición  {"candidates":[<Track>...],
//!            "signals":[{"track":<Track>,"kind":"play|skip|completed|rec_click",
//!                        "context":"manual|queue|autoplay|recommendation",
//!                        "at":<epoch_ms>,
//!            "duration_ms":n,"track_duration_ms":n}...],
//!            "profiles":[{"ref":"<identifier>","profile":<TrackAcousticProfile>}...],
//!            "seed_profile":<TrackAcousticProfile>|null}
//! respuesta {"ok":true,"order":[índice_del_candidato...]}   // índices al input
//!            {"ok":false,"category":"...","error":"..."}
//! ```
//!
//! Fail-open en cuatro capas: sin datos `order:[]` (Java conserva el orden
//! del provider), parseo inválido ⇒ `ok:false`, y cualquier fallo en Java
//! (incluido `UnsatisfiedLinkError` de un binario antiguo) ⇒ orden del
//! provider. `profiles`/`seed_profile` están en el contrato desde v1 aunque
//! v1 no los envíe (el eje sonoro queda dormido hasta que Android produzca
//! perfiles — `Documentacion/docs/recommendations.md`).
//!
//! Regla de frontera (AGENTS.md): este módulo vive en `platform/`, nunca en
//! `providers/youtube/`; habla JSON neutral y no conoce proveedores.

use std::collections::HashMap;

use jni::objects::{JClass, JString};
use jni::sys::jstring;
use jni::JNIEnv;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::domain::track::Track;
use crate::infrastructure::storage::TrackListeningStats;
use crate::recommendation::signals::{PlaySignal, SignalKind};
use crate::recommendation::types::{TrackAcousticProfile, TrackSignals, UserProfile};
use crate::recommendation::{aggregate_signals, rank_sync};

/// Señal entrante con el track sujeto embebido (Java no conoce ids internos).
#[derive(Debug, Deserialize)]
struct SignalIn {
    track: Track,
    kind: String,
    context: String,
    #[serde(default)]
    at: Option<u64>,
    #[serde(default)]
    duration_ms: Option<i64>,
    #[serde(default)]
    track_duration_ms: Option<i64>,
}

/// Perfil acústico referenciado por identificador estable del candidato.
#[derive(Debug, Deserialize)]
struct ProfileIn {
    #[serde(rename = "ref", default)]
    reference: String,
    #[serde(default)]
    profile: TrackAcousticProfile,
}

#[derive(Debug, Default, Deserialize)]
struct RankRequest {
    #[serde(default)]
    candidates: Vec<Track>,
    #[serde(default)]
    signals: Vec<SignalIn>,
    #[serde(default)]
    profiles: Vec<ProfileIn>,
    #[serde(default)]
    seed_profile: Option<TrackAcousticProfile>,
}

/// Núcleo puro del native: petición JSON → respuesta JSON. Exportado como
/// función normal para poder testearlo en host sin JNI.
pub(crate) fn rank_request(raw: &str) -> String {
    let request: RankRequest = match serde_json::from_str(raw) {
        Ok(r) => r,
        Err(error) => {
            return json!({"ok": false, "category": "invalid_request",
                          "error": error.to_string()})
            .to_string()
        }
    };
    rank_parsed(request).to_string()
}

fn rank_parsed(request: RankRequest) -> Value {
    let mut candidates = request.candidates;
    if candidates.is_empty() {
        return json!({"ok": true, "order": Vec::<usize>::new()});
    }

    // Ids: los candidatos frescos de Android no tienen id interno; reciben un
    // id PSEUDO negativo único por posición (mismo truco que la TUI) para
    // emparejar score↔posición sin colisionar con perfiles/señales reales.
    for (idx, t) in candidates.iter_mut().enumerate() {
        if t.id <= 0 {
            t.id = -(idx as i64 + 1);
        }
    }
    let id_by_key: HashMap<String, i64> =
        candidates.iter().map(|t| (t.identifier(), t.id)).collect();
    let index_by_id: HashMap<i64, usize> = candidates
        .iter()
        .enumerate()
        .map(|(idx, t)| (t.id, idx))
        .collect();

    // Mapa de tracks para el perfil de usuario: candidatos + sujetos de
    // señales (los repetidos se resuelven al primer id asignado).
    let mut tracks_map: HashMap<i64, Track> =
        candidates.iter().cloned().map(|t| (t.id, t)).collect();
    let mut next_pseudo = -(candidates.len() as i64 + 1);

    let mut signals: Vec<PlaySignal> = Vec::with_capacity(request.signals.len());
    let mut seen: HashMap<String, i64> = id_by_key.clone();
    for (seq, incoming) in request.signals.into_iter().enumerate() {
        let key = incoming.track.identifier();
        // Duración total de la pista: Java la manda solo en `completed`
        // (escuchado); para normalizar completion/short el native la deriva
        // del propio Track embebido cuando el campo existe.
        let track_duration_ms = incoming
            .track_duration_ms
            .or_else(|| incoming.track.duration.map(|d| d.as_millis() as i64));
        let track_id = match seen.get(&key) {
            Some(id) => *id,
            None => {
                let id = next_pseudo;
                next_pseudo -= 1;
                seen.insert(key, id);
                tracks_map.insert(id, incoming.track);
                id
            }
        };
        let (Some(signal), Some(context)) = (
            SignalKind::parse(&incoming.kind),
            crate::recommendation::signals::PlayContext::parse(&incoming.context),
        ) else {
            continue;
        };
        signals.push(PlaySignal {
            id: seq as i64,
            track_id,
            signal,
            context,
            at: format_at_ms(incoming.at),
            duration_ms: incoming.duration_ms,
            recomm_id: None,
            track_duration_ms,
        });
    }

    // Perfiles acústicos referenciados (v1 Android no envía ninguno).
    let mut acoustic_profiles: HashMap<i64, TrackAcousticProfile> = HashMap::new();
    for mut entry in request.profiles {
        if entry.reference.is_empty() {
            continue;
        }
        if let Some(&id) = seen.get(&entry.reference) {
            entry.profile.track_id = id;
            acoustic_profiles.insert(id, entry.profile);
        }
    }

    // Guard del pipeline: sin NINGÚN dato con qué puntuar, la lista no se
    // toca (devolvemos `order:[]` y Java conserva el orden del provider).
    if signals.is_empty() && acoustic_profiles.is_empty() {
        return json!({"ok": true, "order": Vec::<usize>::new()});
    }

    // Historial sintetizado de las señales: da recencia/popularidad/afinidad
    // de engagement sin base de datos (Java persiste las señales, no stats).
    let history = synthesize_history(&signals, &tracks_map);

    let profile = UserProfile::from_signals(&signals, &tracks_map, &acoustic_profiles);
    let signals_by_track: HashMap<i64, TrackSignals> = aggregate_signals(&signals);

    let ranked = rank_sync(
        &candidates,
        &profile,
        &history,
        &acoustic_profiles,
        &signals_by_track,
        request.seed_profile.as_ref(),
    );

    // De vuelta a ÍNDICES del array de entrada: Java reordena su propia lista
    // (conserva objetos MediaTrack y su rawJson sin re-serializar nada).
    let mut order: Vec<usize> = Vec::with_capacity(ranked.len());
    for score in ranked {
        if let Some(&idx) = index_by_id.get(&score.track_id) {
            order.push(idx);
        }
    }
    json!({"ok": true, "order": order})
}

/// Historial mínimo por identificador: plays (o reclicks/completados) y el
/// último momento, en el formato SQL que consume `days_since`.
fn synthesize_history(
    signals: &[PlaySignal],
    tracks_map: &HashMap<i64, Track>,
) -> Vec<TrackListeningStats> {
    let mut by_id: HashMap<i64, (i64, &str)> = HashMap::new();
    for s in signals {
        let counts = matches!(
            s.signal,
            SignalKind::Play | SignalKind::RecClick | SignalKind::Completed
        );
        let entry = by_id.entry(s.track_id).or_insert((0, s.at.as_str()));
        if counts {
            entry.0 += 1;
        }
        if s.at.as_str() > entry.1 {
            entry.1 = s.at.as_str();
        }
    }
    by_id
        .into_iter()
        .filter_map(|(track_id, (play_count, last_played))| {
            let track = tracks_map.get(&track_id)?;
            Some(TrackListeningStats {
                track_id,
                key: track.identifier(),
                artist_name: track.primary_artist_name().map(str::to_string),
                play_count,
                last_played: last_played.to_string(),
                recently_played: true,
            })
        })
        .collect()
}

/// Epoch ms → `YYYY-MM-DD HH:MM:SS` UTC (el formato que parsea `days_since`).
fn format_at_ms(at: Option<u64>) -> String {
    let secs = at.map(|ms| ms / 1000).unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    });
    chrono::DateTime::from_timestamp(secs as i64, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "1970-01-01 00:00:00".to_string())
}

fn read_string(env: &mut JNIEnv<'_>, value: &JString) -> Result<String, String> {
    env.get_string(value)
        .map(|s| s.to_string_lossy().into_owned())
        .map_err(|error| error.to_string())
}

fn return_json(env: &mut JNIEnv<'_>, value: Value) -> jstring {
    env.new_string(value.to_string())
        .map(|value| value.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_rankRelated(
    mut env: JNIEnv,
    _class: JClass,
    request: JString,
) -> jstring {
    let raw = match read_string(&mut env, &request) {
        Ok(raw) => raw,
        Err(error) => {
            return return_json(
                &mut env,
                json!({"ok": false, "category": "invalid_request", "error": error}),
            )
        }
    };
    // Nunca throw: envelope `ok:false` y Java hace fail-open al orden del
    // provider (mismo contrato que los natives de providers).
    let response = serde_json::from_str::<Value>(&rank_request(&raw))
        .unwrap_or_else(|_| json!({"ok": false, "category": "rank_unavailable"}));
    return_json(&mut env, response)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: i64, title: &str, artist: &str) -> Track {
        let mut t = Track::new(
            title.to_string(),
            vec![crate::domain::artist::Artist::new(
                artist.to_string(),
                None,
                None,
                None,
            )],
            crate::domain::source::Source::YouTube,
        );
        t.id = id;
        t.external_id = Some(format!("vid-{title}"));
        t
    }

    fn raw(t: &Track) -> String {
        serde_json::to_string(t).unwrap()
    }

    fn request(candidates: &[&Track], signals: &str) -> String {
        let cands: Vec<String> = candidates.iter().map(|t| raw(t)).collect();
        format!(
            r#"{{"candidates":[{}],"signals":[{signals}]}}"#,
            cands.join(",")
        )
    }

    #[test]
    fn invalid_request_is_rejected_with_envelope() {
        let out = rank_request("not json");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["category"], "invalid_request");
    }

    #[test]
    fn no_data_returns_empty_order_for_provider_fallback() {
        let a = track(-1, "A", "x");
        let b = track(-1, "B", "y");
        let out = rank_request(&request(&[&a, &b], ""));
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["order"], json!([]), "sin datos ⇒ fail-open");
    }

    #[test]
    fn empty_candidates_ok_with_empty_order() {
        let v: Value = serde_json::from_str(&rank_request(r#"{"candidates":[]}"#)).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["order"], json!([]));
    }

    #[test]
    fn previous_play_of_a_candidate_ranks_it_first() {
        let a = track(-1, "A", "x");
        let b = track(-1, "B", "x");
        // El usuario ya escuchó B (señal de play con contexto manual).
        let signals = format!(
            r#"{{"kind":"play","context":"manual","at":1760000000000,"track":{}}}"#,
            raw(&b)
        );
        let v: Value = serde_json::from_str(&rank_request(&request(&[&a, &b], &signals))).unwrap();
        assert_eq!(v["ok"], true);
        let order: Vec<usize> = v["order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_u64().unwrap() as usize)
            .collect();
        assert_eq!(
            order,
            vec![1, 0],
            "B (ya escuchada) sube; A (sin historia) queda detrás"
        );
    }

    #[test]
    fn skip_of_a_candidate_pushes_it_down() {
        let a = track(-1, "A", "x");
        let b = track(-1, "B", "x");
        // Skip manual significativo sobre B (misma metadata para aislar el
        // efecto de la penalización negativa).
        let signals = format!(
            r#"{{"kind":"skip","context":"manual","at":1760000000000,"track":{}}}"#,
            raw(&b)
        );
        let v: Value = serde_json::from_str(&rank_request(&request(&[&a, &b], &signals))).unwrap();
        let order: Vec<usize> = v["order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_u64().unwrap() as usize)
            .collect();
        assert_eq!(order, vec![0, 1], "B saltado ⇒ A por delante");
    }

    #[test]
    fn order_covers_every_candidate_exactly_once() {
        let tracks: Vec<Track> = (0..5).map(|i| track(-1, &format!("T{i}"), "x")).collect();
        let refs: Vec<&Track> = tracks.iter().collect();
        let signals = format!(
            r#"{{"kind":"play","context":"manual","at":1760000000000,"track":{}}}"#,
            raw(&tracks[3])
        );
        let v: Value = serde_json::from_str(&rank_request(&request(&refs, &signals))).unwrap();
        let mut order: Vec<usize> = v["order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_u64().unwrap() as usize)
            .collect();
        assert_eq!(order.len(), 5);
        order.sort_unstable();
        assert_eq!(order, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn unknown_kinds_are_dropped_not_fatal() {
        let a = track(-1, "A", "x");
        let signals = format!(
            r#"{{"kind":"bogus","context":"manual","track":{}}}"#,
            raw(&a)
        );
        let v: Value = serde_json::from_str(&rank_request(&request(&[&a], &signals))).unwrap();
        // Solo señales inválidas ⇒ sin datos ⇒ fail-open.
        assert_eq!(v["ok"], true);
        assert_eq!(v["order"], json!([]));
    }
}
