//! Publicación de parámetros entre el hilo de control y el hilo de audio.
//!
//! El productor (UI/backend) escribe el estado bajo un mutex; el consumidor
//! (audio) usa `try_lock`, nunca `lock`. Si el control está escribiendo en ese
//! instante, el bloque se procesa con la configuración anterior y el cambio se
//! aplica en el siguiente bloque. El hilo de audio no espera, no asigna y no
//! puede quedar bloqueado por la interfaz.
//!
//! El estado publicado es [`DspParams`], un valor `Copy` de tamaño fijo. La
//! copia bajo el lock es un memcpy acotado, no una operación con I/O.

use std::fmt;
use std::sync::{Arc, Mutex, TryLockError};

use super::DspParams;

/// Handle clonable para leer y publicar la configuración DSP.
#[derive(Clone)]
pub struct DspControl {
    inner: Arc<Mutex<Published>>,
}

struct Published {
    version: u64,
    params: DspParams,
}

impl DspControl {
    /// Crea el control con una configuración inicial.
    pub fn new(params: DspParams) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Published {
                version: 1,
                params: params.sanitized(),
            })),
        }
    }

    /// Publica una configuración nueva y devuelve su versión.
    ///
    /// Si los parámetros saneados son idénticos a los vigentes no se incrementa
    /// la versión: los procesadores no recalculan coeficientes en balde.
    pub fn set(&self, params: DspParams) -> u64 {
        let params = params.sanitized();
        let mut published = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if published.params != params {
            published.version = published.version.wrapping_add(1).max(1);
            published.params = params;
        }
        published.version
    }

    /// Copia la configuración vigente (uso exclusivo del hilo de control).
    pub fn get(&self) -> DspParams {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .params
    }

    /// Versión vigente.
    pub fn version(&self) -> u64 {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .version
    }

    /// Instantánea coherente de versión + parámetros (hilo de control).
    pub(crate) fn snapshot(&self) -> (u64, DspParams) {
        let published = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (published.version, published.params)
    }

    /// Intenta tomar una instantánea sin bloquear.
    ///
    /// Devuelve `None` si el productor mantiene el lock en este instante. Es la
    /// única lectura que puede usar el hilo de audio.
    pub(crate) fn try_snapshot(&self) -> Option<(u64, DspParams)> {
        match self.inner.try_lock() {
            Ok(published) => Some((published.version, published.params)),
            Err(TryLockError::Poisoned(poisoned)) => {
                let published = poisoned.into_inner();
                Some((published.version, published.params))
            }
            Err(TryLockError::WouldBlock) => None,
        }
    }
}

impl fmt::Debug for DspControl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DspControl").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::params::EqParams;

    #[test]
    fn equivalent_params_keep_the_version_stable() {
        let control = DspControl::new(DspParams::flat());
        let first = control.version();
        assert_eq!(control.set(DspParams::flat()), first);

        let mut active = DspParams::flat();
        active.eq = EqParams {
            enabled: true,
            ..EqParams::graphic_flat()
        };
        active.eq.bands[0].gain_db = 3.0;
        let second = control.set(active);
        assert!(second > first);
        assert_eq!(control.get(), active.sanitized());
    }

    #[test]
    fn try_snapshot_never_waits_for_the_producer() {
        let control = DspControl::new(DspParams::flat());
        let guard = control.inner.lock().unwrap();
        assert!(control.try_snapshot().is_none());
        drop(guard);
        assert!(control.try_snapshot().is_some());
    }

    #[test]
    fn non_finite_values_are_sanitized_before_publication() {
        let control = DspControl::new(DspParams::flat());
        let mut params = DspParams::flat();
        params.preamp_db = f32::NAN;
        control.set(params);
        assert_eq!(control.get().preamp_db, 0.0);
    }
}
