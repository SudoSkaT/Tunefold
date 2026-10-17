//! Pre-análisis acústico de candidatos de recomendación (v0.20).
//!
//! El ranking compara perfiles acústicos, pero la mayoría de candidatos de
//! una lista "relacionada" aún no han sonado ⇒ no tienen perfil ⇒ el ranking
//! los empataría en neutro y el orden volvería a ser el del provider. Este
//! módulo cierra ese hueco de forma ECONÓMICA: cuando la cobertura de perfiles
//! de una lista es baja, descarga el PREFIJO (~45 s) de los primeros
//! candidatos sin perfil, lo decodifica SIN dispositivo de audio, lo analiza
//! con el mismo pipeline DSP de la reproducción y persiste el perfil.
//!
//! Reglas:
//! - Solo bajo la feature `rodio` (desktop): en Android el ranking corre en
//!   el `.so` con los perfiles que allí existan.
//! - Respeta `AUDIO_ANALYSIS_ENABLED` (lo decide el caller).
//! - Nunca construye salida de audio: el decoder de rodio es un `Iterator`
//!   de muestras; el dispositivo solo existe en `SharedOutput`.
//! - Barato por diseño: ~45 s × top-K candidatos, serializado, una sola vez
//!   por lista fría; el re-rank resultante se emite como actualización
//!   tardía de la MISMA generación (ver `RecommendationSession::accepts_update`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::domain::track::Track;
use crate::recommendation::acoustic_aggregator::AcousticAggregator;
use crate::recommendation::types::TrackAcousticProfile;

/// Borra la semilla del conjunto "en vuelo" al dropearse (patrón del
/// `PreloadManager`): la limpieza no depende de que la tarea termine bien.
#[cfg(feature = "rodio")]
pub(crate) struct InflightGuard(pub(crate) Arc<Mutex<InflightSet>>, pub(crate) String);

#[cfg(feature = "rodio")]
impl Drop for InflightGuard {
    fn drop(&mut self) {
        if let Ok(mut set) = self.0.lock() {
            set.remove(&self.1);
        }
    }
}

/// Semillas cuyo pre-análisis está en vuelo.
pub(crate) type InflightSet = std::collections::HashSet<String>;

/// Segundos de audio analizados por candidato. 45 s bastan para bandas,
/// BPM, cromática y ~8 buckets estructurales (~5.6 s/bucket).
pub(crate) const PREANALYZE_SECS: u32 = 45;

/// Máximo de candidatos pre-analizados por lista. El resto se beneficia en
/// la siguiente carga (ya con perfil en BD).
pub(crate) const PREANALYZE_TOP_K: usize = 10;

/// Cobertura mínima de perfiles para NO pre-analizar: por encima de este
/// umbral el ranking ya decide con datos suficientes.
pub(crate) const PREANALYZE_COVERAGE: f32 = 0.3;

/// ¿La lista necesita pre-análisis? `with` = candidatos con perfil ya
/// conocido; `total` = longitud de la lista.
pub(crate) fn needs_preanalysis(with: usize, total: usize) -> bool {
    total > 0 && (with as f32) / (total as f32) < PREANALYZE_COVERAGE
}

/// Los primeros `top_k` candidatos SIN perfil, en su orden de ranking
/// actual (así el pre-análisis refuerza justo a los que el usuario verá
/// arriba). `resolved` paralela a `related`: id interno por candidato
/// (`None` si no está en la BD).
pub(crate) fn candidates_missing_profile(
    related: &[Track],
    resolved: &[Option<i64>],
    profiles: &HashMap<i64, TrackAcousticProfile>,
    top_k: usize,
) -> Vec<Track> {
    debug_assert_eq!(related.len(), resolved.len());
    related
        .iter()
        .zip(resolved)
        .filter(|(_, id)| !id.is_some_and(|id| profiles.contains_key(&id)))
        .take(top_k)
        .map(|(t, _)| t.clone())
        .collect()
}

