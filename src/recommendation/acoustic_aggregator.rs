//! Agregador acústico (FASE 8): reduce los frames de `AudioFeatures` en vivo a
//! un `TrackAcousticProfile` promedio persistible.
//!
//! El análisis de audio es EFÍMERO (solo existe durante la reproducción). Para
//! que el sistema de recomendaciones pueda comparar tracks por su sonido, este
//! agregador acumula los frames durante una reproducción y, al terminar, produce
//! un perfil que se guarda en `track_acoustic_profiles`.
//!
//! Además de las medias clásicas (RMS, bandas, centroid, BPM), acumula:
//! - **cromática media** → tonalidad (Krumhansl-Schmuckler) por track;
//! - **cambios armónicos**: cuándo la cromática de un frame difiere mucho de
//!   la del anterior (ritmo armónico, cambios/s);
//! - **firma estructural**: `STRUCT_BUCKETS` sectores temporales iguales del
//!   tramo analizado con [flux, rms, centroid, onset] medios — requiere una
//!   ventana conocida (`window`, normalmente la duración del track; el
//!   pre-análisis de candidatos pasa su propio tramo).

use std::time::Duration;

use crate::analysis::chroma::{estimate_key, KEY_UNKNOWN};
use crate::analysis::features::AudioFeatures;
use crate::recommendation::types::{TrackAcousticProfile, STRUCT_BUCKETS, STRUCT_LEN};

/// Similitud coseno mínima entre las cromáticas de dos frames consecutivos
/// para seguir considerándolos la misma armonía por debajo ⇒ cambio.
const CHORD_CHANGE_SIM: f32 = 0.65;
/// Segundos mínimos de tramo activo para que la tasa de cambios sea fiable.
const MIN_CHORD_SECS: f64 = 1.0;

/// Acumulados de UN bucket estructural.
#[derive(Debug, Clone, Copy, Default)]
struct StructBucket {
    flux: f64,
    rms: f64,
    centroid: f64,
    onset: f64,
    count: u64,
}

/// Acumula frames de `AudioFeatures` de un track para producir su perfil medio.
#[derive(Debug, Default, Clone)]
pub struct AcousticAggregator {
    track_id: i64,
    /// Tramo esperado para bucketizar la estructura (0 = sin estructura).
    window: Duration,
    frames: u64,
    rms: f64,
    bass: f64,
    low_mid: f64,
    mid: f64,
    high_mid: f64,
    high: f64,
    centroid: f64,
    onset: f64,
    flux: f64,
    bpm_sum: f64,
    bpm_sq: f64,
    chroma_sum: [f64; 12],
    prev_chroma: Option<[f32; 12]>,
    last_timestamp: Option<Duration>,
    chord_changes: u64,
    chord_secs: f64,
    buckets: [StructBucket; STRUCT_BUCKETS],
}

impl AcousticAggregator {
    /// `window` es el tramo de tiempo que se bucketiza en la firma
    /// estructural (duración del track; cero = no calcular estructura).
    pub fn new(track_id: i64, window: Duration) -> Self {
        Self {
            track_id,
            window,
            ..Self::default()
        }
    }

