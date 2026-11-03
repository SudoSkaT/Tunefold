//! Adaptador entre una fuente de rodio y la cadena DSP.
//!
//! rodio consume muestras de una en una, pero un filtro biquad o un limitador
//! son mucho más eficientes por bloque. `DspSource` preasigna un bloque de
//! [`DSP_BLOCK_FRAMES`] frames, lo llena desde la fuente interior, lo procesa
//! in place y va entregando las muestras ya procesadas.
//!
//! La ruta Flat no modifica ninguna muestra: cuando [`DspParams::is_flat`] es
//! cierto, la cadena sale antes de tocar el bloque.
//!
//! Elegimos `current_span_len() == None`: al leer por delante no podemos
//! informar con exactitud cuántas muestras quedan en el span interior. El
//! decodificador de Tunefold mantiene un formato constante durante la pista,
//! así que ocultar los cortes de span es correcto y evita que rodio reconstruya
//! conversores a mitad de stream. Un cambio real de formato se detecta al
//! empezar el siguiente bloque y reinicia los procesadores.

use std::num::{NonZeroU16, NonZeroU32};
use std::time::Duration;

use rodio::source::{SeekError, Source};

use super::{DspControl, ProcessorChain, StreamFormat, DSP_BLOCK_FRAMES, DSP_MAX_CHANNELS};

/// Fuente que aplica la cadena DSP a otra fuente de rodio.
pub struct DspSource<S> {
    inner: S,
    chain: ProcessorChain,
    block: Vec<f32>,
    cursor: usize,
    len: usize,
    channels: usize,
    sample_rate: u32,
    passthrough: bool,
}

impl<S> DspSource<S>
where
    S: Source<Item = f32>,
{
    /// Envuelve `inner` con la cadena estándar que lee parámetros de `control`.
    pub fn new(inner: S, control: DspControl) -> Self {
        Self::with_chain(inner, ProcessorChain::standard(control))
    }

    pub(crate) fn with_chain(inner: S, mut chain: ProcessorChain) -> Self {
        let channels = inner.channels().get() as usize;
        let sample_rate = inner.sample_rate().get();
        let passthrough = channels > DSP_MAX_CHANNELS;
        // El bloque se reserva siempre en el hilo de control. Aunque el formato
        // inicial sea >8 canales, un cambio posterior a un formato soportado no
        // puede asignar memoria en el hilo de audio.
        let block = vec![0.0; DSP_BLOCK_FRAMES * DSP_MAX_CHANNELS];
        if !passthrough {
            chain.prepare(StreamFormat::new(sample_rate, channels as u16));
        }
        Self {
            inner,
            chain,
            block,
            cursor: 0,
            len: 0,
            channels,
            sample_rate,
            passthrough,
        }
    }

    fn refill(&mut self) -> bool {
        self.cursor = 0;
        self.len = 0;

        let channels = self.inner.channels().get() as usize;
        let sample_rate = self.inner.sample_rate().get();
        if channels != self.channels || sample_rate != self.sample_rate {
            self.channels = channels;
            self.sample_rate = sample_rate;
            // No se asigna memoria en el hilo de audio: por encima del máximo
            // se deja de procesar. Al volver a un formato soportado hay que
            // preparar de nuevo, aunque antes se hubiera procesado otro.
            self.passthrough = channels > DSP_MAX_CHANNELS;
            if !self.passthrough {
                self.chain
                    .prepare(StreamFormat::new(sample_rate, channels as u16));
                self.chain.reset();
            }
        }
        if self.passthrough {
            return false;
        }

        let target = DSP_BLOCK_FRAMES * channels;
        while self.len < target {
            match self.inner.next() {
                Some(sample) => {
                    self.block[self.len] = sample;
                    self.len += 1;
                }
                None => break,
            }
        }
        if self.len == 0 {
            return false;
        }
        self.chain.process(&mut self.block[..self.len]);
        true
    }
}

