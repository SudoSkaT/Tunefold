//! Parámetros canónicos del pipeline DSP.
//!
//! Este es el único modelo de configuración que viaja del hilo de control al
//! hilo de audio. Es `Copy` y de tamaño fijo: el camino caliente nunca clona
//! colecciones ni asigna memoria para leer una configuración nueva.
//!
//! Los valores se sanean en [`DspParams::sanitized`] antes de publicarse. Un
//! NaN o un infinito procedente de la UI nunca debe llegar a un filtro.

/// Número máximo de bandas paramétricas.
pub const MAX_EQ_BANDS: usize = 12;

/// Frecuencias de la configuración gráfica de referencia (10 bandas).
pub const GRAPHIC_BAND_FREQUENCIES_HZ: [f32; 10] = [
    31.0, 62.0, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0,
];

const PREAMP_LIMIT_DB: f32 = 24.0;
const BAND_GAIN_LIMIT_DB: f32 = 24.0;
const BAND_FREQ_MIN_HZ: f32 = 10.0;
const BAND_FREQ_MAX_HZ: f32 = 24_000.0;
const BAND_Q_MIN: f32 = 0.1;
const BAND_Q_MAX: f32 = 24.0;
const LIMITER_CEILING_MIN_DB: f32 = -12.0;
const LIMITER_CEILING_MAX_DB: f32 = 0.0;
const LIMITER_ATTACK_MIN_MS: f32 = 0.1;
const LIMITER_ATTACK_MAX_MS: f32 = 50.0;
const LIMITER_RELEASE_MIN_MS: f32 = 1.0;
const LIMITER_RELEASE_MAX_MS: f32 = 2_000.0;
const NORMALIZATION_TARGET_MIN_LUFS: f32 = -40.0;
const NORMALIZATION_TARGET_MAX_LUFS: f32 = -5.0;
const NORMALIZATION_GAIN_LIMIT_DB: f32 = 24.0;
const NORMALIZATION_SOURCE_LUFS_MIN: f32 = -70.0;
const NORMALIZATION_SOURCE_LUFS_MAX: f32 = 0.0;
const STEREO_WIDTH_MAX: f32 = 2.0;
/// Diferencia de ganancia por debajo de la cual una banda se considera plana.
const FLAT_GAIN_DB: f32 = 1e-6;
/// Diferencia de anchura estéreo por debajo de la cual el ajuste es neutro.
const FLAT_WIDTH_EPSILON: f32 = 1e-6;

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

fn clamp_or(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    finite_or(value, fallback).clamp(min, max)
}

/// Una banda de ecualización paramétrica.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EqBand {
    /// Frecuencia central en Hz.
    pub frequency_hz: f32,
    /// Ganancia en dB (positiva refuerza, negativa atenúa).
    pub gain_db: f32,
    /// Factor de calidad del filtro.
    pub q: f32,
    /// `false` desactiva la banda sin perder sus valores.
    pub enabled: bool,
}

impl EqBand {
    /// Banda neutra en `frequency_hz`.
    pub const fn flat(frequency_hz: f32) -> Self {
        Self {
            frequency_hz,
            gain_db: 0.0,
            q: 1.0,
            enabled: true,
        }
    }

    /// `true` si la banda no altera la señal.
    pub fn is_flat(&self) -> bool {
        !self.enabled || self.gain_db.abs() <= FLAT_GAIN_DB
    }

    fn sanitize(&mut self) {
        self.frequency_hz = clamp_or(
            self.frequency_hz,
            BAND_FREQ_MIN_HZ,
            BAND_FREQ_MAX_HZ,
            1_000.0,
        );
        self.gain_db = clamp_or(self.gain_db, -BAND_GAIN_LIMIT_DB, BAND_GAIN_LIMIT_DB, 0.0);
        self.q = clamp_or(self.q, BAND_Q_MIN, BAND_Q_MAX, 1.0);
    }
}

impl Default for EqBand {
    fn default() -> Self {
        Self::flat(1_000.0)
    }
}

/// Configuración del ecualizador.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EqParams {
    /// Interruptor propio del ecualizador. Apagarlo conserva las bandas.
    pub enabled: bool,
    /// Número de bandas activas dentro de [`Self::bands`].
    pub band_count: u8,
    pub bands: [EqBand; MAX_EQ_BANDS],
}

impl EqParams {
    /// Perfil plano con las diez bandas gráficas de referencia.
    pub fn graphic_flat() -> Self {
        let mut bands = [EqBand::default(); MAX_EQ_BANDS];
        for (band, frequency) in bands.iter_mut().zip(GRAPHIC_BAND_FREQUENCIES_HZ) {
            *band = EqBand::flat(frequency);
        }
        Self {
            enabled: false,
            band_count: GRAPHIC_BAND_FREQUENCIES_HZ.len() as u8,
            bands,
        }
    }

