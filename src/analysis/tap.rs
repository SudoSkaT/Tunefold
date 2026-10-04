//! Tap de PCM para el motor de análisis.
//!
//! En desktop (feature `rodio`), envuelve la fuente decodificada de rodio y
//! copia cada muestra al anillo del análisis. En Android, el adaptador Oboe
//! llama directamente a `PcmTap::feed()` desde el callback de audio.
//!
//! Coste en el hilo de audio: un `fetch_add`+store por muestra (44-48k/s) más
//! la copia — despreciable frente a la decodificación. NUNCA bloquea: si el
//! anillo está lleno, el análisis pierde muestras (drop-newest) pero el audio
//! sigue intacto.

use super::engine::PcmTap;
use super::engine::StreamMeta;

/// Fuente que reenvía las muestras de `S` y las alimenta al [`PcmTap`].
#[cfg(feature = "rodio")]
pub struct TapSource<S> {
    inner: S,
    tap: PcmTap,
}

#[cfg(feature = "rodio")]
impl<S> TapSource<S>
where
    S: rodio::source::Source<Item = f32>,
{
    /// Envuelve `inner` anunciando su formato al motor de análisis.
    pub fn new(inner: S, tap: PcmTap) -> Self {
        tap.announce(StreamMeta {
            sample_rate: inner.sample_rate().get(),
            channels: inner.channels().get(),
        });
        Self { inner, tap }
    }

    /// Acceso a la fuente interna (tests).
    #[cfg(test)]
    pub fn inner(&self) -> &S {
        &self.inner
    }
}

#[cfg(feature = "rodio")]
impl<S> Iterator for TapSource<S>
where
    S: rodio::source::Source<Item = f32>,
{
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let sample = self.inner.next()?;
        self.tap.feed(&[sample]);
        Some(sample)
    }
}

#[cfg(feature = "rodio")]
impl<S> rodio::source::Source for TapSource<S>
where
    S: rodio::source::Source<Item = f32>,
{
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> std::num::NonZeroU16 {
        self.inner.channels()
    }

    fn sample_rate(&self) -> std::num::NonZeroU32 {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

#[cfg(all(test, feature = "rodio"))]
mod tests {
    use super::*;
    use crate::analysis::engine::AnalysisRuntime;
    use crate::analysis::test_support::sine;
    use rodio::source::Source;

    struct VecSource {
        samples: std::vec::IntoIter<f32>,
        rate: std::num::NonZeroU32,
        channels: std::num::NonZeroU16,
    }
    impl Iterator for VecSource {
        type Item = f32;
        fn next(&mut self) -> Option<f32> {
            self.samples.next()
        }
    }
    impl Source for VecSource {
        fn current_span_len(&self) -> Option<usize> {
            None
        }
        fn total_duration(&self) -> Option<std::time::Duration> {
            None
        }
        fn channels(&self) -> std::num::NonZeroU16 {
            self.channels
        }
        fn sample_rate(&self) -> std::num::NonZeroU32 {
            self.rate
        }
    }

    #[tokio::test]
    async fn tap_forwards_every_sample_and_announces_format() {
        let runtime = AnalysisRuntime::spawn(crate::analysis::AnalysisConfig::default());
        let data = sine(440.0, 44_100.0, 4096, 0.5);
        let source = VecSource {
            samples: data.clone().into_iter(),
            rate: 44_100.try_into().unwrap(),
            channels: 2.try_into().unwrap(),
        };

        let tapped = TapSource::new(source, runtime.tap());
        assert_eq!(tapped.channels().get(), 2);
        assert_eq!(tapped.sample_rate().get(), 44_100);

        let forwarded: Vec<f32> = tapped.collect();
        assert_eq!(forwarded.len(), data.len());
        assert_eq!(forwarded[..100], data[..100], "muestra a muestra intacta");
    }
}
