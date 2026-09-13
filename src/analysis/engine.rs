//! Motor de análisis: hilo dedicado que convierte PCM del anillo en
//! [`AudioFeatures`] publicadas al bus (spec §19-§20).
//!
//! Diseño del camino caliente:
//! - cero allocations por hop salvo los DOS snapshots publicados (features +
//!   envolvente, uno cada ~11 ms);
//! - ventana deslizante de tamaño `fft_size`, procesando cuando hay ≥`hop`
//!   muestras nuevas (overlap 75% con los valores por defecto);
//! - si la fuente cambia de formato o hay un GAP (>300 ms sin datos: cambio
//!   de canción / fin de stream), se resetea TODO el estado (ventana, flujo,
//!   historial de onset/tempo) para no mezclar pistas;
//! - timestamps = tiempo de STREAM analizado (`hops × hop_time`).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::bands::{band_ratios, BandEdges};
use super::beat::BpmEstimator;
use super::features::{AudioFeatures, FeatureBus, RawFeatures};
use super::fft::SpectrumAnalyzer;
use super::onset::{FluxAnalyzer, OnsetDetector};
use super::ring::SpScRing;
use super::smoother::{FeatureSmoother, SMOOTHED_CHANNELS};
use super::waveform::{StereoWaveform, WaveformBus};

/// Configuración del pipeline DSP.
#[derive(Debug, Clone, Copy)]
pub struct AnalysisConfig {
    /// Tamaño de FFT (potencia de dos). 2048 @44.1k ≈ 46 ms de ventana.
    pub fft_size: usize,
    /// Hop size (avance entre análisis). 512 ≈ 11.6 ms → ~86 fps de features.
    pub hop: usize,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            fft_size: 2048,
            hop: 512,
        }
    }
}

impl AnalysisConfig {
    pub fn hop_rate_hz(&self, sample_rate: u32) -> f32 {
        sample_rate as f32 / self.hop as f32
    }
}

/// Metadatos del stream en curso (los fija el [`super::tap`] en cada play).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamMeta {
    pub sample_rate: u32,
    pub channels: u16,
}

/// Handle del lado PRODUCTOR (lo clona cada `TapSource` nuevo).
#[derive(Clone)]
pub struct PcmTap {
    ring: Arc<SpScRing>,
    meta: Arc<Mutex<Option<StreamMeta>>>,
}

impl PcmTap {
    /// Publica el formato del stream que entra por este tap (llamar al
    /// empezar a alimentar; el motor resetea su estado si el formato cambia).
    pub fn announce(&self, meta: StreamMeta) {
        *self.meta.lock().unwrap() = Some(meta);
    }

    /// Empuja muestras interleaveadas (nunca bloquea).
    pub fn feed(&self, samples: &[f32]) {
        self.ring.push(samples);
    }
}

/// Runtime completo: productor + hilo + bus. Dropearlo detiene el hilo.
pub struct AnalysisRuntime {
    tap: PcmTap,
    bus: FeatureBus,
    waveform: WaveformBus,
    stop: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl std::fmt::Debug for AnalysisRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnalysisRuntime").finish_non_exhaustive()
    }
}

