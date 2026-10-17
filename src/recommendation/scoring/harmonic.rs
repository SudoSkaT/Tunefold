//! Similitud armónica (cromática + tonalidad + ritmo armónico) y estructural.
//!
//! Son los dos componentes NUEVOS del scoring que desplazan el peso desde la
//! popularidad hacia el sonido real de la pista:
//!
//! - [`harmonic_similarity`] (track ↔ track): cromática (peso dominante),
//!   proximidad de tonalidad y tasa de cambios armónicos (ritmo de acordes).
//! - [`harmonic_similarity_to_profile`] (track ↔ usuario): afinidad armónica
//!   con la tendencia cromática agregada del gusto del usuario.
//! - [`structure_similarity`]: correlación de las firmas estructurales
//!   (8 sectores temporales de energía/flux/centroide/onset).
//!
//! Los casos "sin datos" (cromática vacía, tonalidad -1, tasa 0, firma
//! estructural cero) devuelven el valor NEUTRO 0.5 donde corresponde: la falta
//! de evidencia no es ni coincidencia ni disonancia, y no debe decidir el
//! ranking por sí sola.

use crate::analysis::chroma::PITCH_CLASSES;
use crate::recommendation::types::{AcousticProfile, TrackAcousticProfile, STRUCT_LEN};

/// Peso de cada sub-componente dentro de la similitud armónica track↔track.
const W_CHROMA: f64 = 0.55;
const W_KEY: f64 = 0.25;
const W_CHORD_RATE: f64 = 0.20;

/// Escala máxima de diferencia de tasa de cambios armónicos considerada
/// "distinta" (cambios/s). Por encima, similitud 0.
const CHORD_RATE_SCALE: f32 = 4.0;

/// Valor neutral para "sin datos".
pub const NEUTRAL: f64 = 0.5;

/// Similitud coseno entre dos cromáticas (no negativas ⇒ 0..1; 0 si alguna
/// no tiene contenido).
pub fn chroma_similarity(a: &[f32; PITCH_CLASSES], b: &[f32; PITCH_CLASSES]) -> f32 {
    let mut dot = 0.0f32;
    let mut mag_a = 0.0f32;
    let mut mag_b = 0.0f32;
    for i in 0..PITCH_CLASSES {
        dot += a[i] * b[i];
        mag_a += a[i] * a[i];
        mag_b += b[i] * b[i];
    }
    let denom = (mag_a * mag_b).sqrt();
    if denom <= f32::EPSILON {
        0.0
    } else {
        (dot / denom).clamp(0.0, 1.0)
    }
}

/// Proximidad tonal entre dos tonalidades (0..23). Misma tónica y modo ⇒ 1;
/// misma tónica con modo distinto ⇒ 0.75 (par relativo aún es afín);
/// distancia circular de tónica ÷ 12 escala el resto. Sin datos ⇒ neutral.
pub fn key_similarity(k1: i16, k2: i16) -> f64 {
    if k1 < 0 || k2 < 0 || k1 >= 24 || k2 >= 24 {
        return NEUTRAL;
    }
    let t1 = (k1 % 12) as i32;
    let t2 = (k2 % 12) as i32;
    let d = (t1 - t2).abs();
    let circular = d.min(12 - d) as f32;
    let tonic_sim = 1.0 - circular / 12.0;
    let same_mode = (k1 / 12) == (k2 / 12);
    let mode_factor = if same_mode { 1.0 } else { 0.75 };
    (tonic_sim as f64) * mode_factor
}

/// Similitud de ritmo armónico: 1 − |Δtasa| / escala (acotado 0..1).
/// Sin datos en alguno de los dos ⇒ neutral.
pub fn chord_rate_similarity(r1: f32, r2: f32) -> f64 {
    if r1 <= 0.0 || r2 <= 0.0 {
        return NEUTRAL;
    }
    let diff = (r1 - r2).abs().min(CHORD_RATE_SCALE);
    1.0 - (diff / CHORD_RATE_SCALE) as f64
}

