//! Benchmark de la infraestructura DSP y sonda de asignaciones.
//!
//! Uso: `cargo run --release --example bench_dsp`
//!
//! Mide dos cosas que no deben degradarse:
//! - el coste de la cadena plana (sin procesadores) frente a un bloque en
//!   crudo, para conocer el precio de la infraestructura;
//! - que `ProcessorChain::process` no asigna memoria en el camino caliente.
//!
//! La sonda de asignaciones usa un allocator global que cuenta llamadas. Se
//! activa solo en este ejemplo; la biblioteca no lo ve.

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use rodio::source::Source;
use tunefold::dsp::{
    DspControl, DspParams, DspSource, EqParams, ProcessorChain, StreamFormat, DSP_BLOCK_FRAMES,
};

struct CountingAllocator;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        System.alloc(layout)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        System.alloc_zeroed(layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        System.realloc(ptr, layout, new_size)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

struct EndlessTone {
    index: u64,
    channels: std::num::NonZeroU16,
    rate: std::num::NonZeroU32,
}

impl EndlessTone {
    fn new() -> Self {
        Self {
            index: 0,
            channels: std::num::NonZeroU16::new(2).unwrap(),
            rate: std::num::NonZeroU32::new(44_100).unwrap(),
        }
    }
}

impl Iterator for EndlessTone {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let phase = (self.index % 128) as f32 / 128.0;
        self.index = self.index.wrapping_add(1);
        Some(phase * 0.5 - 0.25)
    }
}

impl Source for EndlessTone {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> std::num::NonZeroU16 {
        self.channels
    }

    fn sample_rate(&self) -> std::num::NonZeroU32 {
        self.rate
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

fn active_params() -> DspParams {
    let mut params = DspParams::flat();
    params.preamp_db = 3.0;
    params.eq = EqParams {
        enabled: true,
        ..EqParams::graphic_flat()
    };
    params.eq.bands[0].gain_db = 3.0;
    params.eq.bands[4].gain_db = 2.0;
    params.stereo.width = 1.2;
    params.normalization.mode = tunefold::dsp::NormalizationMode::Lufs;
    params.normalization.target_lufs = -14.0;
    params.normalization.source_lufs = -20.0;
    params.limiter.enabled = true;
    params.limiter.ceiling_db = -1.0;
    params
}

fn measure(label: &str, budget: std::time::Duration, mut op: impl FnMut()) {
    for _ in 0..200 {
        op();
    }
    const ITERS: u32 = 20_000;
    let start = Instant::now();
    for _ in 0..ITERS {
        op();
    }
    let per = start.elapsed() / ITERS;
    let ratio = budget.as_secs_f64() / per.as_secs_f64();
    println!("{label:<38} {per:>10.1?}   presupuesto {budget:>10.1?}   margen ×{ratio:>7.0}");
}

fn main() {
    let format = StreamFormat::new(44_100, 2);
    let block_budget =
        std::time::Duration::from_secs_f64(DSP_BLOCK_FRAMES as f64 / f64::from(format.sample_rate));
    let mut raw = vec![0.125f32; DSP_BLOCK_FRAMES * 2];

    println!(
        "== DSP · bloque de {} frames estéreo @ {} Hz (presupuesto {:.2} ms) ==",
        DSP_BLOCK_FRAMES,
        format.sample_rate,
        block_budget.as_secs_f64() * 1000.0
    );

    measure("bloque en crudo (referencia)", block_budget, || {
        for sample in &mut raw {
            *sample = black_box(*sample);
        }
    });

    let control = DspControl::new(DspParams::flat());
    let mut flat = ProcessorChain::new(control);
    flat.prepare(format);
    let mut flat_block = vec![0.125f32; DSP_BLOCK_FRAMES * 2];
    measure("cadena Flat (0 procesadores)", block_budget, || {
        flat.process(&mut flat_block);
    });

    let control = DspControl::new(active_params());
    let mut active = ProcessorChain::standard(control);
    active.prepare(format);
    let mut active_block = vec![0.125f32; DSP_BLOCK_FRAMES * 2];
    measure(
        "cadena estándar activa (5 procesadores)",
        block_budget,
        || {
            active.process(&mut active_block);
        },
    );

    println!("\n== Coste del adaptador por muestra (fuente estéreo sin fin) ==");
    let sample_budget = std::time::Duration::from_secs_f64(1.0 / 88_200.0);
    let mut raw_source = EndlessTone::new();
    measure("Iterator crudo (1 muestra)", sample_budget, || {
        black_box(raw_source.next());
    });
    let mut dsp_source = DspSource::new(EndlessTone::new(), DspControl::new(DspParams::flat()));
    measure("DspSource Flat (1 muestra)", sample_budget, || {
        black_box(dsp_source.next());
    });

    println!("\n== Asignaciones en el camino caliente ==");
    flat.process(&mut flat_block);
    active.process(&mut active_block);

    ALLOCATIONS.store(0, Ordering::Relaxed);
    for _ in 0..10_000 {
        flat.process(&mut flat_block);
        active.process(&mut active_block);
    }
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    println!(
        "20 000 bloques procesados (Flat + activo) → {} asignaciones",
        allocations
    );
    assert_eq!(
        allocations, 0,
        "ProcessorChain::process no debe asignar memoria en el audio"
    );
}