impl AnalysisRuntime {
    /// Arranca el hilo de análisis con la configuración dada.
    pub fn spawn(config: AnalysisConfig) -> Self {
        assert!(config.fft_size.is_power_of_two(), "fft_size potencia de 2");
        assert!(config.hop <= config.fft_size / 2, "hop ≤ fft/2");

        // 512 KiB de f32 ≈ 2.9 s mono / 1.5 s estéreo @44k: margen sobrado
        // para la ventana (2048) + jitter de scheduling, sin retener MB.
        let ring = SpScRing::new(1 << 17);
        let self_bus = FeatureBus::new();
        let self_waveform = WaveformBus::new();
        let meta: Arc<Mutex<Option<StreamMeta>>> = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));

        let join = {
            let ring = Arc::clone(&ring);
            let meta = Arc::clone(&meta);
            let stop = Arc::clone(&stop);
            let bus_for_thread = self_bus.clone();
            let waveform_for_thread = self_waveform.clone();
            std::thread::Builder::new()
                .name("audio-analysis".into())
                .spawn(move || {
                    run(
                        config,
                        ring,
                        meta,
                        stop,
                        bus_for_thread,
                        waveform_for_thread,
                    )
                })
                .expect("spawn del hilo de análisis")
        };

        Self {
            tap: PcmTap { ring, meta },
            bus: self_bus,
            waveform: self_waveform,
            stop,
            join: Some(join),
        }
    }

    /// Handle productor para el motor de reproducción.
    pub fn tap(&self) -> PcmTap {
        self.tap.clone()
    }

    /// Bus de lectura para consumidores (visualización/métricas).
    pub fn bus(&self) -> FeatureBus {
        self.bus.clone()
    }

    /// Bus de envolvente de forma de onda (mismo hilo, mismo cadencia).
    pub fn waveform_bus(&self) -> WaveformBus {
        self.waveform.clone()
    }
}

impl Drop for AnalysisRuntime {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            // El hilo duerme como mucho unos ms: join acotado en la práctica.
            let _ = join.join();
        }
    }
}

