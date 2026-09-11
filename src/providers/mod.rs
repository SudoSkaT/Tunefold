//! Adaptadores de proveedores externos.
//!
//! Cada submódulo encapsula UN servicio externo: sus clientes, parsers,
//! mappers y modelos propios. Nada de aquí se importa desde UI, playback,
//! analysis, visualization o media — solo desde el punto de composición
//! ([`crate::api`]).
//!
//! El adaptador de YouTube solo existe cuando se compila con la feature
//! `youtube` (excluida del binario oficial). [`lyrics`] es un servicio
//! legítimo e independiente (LRCLIB) y por eso siempre se compila.

pub mod lyrics;

#[cfg(feature = "youtube")]
pub mod youtube;