    /// Añade un frame. Ignora frames silenciosos (todos a cero) que todavía no
    /// codifican sonido, y evita que un `bpm=0` "desconocido" arrastre la media.
    pub fn add(&mut self, f: &AudioFeatures) {
        let active = f.rms > 0.0
            || f.bass + f.low_mid + f.mid + f.high_mid + f.high > 0.0
            || f.spectral_centroid > 0.0;
        if !active {
            return;
        }

        // Tiempo activo entre frames consecutivos (denominador de la tasa de
        // cambios armónicos).
        if let Some(prev_ts) = self.last_timestamp {
            if f.timestamp > prev_ts {
                self.chord_secs += (f.timestamp - prev_ts).as_secs_f64();
            }
        }
        self.last_timestamp = Some(f.timestamp);

        // ¿La armonía cambió respecto al frame anterior?
        let chroma_energy: f32 = f.chroma.iter().sum();
        if chroma_energy > 1e-6 {
            if let Some(prev) = self.prev_chroma {
                let prev_energy: f32 = prev.iter().sum();
                if prev_energy > 1e-6 && chroma_cosine(&prev, &f.chroma) < CHORD_CHANGE_SIM {
                    self.chord_changes += 1;
                }
            }
            self.prev_chroma = Some(f.chroma);
        }

        self.frames += 1;
        self.rms += f.rms as f64;
        self.bass += f.bass as f64;
        self.low_mid += f.low_mid as f64;
        self.mid += f.mid as f64;
        self.high_mid += f.high_mid as f64;
        self.high += f.high as f64;
        self.centroid += f.spectral_centroid as f64;
        self.onset += f.onset as f64;
        self.flux += f.spectral_flux as f64;
        for i in 0..12 {
            self.chroma_sum[i] += f.chroma[i] as f64;
        }
        if f.bpm > 0.0 {
            self.bpm_sum += f.bpm as f64;
            self.bpm_sq += (f.bpm as f64) * (f.bpm as f64);
        }

        // Firma estructural: sector = propición del timestamp en la ventana.
        if !self.window.is_zero() {
            let secs = self.window.as_secs_f64().max(1e-6);
            let pos = f.timestamp.as_secs_f64() * STRUCT_BUCKETS as f64 / secs;
            let idx = (pos.floor() as usize).min(STRUCT_BUCKETS - 1);
            let b = &mut self.buckets[idx];
            b.flux += f.spectral_flux as f64;
            b.rms += f.rms as f64;
            b.centroid += f.spectral_centroid as f64;
            b.onset += f.onset as f64;
            b.count += 1;
        }
    }

    /// ¿Hay suficientes frames como para emitir un perfil mínimamente fiable?
    pub fn ready(&self) -> bool {
        self.frames >= 30
    }

    fn mean(&self, sum: f64) -> f32 {
        if self.frames == 0 {
            0.0
        } else {
            (sum / self.frames as f64) as f32
        }
    }

    /// Produce el perfil promedio. Devuelve `None` si no hubo frames útiles o
    /// si aún no hay suficientes para un perfil fiable.
    pub fn into_profile(self) -> Option<TrackAcousticProfile> {
        if !self.ready() {
            return None;
        }
        let frames = self.frames;
        let bpm_mean = self.mean(self.bpm_sum) as f64;
        // Varianza de BPM solo sobre frames con BPM conocido y medido.
        let bpm_variance = {
            let n = self.frames.max(1) as f64;
            let raw = (self.bpm_sq / n - bpm_mean * bpm_mean).max(0.0);
            raw as f32
        };

        // Cromática media, renormalizada (la suma de medias de frames
        // normalizados debería ser 1, pero los frames activos varían).
        let mut chroma_profile =
            std::array::from_fn(|i| (self.chroma_sum[i] / frames as f64) as f32);
        let chroma_total: f32 = chroma_profile.iter().map(|v| v.max(0.0)).sum();
        if chroma_total > f32::EPSILON {
            for v in chroma_profile.iter_mut() {
                *v /= chroma_total;
            }
        }
        let key_est = if chroma_total > f32::EPSILON {
            estimate_key(&chroma_profile)
        } else {
            crate::analysis::chroma::KeyEstimate::default()
        };
        debug_assert!(key_est.key != KEY_UNKNOWN || chroma_total <= f32::EPSILON);

        let chord_change_rate = if self.chord_secs >= MIN_CHORD_SECS {
            (self.chord_changes as f64 / self.chord_secs) as f32
        } else {
            0.0
        };

        let mut structure_profile = [0.0f32; STRUCT_LEN];
        for (i, b) in self.buckets.iter().enumerate() {
            if b.count == 0 {
                continue;
            }
            let n = b.count as f64;
            let base = i * 4;
            structure_profile[base] = (b.flux / n) as f32;
            structure_profile[base + 1] = (b.rms / n) as f32;
            structure_profile[base + 2] = (b.centroid / n) as f32;
            structure_profile[base + 3] = (b.onset / n) as f32;
        }

        Some(TrackAcousticProfile {
            track_id: self.track_id,
            rms_mean: self.mean(self.rms),
            bass_mean: self.mean(self.bass),
            low_mid_mean: self.mean(self.low_mid),
            mid_mean: self.mean(self.mid),
            high_mid_mean: self.mean(self.high_mid),
            high_mean: self.mean(self.high),
            spectral_centroid_mean: self.mean(self.centroid),
            bpm_mean: bpm_mean as f32,
            bpm_variance,
            onset_mean: self.mean(self.onset),
            band_profile: [
                self.mean(self.bass),
                self.mean(self.low_mid),
                self.mean(self.mid),
                self.mean(self.high_mid),
                self.mean(self.high),
            ],
            frame_count: frames as i64,
            chroma_profile,
            key: key_est.key,
            key_confidence: key_est.confidence,
            chord_change_rate,
            structure_profile,
        })
    }
}