/// Descarga el prefijo de `track` (`secs` segundos), lo decodifica y devuelve
/// su perfil acústico. `None` si no se pudo analizar (sin stream, error de
/// red/decodificación o tramo demasiado corto).
#[cfg(feature = "rodio")]
pub(crate) async fn pre_analyze_prefix(
    http: &reqwest::Client,
    resolver: &crate::media::StreamResolver,
    track: &Track,
    secs: u32,
) -> Option<TrackAcousticProfile> {
    use crate::media::{HttpRangeStream, RangePolicy, TransportFailure};

    // 1) URL + headers por la misma infraestructura que la reproducción
    // (caché memoria+SQLite, política de fallos, verificación).
    let res = resolver.resolve(track).await.ok()?;
    if res.uri.trim().is_empty() {
        return None;
    }

    // 2) Presupuesto de bytes del prefijo (~bitrate×tiempo×1.1 + margen).
    //    A ~45 s @128 kbps son ~720 KiB: por debajo del techo posicional
    //    de CDNs que capan a ~1 MiB.
    let bitrate = res.bitrate.unwrap_or(128_000) as f64;
    let budget = (bitrate / 8.0 * secs as f64 * 1.1) as u64 + 128 * 1024;

    // 3) Transporte Range + buffer compartido (mismas piezas que la
    //    reproducción; aquí nadie toca el dispositivo de audio).
    let mut transport = HttpRangeStream::open(
        http.clone(),
        res.uri.clone(),
        res.headers.clone(),
        RangePolicy::default(),
    )
    .await
    .ok()?;
    let buffer = std::sync::Arc::new(crate::infrastructure::playback::StreamBuffer::new());
    let writer = buffer.clone();
    let downloader = tokio::spawn(async move {
        let mut got = 0u64;
        while got < budget {
            match transport.next_chunk(256 * 1024).await {
                Ok(Some(chunk)) => {
                    got += chunk.len() as u64;
                    writer.append(&chunk);
                }
                // Fin de stream: el prefijo ya está completo.
                Ok(None) => break,
                // Techo del CDN: el prefijo servible se decodifica tal cual
                // (mismo trato que la reproducción ante un corte por límite).
                Err(TransportFailure::Restricted { .. }) => break,
                Err(_) => {
                    writer.fail("transporte del pre-análisis".into());
                    break;
                }
            }
        }
        writer.finish();
    });

    // 4) Decode + análisis en hilo de bloqueo (el decoder de rodio es un
    //    Iterator de muestras; jamás abre dispositivo).
    let track_id = track.id;
    let window = Duration::from_secs(secs as u64);
    // Runtime propio y efímero: el anillo SPSC es de un solo productor y
    // el de reproducción pertenece a RodioBackend.
    let runtime =
        crate::analysis::AnalysisRuntime::spawn(crate::analysis::AnalysisConfig::default());
    let tap = runtime.tap();
    let bus = runtime.bus();
    let reader = crate::infrastructure::playback::StreamReader::new(buffer);
    let blocking = tokio::task::spawn_blocking(move || {
        let profile = analyze_reader(reader, tap, bus, track_id, window, secs);
        // `runtime` vive aquí dentro: su Drop une el hilo de análisis.
        drop(runtime);
        profile
    });
    let deadline = Duration::from_secs(secs as u64 * 2 + 30);
    let profile = tokio::time::timeout(deadline, blocking).await.ok()?.ok()?;
    downloader.abort();
    profile
}

