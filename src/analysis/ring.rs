//! Anillo SPSC acotado lock-free (single-producer / single-consumer).
//!
//! El productor es el hilo de audio (a través del [`super::tap`]) y el
//! consumidor el hilo de análisis. Garantías:
//!
//! - **Nunca bloquea al productor**: si está lleno, se descarta lo NUEVO
//!   (política drop-newest). El análisis va por detrás solo transitoriamente;
//!   a largo plazo consume al mismo ritmo que se produce, así que el anillo
//!   se drena solo. Un análisis lento jamás puede cortar el audio.
//! - Capacidad potencia de dos (indexado por máscara, sin módulo).
//! - Ordenación `Acquire/Release`: el consumidor ve las escrituras completas.
//!
//! Guardas de seguridad: los índices avanzan en múltiplos de la capacidad
//! (`& !usize::MAX << shift` no: usamos wrapping puro con máscara), y las
//! operaciones son wait-free para ambos lados.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Anillo de f32 compartido entre dos hilos.
///
/// SEGURIDAD: `buf` vive detrás de [`UnsafeCell`] porque productor y
/// consumidor escriben/leen posiciones DISJUNTAS por diseño — los índices
/// atómicos con ordenación Acquire/Release garantizan que nunca solapan: el
/// productor solo toca `[tail..head)→[head..head+n)` y el consumidor el rango
/// ya publicado. Es el patrón SPSC clásico; `unsafe impl Sync` documenta el
/// invariante en un único lugar.
pub struct SpScRing {
    buf: UnsafeCell<Box<[f32]>>,
    mask: usize,
    head: AtomicUsize, // posición de escritura (solo productor la mueve)
    tail: AtomicUsize, // posición de lectura  (solo consumidor la mueve)
    /// Muestras descartadas por anillo lleno (observabilidad).
    dropped: AtomicUsize,
}

impl SpScRing {
    /// Crea el anillo con capacidad `capacity` (redondeada ARRIBA a potencia
    /// de dos; mínimo 1024).
    pub fn new(capacity: usize) -> Arc<Self> {
        let cap = capacity.max(1024).next_power_of_two();
        Arc::new(Self {
            buf: UnsafeCell::new(vec![0.0; cap].into_boxed_slice()),
            mask: cap - 1,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            dropped: AtomicUsize::new(0),
        })
    }
}

impl SpScRing {
    /// Elementos disponibles para leer.
    pub fn len(&self) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        head.wrapping_sub(tail)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Capacidad útil (cap-1: un hueco distingue lleno de vacío).
    pub fn capacity(&self) -> usize {
        self.mask
    }

    /// Muestras descartadas acumuladas.
    pub fn dropped(&self) -> usize {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Escribe `data`; devuelve cuántas entraron. Si no cabe entero, se
    /// escribe lo que quepa y el resto se cuenta como descartado (drop-newest).
    pub fn push(&self, data: &[f32]) -> usize {
        self.push_aligned(data, 1)
    }

    /// Escribe datos manteniendo grupos completos, por ejemplo pares de
    /// muestras L/R para PCM estéreo.
    pub fn push_aligned(&self, data: &[f32], alignment: usize) -> usize {
        debug_assert!(alignment > 0);
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        let free = self.capacity() - head.wrapping_sub(tail);
        let n = (data.len().min(free) / alignment) * alignment;
        if n < data.len() {
            self.dropped.fetch_add(data.len() - n, Ordering::Relaxed);
        }
        // SAFETY: ver invariante del struct — rango [head..head+n) es propiedad
        // exclusiva del productor en este instante.
        unsafe {
            let buf = &mut *self.buf.get();
            for (i, &v) in data[..n].iter().enumerate() {
                buf[(head + i) & self.mask] = v;
            }
        }
        // Release: el consumidor verá las escrituras tras leer head.
        self.head.store(head + n, Ordering::Release);
        n
    }

    /// Lee hasta `out.len()` elementos; devuelve cuántos copió.
    pub fn pop(&self, out: &mut [f32]) -> usize {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        let available = head.wrapping_sub(tail);
        let n = out.len().min(available);
        // SAFETY: rango [tail..tail+n) ya publicado vía Release del productor.
        unsafe {
            let buf = &*self.buf.get();
            for (i, slot) in out[..n].iter_mut().enumerate() {
                *slot = buf[(tail + i) & self.mask];
            }
        }
        // Release: el productor podrá reutilizar estos huecos.
        self.tail.store(tail + n, Ordering::Release);
        n
    }
}

// El índice crece sin límite teórico pero con wrapping de usize a 44_100/s
// tardaría ~13 millones de años en dar la vuelta: sin riesgo práctico.

// SAFETY: invariante SPSC documentado arriba (posiciones disjuntas por
// índices atómicos); no hay otro estado compartido mutable.
unsafe impl Sync for SpScRing {}
unsafe impl Send for SpScRing {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn push_pop_preserves_order_and_content() {
        let ring = SpScRing::new(1024);
        let data: Vec<f32> = (0..500).map(|i| i as f32 * 0.5).collect();
        assert_eq!(ring.push(&data), 500);