    /// `true` si el ecualizador no altera la señal.
    pub fn is_flat(&self) -> bool {
        if !self.enabled {
            return true;
        }
        self.bands[..self.active_band_count()]
            .iter()
            .all(EqBand::is_flat)
    }

    /// Número de bandas efectivo, siempre dentro de los límites del array.
    pub fn active_band_count(&self) -> usize {
        usize::from(self.band_count).min(MAX_EQ_BANDS)
    }

    fn sanitize(&mut self) {
        self.band_count = self.band_count.min(MAX_EQ_BANDS as u8);
        for band in &mut self.bands {
            band.sanitize();
        }
    }
}

impl Default for EqParams {
    fn default() -> Self {
        Self::graphic_flat()
    }
}

/// Configuración del limitador de picos.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LimiterParams {
    pub enabled: bool,
    /// Techo de salida en dBFS (≤ 0).
    pub ceiling_db: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
}

impl LimiterParams {
    pub fn is_flat(&self) -> bool {
        !self.enabled
    }

    fn sanitize(&mut self) {
        self.ceiling_db = clamp_or(
            self.ceiling_db,
            LIMITER_CEILING_MIN_DB,
            LIMITER_CEILING_MAX_DB,
            -1.0,
        );
        self.attack_ms = clamp_or(
            self.attack_ms,
            LIMITER_ATTACK_MIN_MS,
            LIMITER_ATTACK_MAX_MS,
            5.0,
        );
        self.release_ms = clamp_or(
            self.release_ms,
            LIMITER_RELEASE_MIN_MS,
            LIMITER_RELEASE_MAX_MS,
            100.0,
        );
    }
}

impl Default for LimiterParams {
    fn default() -> Self {
        Self {
            enabled: false,
            ceiling_db: -1.0,
            attack_ms: 5.0,
            release_ms: 100.0,
        }
    }
}

/// Estrategia de normalización de sonoridad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum NormalizationMode {
    #[default]
    Off,
    /// Ganancia medida por pista (ReplayGain track).
    ReplayGainTrack,
    /// Ganancia medida por álbum (ReplayGain album).
    ReplayGainAlbum,
    /// Sonoridad integrada propia (LUFS).
    Lufs,
}

/// Configuración de normalización de volumen.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NormalizationParams {
    pub mode: NormalizationMode,
    /// Objetivo de sonoridad en LUFS.
    pub target_lufs: f32,
    /// Ganancia de ReplayGain/medición previa en dB.
    pub gain_db: f32,
    /// Sonoridad integrada medida de la fuente en LUFS.
    pub source_lufs: f32,
    /// Pico de la fuente en escala lineal; `0.0` significa desconocido.
    pub source_peak: f32,
    /// Evitar que la ganancia de normalización empuje la señal por encima de
    /// 0 dBFS cuando el material ya es caliente.
    pub prevent_clipping: bool,
}

impl NormalizationParams {
    pub fn is_flat(&self) -> bool {
        self.mode == NormalizationMode::Off || self.effective_gain_db().abs() <= FLAT_GAIN_DB
    }

    /// Ganancia final que debe aplicar el procesador, ya limitada y con la
    /// protección de picos aplicada si se conoce el pico de la fuente.
    pub fn effective_gain_db(&self) -> f32 {
        let mut gain_db = match self.mode {
            NormalizationMode::Off => 0.0,
            NormalizationMode::ReplayGainTrack | NormalizationMode::ReplayGainAlbum => self.gain_db,
            NormalizationMode::Lufs => self.target_lufs - self.source_lufs,
        };
        if self.prevent_clipping && self.source_peak > 0.0 {
            let ceiling_gain = 20.0 * (0.999 / self.source_peak).log10();
            if gain_db > ceiling_gain {
                gain_db = ceiling_gain;
            }
        }
        gain_db.clamp(-NORMALIZATION_GAIN_LIMIT_DB, NORMALIZATION_GAIN_LIMIT_DB)
    }

    fn sanitize(&mut self) {
        self.target_lufs = clamp_or(
            self.target_lufs,
            NORMALIZATION_TARGET_MIN_LUFS,
            NORMALIZATION_TARGET_MAX_LUFS,
            -14.0,
        );
        self.gain_db = clamp_or(
            self.gain_db,
            -NORMALIZATION_GAIN_LIMIT_DB,
            NORMALIZATION_GAIN_LIMIT_DB,
            0.0,
        );
        self.source_lufs = clamp_or(
            self.source_lufs,
            NORMALIZATION_SOURCE_LUFS_MIN,
            NORMALIZATION_SOURCE_LUFS_MAX,
            -14.0,
        );
        self.source_peak = clamp_or(self.source_peak, 0.0, 4.0, 0.0);
    }
}

