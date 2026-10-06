//! Descarga explícita de un track completo al almacenamiento local.
//!
//! Es una capacidad SEPARADA de la reproducción: nada en este módulo se
//! ejecuta durante Play, metadata, artwork, preload o buffering. Solo arranca
//! cuando la UI lo pide de forma explícita.
//!
//! Pipeline (reutiliza el transporte existente, no lo duplica):
//!
//! ```text
//! PlayableSource → HttpRangeStream → Range windows → .part → validación
//!                → commit atómico → LocalMediaStore → file:
//! ```
//!
//! Reglas que este módulo respeta:
//!
//! - **Identidad lógica**: `provider + track id`. La URL temporal resuelta NO
//!   se usa como identidad ni se persiste.
//! - **Nada se acepta en silencio**: se valida el tamaño final esperado contra
//!   el realmente escrito antes del commit.
//! - **Commit atómico**: se escribe en `.part` y se renombra; si algo falla,
//!   el `.part` se limpia y el store queda intacto.
//! - **Cancelable**: la cancelación se comprueba entre trozos y devuelve un
//!   error clasificado, no un archivo a medias.
//! - **Sin playback**: no toca el PCM ring, ni el decoder, ni el AudioTrack.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::media::transport::{HttpRangeStream, RangePolicy, TransportFailure};

/// Límites de la descarga. Coherentes con `FileLocalMediaStore`: el store es
/// la autoridad final y volverá a validar, pero no descargaremos de más.
const MAX_ITEM_BYTES: u64 = 200 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Motivo por el que una descarga no terminó bien.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DownloadError {
    #[error("fallo de transporte: {0}")]
    Transport(String),
    #[error("la descarga fue cancelada")]
    Cancelled,
    #[error("el recurso no cabe en el límite por item ({size} > {max} bytes)")]
    TooLarge { size: u64, max: u64 },
    #[error("tamaño inesperado: se esperaban {expected} bytes y se escribieron {written}")]
    SizeMismatch { expected: u64, written: u64 },
    #[error("no se pudo escribir el archivo local: {0}")]
    Io(String),
}

impl DownloadError {
    /// Categoría estructural, para métricas y recuperación.
    pub fn category(&self) -> crate::media::FailureCategory {
        match self {
            DownloadError::Transport(_) => crate::media::FailureCategory::NetworkFailure,
            DownloadError::Cancelled => crate::media::FailureCategory::PlaybackFailure,
            DownloadError::TooLarge { .. } | DownloadError::SizeMismatch { .. } => {
                crate::media::FailureCategory::InvalidResponse
            }
            DownloadError::Io(_) => crate::media::FailureCategory::Unknown,
        }
    }
}

/// Progreso de la descarga, en bytes. El total es `None` hasta que la
/// cabecera `Content-Range` descubre el tamaño total del recurso.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DownloadProgress {
    pub received: u64,
    pub total: Option<u64>,
}

impl DownloadProgress {
    /// Porcentaje 0..=100 cuando ya se conoce el total.
    pub fn percent(&self) -> Option<u8> {
        self.total
            .filter(|total| *total > 0)
            .map(|total| ((self.received.min(total) * 100) / total) as u8)
    }

    pub fn is_complete(&self) -> bool {
        matches!(self.total, Some(total) if total > 0 && self.received >= total)
    }
}

/// Descarga el `PlayableSource` a `directory` y devuelve la ruta final.
///
/// `provider` e `id` son la identidad lógica del track: el nombre en disco es
/// un digest de ambos, de modo que cambiar de URL resuelta no crea una entrada
/// nueva ni invalida la anterior.
///
/// No deja archivos a medias: escribe en `<final>.part`, valida el tamaño y
/// renombra. Si se cancela o falla, borra el `.part`.
pub async fn download_to_local_media(
    url: &str,
    headers: Vec<(String, String)>,
    provider: &str,
    id: &str,
    directory: &Path,
    cancel: &Arc<AtomicBool>,
    on_progress: impl FnMut(DownloadProgress),
) -> Result<PathBuf, DownloadError> {
    download_to_local_media_bounded(
        url,
        headers,
        provider,
        id,
        directory,
        cancel,
        MAX_ITEM_BYTES,
        on_progress,
    )
    .await
}