        let mut out = vec![0.0; 500];
        assert_eq!(ring.pop(&mut out), 500);
        assert_eq!(out, data);
        assert!(ring.is_empty());
    }

    #[test]
    fn full_ring_drops_newest_and_counts() {
        let ring = SpScRing::new(1024); // capacidad útil 1024
        let chunk = vec![1.0f32; 2000];
        let written = ring.push(&chunk);
        assert_eq!(written, ring.capacity());
        assert_eq!(ring.dropped(), 2000 - ring.capacity());
        // Lo leído son los PRIMEROS datos entrantes (drop-newest conserva viejos).
        let mut out = vec![0.0; ring.capacity()];
        assert_eq!(ring.pop(&mut out), ring.capacity());
        assert!(out.iter().all(|&v| v == 1.0));
    }

    #[test]
    fn wraps_across_capacity_repeatedly() {
        // Produce 10× la capacidad manteniendo el invariante "nunca lleno":
        // los índices envuelven muchas veces y el contenido sale en orden.
        let ring = SpScRing::new(1024);
        let mut last = -1.0f32;
        let mut out = [0.0f32; 97]; // tamaño no divisor: cortes irregulares
        let mut value = 0.0f32;
        let mut consumed = 0f32;
        for _ in 0..(10 * 1024) {
            if ring.len() == ring.capacity() {
                let n = ring.pop(&mut out);
                for &v in &out[..n] {
                    assert!(v > last, "orden roto: {v} tras {last}");
                    last = v;
                    consumed += 1.0;
                }
            }
            value += 1.0;
            assert_eq!(ring.push(&[value]), 1);
        }
        while !ring.is_empty() {
            let n = ring.pop(&mut out);
            for &v in &out[..n] {
                assert!(v > last);
                last = v;
                consumed += 1.0;
            }
        }
        assert_eq!(consumed, value, "todo lo producido se consumió");
        assert_eq!(ring.dropped(), 0);
    }

    #[test]
    fn aligned_push_never_splits_interleaved_frames() {
        let ring = SpScRing::new(1024);
        let data = vec![0.25; 1024];
        assert_eq!(ring.push_aligned(&data, 2), 1022);
        let mut out = vec![0.0; 1024];
        assert_eq!(ring.pop(&mut out), 1022);
        assert!(out[..1022].iter().all(|sample| *sample == 0.25));
        assert_eq!(ring.push_aligned(&[0.5; 4], 2), 4);
    }

    #[test]
    fn concurrent_producer_consumer_never_loses_or_corrupts() {
        let ring = SpScRing::new(4096);
        let producer_ring = Arc::clone(&ring);

        const TOTAL: usize = 100_000;
        let producer = std::thread::spawn(move || {
            let mut sent = 0usize;
            let mut value = 0.0f32;
            while sent < TOTAL {
                // Respeta el espacio libre: el contrato es drop-newest, así
                // que un lote mayor que el hueco perdería su cola y la
                // secuencia del test tendría huecos.
                let free = producer_ring.capacity() - producer_ring.len();
                let n = free.min(64);
                if n == 0 {
                    std::thread::yield_now();
                    continue;
                }
                let batch: Vec<f32> = (0..n)
                    .map(|_| {
                        let v = value;
                        value += 1.0;
                        v
                    })
                    .collect();
                sent += producer_ring.push(&batch);
            }
        });

        let mut received = Vec::with_capacity(TOTAL);
        let mut buf = [0.0f32; 128];
        while received.len() < TOTAL {
            let n = ring.pop(&mut buf);
            received.extend_from_slice(&buf[..n]);
        }
        producer.join().unwrap();

        // Secuencia monótona estricta: sin duplicados, saltos ni corrupción.
        for (i, w) in received.windows(2).enumerate() {
            assert!(
                w[1] > w[0],
                "corrupción en {i}: {w:?} — el orden SPSC se rompió"
            );
        }
        assert_eq!(received[TOTAL - 1], (TOTAL - 1) as f32);
        assert_eq!(
            ring.dropped(),
            0,
            "el consumidor iba al ritmo del productor"
        );
    }
}
