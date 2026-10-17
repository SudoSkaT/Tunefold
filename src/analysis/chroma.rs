//! Cromática: energía por clase de pica (12 bins) y estimación de tonalidad.
//!
//! El espectro de magnitudes se pliega sobre las 12 clases de pica (C, C#, …,
//! B) sumando la potencia de todos los bins cuya frecuencia cae en el rango
//! musical (C3..C7, 130–2200 Hz) y redondeando a la nota MIDI más cercana.
//! El resultado está normalizado a suma 1 (o cero si no hay contenido) y es
//! la base de la similitud armónica del scoring de recomendaciones.
//!
//! La estimación de tonalidad usa perfiles Krumhansl-Schmuckler (mayor y
//! menor) correlados con la cromática media: 24 rotaciones (12 tónicas × 2
//! modos), la mejor correlación de Pearson gana.

/// Clases de pica por octava.
pub const PITCH_CLASSES: usize = 12;

/// Tonalidades codificadas: 0..11 mayor (C..B), 12..23 menor (C..B).
pub const KEY_COUNT: i16 = 24;

/// Sin tonalidad estimada (perfil sin contenido o demasiado tenue).
pub const KEY_UNKNOWN: i16 = -1;

/// Límite inferior del rango plegado (C3 ≈ 130 Hz: por debajo los bins de la
/// FFT lineal son demasiado gruesos para separar semitonos).
const FMIN_HZ: f32 = 130.0;
/// Límite superior (C7 ≈ 2093 Hz: los armónicos superiores aportan ruido de
/// octava más que armonía).
const FMAX_HZ: f32 = 2200.0;

/// Potencia mínima dentro del rango musical (fracción de la potencia total
/// del espectro) para considerar que hay contenido cromático. Medido con la
/// ventana de Hann del motor (FFT 2048 @44.1k): un seno a 50 Hz filtra
/// −51 dB y uno a 100 Hz −33 dB por leakage; por debajo de −30 dB la
/// "cromática" sería solo leakage, no armonía audible.
const MIN_RANGE_POWER_RATIO: f32 = 1e-3;

/// Perfil Krumhansl-Schmuckler de la escala mayor (grados C..B).
const MAJOR_PROFILE: [f32; PITCH_CLASSES] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];

/// Perfil Krumhansl-Schmuckler de la escala menor armónica (grados C..B).
const MINOR_PROFILE: [f32; PITCH_CLASSES] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

/// Tonalidad estimada de un perfil cromático.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyEstimate {
    /// 0..23 (mayor/menor) o [`KEY_UNKNOWN`].
    pub key: i16,
    /// Correlación de Pearson de la mejor rotación, acotada a 0..1.
    pub confidence: f32,
}

impl Default for KeyEstimate {
    fn default() -> Self {
        Self {
            key: KEY_UNKNOWN,
            confidence: 0.0,
        }
    }
}

/// Pliega un espectro de magnitudes sobre las 12 clases de pica.
///
/// Devuelve un vector normalizado a suma 1. Si casi toda la potencia del
/// espectro cae fuera del rango musical (ver [`MIN_RANGE_POWER_RATIO`]) el
/// frame se trata como "sin contenido armónico" y se devuelven ceros — el
/// leakage de un sub-bass puro no debe fabricar una tonalidad.
/// `sample_rate` es la tasa de la SEÑAL analizada.
pub fn chroma_from_magnitudes(magnitudes: &[f32], sample_rate: f32) -> [f32; PITCH_CLASSES] {
    let mut chroma = [0.0f32; PITCH_CLASSES];
    if sample_rate <= 0.0 || magnitudes.is_empty() {
        return chroma;
    }
    let nyquist = sample_rate / 2.0;
    let bin_hz = nyquist / magnitudes.len() as f32;
    let fmax = FMAX_HZ.min(nyquist);
    let mut spectrum_power = 0.0f32;

    for (i, &m) in magnitudes.iter().enumerate() {
        if m <= 0.0 {
            continue;
        }
        // Potencia (m²): como en las bandas, el oído percibe energía.
        let power = m * m;
        spectrum_power += power;
        let freq = i as f32 * bin_hz;
        if freq < FMIN_HZ || freq > fmax {
            continue;
        }
        let midi = 69.0 + 12.0 * (freq / 440.0).log2();
        let note = midi.round() as i32;
        chroma[note.rem_euclid(PITCH_CLASSES as i32) as usize] += power;
    }

    let total: f32 = chroma.iter().sum();
    if total <= f32::EPSILON || total <= spectrum_power * MIN_RANGE_POWER_RATIO {
        return [0.0; PITCH_CLASSES];
    }
    let inv = 1.0 / total;
    for v in chroma.iter_mut() {
        *v *= inv;
    }
    chroma
}

/// Estima la tonalidad de una cromática media (Krumhansl-Schmuckler).
///
/// Correlación de Pearson entre la cromática y cada rotación de los perfiles
/// mayor/menor; gana la mayor correlación. Sin contenido ⇒ [`KEY_UNKNOWN`].
pub fn estimate_key(chroma: &[f32; PITCH_CLASSES]) -> KeyEstimate {
    let energy: f32 = chroma.iter().map(|v| v.max(0.0)).sum();
    if energy <= 1e-6 {
        return KeyEstimate::default();
    }

    let mut best_r = -2.0f32;
    let mut best_key = KEY_UNKNOWN;
    for key in 0..KEY_COUNT {
        let profile = if key < 12 {
            &MAJOR_PROFILE
        } else {
            &MINOR_PROFILE
        };
        let offset = (key.rem_euclid(12)) as usize;
        // Rotación: el grado tónico del perfil (índice 0) va a la clase `offset`.
        let mut rotated = [0.0f32; PITCH_CLASSES];
        for (i, slot) in rotated.iter_mut().enumerate() {
            *slot = profile[(i + PITCH_CLASSES - offset) % PITCH_CLASSES];
        }
        let r = pearson(chroma, &rotated);
        if r > best_r {
            best_r = r;
            best_key = key;
        }
    }

    KeyEstimate {
        key: best_key,
        confidence: best_r.clamp(0.0, 1.0),
    }
}

