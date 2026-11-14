//! Capa de Dominio: solo estructuras puras y enums, sin lógica de negocio.
//! Esta capa no depende de ninguna infraestructura (DB, HTTP, UI).

pub mod album;
pub mod artist;
pub mod external_playlist;
pub mod genre;
pub mod lyrics;
pub mod playlist;
pub mod source;
pub mod stream;
pub mod track;
