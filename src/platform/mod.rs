//! Platform-specific implementations.
//!
//! This module provides platform abstractions and implementations.
//! Currently supports:
//! - Desktop (default): rodio/cpal audio backend, TUI
//! - Android (feature `android`): JNI bridge, Oboe audio backend

#[cfg(feature = "android")]
pub mod android;

#[cfg(feature = "android")]
pub mod android_decoder;

#[cfg(feature = "android")]
pub mod android_download;

#[cfg(feature = "android")]
pub mod android_playback;

/// Ranking de recomendaciones compartido por JNI (Fase 5): el mismo
/// `recommendation::rank_sync` de la TUI, expuesto como envelope JSON
/// estadoless. Compila solo con la feature `android` (es el `.so` móvil).
#[cfg(feature = "android")]
pub mod android_rank;
