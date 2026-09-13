//! Banco de pruebas de los caminos calientes (Fase 8, spec §34/§42: MEDIR
//! primero). Sin dependencias externas: `Instant` + `std::hint::black_box`.
//!
//! Uso: `cargo run --release --example bench_hotpaths`
//!
//! Presupuestos de referencia (hop 512 @44.1 kHz):
//! - un hop de análisis debe costar << 11.6 ms (tiempo real de un hop);
//! - un frame visual completo debe costar << 66 ms (tick de ~15 Hz).

use std::hint::black_box;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tunefold::analysis::{
    bands::band_ratios,
    beat::BpmEstimator,
    features::AudioFeatures,
    fft::SpectrumAnalyzer,
    onset::{FluxAnalyzer, OnsetDetector},
    ring::SpScRing,
    smoother::FeatureSmoother,
    AnalysisConfig, AnalysisRuntime, StreamMeta, WaveformEnvelope,
};
use tunefold::domain::source::Source;
use tunefold::domain::track::Track;
use tunefold::visualization::{ParameterMapper, VisualEngine};

const SR: f32 = 44_100.0;
const FFT: usize = 2048;

fn sine_frame(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| {
            let t = i as f32 / SR;
            0.6 * (2.0 * std::f32::consts::PI * 220.0 * t).sin()
                + 0.2 * (2.0 * std::f32::consts::PI * 3000.0 * t).sin()
        })
        .collect()
}

fn bench<F: FnMut()>(name: &str, budget: Duration, mut f: F) {
    // Calentamiento de cachés/planificador.
    for _ in 0..50 {
        f();
    }
    const ITERS: u32 = 2000;
    let start = Instant::now();
    for _ in 0..ITERS {
        f();
    }
    let per = start.elapsed() / ITERS;
    let ratio = budget.as_secs_f64() / per.as_secs_f64();
    println!("{name:<34} {per:>10.1?}   presupuesto {budget:>10.1?}   margen ×{ratio:>7.0}");
}