/// Similitud armónica completa entre dos perfiles acústicos (track ↔ track):
/// cromática 0.55 + tonalidad 0.25 + ritmo armónico 0.20.
pub fn harmonic_similarity(a: &TrackAcousticProfile, b: &TrackAcousticProfile) -> f64 {
    let chroma = chroma_similarity(&a.chroma_profile, &b.chroma_profile) as f64;
    let key = key_similarity(a.key, b.key);
    let rate = chord_rate_similarity(a.chord_change_rate, b.chord_change_rate);
    W_CHROMA * chroma + W_KEY * key + W_CHORD_RATE * rate
}

/// Similitud armónica track ↔ perfil del usuario (solo cromática: el usuario
/// no tiene "una" tonalidad ni una tasa propia de cambios).
pub fn harmonic_similarity_to_profile(track: &TrackAcousticProfile, user: &AcousticProfile) -> f64 {
    chroma_similarity(&track.chroma_profile, &user.chroma) as f64
}

/// Similitud estructural entre las firmas de 8 sectores (1 − diferencia media;
/// ambos vectores normalizados 0..1 ⇒ resultado 0..1). Si alguna firma está
/// vacía (sin ventana conocida) ⇒ neutral.
pub fn structure_similarity(a: &TrackAcousticProfile, b: &TrackAcousticProfile) -> f64 {
    structure_vec_similarity(&a.structure_profile, &b.structure_profile)
}