/// Correlación de Pearson entre dos vectores de 12 (0 si alguna varianza es 0).
fn pearson(a: &[f32; PITCH_CLASSES], b: &[f32; PITCH_CLASSES]) -> f32 {
    const N: f32 = PITCH_CLASSES as f32;
    let mean_a = a.iter().sum::<f32>() / N;
    let mean_b = b.iter().sum::<f32>() / N;
    let mut num = 0.0f32;
    let mut var_a = 0.0f32;
    let mut var_b = 0.0f32;
    for i in 0..PITCH_CLASSES {
        let da = a[i] - mean_a;
        let db = b[i] - mean_b;
        num += da * db;
        var_a += da * da;
        var_b += db * db;
    }
    let denom = (var_a * var_b).sqrt();
    if denom <= f32::EPSILON {
        0.0
    } else {
        num / denom
    }
}

/// Nombre legible de la tónica (`"C"`, `"C"`…; `"?"` si es desconocida).
/// Solo para tests/diagnóstico (no distingue mayor/menor).
pub fn key_label(key: i16) -> &'static str {
    const NAMES: [&str; PITCH_CLASSES] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    match key {
        k if (0..12).contains(&k) => NAMES[k as usize],
        k if (12..24).contains(&k) => NAMES[(k - 12) as usize],
        _ => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 44_100.0;
    const FFT_SIZE: usize = 2048;

    fn sine_magnitudes(freq: f32) -> Vec<f32> {
        let mut analyzer = crate::analysis::fft::SpectrumAnalyzer::new(FFT_SIZE);
        let samples: Vec<f32> = (0..FFT_SIZE)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / SR).sin())
            .collect();
        analyzer.magnitudes(&samples)
    }

    #[test]
    fn pure_a440_lands_on_pitch_class_a() {
        let mags = sine_magnitudes(440.0);
        let chroma = chroma_from_magnitudes(&mags, SR);
        let sum: f32 = chroma.iter().sum();
        assert!((sum - 1.0).abs() < 1e-5, "normalizado a 1: {sum}");
        let (best, &power) = chroma
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap();
        assert_eq!(best, 9, "440 Hz = clase A (9)");
        assert!(power > 0.95, "el tono domina la cromática: {power}");
    }

    #[test]
    fn c4_lands_on_pitch_class_c() {
        let mags = sine_magnitudes(261.63);
        let chroma = chroma_from_magnitudes(&mags, SR);
        let (best, _) = chroma
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap();
        assert_eq!(best, 0, "C4 = clase C (0)");
    }

    #[test]
    fn out_of_range_content_yields_zero_chroma() {
        // 50 Hz (bajo C3) y 100 Hz (E2, también bajo): el leakage de la
        // ventana de Hann filtra por debajo del umbral ⇒ sin contenido.
        for freq in [50.0, 100.0] {
            let mags = sine_magnitudes(freq);
            let chroma = chroma_from_magnitudes(&mags, SR);
            assert_eq!(
                chroma, [0.0; PITCH_CLASSES],
                "fuera de rango ({freq} Hz) ⇒ cromática cero"
            );
        }
    }

    #[test]
    fn silence_yields_zero_chroma() {
        let chroma = chroma_from_magnitudes(&vec![0.0; FFT_SIZE / 2], SR);
        assert_eq!(chroma, [0.0; PITCH_CLASSES]);
        assert_eq!(estimate_key(&chroma).key, KEY_UNKNOWN);
    }

    #[test]
    fn detects_c_major_from_diatonic_chroma() {
        // Perfil C mayor "ideal" rotado a C (offset 0) ⇒ key 0 con confianza alta.
        let mut chroma = [0.0f32; PITCH_CLASSES];
        for (i, &w) in MAJOR_PROFILE.iter().enumerate() {
            chroma[i] = w;
        }
        let total: f32 = chroma.iter().sum();
        for v in chroma.iter_mut() {
            *v /= total;
        }
        let est = estimate_key(&chroma);
        assert_eq!(est.key, 0, "debería detectar C mayor");
        assert!(est.confidence > 0.9, "confianza: {}", est.confidence);
    }

    #[test]
    fn detects_a_minor_from_minor_profile() {
        // Perfil menor rotado a A (offset 9): A menor.
        let mut chroma = [0.0f32; PITCH_CLASSES];
        for (i, slot) in chroma.iter_mut().enumerate() {
            *slot = MINOR_PROFILE[(i + PITCH_CLASSES - 9) % PITCH_CLASSES];
        }
        let total: f32 = chroma.iter().sum();
        for v in chroma.iter_mut() {
            *v /= total;
        }
        let est = estimate_key(&chroma);
        assert_eq!(est.key, 12 + 9, "debería detectar A menor");
        assert!(est.confidence > 0.9, "confianza: {}", est.confidence);
    }

    #[test]
    fn key_label_formats_major_and_minor() {
        assert_eq!(key_label(0), "C");
        assert_eq!(key_label(9), "A");
        assert_eq!(key_label(12 + 9), "A");
        assert_eq!(key_label(KEY_UNKNOWN), "?");
    }
}
