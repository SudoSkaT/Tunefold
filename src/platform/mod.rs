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
pub mod android_playback;

#[cfg(feature = "android")]
pub mod android_download;