impl Default for NormalizationParams {
    fn default() -> Self {
        Self {
            mode: NormalizationMode::Off,
            target_lufs: -14.0,
            gain_db: 0.0,
            source_lufs: -14.0,
            source_peak: 0.0,
            prevent_clipping: true,
        }
    }
}

/// Ajustes estéreo opcionales.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StereoParams {
    /// −1 (izquierda) … 0 (centro) … +1 (derecha).
    pub balance: f32,
    /// 1.0 conserva la anchura original; 0 colapsa a mono.
    pub width: f32,
    /// Mezcla de crossfeed para auriculares: 0 desactivado.
    pub crossfeed: f32,
}

impl StereoParams {
    pub fn is_flat(&self) -> bool {
        self.balance.abs() <= FLAT_WIDTH_EPSILON
            && (self.width - 1.0).abs() <= FLAT_WIDTH_EPSILON
            && self.crossfeed.abs() <= FLAT_WIDTH_EPSILON
    }

    fn sanitize(&mut self) {
        self.balance = clamp_or(self.balance, -1.0, 1.0, 0.0);
        self.width = clamp_or(self.width, 0.0, STEREO_WIDTH_MAX, 1.0);
        self.crossfeed = clamp_or(self.crossfeed, 0.0, 1.0, 0.0);
    }
}

impl Default for StereoParams {
    fn default() -> Self {
        Self {
            balance: 0.0,
            width: 1.0,
            crossfeed: 0.0,
        }
    }
}

/// Configuración completa del pipeline.
///
/// `Copy` y sin asignaciones: el hilo de audio puede copiarla entera en cada
/// bloque sin reservar memoria.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DspParams {
    /// Interruptor general. En `false` la señal pasa sin tocar.
    pub enabled: bool,
    /// Preamplificación global en dB.
    pub preamp_db: f32,
    pub eq: EqParams,
    pub limiter: LimiterParams,
    pub normalization: NormalizationParams,
    pub stereo: StereoParams,
}

impl DspParams {
    /// Ruta de referencia: sin procesamiento tonal, normalización ni efectos.
    pub fn flat() -> Self {
        Self {
            enabled: true,
            preamp_db: 0.0,
            eq: EqParams::graphic_flat(),
            limiter: LimiterParams::default(),
            normalization: NormalizationParams::default(),
            stereo: StereoParams::default(),
        }
    }

    /// Pipeline apagado por completo (equivalente sónico a [`Self::flat`]).
    pub fn bypassed() -> Self {
        Self {
            enabled: false,
            ..Self::flat()
        }
    }

    /// `true` si el pipeline no debe modificar ninguna muestra.
    pub fn is_flat(&self) -> bool {
        !self.enabled
            || (self.preamp_db.abs() <= FLAT_GAIN_DB
                && self.eq.is_flat()
                && self.limiter.is_flat()
                && self.normalization.is_flat()
                && self.stereo.is_flat())
    }

    /// `true` si hay al menos un procesador con trabajo real que hacer.
    pub fn is_active(&self) -> bool {
        !self.is_flat()
    }

    /// Copia saneada: valores no finitos vuelven a su neutro y los rangos se
    /// recortan antes de publicar la configuración al audio.
    pub fn sanitized(mut self) -> Self {
        self.preamp_db = clamp_or(self.preamp_db, -PREAMP_LIMIT_DB, PREAMP_LIMIT_DB, 0.0);
        self.eq.sanitize();
        self.limiter.sanitize();
        self.normalization.sanitize();
        self.stereo.sanitize();
        self
    }
}

