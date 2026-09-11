//! Servicio de miniaturas: descarga, decodificación y caché.
//!
//! Separa la obtención de las miniaturas de la presentación:
//!
//! ```text
//! Provider → Metadata → Thumbnail Service → Thumbnail Cache → Decoded Image
//!      → UI State → Ratatui Widget
//! ```
//!
//! El servicio resuelve las URLs candidatas a través del agregador (que delega
//! en el proveedor del track, p. ej. YouTube y su `i.ytimg.com`), descarga con
//! el cliente HTTP del proyecto, decodifica con `image` en `spawn_blocking`
//! (no bloquea el loop de tokio ni el renderizado) y cachea tanto las URLs
//! fallidas en disco como las imágenes decodificadas en memoria. Las peticiones
//! duplicadas e idénticas en vuelo para el mismo `video_id` se deduplican.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use crate::app::aggregator::MetadataAggregator;
use crate::domain::track::Track;

/// Tamaño máximo de un archivo de imagen descargado (bytes).
const MAX_BYTES: u64 = 1_500_000;
/// Dimensión máxima del lado mayor tras decodificar (píxeles).
const MAX_DIM: u32 = 256;
/// Nº máximo de imágenes decodificadas retenidas en memoria (LRU).
const MEMORY_CACHE_ITEMS: usize = 48;
/// Timeout por cada petición HTTP de miniatura.
const HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Miniatura decodificada lista para representar (RGBA8 con filas en orden
/// mayor). Se comparte vía `Arc` entre el servicio y la UI sin copiar bytes.
#[derive(Debug)]
pub struct DecodedThumb {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// Tres colores dominantes (RGB) extraídos al decodificar; se usan para el
    /// degradado del marco en la UI. `None` si la imagen no dio paleta.
    pub palette: Option<[[u8; 3]; 3]>,
}

/// Estado explícito que la UI puede representar.
#[derive(Debug, Clone)]
pub enum ThumbnailState {
    None,
    Loading,
    Loaded(Arc<DecodedThumb>),
    Failed(String),
}

/// Celda compartida por las peticiones en vuelo del mismo `video_id`:
/// la primera crea y ejecuta la descarga; las concurrentes esperan su notify.
struct Watch {
    notify: Arc<tokio::sync::Notify>,
    result: tokio::sync::Mutex<Option<ThumbnailState>>,
}

impl Watch {
    fn new() -> Self {
        Self {
            notify: Arc::new(tokio::sync::Notify::new()),
            result: tokio::sync::Mutex::new(None),
        }
    }

    async fn set(&self, state: ThumbnailState) {
        *self.result.lock().await = Some(state);
        self.notify.notify_waiters();
    }

    /// Espera el resultado de la petición iniciada por otro hilo.
    async fn await_result(&self) -> ThumbnailState {
        loop {
            if let Some(state) = self.result.lock().await.clone() {
                return state;
            }
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(state) = self.result.lock().await.clone() {
                return state;
            }
            notified.as_mut().await;
        }
    }
}

struct Inner {
    /// Imágenes decodificadas (caché de memoria, LRU limitado).
    decoded: HashMap<String, Arc<DecodedThumb>>,
    /// Orden LRU: frente = recientes, atrás = candidatas a evictar.
    order: VecDeque<String>,
    /// Descargas en curso por clave estable.
    pending: HashMap<String, Arc<Watch>>,
}

pub struct ThumbnailService {
    http: reqwest::Client,
    aggregator: Arc<MetadataAggregator>,
    inner: tokio::sync::Mutex<Inner>,
    cache_dir: PathBuf,
}

impl ThumbnailService {
    /// `http` es el cliente HTTP del proyecto (se comparte, no se crea otro).
    pub fn new(http: reqwest::Client, aggregator: Arc<MetadataAggregator>) -> Self {
        Self {
            http,
            aggregator,
            inner: tokio::sync::Mutex::new(Inner {
                decoded: HashMap::new(),
                order: VecDeque::new(),
                pending: HashMap::new(),
            }),
            cache_dir: cache_dir(),
        }
    }

    /// Resuelve la miniatura de un track: caché (memoria → disco) → HTTP con
    /// fallback de URLs → decodificación. Devuelve el estado resultante.
    pub async fn prepare(&self, track: &Track) -> ThumbnailState {
        let urls = self.aggregator.thumbnail_candidates(track);
        if urls.is_empty() {
            return ThumbnailState::None;
        }
        let key = self.stable_key(track, &urls);

        // Fast path: ya decodificada en memoria.
        {
            let mut inner = self.inner.lock().await;
            if let Some(img) = inner.decoded.get(&key) {
                let img = img.clone();
                inner.touch(&key);
                return ThumbnailState::Loaded(img);
            }
        }

        // Dedup de peticiones idénticas en vuelo para la misma clave.
        let (watch, founder) = {
            let mut inner = self.inner.lock().await;
            match inner.pending.get(&key) {
                Some(w) => (w.clone(), false),
                None => {
                    let w = Arc::new(Watch::new());
                    inner.pending.insert(key.clone(), w.clone());
                    (w, true)
                }
            }
        };
        if !founder {
            return watch.await_result().await;
        }

        let state = self.fetch(&key, &urls).await;
        {
            let mut inner = self.inner.lock().await;
            if let ThumbnailState::Loaded(img) = &state {
                inner.insert_decoded(&key, img.clone());
            }
            inner.pending.remove(&key);
        }
        watch.set(state.clone()).await;
        state
    }

    /// Clave estable de caché: el id externo (`video_id`) cuando existe y, si
    /// no, un hash de la primera URL candidata (cualquier proveedor).
    fn stable_key(&self, track: &Track, urls: &[String]) -> String {
        if let Some(id) = track.external_id.as_deref().filter(|s| !s.is_empty()) {
            return id.to_string();
        }
        urls.first()
            .map(|u| fnv1a_hex(u))
            .unwrap_or_else(|| track.identifier())
    }

    /// Lee el caché de disco o descarga con fallback de URLs. Nunca se
    /// reinventa el cliente HTTP: usa el compartido del proyecto.
    async fn fetch(&self, key: &str, urls: &[String]) -> ThumbnailState {
        // 1) Caché de disco.
        let path = self.cache_dir.join(format!("{key}.img"));
        if let Ok(bytes) = tokio::fs::read(&path).await {
            match decode_background(bytes).await {
                Some(img) => return ThumbnailState::Loaded(img),
                None => {
                    // Archivo corrupto / formato no soportado: se redistribuye.
                    let _ = tokio::fs::remove_file(&path).await;
                }
            }
        }

        // 2) HTTP: prueba cada URL candidata en orden.
        for url in urls {
            let resp = match self.http.get(url).timeout(HTTP_TIMEOUT).send().await {
                Ok(r) if r.status().is_success() => r,
                // 4xx/5xx o error de red: prueba la siguiente resolución.
                _ => continue,
            };
            if resp.content_length().is_some_and(|l| l > MAX_BYTES) {
                continue;
            }
            let bytes = match collect_bounded(resp, MAX_BYTES).await {
                Ok(b) if !b.is_empty() => b,
                _ => continue,
            };
            match decode_background(bytes.clone()).await {
                Some(img) => {
                    let _ = tokio::fs::create_dir_all(&self.cache_dir).await;
                    let _ = tokio::fs::write(&path, bytes).await;
                    return ThumbnailState::Loaded(img);
                }
                None => continue, // bytes inválidos: siguiente candidato.
            }
        }

        ThumbnailState::Failed(
            "miniatura no disponible (red, o el video no tiene portada)".to_string(),
        )
    }
}

impl Inner {
    /// Inserta en el caché LRU (se evicta el menos reciente).
    fn insert_decoded(&mut self, key: &str, img: Arc<DecodedThumb>) {
        self.order.retain(|k| k != key);
        self.order.push_front(key.to_string());
        self.decoded.insert(key.to_string(), img);
        while self.order.len() > MEMORY_CACHE_ITEMS {
            if let Some(old) = self.order.pop_back() {
                self.decoded.remove(&old);
            }
        }
    }

    fn touch(&mut self, key: &str) {
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            let k = self.order.remove(pos).unwrap();
            self.order.push_front(k);
        }
    }
}

