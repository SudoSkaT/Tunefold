//! Capa de Visualization (spec §22-§25):
//!
//! ```text
//! AudioFeatures ──▶ ParameterMapper ──▶ VisualParameters ──▶ VisualEngine
//! PlaybackPosition ────────┐                    VisualPalette ─┘        │
//!                          ▼                       (de la portada)      ▼
//!                                              VisualState (barras+escena)
//!                                                                       ▼
//!                                                       Renderer compuesto (TUI)
//! ```
//!
//! Reglas duras:
//! - El renderer NO analiza audio, NO resuelve streams, NO hace HTTP y NO
//!   conoce proveedores: solo dibuja el [`engine::VisualState`] que le dan.
//! - NO existe reloj visual independiente: toda la fase/tempo deriva de la
//!   posición real de reproducción ([`PositionClock`](crate::playback)) que
//!   entra al motor visual (spec §17/§43).
//! - El mapeo bass→píxel pasa SIEMPRE por el mapper configurable (curvas,
//!   sensibilidad, gate), nunca hardcodeado en el renderer (spec §23/§24).

pub mod engine;
pub mod palette;
pub mod params;
pub mod render;

pub use engine::{VisualEngine, VisualState};
pub use palette::{ChannelColors, VisualPalette};
pub use params::{MapperConfig, ParameterMapper};

/// Nº de barras del espectro TUI v0.
pub const VISUAL_BARS: usize = 24;
