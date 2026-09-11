//! Capa de Aplicación: orquesta la lógica de negocio entre dominio e infraestructura.
//!
//! - [`aggregator::MetadataAggregator`]: consulta y fusiona resultados de todas las fuentes.
//! - [`search::SearchEngine`]: búsqueda local (SQLite) + remota (proveedores).
//! - [`history::History`]: registro y consulta del historial de reproducción.

pub mod aggregator;
pub mod audio;
pub mod history;
pub mod playback;
pub mod search;
pub mod thumbnail;
pub mod updater;