fn run(
    config: AnalysisConfig,
    ring: Arc<SpScRing>,
    meta_cell: Arc<Mutex<Option<StreamMeta>>>,
    stop: Arc<AtomicBool>,
    bus: FeatureBus,
    waveform: WaveformBus,
) {
    let mut analyzer = SpectrumAnalyzer::new(config.fft_size);
    let mut flux = FluxAnalyzer::new();
    let mut onset = OnsetDetector::new(43, 0.005);
    let mut bpm = BpmEstimator::new(86.13); // recalibrado al conocer sample_rate
    let mut smoother = FeatureSmoother::new(12.0, 4.0);

    let mut left_window: VecDeque<f32> = VecDeque::with_capacity(config.fft_size);
    let mut right_window: VecDeque<f32> = VecDeque::with_capacity(config.fft_size);
    // Buffers reutilizados hop tras hop: cero allocations en el camino caliente
    // (los únicos allocs por frame son los snapshots Arc de los dos buses).
    // `left_buf`/`right_buf` son las fotos de ventana por canal (envelope); el
    // FFT se alimenta con `frame_buf` = (L+R)/2 (downmix mono, sin allocs).
    let mut frame_buf: Vec<f32> = Vec::with_capacity(config.fft_size);
    let mut left_buf: Vec<f32> = Vec::with_capacity(config.fft_size);
    let mut right_buf: Vec<f32> = Vec::with_capacity(config.fft_size);
    let mut mags_buf: Vec<f32> = Vec::with_capacity(config.fft_size / 2);
    let mut since_hop = 0usize;
    let mut hops_analyzed = 0u64;
    let mut current_meta: Option<StreamMeta> = None;
    let mut last_data = Instant::now();
    let mut flushed_on_gap = true;
    let mut bpm_hold = 0.0f32;

    let mut buf = vec![0.0f32; 8192];

    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let n = ring.pop(&mut buf);

        // GAP: sin datos un rato ⇒ nueva canción/fin de stream. Reset total
        // para que la pista siguiente arranque limpia (sin cola de la vieja).
        if n == 0 {
            if !flushed_on_gap && last_data.elapsed() > Duration::from_millis(300) {
                left_window.clear();
                right_window.clear();
                since_hop = 0;
                hops_analyzed = 0;
                flux = FluxAnalyzer::new();
                onset = OnsetDetector::new(43, 0.005);
                bpm = BpmEstimator::new(hop_rate_of(current_meta, &config));
                smoother.reset();
                bpm_hold = 0.0;
                flushed_on_gap = true;
                let _ = bus.publish(AudioFeatures::silent(Duration::ZERO));
                let _ = waveform.publish(StereoWaveform::silent());
            }
            std::thread::sleep(Duration::from_millis(4));
            continue;
        }
        flushed_on_gap = false;
        last_data = Instant::now();

        // ¿Cambió el formato? Reset para no analizar mezcla de tasas.
        let announced = *meta_cell.lock().unwrap();
        if announced != current_meta {
            current_meta = announced;
            left_window.clear();
            right_window.clear();
            since_hop = 0;
            hops_analyzed = 0;
            flux = FluxAnalyzer::new();
            onset = OnsetDetector::new(43, 0.005);
            bpm = BpmEstimator::new(hop_rate_of(current_meta, &config));
            smoother.reset();
            bpm_hold = 0.0;
            let _ = waveform.publish(StereoWaveform::silent());
        }
        let Some(meta) = current_meta else {
            // Sin formato anunciado aún: descartar datos hasta el announce.
            continue;
        };
        let hop_time = config.hop as f32 / meta.sample_rate as f32;

        // Acumulación en las ventanas deslizantes POR CANAL (el osciloscopio
        // es estéreo; el downmix a mono solo alimenta el FFT).
        let ch = meta.channels.max(1) as usize;
        let frames = n / ch;
        if ch == 1 {
            // Mono: L = R = la señal (el renderer la pinta centrada).
            for &s in &buf[..frames] {
                push_window(&mut left_window, config.fft_size, s);
                push_window(&mut right_window, config.fft_size, s);
                since_hop += 1;
            }
        } else {
            // Estéreo/multicanal: L = ch0, R = ch1 (muestras interlapadas).
            for f in 0..frames {
                let base = f * ch;
                push_window(&mut left_window, config.fft_size, buf[base]);
                push_window(&mut right_window, config.fft_size, buf[base + 1]);
                since_hop += 1;
            }
        }

        // Analizar cada `hop` muestras nuevas (overlap natural de la ventana).
        while since_hop >= config.hop
            && left_window.len() == config.fft_size
            && right_window.len() == config.fft_size
        {
            since_hop -= config.hop;
            frame_buf.clear();
            left_buf.clear();
            right_buf.clear();
            for (i, &l) in left_window.iter().enumerate() {
                let r = right_window[i];
                frame_buf.push((l + r) * 0.5);
                left_buf.push(l);
                right_buf.push(r);
            }
            hops_analyzed += 1;

            let raw = analyze_frame(meta, &mut analyzer, &mut flux, &frame_buf, &mut mags_buf);
            let onset_out = onset.observe(raw.flux);
            let tempo = bpm.observe(onset_out.strength);
            if tempo.confidence >= 0.25 && tempo.bpm > 0.0 {
                bpm_hold = tempo.bpm;
            }

            let targets: [f32; SMOOTHED_CHANNELS] = [
                raw.bands.bass,
                raw.bands.low_mid,
                raw.bands.mid,
                raw.bands.high_mid,
                raw.bands.high,
                raw.centroid_norm,
                raw.flux.min(1.0),
                raw.rms.min(1.0),
                raw.amplitude.min(1.0),
            ];
            let sm = smoother.step(&targets, hop_time);

            let features = AudioFeatures {
                timestamp: Duration::from_secs_f64(hops_analyzed as f64 * hop_time as f64),
                rms: sm[7],
                amplitude: sm[8],
                bass: sm[0],
                low_mid: sm[1],
                mid: sm[2],
                high_mid: sm[3],
                high: sm[4],
                spectral_centroid: sm[5],
                spectral_flux: sm[6],
                // El onset va CRUDO (sin retardo de suavizado): es un pico.
                onset: onset_out.strength,
                beat: onset_out.triggered && tempo.confidence >= 0.35,
                beat_confidence: tempo.confidence,
                bpm: bpm_hold,
            };
            bus.publish(features);
            // La envolvente ESTÉREO del MISMO hop (misma ventana): el
            // consumidor visual la decima al ancho del terminal y la pinta por
            // puntos, conservando pico Y valle de cada canal. Un Arc por frame
            // ≈ 1 alloc extra por hop (documentada; la ruta del audio no la ve).
            waveform.publish(StereoWaveform::from_windows(&left_buf, &right_buf));
        }
    }
}