fn main() {
    println!("== Camino DSP (presupuesto = tiempo real de un hop) ==");
    let hop_budget = Duration::from_secs_f64(512.0 / SR as f64);
    let frame = sine_frame(FFT);

    let mut analyzer = SpectrumAnalyzer::new(FFT);
    bench("fft+magnitudes (alloc actual)", hop_budget, || {
        black_box(analyzer.magnitudes(black_box(&frame)));
    });

    let mags = analyzer.magnitudes(&frame);
    let edges = Default::default();
    bench("bandas+ratios", hop_budget, || {
        black_box(band_ratios(black_box(&mags), SR, &edges));
    });

    let mut flux = FluxAnalyzer::new();
    flux.flux(&mags); // cebar prev
    bench("flujo espectral", hop_budget, || {
        black_box(flux.flux(black_box(&mags)));
    });

    let mut onset = OnsetDetector::new(43, 0.005);
    for _ in 0..43 {
        onset.observe(0.002);
    }
    bench("detector de onset", hop_budget, || {
        black_box(onset.observe(black_box(0.02)));
    });

    let mut bpm = BpmEstimator::new(86.13);
    for i in 0..300 {
        bpm.observe(if i % 43 == 0 { 0.8 } else { 0.0 });
    }
    bench("BPM observe", hop_budget, || {
        black_box(bpm.observe(black_box(0.01)));
    });

    let mut sm = FeatureSmoother::new(12.0, 4.0);
    bench("smoother step", hop_budget, || {
        black_box(sm.step(black_box(&[0.4; 9]), 512.0 / SR));
    });

    // Pipeline completo por hop (lo que hace el hilo de análisis).
    let meta = StreamMeta {
        sample_rate: 44_100,
        channels: 2,
    };
    let mut flux2 = FluxAnalyzer::new();
    bench("analyze_frame completo", hop_budget, || {
        let m = analyzer.magnitudes(&frame);
        let b = band_ratios(&m, SR, &edges);
        let fl = flux2.flux(&m);
        black_box((b.bass, fl));
        let _ = meta;
    });

    println!("\n== Camino visual (presupuesto = tick de ~15 Hz) ==");
    let vis_budget = Duration::from_millis(66);

    let feats = Arc::new(AudioFeatures {
        timestamp: Duration::from_secs(1),
        rms: 0.3,
        amplitude: 0.5,
        bass: 0.7,
        low_mid: 0.4,
        mid: 0.25,
        high_mid: 0.15,
        high: 0.1,
        spectral_centroid: 0.35,
        spectral_flux: 0.2,
        onset: 0.1,
        beat: false,
        beat_confidence: 0.7,
        bpm: 120.0,
    });
    let mapper = ParameterMapper::default();
    bench("parameter mapper", vis_budget, || {
        black_box(mapper.map(black_box(&feats)));
    });

    let mut engine = VisualEngine::new(ParameterMapper::default());
    let mut pos_ms = 0u64;
    let envelope = Arc::new(WaveformEnvelope::from_window(&[0.5; 2048]));
    let palette = tunefold::visualization::VisualPalette::from_cover(Some([
        [220, 60, 120],
        [60, 160, 240],
        [240, 180, 90],
    ]));
    bench("visual engine update", vis_budget, || {
        pos_ms += 66;
        black_box(engine.update(
            Some(&feats),
            Some(&envelope),
            Duration::from_millis(pos_ms),
            &palette,
        ));
    });

    let state = engine.update(
        Some(&feats),
        Some(&envelope),
        Duration::from_secs(3),
        &palette,
    );
    bench("render TUI completo (80×5)", vis_budget, || {
        let backend = ratatui::backend::TestBackend::new(80, 5);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| {
            tunefold::visualization::render::render(f, f.area(), black_box(&state), 42.0)
        })
        .unwrap();
    });

    println!("\n== Ring SPSC (throughput) ==");
    let ring = SpScRing::new(1 << 17);
    let chunk = vec![0.5f32; 1024];
    let start = Instant::now();
    let rounds = 400; // 400*1024 muestras ≈ 409k
    for _ in 0..rounds {
        ring.push(&chunk);
        let mut out = [0.0f32; 1024];
        loop {
            let n = ring.pop(&mut out);
            if n == out.len() {
                break;
            }
        }
    }
    let elapsed = start.elapsed();
    let samples = (rounds * 1024) as f64;
    println!(
        "push+pop {:>9.1} muestras/s ({:.1}× tiempo real estéreo 48k)",
        samples / elapsed.as_secs_f64(),
        samples / elapsed.as_secs_f64() / (48_000.0 * 2.0),
    );

    println!("\n== Hilo de análisis end-to-end (features/s sostenidas) ==");
    measure_engine_thread();
}

fn measure_engine_thread() {
    let runtime = AnalysisRuntime::spawn(AnalysisConfig::default());
    let tap = runtime.tap();
    tap.announce(StreamMeta {
        sample_rate: 44_100,
        channels: 2,
    });
    let bus = runtime.bus();

    // Alimenta ~3 s de audio a ritmo más rápido que real y cuenta publishes.
    let data = sine_frame(8192);
    let start = Instant::now();
    let total_samples = 44_100 * 3 * 2;
    let mut fed = 0usize;
    let mut idx = 0usize;
    while fed < total_samples {
        let n = (data.len()).min(total_samples - fed);
        let mut batch = Vec::with_capacity(n);
        for _ in 0..n {
            batch.push(data[idx % data.len()]);
            idx += 1;
        }
        tap.feed(&batch);
        fed += batch.len();
        std::thread::sleep(Duration::from_micros(200)); // empuja ~4× tiempo real
    }
    std::thread::sleep(Duration::from_millis(120));
    let latest = bus.latest().expect("hay features");
    let secs = latest.timestamp.as_secs_f64().max(1e-6);
    let wall = start.elapsed().as_secs_f64();
    println!(
        "stream analizado {:.2}s en {:.2}s de pared → {:.2}× tiempo real; último timestamp {:.3}s",
        secs,
        wall,
        secs / wall,
        latest.timestamp.as_secs_f64()
    );
}

// Referencia viva para el lector del bench (evita import muerto si evoluciona).
#[allow(dead_code)]
fn _touch_track(t: Track) -> Source {
    t.source
}