/// Consume un `Read + Seek` (stream HTTP o un WAV en memoria, en tests) y
/// produce el perfil acústico del tramo. Bloqueante: ir en hilo dedicado.
#[cfg(feature = "rodio")]
fn analyze_reader<R>(
    reader: R,
    tap: crate::analysis::PcmTap,
    bus: crate::analysis::FeatureBus,
    track_id: i64,
    window: Duration,
    secs: u32,
) -> Option<TrackAcousticProfile>
where
    R: std::io::Read + std::io::Seek + Send + Sync + 'static,
{
    use rodio::source::Source;

    let mut decoder = rodio::Decoder::new(reader).ok()?;
    let rate = decoder.sample_rate().get();
    let channels = decoder.channels().get() as u64;
    tap.announce(crate::analysis::StreamMeta {
        sample_rate: rate,
        channels: decoder.channels().get(),
    });
    tap.begin_source();

    let mut agg = AcousticAggregator::new(track_id, window);
    let mut last_ts: Option<Duration> = None;
    drain_bus(&bus, &mut last_ts, &mut agg);

    // Cap duro: un stream que nunca avanza no puede colgar el worker.
    let deadline = Instant::now() + Duration::from_secs(secs as u64 + 30);
    let limit = u64::from(rate) * channels * u64::from(secs);
    let mut fed: u64 = 0;
    let mut chunk: Vec<f32> = Vec::with_capacity(4096);
    for sample in decoder.by_ref().take(limit as usize) {
        chunk.push(sample);
        if chunk.len() < 4096 {
            continue;
        }
        fed += tap.feed(&chunk) as u64;
        chunk.clear();
        drain_bus(&bus, &mut last_ts, &mut agg);
        // El anillo SPSC retiene ~1.5 s (1<<17 f32 estéreo): si el feeder
        // se adelanta demasiado al hilo de análisis se descartarían
        // muestras (drop-newest). Cada chunk se frena si va muy por delante.
        while Instant::now() < deadline {
            let analyzed = bus
                .latest()
                .map(|f| analyzed_samples(&f, rate, channels))
                .unwrap_or(0);
            if fed.saturating_sub(analyzed) <= RING_CAPACITY * 3 / 4 {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
            drain_bus(&bus, &mut last_ts, &mut agg);
        }
        if Instant::now() >= deadline {
            break;
        }
    }

    // Drenaje final: el hilo de análisis publica el tramo restante con un
    // pequeño retardo; se espera hasta agotar la ventana o 2 s.
    let drain_deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < drain_deadline {
        drain_bus(&bus, &mut last_ts, &mut agg);
        if let Some(f) = bus.latest() {
            if f.timestamp + Duration::from_millis(100) >= window {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    agg.into_profile()
}

/// Capacidad del anillo del [`crate::analysis::AnalysisRuntime`]
/// (`SpScRing::new(1 << 17)` en `engine.rs`): se replica aquí para el
/// pacing del feeder.
#[cfg(feature = "rodio")]
const RING_CAPACITY: u64 = 1 << 17;

/// Muestras (interleaveadas) que el hilo de análisis ya procesó, según el
/// timestamp del último frame publicado.
#[cfg(feature = "rodio")]
fn analyzed_samples(f: &crate::analysis::AudioFeatures, rate: u32, channels: u64) -> u64 {
    (f.timestamp.as_secs_f64() * f64::from(rate) * channels as f64) as u64
}

/// Consume el bus (que solo retiene el ÚLTIMO frame) deduplicando por
/// timestamp: sin esto se perderían hops intermedios entre sondeos.
#[cfg(feature = "rodio")]
fn drain_bus(
    bus: &crate::analysis::FeatureBus,
    last_ts: &mut Option<Duration>,
    agg: &mut AcousticAggregator,
) {
    if let Some(f) = bus.latest() {
        if *last_ts != Some(f.timestamp) {
            *last_ts = Some(f.timestamp);
            agg.add(&f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::artist::Artist;
    use crate::domain::source::Source;

    fn track_with_id(id: i64) -> Track {
        let mut t = Track::new(
            format!("t{id}"),
            vec![Artist::new("a".to_string(), None, None, None)],
            Source::YouTube,
        );
        t.id = id;
        t
    }

    #[test]
    fn coverage_below_threshold_needs_preanalysis() {
        assert!(needs_preanalysis(0, 10));
        assert!(needs_preanalysis(2, 10), "20% < 30%");
        assert!(!needs_preanalysis(3, 10), "30% ya no pre-analiza");
        assert!(!needs_preanalysis(10, 10));
        assert!(!needs_preanalysis(0, 0), "lista vacía no pre-analiza");
    }

    #[test]
    fn missing_profile_candidates_keep_provider_order_and_cap() {
        let mut profiles = HashMap::new();
        profiles.insert(2, TrackAcousticProfile::default());
        let related: Vec<Track> = (1..=5).map(track_with_id).collect();
        let resolved: Vec<Option<i64>> = vec![None, Some(2), Some(3), None, Some(4)];

        let missing = candidates_missing_profile(&related, &resolved, &profiles, 2);
        assert_eq!(
            missing.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![1, 3],
            "respeta el orden del ranking y omite al que ya tiene perfil"
        );

        let all = candidates_missing_profile(&related, &resolved, &profiles, 10);
        assert_eq!(
            all.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![1, 3, 4, 5],
            "sin tope, todos los que faltan (solo el 2 tiene perfil)"
        );
    }
}

/// Decodificación offline del prefijo: un WAV PCM16 en memoria pasa por el
/// MISMO camino que el stream HTTP (decoder rodio + runtime de análisis).
#[cfg(all(test, feature = "rodio"))]
mod analyze_tests {
    use super::*;
    use std::io::Cursor;

    /// WAV PCM16 little-endian con un seno puro (sin device, sin red).
    fn wav_sine(freq: f32, rate: u32, channels: u16, secs: f32) -> Vec<u8> {
        let frames = (rate as f32 * secs) as usize;
        let mut samples = Vec::with_capacity(frames * channels as usize);
        for i in 0..frames {
            let v = (f32::sin(2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32)
                * 0.4
                * f32::from(i16::MAX)) as i16;
            for _ in 0..channels {
                samples.extend_from_slice(&v.to_le_bytes());
            }
        }
        let data_len = samples.len() as u32;
        let mut out = Vec::with_capacity(44 + samples.len());
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
        out.extend_from_slice(&(channels * 2).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        out.extend_from_slice(&samples);
        out
    }

    #[test]
    fn sine_wave_produces_a_profile_with_concentrated_chroma() {
        let runtime =
            crate::analysis::AnalysisRuntime::spawn(crate::analysis::AnalysisConfig::default());
        let wav = wav_sine(440.0, 44_100, 2, 4.0);
        let window = Duration::from_secs(4);

        let profile = analyze_reader(Cursor::new(wav), runtime.tap(), runtime.bus(), 7, window, 4)
            .expect("un seno de 4 s debe producir perfil");

        assert_eq!(profile.track_id, 7);
        assert!(profile.frame_count > 30, "sobran hops para el mínimo");
        assert!(profile.rms_mean > 0.0, "hay señal");
        let chroma_sum: f32 = profile.chroma_profile.iter().sum();
        assert!(
            (chroma_sum - 1.0).abs() < 0.01,
            "cromática normalizada: {chroma_sum}"
        );
        // 440 Hz = La (clase de pica 9 si Do=0): la energía debe concentrarse
        // en una sola clase, no repartirse.
        let max = profile
            .chroma_profile
            .iter()
            .cloned()
            .fold(0.0_f32, f32::max);
        assert!(max > 0.5, "seno puro ⇒ una clase domina: {max:?}");
    }
}
