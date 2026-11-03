//! Contrato de un procesador de audio en tiempo real.
//!
//! Los procesadores trabajan **in place** sobre muestras `f32` intercaladas.
//! El pipeline los llama por bloque, nunca por muestra, y conserva sus estados
//! entre bloques. Un procesador puede leer toda la configuración y decidir si
//! tiene trabajo; el pipeline ya evita la llamada cuando el perfil es plano.

use super::DspParams;

/// Formato del PCM que entra al pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

impl StreamFormat {
    pub const fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            sample_rate,
            channels,
        }
    }

    /// Número de frames completos en un bloque de `samples` muestras.
    pub const fn frames(&self, samples: usize) -> usize {
        if self.channels == 0 {
            0
        } else {
            samples / self.channels as usize
        }
    }
}

/// Procesador independiente del decodificador y de la UI.
///
/// Contrato del camino caliente:
/// - `process` no asigna memoria, no hace I/O y no espera a otro hilo.
/// - `samples` está intercalado y pertenece a un solo bloque; la longitud es
///   múltiplo de los canales salvo, como mucho, el último bloque incompleto.
/// - `prepare` se llama solo cuando cambia el formato del stream.
/// - `reset` limpia el estado tras un seek o una discontinuidad; nunca debe
///   dejar que un filtro arrastre historia de otra posición.
pub trait AudioProcessor: Send {
    /// Nombre estable para diagnóstico y benchmarks.
    fn name(&self) -> &'static str;

    /// Ajusta coeficientes dependientes de la frecuencia de muestreo/canales.
    fn prepare(&mut self, format: StreamFormat);

    /// Procesa un bloque intercalado in place.
    fn process(&mut self, samples: &mut [f32], params: &DspParams);

    /// Borra el estado interno (historia de filtros, detectores, etc.).
    fn reset(&mut self);
}
