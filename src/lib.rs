//! Tunefold: reproductor de música en TUI con arquitectura de media engine
//! donde YouTube es solamente una posible fuente (spec §45).
//!
//! Capas:
//! - `domain`: modelos propios (Track, StreamResolution...), sin proveedores.
//! - `catalog` / `media`: contratos de catálogo y resolución de streams
//!   independientes del proveedor (registry/router/resolver/caché/fallos).
//! - `providers/*`: adaptadores aislados por servicio externo. TODO el código
//!   específico de YouTube vive aquí y solo se referencia desde `api`.
//! - `api`: punto de COMPOSICIÓN (registros + resolver según feature flags).
//! - `playback`: cola, reloj de posición, preload y recuperación.
//! - `app` / `infrastructure` / `ui`: aplicación, persistencia/motores y TUI.
//!
//! Metadatos, portadas, letras, recomendados y streaming via providers; los
//! datos se cachean en SQLite (playlists, letras, historial) y los ajustes en
//! `.env`.

pub mod analysis;
pub mod api;
pub mod app;
pub mod catalog;
pub mod domain;
pub mod infrastructure;
pub mod media;
pub mod playback;
pub mod providers;
pub mod recommendation;
pub mod ui;
pub mod visualization;