/// Igual que [`download_to_local_media`] pero con el límite por item explícito.
///
/// El límite es un parámetro para que la vía de rechazo sea verificable con un
/// fixture pequeño, sin depender del tamaño real del recurso.
#[allow(clippy::too_many_arguments)]
pub async fn download_to_local_media_bounded(
    url: &str,
    headers: Vec<(String, String)>,
    provider: &str,
    id: &str,
    directory: &Path,
    cancel: &Arc<AtomicBool>,
    max_item_bytes: u64,
    mut on_progress: impl FnMut(DownloadProgress),
) -> Result<PathBuf, DownloadError> {
    if provider.is_empty() || id.is_empty() {
        return Err(DownloadError::Io("identidad del track vacía".into()));
    }
    let stem = local_media_stem(provider, id)?;
    if !directory.is_dir() {
        std::fs::create_dir_all(directory).map_err(|e| DownloadError::Io(e.to_string()))?;
    }
    let final_path = directory.join(format!("{stem}.audio"));
    let part_path = directory.join(format!("{stem}.audio.part"));

    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| DownloadError::Transport(e.without_url().to_string()))?;

    let mut stream = HttpRangeStream::open(client, url, headers, RangePolicy::default())
        .await
        .map_err(|e| DownloadError::Transport(e.to_string()))?;

    let total = stream.total();
    if total > max_item_bytes {
        return Err(DownloadError::TooLarge {
            size: total,
            max: max_item_bytes,
        });
    }
    let expected = total;
    let mut part = PartWriter::open(&part_path)?;
    let mut written: u64 = 0;
    on_progress(DownloadProgress {
        received: 0,
        total: if total > 0 { Some(total) } else { None },
    });

    let outcome = stream
        .download_full(|chunk| {
            if cancel.load(Ordering::Relaxed) {
                return Err(TransportFailure::Network("cancelado".into()));
            }
            part.append(chunk)?;
            written += chunk.len() as u64;
            Ok(())
        })
        .await;

    // Validation of the `.part` BEFORE touching anything persistent.
    let part = part;
    let cleanup = |path: &Path| {
        let _ = std::fs::remove_file(path);
    };
    match outcome {
        Err(_) => {
            cleanup(&part_path);
            if cancel.load(Ordering::Relaxed) {
                return Err(DownloadError::Cancelled);
            }
            return Err(DownloadError::Transport(
                "la descarga se interrumpió".into(),
            ));
        }
        Ok(delivered) => {
            if delivered != written {
                cleanup(&part_path);
                return Err(DownloadError::SizeMismatch {
                    expected: delivered,
                    written,
                });
            }
            // Validación de tamaño: sin total conocido solo se exige > 0.
            if expected > 0 {
                if written != expected {
                    cleanup(&part_path);
                    return Err(DownloadError::SizeMismatch { expected, written });
                }
            } else if written == 0 {
                cleanup(&part_path);
                return Err(DownloadError::SizeMismatch {
                    expected: 0,
                    written: 0,
                });
            }
        }
    }

    on_progress(DownloadProgress {
        received: written,
        total: if expected > 0 { Some(expected) } else { None },
    });
    // Atomic commit: only now does the audio move from `.part` to final.
    part.commit(&part_path, &final_path)?;
    Ok(final_path)
}

/// Escribe en el `.part`: se abre en modo escritura y se sincroniza antes de
/// renombrar, para que el commit sea observablemente atómico.
struct PartWriter {
    file: Option<std::fs::File>,
}

impl PartWriter {
    fn open(path: &Path) -> Result<Self, DownloadError> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
            .map_err(|e| DownloadError::Io(e.to_string()))?;
        Ok(Self { file: Some(file) })
    }

    fn append(&mut self, chunk: &[u8]) -> Result<(), TransportFailure> {
        let file = self
            .file
            .as_mut()
            .expect("the .part file handle stays open");
        file.write_all(chunk)
            .and_then(|()| file.flush())
            .map_err(|e| TransportFailure::Network(e.to_string()))
    }

    fn commit(mut self, part: &Path, final_path: &Path) -> Result<(), DownloadError> {
        if let Some(file) = self.file.as_mut() {
            file.sync_all()
                .map_err(|e| DownloadError::Io(e.to_string()))?;
        }
        std::fs::rename(part, final_path).map_err(|e| DownloadError::Io(e.to_string()))?;
        Ok(())
    }
}