impl Default for DspParams {
    fn default() -> Self {
        Self::flat()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_flat_and_inactive() {
        let params = DspParams::default();
        assert!(params.is_flat());
        assert!(!params.is_active());
        assert!(params.enabled);
    }

    #[test]
    fn bypass_is_flat_but_remembers_it_is_disabled() {
        let params = DspParams::bypassed();
        assert!(!params.enabled);
        assert!(params.is_flat());
    }

    #[test]
    fn a_non_zero_band_breaks_flat() {
        let mut params = DspParams::flat();
        params.eq.enabled = true;
        params.eq.bands[0].gain_db = 3.0;
        assert!(!params.is_flat());
        assert!(params.is_active());
    }

    #[test]
    fn disabled_eq_with_retained_gains_is_still_flat() {
        let mut params = DspParams::flat();
        params.eq.bands[0].gain_db = 8.0;
        params.eq.enabled = false;
        assert!(params.is_flat());
    }

    #[test]
    fn sanitize_clamps_and_replaces_non_finite_values() {
        let mut params = DspParams::flat();
        params.preamp_db = f32::NAN;
        params.eq.band_count = 200;
        params.eq.bands[0].frequency_hz = f32::INFINITY;
        params.eq.bands[0].gain_db = 90.0;
        params.eq.bands[0].q = -4.0;
        params.limiter.ceiling_db = 12.0;
        params.limiter.attack_ms = 0.0;
        params.limiter.release_ms = f32::NAN;
        params.normalization.target_lufs = -80.0;
        params.stereo.balance = 9.0;
        params.stereo.width = -1.0;
        params.stereo.crossfeed = f32::NAN;
        let params = params.sanitized();

        assert_eq!(params.preamp_db, 0.0);
        assert_eq!(params.eq.band_count, MAX_EQ_BANDS as u8);
        assert_eq!(params.eq.bands[0].frequency_hz, 1_000.0);
        assert_eq!(params.eq.bands[0].gain_db, BAND_GAIN_LIMIT_DB);
        assert_eq!(params.eq.bands[0].q, BAND_Q_MIN);
        assert_eq!(params.limiter.ceiling_db, LIMITER_CEILING_MAX_DB);
        assert_eq!(params.limiter.attack_ms, LIMITER_ATTACK_MIN_MS);
        assert_eq!(params.limiter.release_ms, 100.0);
        assert_eq!(
            params.normalization.target_lufs,
            NORMALIZATION_TARGET_MIN_LUFS
        );
        assert_eq!(params.stereo.balance, 1.0);
        assert_eq!(params.stereo.width, 0.0);
        assert_eq!(params.stereo.crossfeed, 0.0);
        assert!(params
            .eq
            .bands
            .iter()
            .all(|band| band.frequency_hz.is_finite()));
    }

    #[test]
    fn graphic_profile_uses_the_ten_reference_frequencies() {
        let eq = EqParams::graphic_flat();
        assert_eq!(eq.active_band_count(), 10);
        let frequencies: Vec<f32> = eq.bands[..10]
            .iter()
            .map(|band| band.frequency_hz)
            .collect();
        assert_eq!(frequencies, GRAPHIC_BAND_FREQUENCIES_HZ.to_vec());
        assert!(eq.is_flat());
    }

    #[test]
    fn normalization_effective_gain_uses_the_selected_mode() {
        let mut params = DspParams::flat();
        params.normalization.mode = NormalizationMode::ReplayGainTrack;
        params.normalization.gain_db = -7.5;
        assert_eq!(params.normalization.effective_gain_db(), -7.5);

        params.normalization.mode = NormalizationMode::Lufs;
        params.normalization.target_lufs = -14.0;
        params.normalization.source_lufs = -19.0;
        params.normalization.source_peak = 0.0;
        assert_eq!(params.normalization.effective_gain_db(), 5.0);
    }

    #[test]
    fn normalization_peak_protection_reduces_positive_gain() {
        let mut params = DspParams::flat();
        params.normalization.mode = NormalizationMode::ReplayGainTrack;
        params.normalization.gain_db = 12.0;
        params.normalization.source_peak = 0.8;
        params.normalization.prevent_clipping = true;
        let gain = params.normalization.effective_gain_db();
        let expected = 20.0 * (0.999f32 / 0.8).log10();
        assert!((gain - expected).abs() < 1e-4);

        params.normalization.prevent_clipping = false;
        assert_eq!(params.normalization.effective_gain_db(), 12.0);
    }

    #[test]
    fn normalization_fields_are_sanitized() {
        let mut params = DspParams::flat();
        params.normalization.gain_db = f32::INFINITY;
        params.normalization.source_lufs = f32::NAN;
        params.normalization.source_peak = -2.0;
        let params = params.sanitized();
        assert_eq!(params.normalization.gain_db, 0.0);
        assert_eq!(params.normalization.source_lufs, -14.0);
        assert_eq!(params.normalization.source_peak, 0.0);
    }

    #[test]
    fn neutral_stereo_and_limiter_do_not_activate_the_chain() {
        let mut params = DspParams::flat();
        params.stereo.width = 1.0;
        params.limiter.enabled = false;
        assert!(params.is_flat());
        params.stereo.width = 1.5;
        assert!(!params.is_flat());
    }
}
