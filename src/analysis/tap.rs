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

#[cfg(feature = "rodio")]
use super::engine::PcmTap;
#[cfg(feature = "rodio")]
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
    /// Envuelve `inner` anunciando su formato y declarando una fuente nueva.
    ///
    /// El anuncio solo resetea el análisis si cambia el formato; `begin_source`
    /// garantiza el reset aunque dos pistas consecutivas compartan formato.
    pub fn new(inner: S, tap: PcmTap) -> Self {
        tap.announce(StreamMeta {
            sample_rate: inner.sample_rate().get(),
            channels: inner.channels().get(),
        });
        tap.begin_source();
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

    fn try_seek(&mut self, pos: std::time::Duration) -> Result<(), rodio::source::SeekError> {
        self.inner.try_seek(pos)?;
        // Las features describen la posición nueva desde cero; sin este reset
        // el reloj de análisis seguiría sumando hops de la posición anterior.
        self.tap.begin_source();
        Ok(())
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
        fn try_seek(&mut self, _pos: std::time::Duration) -> Result<(), rodio::source::SeekError> {
            Ok(())
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

    #[test]
    fn tap_forwards_seek_to_the_decoder() {
        let runtime = AnalysisRuntime::spawn(crate::analysis::AnalysisConfig::default());
        let source = VecSource {
            samples: Vec::new().into_iter(),
            rate: 44_100.try_into().unwrap(),
            channels: 2.try_into().unwrap(),
        };
        let mut tapped = TapSource::new(source, runtime.tap());
        assert!(tapped.try_seek(std::time::Duration::from_secs(3)).is_ok());
    }
}