/// Similitud coseno entre dos cromáticas (no negativas ⇒ resultado 0..1).
fn chroma_cosine(a: &[f32; 12], b: &[f32; 12]) -> f32 {
    let mut dot = 0.0f32;
    let mut mag_a = 0.0f32;
    let mut mag_b = 0.0f32;
    for i in 0..12 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::features::AudioFeatures;

    /// Cromática con picos en las clases dadas (normalizada a suma 1).
    fn chroma_with(pitches: &[usize]) -> [f32; 12] {
        let mut c = [0.0f32; 12];
        for &p in pitches {
            c[p] = 1.0;
        }
        let total: f32 = c.iter().sum();
        for v in c.iter_mut() {
            *v /= total;
        }
        c
    }

    fn frame(bass: f32, mid: f32, high: f32, bpm: f32) -> AudioFeatures {
        AudioFeatures {
            timestamp: std::time::Duration::ZERO,
            rms: 0.2,
            amplitude: 0.3,
            bass,
            low_mid: 0.0,
            mid,
            high_mid: 0.0,
            high,
            spectral_centroid: 0.5,
            spectral_flux: 0.1,
            onset: 0.0,
            beat: false,
            beat_confidence: 0.0,
            bpm,
            chroma: chroma_with(&[0, 4, 7]),
        }
    }

    fn frame_at(ts: Duration, chroma: [f32; 12]) -> AudioFeatures {
        AudioFeatures {
            timestamp: ts,
            rms: 0.2,
            amplitude: 0.3,
            bass: 0.1,
            low_mid: 0.0,
            mid: 0.2,
            high_mid: 0.0,
            high: 0.1,
            spectral_centroid: 0.5,
            spectral_flux: 0.1,
            onset: 0.1,
            beat: false,
            beat_confidence: 0.0,
            bpm: 120.0,
            chroma,
        }
    }

    #[test]
    fn aggregates_mean_per_band() {
        let mut agg = AcousticAggregator::new(7, Duration::from_secs(200));
        for _ in 0..100 {
            agg.add(&frame(0.1, 0.2, 0.3, 120.0));
        }
        let p = agg.into_profile().unwrap();
        assert_eq!(p.track_id, 7);
        assert_eq!(p.frame_count, 100);
        assert!((p.bass_mean - 0.1).abs() < 1e-4);
        assert!((p.mid_mean - 0.2).abs() < 1e-4);
        assert!((p.high_mean - 0.3).abs() < 1e-4);
        assert!((p.bpm_mean - 120.0).abs() < 1e-3);
    }

    #[test]
    fn not_ready_with_few_frames() {
        let mut agg = AcousticAggregator::new(1, Duration::from_secs(200));
        for _ in 0..5 {
            agg.add(&frame(0.1, 0.1, 0.1, 100.0));
        }
        assert!(!agg.ready());
        assert!(agg.into_profile().is_none());
    }

    #[test]
    fn ignores_silent_frames() {
        let mut agg = AcousticAggregator::new(1, Duration::from_secs(200));
        for _ in 0..50 {
            agg.add(&AudioFeatures::silent(std::time::Duration::ZERO));
        }
        assert!(!agg.ready(), "frames silenciosos no cuentan");
    }

    #[test]
    fn accumulates_chroma_and_estimates_key() {
        // C mayor diatónico en todos los frames ⇒ key 0 (C mayor).
        let c_major = chroma_with(&[0, 2, 4, 5, 7, 9, 11]);
        let mut agg = AcousticAggregator::new(3, Duration::from_secs(200));
        for i in 0..100 {
            agg.add(&frame_at(Duration::from_millis(i * 33), c_major));
        }
        let p = agg.into_profile().unwrap();
        assert!((p.chroma_profile.iter().sum::<f32>() - 1.0).abs() < 1e-4);
        assert_eq!(p.key, 0, "cromática de C mayor ⇒ key 0");
        assert!(p.key_confidence > 0.6, "confianza: {}", p.key_confidence);
    }

    #[test]
    fn counts_harmonic_changes_between_chord_frames() {
        // Dos acordes claramente distintos alternando cada frame.
        let c = chroma_with(&[0, 4, 7]); // C mayor
        let f = chroma_with(&[5, 9, 0]); // F mayor
        let mut agg = AcousticAggregator::new(1, Duration::from_secs(10));
        for i in 0..100 {
            let ts = Duration::from_millis(i * 40); // 25 fps
            let ch = if i % 2 == 0 { c } else { f };
            agg.add(&frame_at(ts, ch));
        }
        let p = agg.into_profile().unwrap();
        assert!(
            p.chord_change_rate > 5.0,
            "alterna ~25 cambios/s ⇒ tasa alta: {}",
            p.chord_change_rate
        );
    }

    #[test]
    fn steady_chord_has_low_change_rate() {
        let c = chroma_with(&[0, 4, 7]);
        let mut agg = AcousticAggregator::new(1, Duration::from_secs(10));
        for i in 0..100 {
            agg.add(&frame_at(Duration::from_millis(i * 40), c));
        }
        let p = agg.into_profile().unwrap();
        assert_eq!(
            p.chord_change_rate, 0.0,
            "misma armonía continuada ⇒ sin cambios"
        );
    }

    #[test]
    fn structure_buckets_fill_across_window() {
        let c = chroma_with(&[0, 4, 7]);
        let mut agg = AcousticAggregator::new(1, Duration::from_secs(8));
        // 8 s de ventana, 10 fps ⇒ ~10 frames por bucket.
        for i in 0..80 {
            agg.add(&frame_at(Duration::from_millis(i * 100), c));
        }
        let p = agg.into_profile().unwrap();
        // Los primeros buckets se llenan; rms medio ≈ 0.2 en los usados.
        assert!(p.structure_profile[1] > 0.0, "bucket 0 relleno (flux)");
        assert!((p.structure_profile[1] - 0.2).abs() < 1e-3, "rms bucket 0");
        assert!(
            p.structure_profile[STRUCT_LEN - 3] > 0.0,
            "bucket 7 también relleno (timestamp al final de la ventana)"
        );
    }

    #[test]
    fn no_window_yields_zero_structure() {
        let mut agg = AcousticAggregator::new(1, Duration::ZERO);
        for _ in 0..50 {
            agg.add(&frame(0.1, 0.2, 0.3, 120.0));
        }
        let p = agg.into_profile().unwrap();
        assert_eq!(p.structure_profile, [0.0; STRUCT_LEN]);
    }
}
