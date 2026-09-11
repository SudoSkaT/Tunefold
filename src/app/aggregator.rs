//! Agregador de metadatos: consulta todos los proveedores registrados y fusiona
//! los resultados en una única lista canónica (deduplicada por artista+título).

use std::collections::HashMap;

use crate::catalog::{CatalogError, CatalogRegistry};
use crate::domain::{album::Album, artist::Artist, track::Track};

/// Resultado de una búsqueda agregada: resultados + errores por proveedor.
#[derive(Debug, Default)]
pub struct SearchOutcome<T> {
    pub items: Vec<T>,
    pub errors: Vec<CatalogError>,
    /// Índice en `items` a partir del cual hay recomendaciones relacionadas con
    /// la búsqueda (0 si la búsqueda no fue enriquecida).
    pub related_from: usize,
}

#[derive(Debug)]
pub struct MetadataAggregator {
    providers: CatalogRegistry,
    /// Cliente HTTP dedicado a LRCLIB (letras), independiente de los
    /// proveedores de streaming: disponible siempre, incluso en binarios sin
    /// la feature YouTube.
    lrclib: reqwest::Client,
}

impl MetadataAggregator {
    pub fn new(providers: CatalogRegistry) -> Self {
        Self {
            providers,
            lrclib: crate::providers::lyrics::lrclib_client(),
        }
    }

    pub fn providers(&self) -> &CatalogRegistry {
        &self.providers
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    /// Busca canciones en todas las fuentes y las deduplica.
    /// La primera aparición de una canción gana; se conserva su `source`.
    pub async fn search_tracks(&self, query: &str, limit: u32) -> SearchOutcome<Track> {
        let mut seen: HashMap<String, Track> = HashMap::new();
        let mut errors = Vec::new();

        for provider in self.providers.providers() {
            match provider.search_tracks(query, limit).await {
                Ok(tracks) => {
                    for track in tracks {
                        seen.entry(dedupe_key(&track)).or_insert(track);
                    }
                }
                Err(e) => errors.push(e),
            }
        }

        SearchOutcome {
            items: seen.into_values().collect(),
            errors,
            related_from: 0,
        }
    }

    /// Recomendados de un video (delegado al único proveedor).
    pub async fn related(&self, video_id: &str) -> Vec<Track> {
        let Some(provider) = self.providers.providers().first() else {
            return Vec::new();
        };
        provider.related(video_id).await.unwrap_or_default()
    }

    /// Completa únicamente las duraciones ausentes mediante la ficha puntual
    /// del proveedor. Cada petición tiene un timeout corto: la lista no queda
    /// retenida indefinidamente y una duración nunca se inventa.
    pub async fn hydrate_missing_durations(&self, tracks: &mut [Track]) {
        for track in tracks.iter_mut().filter(|track| track.duration.is_none()) {
            let Some(provider) = self.providers.get(track.source) else {
                continue;
            };
            let Some(external_id) = track.external_id.clone() else {
                continue;
            };
            if let Some(detail) = tokio::time::timeout(
                std::time::Duration::from_secs(4),
                provider.get_track(&external_id),
            )
            .await
            .ok()
            .and_then(Result::ok)
            {
                track.duration = detail.duration;
            }
        }
    }

    /// Letra sincronizada (LRC) del track: primero el proveedor de la fuente
    /// (si lo hay); si no ofrece letras, LRCLIB como cliente propio
    /// independiente. Es la única fuente del karaoke.
    pub async fn synced_lyrics(&self, track: &Track) -> Option<String> {
        if let Some(provider) = self.providers.get(track.source) {
            if let Some(lyrics) = provider.synced_lyrics(track).await.ok().flatten() {
                return Some(lyrics);
            }
        }
        crate::providers::lyrics::fetch_lrclib_lyrics(&self.lrclib, track).await
    }

    /// URLs candidatas de miniatura para un track, según la estrategia del
    /// proveedor que lo soporta. Sin proveedor, cae a la miniatura adjunta.
    pub fn thumbnail_candidates(&self, track: &Track) -> Vec<String> {
        self.providers
            .get(track.source)
            .map(|p| p.thumbnail_candidates(track))
            .unwrap_or_else(|| {
                track
                    .thumbnail
                    .as_ref()
                    .map(|t| vec![t.url.clone()])
                    .unwrap_or_default()
            })
    }

    pub async fn search_artists(&self, query: &str, limit: u32) -> SearchOutcome<Artist> {
        let mut seen: HashMap<String, Artist> = HashMap::new();
        let mut errors = Vec::new();

        for provider in self.providers.providers() {
            match provider.search_artists(query, limit).await {
                Ok(artists) => {
                    for artist in artists {
                        seen.entry(artist.name.to_lowercase()).or_insert(artist);
                    }
                }
                Err(e) => errors.push(e),
            }
        }

        SearchOutcome {
            items: seen.into_values().collect(),
            errors,
            related_from: 0,
        }
    }

    pub async fn search_albums(&self, query: &str, limit: u32) -> SearchOutcome<Album> {
        let mut seen: HashMap<String, Album> = HashMap::new();
        let mut errors = Vec::new();

        for provider in self.providers.providers() {
            match provider.search_albums(query, limit).await {
                Ok(albums) => {
                    for album in albums {
                        seen.entry(album.title.to_lowercase()).or_insert(album);
                    }
                }
                Err(e) => errors.push(e),
            }
        }

        SearchOutcome {
            items: seen.into_values().collect(),
            errors,
            related_from: 0,
        }
    }
}

/// Clave de deduplicación: "artista | título" en minúsculas.
fn dedupe_key(track: &Track) -> String {
    let artist = track.primary_artist_name().unwrap_or("").to_lowercase();
    format!("{artist} | {}", track.title.to_lowercase())
}
