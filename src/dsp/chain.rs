//! Cadena de procesadores con parámetros publicados por el hilo de control.
//!
//! La cadena se construye en el hilo de control (antes de arrancar la
//! reproducción), así que el vector de procesadores y sus buffers internos
//! quedan asignados de antemano. En el hilo de audio solo se hacen:
//!
//! 1. un `try_lock` no bloqueante para detectar una versión nueva;
//! 2. una copia `Copy` de [`DspParams`] (sin heap);
//! 3. la llamada a cada procesador con el mismo bloque in place.
//!
//! Si el perfil es plano (o el pipeline está apagado) no se llama a ningún
//! procesador: la ruta Flat es una salida temprana bit-exacta.

use super::{
    AudioProcessor, DspControl, DspParams, EqProcessor, GainProcessor, LimiterProcessor,
    NormalizationProcessor, StereoProcessor, StreamFormat,
};

/// Cadena DSP de una fuente de reproducción.
pub struct ProcessorChain {
    control: DspControl,
    applied: DspParams,
    applied_version: u64,
    format: Option<StreamFormat>,
    processors: Vec<Box<dyn AudioProcessor>>,
}

impl ProcessorChain {
    /// Cadena sin procesadores (ruta Flat de infraestructura / benchmarks).
    pub fn new(control: DspControl) -> Self {
        Self::with_processors(control, Vec::new())
    }

    /// Cadena estándar en orden de señal:
    /// preamp → EQ → estéreo → normalización → limitador.
    pub fn standard(control: DspControl) -> Self {
        Self::with_processors(
            control,
            vec![
                Box::new(GainProcessor::new()),
                Box::new(EqProcessor::new()),
                Box::new(StereoProcessor::new()),
                Box::new(NormalizationProcessor::new()),
                Box::new(LimiterProcessor::new()),
            ],
        )
    }

    /// Cadena con una composición fija de procesadores.
    ///
    /// Debe construirse en el hilo de control. La composición no cambia
    /// durante la reproducción; lo que cambia son los parámetros.
    pub fn with_processors(control: DspControl, processors: Vec<Box<dyn AudioProcessor>>) -> Self {
        let (applied_version, applied) = control.snapshot();
        Self {
            control,
            applied,
            applied_version,
            format: None,
            processors,
        }
    }

    /// Prepara los procesadores para el formato del stream.
    pub fn prepare(&mut self, format: StreamFormat) {
        self.format = Some(format);
        for processor in &mut self.processors {
            processor.prepare(format);
        }
    }

    /// Procesa un bloque intercalado in place.
    pub fn process(&mut self, samples: &mut [f32]) {
        if samples.is_empty() {
            return;
        }
        self.refresh_params();
        if self.applied.is_flat() || self.format.is_none() {
            return;
        }
        for processor in &mut self.processors {
            processor.process(samples, &self.applied);
        }
    }

    /// Limpia el estado de todos los procesadores (seek/discontinuidad).
    pub fn reset(&mut self) {
        for processor in &mut self.processors {
            processor.reset();
        }
    }

    /// Configuración efectivamente aplicada en el último bloque.
    pub fn applied_params(&self) -> DspParams {
        self.applied
    }

    /// Formato actualmente preparado.
    pub fn format(&self) -> Option<StreamFormat> {
        self.format
    }

    /// Nombres de los procesadores en orden de ejecución (diagnóstico).
    pub fn processor_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.processors.iter().map(|processor| processor.name())
    }

    fn refresh_params(&mut self) {
        if let Some((version, params)) = self.control.try_snapshot() {
            if version != self.applied_version {
                self.applied = params;
                self.applied_version = version;
            }
        }
    }
}