impl<S> Iterator for DspSource<S>
where
    S: Source<Item = f32>,
{
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.cursor >= self.len {
            if self.passthrough {
                return self.inner.next();
            }
            if !self.refill() {
                if self.passthrough {
                    return self.inner.next();
                }
                return None;
            }
        }
        let sample = self.block[self.cursor];
        self.cursor += 1;
        Some(sample)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, None)
    }
}

impl<S> Source for DspSource<S>
where
    S: Source<Item = f32>,
{
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> NonZeroU16 {
        self.inner.channels()
    }

    fn sample_rate(&self) -> NonZeroU32 {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.inner.try_seek(pos)?;
        self.cursor = 0;
        self.len = 0;
        self.chain.reset();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::params::EqParams;
    use crate::dsp::{AudioProcessor, DspParams};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct VecSource {
        samples: Vec<f32>,
        index: usize,
        rate: NonZeroU32,
        channels: NonZeroU16,
    }

    impl VecSource {
        fn stereo(data: Vec<f32>) -> Self {
            Self {
                samples: data,
                index: 0,
                rate: NonZeroU32::new(44_100).unwrap(),
                channels: NonZeroU16::new(2).unwrap(),
            }
        }

        fn mono(data: Vec<f32>) -> Self {
            Self {
                samples: data,
                index: 0,
                rate: NonZeroU32::new(1_000).unwrap(),
                channels: NonZeroU16::new(1).unwrap(),
            }
        }

        fn with_channels(data: Vec<f32>, channels: u16) -> Self {
            Self {
                samples: data,
                index: 0,
                rate: NonZeroU32::new(44_100).unwrap(),
                channels: NonZeroU16::new(channels).unwrap(),
            }
        }
    }

    impl Iterator for VecSource {
        type Item = f32;

        fn next(&mut self) -> Option<f32> {
            let sample = self.samples.get(self.index).copied()?;
            self.index += 1;
            Some(sample)
        }
    }

    impl Source for VecSource {
        fn current_span_len(&self) -> Option<usize> {
            None
        }

        fn channels(&self) -> NonZeroU16 {
            self.channels
        }

        fn sample_rate(&self) -> NonZeroU32 {
            self.rate
        }

        fn total_duration(&self) -> Option<Duration> {
            Some(Duration::from_millis(100))
        }

        fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
            let frames = (pos.as_secs_f64() * self.rate.get() as f64) as usize;
            self.index = (frames * self.channels.get() as usize).min(self.samples.len());
            Ok(())
        }
    }

    fn active_params() -> DspParams {
        let mut params = DspParams::flat();
        params.eq = EqParams {
            enabled: true,
            ..EqParams::graphic_flat()
        };
        params.eq.bands[0].gain_db = 3.0;
        params
    }

    struct Double {
        calls: Arc<AtomicUsize>,
    }

    impl AudioProcessor for Double {
        fn name(&self) -> &'static str {
            "double"
        }

        fn prepare(&mut self, _format: StreamFormat) {}

        fn process(&mut self, samples: &mut [f32], _params: &DspParams) {
            self.calls.fetch_add(1, Ordering::Relaxed);
            for sample in samples {
                *sample *= 2.0;
            }
        }

        fn reset(&mut self) {}
    }

    struct Accumulator {
        last: f32,
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
        }
    }

    #[test]
    fn flat_path_is_bit_exact_across_block_boundaries() {
        let data: Vec<f32> = (0..(DSP_BLOCK_FRAMES * 2 * 3 + 7))
            .map(|i| ((i as f32) * 0.017).sin())
            .collect();
        let source = DspSource::new(
            VecSource::stereo(data.clone()),
            DspControl::new(DspParams::flat()),
        );
        let decoded: Vec<f32> = source.collect();
        assert_eq!(decoded, data);
    }

    #[test]
    fn active_chain_processes_every_block_in_place() {
        let calls = Arc::new(AtomicUsize::new(0));
        let chain = ProcessorChain::with_processors(
            DspControl::new(active_params()),
            vec![Box::new(Double {
                calls: calls.clone(),
            })],
        );
        let data = vec![0.25f32; DSP_BLOCK_FRAMES * 2 + 2];
        let source = DspSource::with_chain(VecSource::stereo(data), chain);
        let decoded: Vec<f32> = source.collect();
        assert!(decoded.iter().all(|sample| *sample == 0.5));
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn standard_chain_processes_a_non_flat_profile() {
        let mut params = DspParams::flat();
        params.preamp_db = 6.020_6;
        let data = vec![0.25f32; DSP_BLOCK_FRAMES * 2 * 200];
        let source = DspSource::new(VecSource::stereo(data), DspControl::new(params));
        let decoded: Vec<f32> = source.collect();
        assert!((decoded[decoded.len() - 1] - 0.5).abs() < 1e-3);
    }

    #[test]
    fn short_streams_and_empty_streams_keep_their_length() {
        let short = DspSource::new(
            VecSource::stereo(vec![0.1, -0.2, 0.3]),
            DspControl::new(DspParams::flat()),
        );
        assert_eq!(short.collect::<Vec<_>>(), vec![0.1, -0.2, 0.3]);

        let empty = DspSource::new(
            VecSource::stereo(Vec::new()),
            DspControl::new(DspParams::flat()),
        );
        assert_eq!(empty.count(), 0);
    }

    #[test]
    fn more_than_eight_channels_bypass_and_keep_samples_intact() {
        let data: Vec<f32> = (0..100).map(|i| i as f32 * 0.01).collect();
        let source = DspSource::new(
            VecSource::with_channels(data.clone(), 9),
            DspControl::new(active_params()),
        );
        assert!(source.passthrough, "más de 8 canales no se procesan");
        assert_eq!(source.collect::<Vec<_>>(), data);
    }

    #[test]
    fn metadata_and_duration_are_forwarded() {
        let source = DspSource::new(
            VecSource::stereo(vec![0.0, 0.0]),
            DspControl::new(DspParams::flat()),
        );
        assert_eq!(source.channels().get(), 2);
        assert_eq!(source.sample_rate().get(), 44_100);
        assert_eq!(source.total_duration(), Some(Duration::from_millis(100)));
    }

    #[test]
    fn seek_clears_pending_samples_and_processor_state() {
        let chain = ProcessorChain::with_processors(
            DspControl::new(active_params()),
            vec![Box::new(Accumulator { last: 0.0 })],
        );
        let data: Vec<f32> = (1..=20).map(|v| v as f32).collect();
        let mut source = DspSource::with_chain(VecSource::mono(data), chain);
        assert_eq!(source.next(), Some(1.0));
        assert_eq!(source.next(), Some(3.0));

        source.try_seek(Duration::ZERO).unwrap();
        assert_eq!(source.next(), Some(1.0), "el estado del filtro se reinició");
        assert_eq!(source.next(), Some(3.0));
    }

    #[test]
    fn parameter_changes_take_effect_without_restarting_the_source() {
        let control = DspControl::new(DspParams::flat());
        let chain = ProcessorChain::with_processors(
            control.clone(),
            vec![Box::new(Double {
                calls: Arc::new(AtomicUsize::new(0)),
            })],
        );
        let mut source = DspSource::with_chain(
            VecSource::stereo(vec![0.25f32; DSP_BLOCK_FRAMES * 2 * 2]),
            chain,
        );
        for _ in 0..DSP_BLOCK_FRAMES * 2 {
            assert_eq!(source.next(), Some(0.25));
        }

        control.set(active_params());
        for _ in 0..DSP_BLOCK_FRAMES * 2 {
            assert_eq!(source.next(), Some(0.5));
        }
    }
}
