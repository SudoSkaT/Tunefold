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

use crate::visualization::palette::CoverColorMatrix;

/// Miniatura decodificada lista para representar (RGBA8 con filas en orden
/// mayor). Se comparte vía `Arc` entre el servicio y la UI sin copiar bytes.
#[derive(Debug)]
pub struct DecodedThumb {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// Tres colores dominantes (RGB) extraídos al decodificar; se usan para el
    /// degradado del marco en la UI. `None` si la imagen no dio paleta.
    /// Derivan de [`Self::matrix`] (fuente única).
    pub palette: Option<[[u8; 3]; 3]>,
    /// Matriz cromática compacta de la portada (fuente del tema visual de la
    /// canción). Se extrae una vez aquí y viaja cacheada con la imagen.
    pub matrix: Option<CoverColorMatrix>,
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
    // Fuente cromática única: matriz compacta una vez por portada; el trío
    // para el marco deriva de ella (regiones dominantes por población).
    let matrix = CoverColorMatrix::from_rgba(&raw, width, height);
    let palette = matrix.as_ref().map(|m| m.dominants());
    Some(Arc::new(DecodedThumb {
        width,
        height,
        rgba: raw,
        palette,
        matrix,
    }))
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
            matrix: None,
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
        // Tres franjas horizontales (rojo arriba, verde en medio, azul abajo)
        // en imagen 6x6: cada region domina 12 celdas de la matriz y el trio
        // las recupera como dominantes.
        let mut rgba = Vec::new();
        for row in 0..6u8 {
            let px = match row {
                0 | 1 => [255, 0, 0, 255],
                2 | 3 => [0, 255, 0, 255],
                _ => [0, 0, 255, 255],
            };
            for _ in 0..6 {
                rgba.extend_from_slice(&px);
            }
        }
        let matrix = CoverColorMatrix::from_rgba(&rgba, 6, 6).expect("matriz 6x6");
        let pal = matrix.dominants();
        assert!(pal.iter().any(|c| c == &[255, 0, 0]));
        assert!(pal.iter().any(|c| c == &[0, 255, 0]));
        assert!(pal.iter().any(|c| c == &[0, 0, 255]));
    }

    #[test]
    fn decoded_thumb_carries_matrix_for_theme() {
        // PNG solido 64x48: el decode produce matriz y paleta coherentes
        // (misma fuente), listas para el tema sin analisis extra.
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
        let thumb = decode_blocking(&png).expect("PNG valido");
        let matrix = thumb.matrix.expect("matriz extraida al decodificar");
        assert_eq!(matrix.dominants()[0], [10, 200, 30]);
        assert_eq!(
            thumb.palette.expect("paleta derivada de la matriz"),
            matrix.dominants()
        );
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