/// Descarga el cuerpo con un límite de bytes razonable.
async fn collect_bounded(
    mut resp: reqwest::Response,
    limit: u64,
) -> Result<Vec<u8>, reqwest::Error> {
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        if out.len() as u64 + chunk.len() as u64 > limit {
            return Ok(Vec::new()); // respuesta más grande de lo razonable
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// Decodifica (y redimensiona a `MAX_DIM`) en un hilo de bloqueo: la
/// decodificación es CPU-bound y no debe ocupar un worker de tokio.
async fn decode_background(bytes: Vec<u8>) -> Option<Arc<DecodedThumb>> {
    tokio::task::spawn_blocking(move || decode_blocking(&bytes))
        .await
        .ok()
        .flatten()
}

fn decode_blocking(bytes: &[u8]) -> Option<Arc<DecodedThumb>> {
    use image::{GenericImageView, ImageFormat};
    let format = image::guess_format(bytes).ok()?;
    // Solo formatos de miniatura corrientes; evita sorpresas de memoria.
    match format {
        ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP => {}
        _ => return None,
    }
    let img = image::load_from_memory(bytes).ok()?;
    let (w, h) = img.dimensions();
    let rgba = if w > MAX_DIM || h > MAX_DIM {
        let scale = (MAX_DIM as f64 / w.max(h) as f64).min(1.0);
        let nw = ((w as f64 * scale).round() as u32).max(1);
        let nh = ((h as f64 * scale).round() as u32).max(1);
        // `imageops::thumbnail` devuelve ya RGBA; evita convertir dos veces.
        image::imageops::thumbnail(&img, nw, nh)
    } else {
        img.to_rgba8()
    };
    let (width, height) = rgba.dimensions();
    let raw = rgba.into_raw();
    let palette = dominant_colors(&raw);
    Some(Arc::new(DecodedThumb {
        width,
        height,
        rgba: raw,
        palette,
    }))
}

/// Extrae hasta tres colores dominantes de una imagen RGBA8 cuantizando a
/// cubos de 4 bits por canal y promediando los cubos más frecuentes. Se
/// descartan los píxeles casi transparentes y los colores casi idénticos
/// entre sí, para que el degradado use tonos realmente distintos.
fn dominant_colors(rgba: &[u8]) -> Option<[[u8; 3]; 3]> {
    let mut hist: HashMap<u32, (u64, [u64; 3])> = HashMap::new();
    let mut total: u64 = 0;
    for px in rgba.chunks_exact(4) {
        if px[3] < 16 {
            continue;
        }
        let key = ((px[0] as u32 >> 4) << 8) | ((px[1] as u32 >> 4) << 4) | (px[2] as u32 >> 4);
        let e = hist.entry(key).or_insert((0, [0; 3]));
        e.0 += 1;
        e.1[0] += px[0] as u64;
        e.1[1] += px[1] as u64;
        e.1[2] += px[2] as u64;
        total += 1;
    }
    if total == 0 {
        return None;
    }

    let mut buckets: Vec<([u8; 3], u64)> = hist
        .into_iter()
        .map(|(_, (count, sums))| {
            let avg = [
                (sums[0] / count.max(1)) as u8,
                (sums[1] / count.max(1)) as u8,
                (sums[2] / count.max(1)) as u8,
            ];
            (avg, count)
        })
        .collect();
    buckets.sort_by_key(|&(_, count)| std::cmp::Reverse(count));

    let mut picked: Vec<[u8; 3]> = Vec::with_capacity(3);
    for (avg, _) in buckets {
        if picked.iter().all(|p| rgb_dist_sq(*p, avg) > 60 * 60) {
            picked.push(avg);
            if picked.len() == 3 {
                break;
            }
        }
    }
    if picked.is_empty() {
        return None;
    }
    while picked.len() < 3 {
        let last = picked[picked.len() - 1];
        picked.push(last);
    }
    Some([picked[0], picked[1], picked[2]])
}

fn rgb_dist_sq(a: [u8; 3], b: [u8; 3]) -> u64 {
    let dr = a[0] as i64 - b[0] as i64;
    let dg = a[1] as i64 - b[1] as i64;
    let db = a[2] as i64 - b[2] as i64;
    (dr * dr + dg * dg + db * db) as u64
}

/// `~/.cache/tunefold/thumbnails` (XDG) o `data/thumbnails` como respaldo.
fn cache_dir() -> PathBuf {
    crate::infrastructure::dirs::cache_dir()
        .join("thumbnails")
        .into_os_string()
        .into()
}

/// Hash estable (FNV-1a) para claves cuando no hay `video_id`.
fn fnv1a_hex(input: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in input.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thumb() -> DecodedThumb {
        DecodedThumb {
            width: 2,
            height: 2,
            rgba: vec![0xff; 16],
            palette: None,
        }
    }

    #[test]
    fn lru_evicts_oldest() {
        let mut inner = Inner {
            decoded: HashMap::new(),
            order: VecDeque::new(),
            pending: HashMap::new(),
        };
        for i in 0..(MEMORY_CACHE_ITEMS + 10) {
            inner.insert_decoded(&format!("k{i}"), Arc::new(thumb()));
        }
        assert_eq!(inner.decoded.len(), MEMORY_CACHE_ITEMS);
        assert!(
            inner
                .decoded
                .contains_key(&format!("k{}", MEMORY_CACHE_ITEMS + 9)),
            "las más recientes se conservan"
        );
        assert!(
            !inner.decoded.contains_key("k0"),
            "la más antigua se evicta"
        );

        // Un acceso reciente (k10 estaba por caer) retrasa su evictación.
        assert!(inner.decoded.contains_key("k10"));
        inner.touch("k10");
        for i in 0..3 {
            inner.insert_decoded(&format!("x{i}"), Arc::new(thumb()));
        }
        assert!(inner.decoded.contains_key("k10"), "el accesado se mantiene");
        assert!(
            !inner.decoded.contains_key("k11"),
            "el siguiente más antiguo cae"
        );
    }

    #[test]
    fn stable_key_uses_video_id() {
        let service = ThumbnailService::new(
            reqwest::Client::new(),
            Arc::new(MetadataAggregator::new(
                crate::catalog::CatalogRegistry::default(),
            )),
        );
        let mut track = Track::new(
            "T".to_string(),
            Vec::new(),
            crate::domain::source::Source::YouTube,
        );
        track.external_id = Some("dQw4w9WgXcQ".to_string());
        let urls = vec!["https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg".to_string()];
        assert_eq!(service.stable_key(&track, &urls), "dQw4w9WgXcQ");

        track.external_id = None;
        let k1 = service.stable_key(&track, &urls);
        assert_eq!(k1, fnv1a_hex(urls[0].as_str()));
        assert_eq!(service.stable_key(&track, &urls), k1);
    }

    #[test]
    fn palette_extracts_three_dominant() {
        let mut rgba = Vec::new();
        for _ in 0..2 {
            rgba.extend_from_slice(&[255, 0, 0, 255]);
        }
        for _ in 0..2 {
            rgba.extend_from_slice(&[0, 255, 0, 255]);
        }
        for _ in 0..2 {
            rgba.extend_from_slice(&[0, 0, 255, 255]);
        }
        let pal = dominant_colors(&rgba).expect("paleta de una imagen de 3 colores");
        assert!(pal.iter().any(|c| c == &[255, 0, 0]));
        assert!(pal.iter().any(|c| c == &[0, 255, 0]));
        assert!(pal.iter().any(|c| c == &[0, 0, 255]));
    }

    #[test]
    fn rejects_unsupported_formats() {
        // Garbage: no es JPEG/PNG/WebP, debe rechazarse.
        assert!(decode_blocking(b"not-an-image").is_none());
    }

    #[test]
    fn decodes_and_clamps_to_max_dim() {
        // Genera un PNG 64x48 en memoria (4:3) y lo decodifica.
        use image::{ImageEncoder, RgbaImage};
        let img = RgbaImage::from_pixel(64, 48, image::Rgba([10, 200, 30, 255]));
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(
                img.as_raw(),
                img.width(),
                img.height(),
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        let thumb = decode_blocking(&png).expect("envuelve un PNG válido");
        assert_eq!((thumb.width, thumb.height), (64, 48));
        // El píxel decodificado conserva el color.
        assert_eq!(&thumb.rgba[..4], &[10, 200, 30, 255]);
    }

    #[test]
    fn large_image_is_resized() {
        // 512x288 (> MAX_DIM 256): debe quedar dentro del límite.
        use image::{ImageEncoder, RgbaImage};
        let img = RgbaImage::new(512, 288);
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(
                img.as_raw(),
                img.width(),
                img.height(),
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        let thumb = decode_blocking(&png).expect("PNG válido");
        assert!(thumb.width <= MAX_DIM && thumb.height <= MAX_DIM);
    }
}