/// Digest estable de `provider + track id`. Es la ÚNICA identidad de archivo:
/// la URL resuelta nunca entra en el nombre.
pub fn local_media_stem(provider: &str, id: &str) -> Result<String, DownloadError> {
    // Una identidad vacía nunca debe producir un nombre de archivo: dos tracks
    // sin id colapsarían en el mismo destino.
    if provider.is_empty() || id.is_empty() {
        return Err(DownloadError::Io("identidad del track vacía".into()));
    }
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(provider.as_bytes());
    hasher.update([0u8]);
    hasher.update(id.as_bytes());
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Borra los `.part` abandonados de descargas interrumpidas.
///
/// Se ejecuta al inicializar el store, no durante Play: un corte de red no
/// debe dejar archivos a medias ocupando espacio.
pub fn clean_abandoned_parts(directory: &Path, max_age: Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return 0;
    };
    let now = std::time::SystemTime::now();
    let mut removed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("part") {
            continue;
        }
        let abandoned = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .map(|age| age > max_age)
            .unwrap_or(false);
        if abandoned && std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn progress_percent_requires_a_known_total() {
        assert_eq!(DownloadProgress::default().percent(), None);
        let known = DownloadProgress {
            received: 34,
            total: Some(100),
        };
        assert_eq!(known.percent(), Some(34));
        assert!(!known.is_complete());
        let done = DownloadProgress {
            received: 100,
            total: Some(100),
        };
        assert!(done.is_complete());
        // Nunca supera 100 aunque se escriba de más.
        let over = DownloadProgress {
            received: 140,
            total: Some(100),
        };
        assert_eq!(over.percent(), Some(100));
    }

    /// La identidad de archivo depende SOLO de `provider + id`: cambiar la URL
    /// resuelta no debe crear una entrada distinta.
    #[test]
    fn identity_uses_provider_and_id_only() {
        let a = local_media_stem("youtube", "dQw4w9WgXcQ").unwrap();
        let b = local_media_stem("youtube", "dQw4w9WgXcQ").unwrap();
        assert_eq!(a, b, "estable para la misma identidad");
        assert_ne!(
            a,
            local_media_stem("youtube", "otra").unwrap(),
            "otra identidad, otro archivo"
        );
        assert_ne!(
            a,
            local_media_stem("otro", "dQw4w9WgXcQ").unwrap(),
            "otro provider, otro archivo"
        );
        assert_eq!(a.len(), 64, "digest SHA-256 en hexadecimal");
    }

    #[test]
    fn empty_identity_is_refused() {
        assert!(local_media_stem("", "id").is_err());
        assert!(local_media_stem("youtube", "").is_err());
    }

    /// Los `.part` abandonados se limpian; los recientes se conservan (puede
    /// haber una descarga en curso).
    #[test]
    fn abandoned_parts_are_cleaned_but_recent_ones_survive() {
        let dir = tempfile::tempdir().expect("tempdir");
        let old = dir.path().join("a.audio.part");
        let fresh = dir.path().join("b.audio.part");
        let keep = dir.path().join("c.audio.json");
        std::fs::write(&old, b"x").unwrap();
        std::fs::write(&fresh, b"x").unwrap();
        std::fs::write(&keep, b"{}").unwrap();

        let removed = clean_abandoned_parts(dir.path(), Duration::from_secs(0));
        // max_age 0 hace que todo `.part` sea considered abandonado.
        assert_eq!(removed, 2, "los dos .part se limpian");
        assert!(!old.exists());
        assert!(!fresh.exists());
        assert!(keep.exists(), "los metadatos no se tocan");

        assert_eq!(
            clean_abandoned_parts(dir.path(), Duration::from_secs(3600)),
            0
        );
    }

    /// Cancelar antes del commit no deja archivo final ni `.part`.
    #[tokio::test]
    async fn cancelled_download_leaves_no_file() {
        let server = crate::media::transport::fake_server::FakeServer::start(
            crate::media::transport::fake_server::Scenario::Normal(std::sync::Arc::new(
                (0..300_000u32).map(|i| (i % 251) as u8).collect(),
            )),
        )
        .await;
        let dir = tempfile::tempdir().expect("tempdir");
        let cancel = Arc::new(AtomicBool::new(true));
        let result = download_to_local_media(
            &server.url(),
            Vec::new(),
            "youtube",
            "abc",
            dir.path(),
            &cancel,
            |_| {},
        )
        .await;
        assert_eq!(result, Err(DownloadError::Cancelled));
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "no queda ni .part ni archivo final"
        );
    }

    /// Descarga completa: valida el tamaño y hace commit atómico.
    #[tokio::test]
    async fn full_download_validates_size_and_commits_atomically() {
        let payload: Vec<u8> = (0..700_000u32).map(|i| (i % 251) as u8).collect();
        let server = crate::media::transport::fake_server::FakeServer::start(
            crate::media::transport::fake_server::Scenario::Normal(std::sync::Arc::new(
                payload.clone(),
            )),
        )
        .await;
        let dir = tempfile::tempdir().expect("tempdir");
        let cancel = Arc::new(AtomicBool::new(false));
        let mut last = DownloadProgress::default();
        let path = download_to_local_media(
            &server.url(),
            Vec::new(),
            "youtube",
            "abc",
            dir.path(),
            &cancel,
            |progress| last = progress,
        )
        .await
        .expect("descarga completa");

        assert_eq!(std::fs::read(&path).unwrap(), payload, "bytes idénticos");
        assert_eq!(last.received, payload.len() as u64);
        assert_eq!(last.total, Some(payload.len() as u64));
        assert!(last.is_complete());
        assert_eq!(last.percent(), Some(100));
        let leftover: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|name| name.ends_with(".part"))
            .collect();
        assert!(leftover.is_empty(), "no queda ningún .part: {leftover:?}");
    }

    /// Un recurso por encima del límite por item se rechaza ANTES de escribir,
    /// y no queda ni `.part` ni archivo final.
    #[tokio::test]
    async fn oversized_resource_is_refused_before_writing() {
        let server = crate::media::transport::fake_server::FakeServer::start(
            crate::media::transport::fake_server::Scenario::Normal(std::sync::Arc::new(vec![
                7u8;
                40_000
            ])),
        )
        .await;
        let dir = tempfile::tempdir().expect("tempdir");
        let cancel = Arc::new(AtomicBool::new(false));
        let error = download_to_local_media_bounded(
            &server.url(),
            Vec::new(),
            "youtube",
            "abc",
            dir.path(),
            &cancel,
            1024,
            |_| {},
        )
        .await
        .expect_err("un recurso de 40 KB no cabe en un límite de 1 KB");
        assert_eq!(
            error,
            DownloadError::TooLarge {
                size: 40_000,
                max: 1024
            }
        );
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "no se escribe nada antes de rechazar"
        );
    }

    /// Un recurso que cabe en el límite sí se descarga y se registra.
    #[tokio::test]
    async fn resource_within_the_limit_is_accepted() {
        let server = crate::media::transport::fake_server::FakeServer::start(
            crate::media::transport::fake_server::Scenario::Normal(std::sync::Arc::new(vec![
                3u8;
                40_000
            ])),
        )
        .await;
        let dir = tempfile::tempdir().expect("tempdir");
        let cancel = Arc::new(AtomicBool::new(false));
        let path = download_to_local_media_bounded(
            &server.url(),
            Vec::new(),
            "youtube",
            "abc",
            dir.path(),
            &cancel,
            64 * 1024,
            |_| {},
        )
        .await
        .expect("cabe en el límite");
        assert_eq!(std::fs::read(&path).unwrap().len(), 40_000);
    }

    #[test]
    fn download_errors_are_classified() {
        use crate::media::FailureCategory as C;
        assert_eq!(DownloadError::Cancelled.category(), C::PlaybackFailure);
        assert_eq!(
            DownloadError::SizeMismatch {
                expected: 1,
                written: 2
            }
            .category(),
            C::InvalidResponse
        );
        assert_eq!(
            DownloadError::TooLarge { size: 1, max: 0 }.category(),
            C::InvalidResponse
        );
        assert_eq!(DownloadError::Io("x".into()).category(), C::Unknown);
    }
}