/// Empuja una muestra a una ventana deslizante de longitud fija (reusa el
/// VecDeque sin realojar: push_back + pop_front cuando excede).
fn push_window(window: &mut VecDeque<f32>, fft_size: usize, sample: f32) {
    window.push_back(sample);
    if window.len() > fft_size {
        window.pop_front();
    }
}

fn hop_rate_of(meta: Option<StreamMeta>, config: &AnalysisConfig) -> f32 {
    meta.map(|m| config.hop_rate_hz(m.sample_rate))
        .unwrap_or(86.13)
}

fn analyze_frame(
    meta: StreamMeta,
    analyzer: &mut SpectrumAnalyzer,
    flux: &mut FluxAnalyzer,
    frame: &[f32],
    mags_out: &mut Vec<f32>,
) -> RawFeatures {
    let rms_v = super::rms::rms(frame);
    let peak_v = super::rms::peak(frame);
    analyzer.magnitudes_into(frame, mags_out);

    let bands = band_ratios(mags_out, meta.sample_rate as f32, &BandEdges::default());

    // Centroide normalizado por Nyquist: Σ(f·m)/Σm / nyquist.
    let bin_hz = (meta.sample_rate as f32 / 2.0) / mags_out.len() as f32;
    let mut weighted = 0.0f32;
    let mut total = 0.0f32;
    for (i, m) in mags_out.iter().enumerate() {
        weighted += m * (i as f32 * bin_hz);
        total += m;
    }
    let centroid_hz = if total > 1e-9 { weighted / total } else { 0.0 };
    let centroid_norm = (centroid_hz / (meta.sample_rate as f32 / 2.0)).clamp(0.0, 1.0);

    RawFeatures {
        timestamp: Duration::ZERO, // lo completa el llamador
        rms: rms_v,
        amplitude: peak_v,
        bands,
        centroid_norm,
        flux: flux.flux(mags_out),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::test_support::sine_wave;

    /// Integración del hilo completo: seno constante → features estables con
    /// energía concentrada en graves y RMS > 0.
    #[test]
    fn engine_thread_publishes_features_from_sine() {
        const SR: u32 = 44_100;
        const CH: u16 = 2;
        let runtime = AnalysisRuntime::spawn(AnalysisConfig::default());
        let bus = runtime.bus();

        let tap = runtime.tap();
        tap.announce(StreamMeta {
            sample_rate: SR,
            channels: CH,
        });

        // ~1.2 s de audio estéreo entrelazado a trozos realistas.
        let total = (SR as usize * 6 / 5) * CH as usize;
        let mut fed = 0usize;
        let mut i = 0usize;
        while fed < total {
            let batch_len = (4096).min(total - fed);
            let batch: Vec<f32> = (0..batch_len)
                .map(|_| {
                    let s = sine_wave(120.0, SR as f32, i as f32 / SR as f32, 0.5);
                    i += 1;
                    s
                })
                .collect();
            tap.feed(&batch);
            fed += batch.len();
            std::thread::sleep(Duration::from_millis(1)); // ritmo realista
        }

        // Espera a ver features con contenido (el hilo va por detrás).
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut got = None;
        while Instant::now() < deadline {
            if let Some(f) = bus.latest() {
                if f.rms > 0.05 && f.timestamp > Duration::from_secs(1) {
                    got = Some(f);
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let f = got.expect("el motor publica features con contenido");
        assert!(f.bass > 0.15, "seno grave concentra bass: {}", f.bass);
        assert!(f.high < 0.05, "sin agudos: {}", f.high);
    }

    #[test]
    fn drop_stops_the_thread_promptly() {
        let started = Instant::now();
        {
            let rt = AnalysisRuntime::spawn(AnalysisConfig::default());
            rt.tap().announce(StreamMeta {
                sample_rate: 44_100,
                channels: 2,
            });
        }
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "el Drop hace join sin colgarse"
        );
    }

    #[test]
    fn engine_publishes_stereo_waveform_from_sine() {
        const SR: u32 = 44_100;
        const CH: u16 = 2;
        let runtime = AnalysisRuntime::spawn(AnalysisConfig::default());
        let waveform = runtime.waveform_bus();
        let tap = runtime.tap();
        tap.announce(StreamMeta {
            sample_rate: SR,
            channels: CH,
        });

        // Sin samples: la ventana está vacía y el bus aún no tiene nada.
        assert!(waveform.latest().is_none(), "sin audio ⇒ sin envolvente");

        // El mismo seno que alimenta features debe publicar envolventes:
        // llenar ~1 s de audio interleaveado (L y R comparten la señal ⇒
        // ambas curvas deben coincidir). El valor de cada frame se duplica
        // a L y R para que la/envolvente resultante sea idéntica.
        let total = (SR as usize) * CH as usize;
        let mut fed = 0usize;
        let mut i = 0usize;
        while fed < total {
            let batch_len = (4096).min(total - fed);
            let batch: Vec<f32> = (0..batch_len)
                .map(|_| {
                    let frame = i / CH as usize;
                    let s = sine_wave(120.0, SR as f32, frame as f32 / SR as f32, 0.5);
                    i += 1;
                    s
                })
                .collect();
            tap.feed(&batch);
            fed += batch.len();
            std::thread::sleep(Duration::from_millis(1));
        }

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut got = None;
        while Instant::now() < deadline {
            if let Some(st) = waveform.latest() {
                if st.peak() > 0.05 {
                    got = Some(st);
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let st = got.expect("el motor publica envolventes estéreo con contenido");
        assert!(
            st.left
                .min
                .iter()
                .zip(st.left.max.iter())
                .all(|(lo, hi)| lo <= hi)
                && st
                    .right
                    .min
                    .iter()
                    .zip(st.right.max.iter())
                    .all(|(lo, hi)| lo <= hi),
            "min ≤ max en todos los buckets de ambos canales"
        );
        assert!(
            st.peak() <= 1.0,
            "amplitud normalizada (peaks ≤ 1): {}",
            st.peak()
        );
        // Misma señal en L y R ⇒ curvas idénticas en cada bucket.
        assert_eq!(st.left, st.right, "el seno compartido clona ambas curvas");
    }

    #[test]
    fn engine_splits_stereo_channels_into_distinct_waveforms() {
        const SR: u32 = 44_100;
        let runtime = AnalysisRuntime::spawn(AnalysisConfig::default());
        let waveform = runtime.waveform_bus();
        let tap = runtime.tap();
        tap.announce(StreamMeta {
            sample_rate: SR,
            channels: 2,
        });

        // L = seno de 120 Hz, R = seno de 660 Hz: curvas distinta energía
        // por bucket (no se mezclan ni se cancelan).
        let total = (SR as usize) * 2usize;
        let mut fed = 0usize;
        let mut i = 0usize;
        while fed < total {
            let batch_len = (4096).min(total - fed);
            let batch: Vec<f32> = (0..batch_len)
                .map(|_| {
                    let t = i as f32 / SR as f32;
                    let s = if i.is_multiple_of(2) {
                        sine_wave(120.0, SR as f32, t, 0.6)
                    } else {
                        sine_wave(660.0, SR as f32, t, 0.6)
                    };
                    i += 1;
                    s
                })
                .collect();
            tap.feed(&batch);
            fed += batch.len();
            std::thread::sleep(Duration::from_millis(1));
        }

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut got = None;
        while Instant::now() < deadline {
            if let Some(st) = waveform.latest() {
                if st.left.peak() > 0.1 {
                    got = Some(st);
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let st = got.expect("el canal L dominante se publica con contenido");
        assert!(
            st.left != st.right,
            "señales distintas ⇒ curvas distintas (no se mezclan)"
        );
        assert!(
            st.left.peak() > 0.3,
            "L conserva su energía: {}",
            st.left.peak()
        );
    }
}