/// Igual que [`structure_similarity`] sobre vectores crudos (tests/agentes
/// que no tengan el perfil completo a mano).
pub fn structure_vec_similarity(a: &[f32; STRUCT_LEN], b: &[f32; STRUCT_LEN]) -> f64 {
    let energy_a: f32 = a.iter().map(|v| v.abs()).sum();
    let energy_b: f32 = b.iter().map(|v| v.abs()).sum();
    if energy_a <= f32::EPSILON || energy_b <= f32::EPSILON {
        return NEUTRAL;
    }
    let mut diff = 0.0f32;
    for i in 0..STRUCT_LEN {
        diff += (a[i] - b[i]).abs();
    }
    1.0 - (diff / STRUCT_LEN as f32) as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::chroma::KEY_UNKNOWN;
    use crate::recommendation::types::STRUCT_BUCKETS;

    fn profile_with(
        chroma: [f32; 12],
        key: i16,
        rate: f32,
        structure: [f32; STRUCT_LEN],
    ) -> TrackAcousticProfile {
        TrackAcousticProfile {
            chroma_profile: chroma,
            key,
            key_confidence: if key == KEY_UNKNOWN { 0.0 } else { 0.8 },
            chord_change_rate: rate,
            structure_profile: structure,
            ..TrackAcousticProfile::default()
        }
    }

    fn chroma_on(pitches: &[usize]) -> [f32; 12] {
        let mut c = [0.0f32; 12];
        for &p in pitches {
            c[p] = 1.0;
        }
        let total: f32 = c.iter().sum();
        for v in c.iter_mut() {
            *v /= total.max(1.0);
        }
        c
    }

    #[test]
    fn identical_chroma_scores_one() {
        let c = chroma_on(&[0, 4, 7]);
        assert!((chroma_similarity(&c, &c) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn orthogonal_chroma_scores_zero() {
        let c1 = chroma_on(&[0]); // C
        let c2 = chroma_on(&[1]); // C#
        assert_eq!(chroma_similarity(&c1, &c2), 0.0);
    }

    #[test]
    fn empty_chroma_scores_zero() {
        let c = chroma_on(&[0, 4, 7]);
        assert_eq!(chroma_similarity(&[0.0; 12], &c), 0.0);
    }

    #[test]
    fn same_key_is_closer_than_tritone() {
        let same = key_similarity(0, 0); // C mayor ↔ C mayor
        let relative = key_similarity(0, 9); // C mayor ↔ A menor (relativo)
        let tritone = key_similarity(0, 6); // C ↔ F#
        assert!((same - 1.0).abs() < 1e-9);
        assert!(relative > 0.7, "relativo aún afín: {relative}");
        assert!(tritone < 0.6, "tritón lejano: {tritone}");
        assert!(same > relative && relative > tritone);
    }

    #[test]
    fn unknown_key_is_neutral() {
        assert_eq!(key_similarity(KEY_UNKNOWN, 0), NEUTRAL);
        assert_eq!(key_similarity(0, KEY_UNKNOWN), NEUTRAL);
        assert_eq!(key_similarity(-5, KEY_UNKNOWN), NEUTRAL);
    }

    #[test]
    fn chord_rate_close_is_high_and_missing_is_neutral() {
        assert!((chord_rate_similarity(1.0, 1.0) - 1.0).abs() < 1e-9);
        assert!(chord_rate_similarity(1.0, 3.0) < 0.6);
        assert_eq!(chord_rate_similarity(0.0, 2.0), NEUTRAL, "sin datos");
        assert_eq!(chord_rate_similarity(1.5, 0.0), NEUTRAL, "sin datos");
    }

    #[test]
    fn harmonic_combines_weighted_subscores() {
        let a = profile_with(chroma_on(&[0, 4, 7]), 0, 1.0, [0.0; STRUCT_LEN]);
        let b = profile_with(chroma_on(&[0, 4, 7]), 0, 1.0, [0.0; STRUCT_LEN]);
        // Todo coincide: 0.55·1 + 0.25·1 + 0.20·1 = 1.0.
        let h = harmonic_similarity(&a, &b);
        assert!((h - 1.0).abs() < 1e-9, "debería ser 1.0: {h}");
    }

    #[test]
    fn harmonic_unknown_key_still_rewards_matching_chroma() {
        let a = profile_with(chroma_on(&[0, 4, 7]), KEY_UNKNOWN, 0.0, [0.0; STRUCT_LEN]);
        let b = profile_with(chroma_on(&[0, 4, 7]), KEY_UNKNOWN, 0.0, [0.0; STRUCT_LEN]);
        let h = harmonic_similarity(&a, &b);
        // 0.55·1 + 0.25·0.5 (key neutral) + 0.20·0.5 (tasa neutral) = 0.775.
        assert!((h - 0.775).abs() < 1e-9, "esperado 0.775: {h}");
    }

    #[test]
    fn structure_empty_is_neutral_and_identical_is_one() {
        let a = profile_with([0.0; 12], -1, 0.0, [0.0; STRUCT_LEN]);
        let mut sig = [0.0f32; STRUCT_LEN];
        for (i, v) in sig.iter_mut().enumerate() {
            *v = (i as f32) / STRUCT_LEN as f32;
        }
        let b = profile_with([0.0; 12], -1, 0.0, sig);
        assert_eq!(structure_similarity(&a, &b), NEUTRAL, "firma vacía");
        assert_eq!(structure_similarity(&b, &b), 1.0, "idénticas");
    }

    #[test]
    fn structure_different_profiles_score_below_one() {
        let mut a = [0.0f32; STRUCT_LEN];
        let mut b = [0.0f32; STRUCT_LEN];
        for i in 0..STRUCT_BUCKETS {
            a[i * 4 + 1] = 0.9; // rms alto en todos los sectores
            b[i * 4 + 1] = 0.1; // rms bajo en todos
        }
        // Solo difiere rms (1/4 de las métricas) en 0.8 por bucket:
        // 1 − (8·0.8)/32 = 0.8 — menos similar que idénticas, más que nada.
        let s = structure_vec_similarity(&a, &b);
        assert!((s - 0.8).abs() < 1e-6, "esperado 0.8: {s}");
    }

    #[test]
    fn to_user_profile_uses_chroma_only() {
        let user = AcousticProfile {
            chroma: chroma_on(&[0, 4, 7]),
            ..AcousticProfile::default()
        };
        let matching = profile_with(chroma_on(&[0, 4, 7]), 0, 1.0, [0.0; STRUCT_LEN]);
        let other = profile_with(chroma_on(&[1, 5, 8]), 0, 1.0, [0.0; STRUCT_LEN]);
        assert!(
            harmonic_similarity_to_profile(&matching, &user) > 0.99,
            "misma cromática ⇒ afinidad alta"
        );
        assert!(
            harmonic_similarity_to_profile(&other, &user) < 0.3,
            "cromática distinta ⇒ afinidad baja"
        );
    }
}
