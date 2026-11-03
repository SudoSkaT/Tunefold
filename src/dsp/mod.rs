//! Infraestructura DSP del motor de audio.
//!
//! La cadena de reproducción es:
//!
//! ```text
//! decoder (Symphonia/rodio)
//!   └─ TapSource (análisis PRE-DSP, sin cambios)
//!        └─ DspSource (bloques de 256 frames)
//!             └─ ProcessorChain: gain → EQ → estéreo → normalización → limitador
//!                  └─ rodio Player → dispositivo
//! ```
//!
//! El análisis se mantiene **antes** del DSP a propósito: las features
//! acústicas alimentan recomendaciones y visualización, y no deben cambiar
//! porque el usuario mueva un ecualizador. La señal que suena es la que sale
//! de la cadena; la que se analiza es la del decodificador.
//!
//! # Tiempo real
//!
//! - La composición de procesadores y sus buffers se crean en el hilo de
//!   control, antes de `play`.
//! - El hilo de audio solo copia parámetros `Copy` de tamaño fijo y procesa el
//!   bloque in place. No hay asignaciones, I/O ni esperas.
//! - La lectura de parámetros usa `try_lock`: si el control está publicando,
//!   se conserva la configuración anterior hasta el bloque siguiente.
//! - Un perfil plano sale antes de llamar a ningún procesador, de modo que la
//!   ruta Flat es bit-exacta.
//!
//! La Fase 2 incorpora los cinco procesadores sobre esta misma infraestructura,
//! sin tocar el decodificador. La UI y la persistencia llegan en la Fase 3; la
//! integración Android sigue pendiente de un contrato JNI y validación en
//! hardware.

mod biquad;
mod chain;
mod control;
mod gain;
mod limiter;
mod normalize;
mod params;
mod processor;
#[cfg(feature = "rodio")]
mod rodio_source;
mod smooth;
mod stereo;

/// Frames procesados por bloque. 256 @44.1 kHz ≈ 5.8 ms.
pub const DSP_BLOCK_FRAMES: usize = 256;

/// Canales máximos con procesamiento. Por encima se hace passthrough.
pub const DSP_MAX_CHANNELS: usize = 8;

pub use biquad::EqProcessor;
pub use chain::ProcessorChain;
pub use control::DspControl;
pub use gain::GainProcessor;
pub use limiter::LimiterProcessor;
pub use normalize::NormalizationProcessor;
pub use params::{
    DspParams, EqBand, EqParams, LimiterParams, NormalizationMode, NormalizationParams,
    StereoParams, GRAPHIC_BAND_FREQUENCIES_HZ, MAX_EQ_BANDS,
};
pub use processor::{AudioProcessor, StreamFormat};
#[cfg(feature = "rodio")]
pub use rodio_source::DspSource;
pub use stereo::StereoProcessor;