impl std::fmt::Debug for ProcessorChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessorChain")
            .field("applied_version", &self.applied_version)
            .field("format", &self.format)
            .field("processors", &self.processor_names().collect::<Vec<_>>())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::params::EqParams;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn active_params() -> DspParams {
        let mut params = DspParams::flat();
        params.eq = EqParams {
            enabled: true,
            ..EqParams::graphic_flat()
        };
        params.eq.bands[0].gain_db = 3.0;
        params
    }

    struct Counter {
        calls: Arc<AtomicUsize>,
    }

    impl AudioProcessor for Counter {
        fn name(&self) -> &'static str {
            "counter"
        }

        fn prepare(&mut self, _format: StreamFormat) {}

        fn process(&mut self, _samples: &mut [f32], _params: &DspParams) {
            self.calls.fetch_add(1, Ordering::Relaxed);
        }

        fn reset(&mut self) {}
    }

    struct Accumulator {
        last: f32,
        resets: Arc<AtomicUsize>,
    }

    impl AudioProcessor for Accumulator {
        fn name(&self) -> &'static str {
            "accumulator"
        }

        fn prepare(&mut self, _format: StreamFormat) {}

        fn process(&mut self, samples: &mut [f32], _params: &DspParams) {
            for sample in samples {
                let input = *sample;
                *sample += self.last;
                self.last = input;
            }
        }

        fn reset(&mut self) {
            self.last = 0.0;
            self.resets.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn flat_profile_never_calls_processors() {
        let calls = Arc::new(AtomicUsize::new(0));
        let control = DspControl::new(DspParams::flat());
        let mut chain = ProcessorChain::with_processors(
            control,
            vec![Box::new(Counter {
                calls: calls.clone(),
            })],
        );
        chain.prepare(StreamFormat::new(44_100, 2));
        let mut block = [0.25f32; 128];
        chain.process(&mut block);
        assert_eq!(calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn bypassed_pipeline_never_calls_processors() {
        let calls = Arc::new(AtomicUsize::new(0));
        let control = DspControl::new(DspParams::bypassed());
        let mut chain = ProcessorChain::with_processors(
            control,
            vec![Box::new(Counter {
                calls: calls.clone(),
            })],
        );
        chain.prepare(StreamFormat::new(44_100, 2));
        let mut block = [0.25f32; 128];
        chain.process(&mut block);
        assert_eq!(calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn active_profile_calls_processors_once_per_block() {
        let calls = Arc::new(AtomicUsize::new(0));
        let control = DspControl::new(active_params());
        let mut chain = ProcessorChain::with_processors(
            control,
            vec![Box::new(Counter {
                calls: calls.clone(),
            })],
        );
        chain.prepare(StreamFormat::new(44_100, 2));
        let mut block = [0.25f32; 128];
        chain.process(&mut block);
        chain.process(&mut block);
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn a_new_publication_is_applied_between_blocks() {
        let calls = Arc::new(AtomicUsize::new(0));
        let control = DspControl::new(DspParams::flat());
        let mut chain = ProcessorChain::with_processors(
            control.clone(),
            vec![Box::new(Counter {
                calls: calls.clone(),
            })],
        );
        chain.prepare(StreamFormat::new(44_100, 2));
        let mut block = [0.25f32; 128];
        chain.process(&mut block);
        assert_eq!(calls.load(Ordering::Relaxed), 0);

        control.set(active_params());
        chain.process(&mut block);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(chain.applied_params(), active_params().sanitized());
    }

    #[test]
    fn processor_state_survives_block_boundaries() {
        let control = DspControl::new(active_params());
        let mut chain = ProcessorChain::with_processors(
            control,
            vec![Box::new(Accumulator {
                last: 0.0,
                resets: Arc::new(AtomicUsize::new(0)),
            })],
        );
        chain.prepare(StreamFormat::new(44_100, 1));
        let mut first = [1.0, 2.0];
        let mut second = [3.0, 4.0];
        chain.process(&mut first);
        chain.process(&mut second);
        assert_eq!(first, [1.0, 3.0]);
        assert_eq!(second, [5.0, 7.0]);
    }

    #[test]
    fn reset_clears_processor_state() {
        let resets = Arc::new(AtomicUsize::new(0));
        let control = DspControl::new(active_params());
        let mut chain = ProcessorChain::with_processors(
            control,
            vec![Box::new(Accumulator {
                last: 0.0,
                resets: resets.clone(),
            })],
        );
        chain.prepare(StreamFormat::new(44_100, 1));
        let mut first = [1.0, 2.0];
        chain.process(&mut first);
        chain.reset();
        assert_eq!(resets.load(Ordering::Relaxed), 1);

        let mut after_seek = [3.0, 4.0];
        chain.process(&mut after_seek);
        assert_eq!(after_seek, [3.0, 7.0]);
    }

    #[test]
    fn standard_chain_has_the_canonical_signal_order() {
        let chain = ProcessorChain::standard(DspControl::new(DspParams::flat()));
        assert_eq!(
            chain.processor_names().collect::<Vec<_>>(),
            ["preamp", "eq", "stereo", "normalization", "limiter"]
        );
    }

    #[test]
    fn standard_chain_flat_is_bit_exact() {
        let control = DspControl::new(DspParams::flat());
        let mut chain = ProcessorChain::standard(control);
        chain.prepare(StreamFormat::new(44_100, 2));
        let original = [0.1f32, -0.25, 0.5, -0.75, 0.0, 1.0, -1.0, 0.125];
        let mut block = original;
        chain.process(&mut block);
        assert_eq!(block, original);
    }

    #[test]
    fn no_processor_composition_is_a_true_bypass() {
        let control = DspControl::new(active_params());
        let mut chain = ProcessorChain::new(control);
        chain.prepare(StreamFormat::new(44_100, 2));
        let original = [0.1f32, -0.25, 0.5, -0.75];
        let mut block = original;
        chain.process(&mut block);
        assert_eq!(block, original);
    }
}
