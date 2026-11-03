//! Backends de reproducción de audio (Infraestructura).
//!
//! - [`output`]: salida de audio local compartida (rodio/cpal). Desktop only.
//! - [`rodio_backend`]: streaming HTTP decodificado con symphonia y emitido por
//!   el dispositivo de audio local. Desktop only.
//!
//! Ninguno de estos tipos se exporta fuera de esta capa; el enrutador de la
//! capa de Aplicación solo conoce el trait.

#[cfg(feature = "rodio")]
mod output;
#[cfg(feature = "rodio")]
mod rodio_backend;

use std::sync::Arc;

use crate::app::audio::{EventBus, PlaybackEngine};
use crate::app::playback::RouterConfig;
use crate::domain::source::Source;
use crate::dsp::DspControl;
use crate::infrastructure::config::Config;

#[cfg(feature = "rodio")]
pub use output::SharedOutput;
#[cfg(feature = "rodio")]
pub use rodio_backend::RodioBackend;
/// Buffer bloqueante `Read + Seek` sobre un stream HTTP con Range: lo
/// comparten la reproducción (rodio) y el pre-análisis acústico de
/// recomendaciones (`ui::preanalysis`), que decodifica sin dispositivo.
#[cfg(feature = "rodio")]
pub(crate) use rodio_backend::{StreamBuffer, StreamReader};

/// Construye los motores a partir de la configuración. `http` es el cliente
/// HTTP compartido para resolver/descargar streams. Todos los motores notifican
/// al `bus` agrupado. Devuelve también los buses del análisis (features y
/// envolvente de forma de onda), ambos `None` si el flag de análisis está OFF.
pub fn build_engines(
    config: &Config,
    bus: EventBus,
    http: reqwest::Client,
) -> (
    RouterConfig,
    Option<crate::analysis::FeatureBus>,
    Option<crate::analysis::WaveformBus>,
    DspControl,
) {
    let mut engines: Vec<Arc<dyn PlaybackEngine>> = Vec::new();
    let dsp = DspControl::new(config.dsp);

    // Análisis de audio opcional: un runtime por construcción de motores; su
    // Drop detiene el hilo al reconstruirse (ajustes). Los buses viajan hacia
    // fuera para los consumidores (visualización, métricas).
    let analysis = config.flags.audio_analysis.then(|| {
        crate::analysis::AnalysisRuntime::spawn(crate::analysis::AnalysisConfig::default())
    });
    let features = analysis.as_ref().map(|rt| rt.bus());
    let waveform = analysis.as_ref().map(|rt| rt.waveform_bus());

    #[cfg(feature = "rodio")]
    {
        let output =
            Arc::new(SharedOutput::try_new(bus.clone()).expect("dispositivo de audio disponible"));

        engines.push(Arc::new(RodioBackend::new(
            http.clone(),
            output.clone(),
            bus.clone(),
            !config.flags.proxy,
            analysis,
            dsp.clone(),
        )));
    }

    let policy = config.playback_policy.clone();
    (RouterConfig { engines, policy }, features, waveform, dsp)
}

/// Fuentes que reproducen stream HTTP directo. Todos los orígenes actuales lo son.
pub(crate) fn is_http_source(source: Source) -> bool {
    matches!(source, Source::YouTube)
}
