//! Persistencia de entidades y eventos en SQLite.
//!
//! Funciones de escritura/lectura sobre el pool de [`Db`], usadas por la capa
//! de Aplicación. No contienen lógica de negocio, solo SQL.

use std::collections::HashMap;

use anyhow::Result;
use chrono::NaiveDate;
use sqlx::sqlite::{Sqlite, SqliteQueryResult};
use sqlx::{Row, Transaction};

use crate::recommendation::signals::{PlayContext, PlaySignal, SignalKind};
use crate::recommendation::types::TrackAcousticProfile;

use crate::domain::playlist::{Playlist, PlaylistItem, PlaylistKind};
use crate::domain::{album::Album, artist::Artist, genre::Genre, source::Source, track::Track};

/// Convierte `"#rrggbb"` a `[r,g,b]` (fallback determinista si está corrupto).
fn parse_hex(hex: String) -> [u8; 3] {
    let bytes = hex.trim_start_matches('#').as_bytes();
    let nib = |i: usize| bytes.get(i).and_then(|b| (*b as char).to_digit(16));
    if let (Some(rh), Some(rl), Some(gh), Some(gl), Some(bh), Some(bl)) =
        (nib(0), nib(1), nib(2), nib(3), nib(4), nib(5))
    {
        return [
            ((rh << 4) | rl) as u8,
            ((gh << 4) | gl) as u8,
            ((bh << 4) | bl) as u8,
        ];
    }
    [20, 14, 26]
}

use super::db::Db;

/// Una entrada del historial de reproducción, con datos de display unidos.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub track_id: i64,
    pub played_at: String,
    pub source: Source,
    pub duration: Option<i64>,
    pub title: String,
    pub artist_name: Option<String>,
    /// Número total de reproducciones persistidas de esta canción.
    pub play_count: i64,
}

/// Resumen persistente de escucha para decorar cualquier copia de un track.
#[derive(Debug, Clone)]
pub struct TrackListeningStats {
    /// La misma clave estable usada por `Track::identifier()`.
    pub track_id: i64,
    pub key: String,
    pub artist_name: Option<String>,
    pub play_count: i64,
    pub last_played: String,
    pub recently_played: bool,
}

/// Una playlist local (no sincronizada con YouTube).
#[derive(Debug, Clone)]
pub struct PlaylistRow {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub track_count: i64,
    /// Tipo de la playlist (User/System). L1K3D es `System` y está protegida.
    pub kind: PlaylistKind,
}

/// Vínculo de pertenencia de un track: en qué playlists está y de qué tipo
/// (para resolver el estado "liked" y pintar etiquetas sin N+1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistMembership {
    pub playlist_id: i64,
    pub playlist_name: String,
    pub kind: PlaylistKind,
    pub track_id: i64,
}

/// Paleta persistida de un track (tres colores dominantes del artwork).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtworkPalette {
    pub track_id: i64,
    pub primary: [u8; 3],
    pub secondary: [u8; 3],
    pub accent: [u8; 3],
}

impl Db {
    /// Inserta (o actualiza) una canción y sus relaciones de forma atómica.
    /// Devuelve el `id` interno canónico del track.
    pub async fn upsert_track(
        &self,
        track: &Track,
        provider_ids: &HashMap<Source, String>,
    ) -> Result<i64> {
        let mut tx = self.pool().begin().await?;
        let id = upsert_track_inner(&mut tx, track, provider_ids).await?;
        tx.commit().await?;
        Ok(id)
    }

    /// Registra una reproducción en el historial.
    pub async fn record_history(
        &self,
        track_id: i64,
        source: Source,
        duration: Option<i64>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO history (track_id, played_at, source, duration) \
             VALUES (?1, datetime('now'), ?2, ?3)",
        )
        .bind(track_id)
        .bind(source.as_str())
        .bind(duration)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Devuelve las últimas `limit` reproducciones, ordenadas por fecha.
    pub async fn recent_history(&self, limit: i64) -> Result<Vec<HistoryEntry>> {
        let rows = sqlx::query(
            "SELECT h.track_id, h.played_at, h.source, h.duration, t.title, \
                    a.name AS artist_name, \
                    COUNT(h.id) OVER (PARTITION BY h.track_id) AS play_count \
             FROM history h \
             JOIN tracks t ON t.id = h.track_id \
             LEFT JOIN track_artists ta ON ta.track_id = t.id AND ta.position = 0 \
             LEFT JOIN artists a ON a.id = ta.artist_id \
             ORDER BY h.played_at DESC, h.id DESC \
             LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(self.pool())
        .await?;

        Ok(rows
            .iter()
            .map(|r| HistoryEntry {
                track_id: r.get("track_id"),
                played_at: r.get("played_at"),
                source: r.get::<Option<String>, _>("source").as_deref().map_or(
                    Source::YouTube,
                    |s| match s {
                        "youtube" => Source::YouTube,
                        _ => Source::YouTube,
                    },
                ),
                duration: r.get("duration"),
                title: r.get("title"),
                artist_name: r.get("artist_name"),
                play_count: r.get("play_count"),
            })
            .collect())
    }

    /// Obtiene el perfil acústico de un track específico.
    pub async fn acoustic_profile_for_track(
        &self,
        track_id: i64,
    ) -> Result<Option<TrackAcousticProfile>> {
        let row = sqlx::query(
            "SELECT track_id, rms_mean, bass_mean, low_mid_mean, mid_mean, high_mid_mean, high_mean, \
                    spectral_centroid_mean, bpm_mean, bpm_variance, onset_mean, band_profile, \
                    frame_count \
             FROM track_acoustic_profiles \
             WHERE track_id = ?1",
        )
        .bind(track_id)
        .fetch_optional(self.pool())
        .await?;

        Ok(row.map(|r| TrackAcousticProfile {
            track_id: r.get("track_id"),
            rms_mean: r.get("rms_mean"),
            bass_mean: r.get("bass_mean"),
            low_mid_mean: r.get("low_mid_mean"),
            mid_mean: r.get("mid_mean"),
            high_mid_mean: r.get("high_mid_mean"),
            high_mean: r.get("high_mean"),
            spectral_centroid_mean: r.get("spectral_centroid_mean"),
            bpm_mean: r.get("bpm_mean"),
            bpm_variance: r.get("bpm_variance"),
            onset_mean: r.get("onset_mean"),
            band_profile: {
                let s: String = r.get("band_profile");
                band_profile_from_str(&s)
            },
            frame_count: r.get("frame_count"),
        }))
    }

    /// Guarda (o acumula) el perfil acústico agregado de un track. FASE 8:
    /// el análisis en vivo produce un `TrackAcousticProfile` promedio al
    /// terminar una reproducción; aquí se persiste para poder comparar tracks
    /// sin depender de un análisis en vivo posterior.
    pub async fn save_acoustic_profile(&self, profile: &TrackAcousticProfile) -> Result<()> {
        let band = format!(
            "[{},{},{},{},{}]",
            profile.band_profile[0],
            profile.band_profile[1],
            profile.band_profile[2],
            profile.band_profile[3],
            profile.band_profile[4]
        );
        sqlx::query(
            "INSERT INTO track_acoustic_profiles \
                (track_id, rms_mean, bass_mean, low_mid_mean, mid_mean, high_mid_mean, \
                 high_mean, spectral_centroid_mean, bpm_mean, bpm_variance, onset_mean, \
                 band_profile, frame_count) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13) \
             ON CONFLICT(track_id) DO UPDATE SET \
                 rms_mean = excluded.rms_mean, \
                 bass_mean = excluded.bass_mean, \
                 low_mid_mean = excluded.low_mid_mean, \
                 mid_mean = excluded.mid_mean, \
                 high_mid_mean = excluded.high_mid_mean, \
                 high_mean = excluded.high_mean, \
                 spectral_centroid_mean = excluded.spectral_centroid_mean, \
                 bpm_mean = excluded.bpm_mean, \
                 bpm_variance = excluded.bpm_variance, \
                 onset_mean = excluded.onset_mean, \
                 band_profile = excluded.band_profile, \
                 frame_count = excluded.frame_count",
        )
        .bind(profile.track_id)
        .bind(profile.rms_mean)
        .bind(profile.bass_mean)
        .bind(profile.low_mid_mean)
        .bind(profile.mid_mean)
        .bind(profile.high_mid_mean)
        .bind(profile.high_mean)
        .bind(profile.spectral_centroid_mean)
        .bind(profile.bpm_mean)
        .bind(profile.bpm_variance)
        .bind(profile.onset_mean)
        .bind(band)
        .bind(profile.frame_count)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Todos los perfiles acústicos persistidos, indexados por track_id. Es lo
    /// que alimenta la comparación acústica del ranking local (FASE 8/9): se
    /// carga una vez por sesión de recomendación.
    pub async fn all_acoustic_profiles(&self) -> Result<HashMap<i64, TrackAcousticProfile>> {
        let rows = sqlx::query(
            "SELECT track_id, rms_mean, bass_mean, low_mid_mean, mid_mean, high_mid_mean, high_mean, \
                    spectral_centroid_mean, bpm_mean, bpm_variance, onset_mean, band_profile, \
                    frame_count \
             FROM track_acoustic_profiles",
        )
        .fetch_all(self.pool())
        .await?;
        let mut map = HashMap::with_capacity(rows.len());
        for row in rows {
            let p = TrackAcousticProfile {
                track_id: row.get("track_id"),
                rms_mean: row.get("rms_mean"),
                bass_mean: row.get("bass_mean"),
                low_mid_mean: row.get("low_mid_mean"),
                mid_mean: row.get("mid_mean"),
                high_mid_mean: row.get("high_mid_mean"),
                high_mean: row.get("high_mean"),
                spectral_centroid_mean: row.get("spectral_centroid_mean"),
                bpm_mean: row.get("bpm_mean"),
                bpm_variance: row.get("bpm_variance"),
                onset_mean: row.get("onset_mean"),
                band_profile: {
                    let s: String = row.get("band_profile");
                    band_profile_from_str(&s)
                },
                frame_count: row.get("frame_count"),
            };
            map.insert(p.track_id, p);
        }
        Ok(map)
    }

    /// Frecuencia y última escucha de cada track. La agregación ocurre en
    /// SQLite, no durante el render de la TUI.
    pub async fn listening_stats(&self) -> Result<Vec<TrackListeningStats>> {
        let rows = sqlx::query(
            "SELECT h.track_id, t.title, a.name AS artist_name, p.youtube_id, \
                    COUNT(h.id) AS play_count, MAX(h.played_at) AS last_played, \
                    MAX(h.played_at) >= datetime('now', '-7 days') AS recently_played \
             FROM history h \
             JOIN tracks t ON t.id = h.track_id \
             LEFT JOIN track_artists ta ON ta.track_id = t.id AND ta.position = 0 \
             LEFT JOIN artists a ON a.id = ta.artist_id \
             LEFT JOIN providers p ON p.track_id = t.id \
             GROUP BY h.track_id, t.title, a.name, p.youtube_id",
        )
        .fetch_all(self.pool())
        .await?;

        Ok(rows
            .iter()
            .map(|r| {
                let track_id: i64 = r.get("track_id");
                let title: String = r.get("title");
                let artist: Option<String> = r.get("artist_name");
                let external: Option<String> = r.get("youtube_id");
                TrackListeningStats {
                    track_id,
                    key: external.unwrap_or_else(|| {
                        format!("{}|{}", title, artist.clone().unwrap_or_default())
                    }),
                    artist_name: artist,
                    play_count: r.get("play_count"),
                    last_played: r.get("last_played"),
                    recently_played: r.get::<i64, _>("recently_played") != 0,
                }
            })
            .collect())
    }

    /// Búsqueda local por título de canción o nombre de artista. Incluye el id
    /// externo de YouTube para poder reproducir/enriquecer con recomendados.
    pub async fn search_local(&self, query: &str, limit: i64) -> Result<Vec<Track>> {
        let rows = sqlx::query(
            "SELECT t.id, t.title, t.duration, t.isrc, a.name AS artist_name, \
                    al.title AS album_title, p.youtube_id AS youtube_id \
             FROM tracks t \
             LEFT JOIN track_artists ta ON ta.track_id = t.id AND ta.position = 0 \
             LEFT JOIN artists a ON a.id = ta.artist_id \
             LEFT JOIN albums al ON al.id = t.album_id \
             LEFT JOIN providers p ON p.track_id = t.id \
             WHERE t.title LIKE '%' || ?1 || '%' \
                OR a.name LIKE '%' || ?1 || '%' \
             ORDER BY t.title LIMIT ?2",
        )
        .bind(query)
        .bind(limit)
        .fetch_all(self.pool())
        .await?;

        Ok(rows.iter().map(row_to_track).collect())
    }

    /// Catálogo completo de tracks conocidos, con artistas, álbum y géneros.
    /// Es la fuente de candidatos del ranking local (FASE 8/9): permite ofrecer
    /// recomendaciones basadas en el gusto del usuario SIN depender de la red.
    pub async fn all_tracks(&self) -> Result<Vec<Track>> {
        let rows = sqlx::query(
            "SELECT t.id, t.title, t.duration, t.isrc, \
                    al.title AS album_title, al.id AS album_id, al.release_date, \
                    p.youtube_id AS youtube_id, \
                    a.name AS artist_name, a.id AS artist_id \
             FROM tracks t \
             LEFT JOIN track_artists ta ON ta.track_id = t.id AND ta.position = 0 \
             LEFT JOIN artists a ON a.id = ta.artist_id \
             LEFT JOIN albums al ON al.id = t.album_id \
             LEFT JOIN providers p ON p.track_id = t.id \
             ORDER BY t.id",
        )
        .fetch_all(self.pool())
        .await?;

        let genre_rows = sqlx::query(
            "SELECT track_id, name \
             FROM tags \
             ORDER BY track_id, name",
        )
        .fetch_all(self.pool())
        .await?;
        let mut genres_by_track: HashMap<i64, Vec<String>> = HashMap::new();
        for r in &genre_rows {
            let track_id: i64 = r.get("track_id");
            let name: String = r.get("name");
            let set = genres_by_track.entry(track_id).or_default();
            if !set.contains(&name) {
                set.push(name);
            }
        }

        Ok(rows
            .iter()
            .map(|r| {
                let id: i64 = r.get("id");
                let mut artist = Artist::new(
                    r.get::<Option<String>, _>("artist_name")
                        .unwrap_or_default(),
                    None,
                    None,
                    None,
                );
                if artist.name.is_empty() {
                    artist = Artist::new("Unknown".to_string(), None, None, None);
                }
                let mut track = Track::new(r.get("title"), vec![artist], Source::YouTube);
                track.id = id;
                track.duration = r
                    .get::<Option<i64>, _>("duration")
                    .map(|ms| std::time::Duration::from_millis(ms as u64));
                track.isrc = r.get("isrc");
                track.external_id = r.get::<Option<String>, _>("youtube_id");
                track.album = match r.get::<Option<String>, _>("album_title") {
                    Some(title) => {
                        let mut album = Album::new(title, None, None, None);
                        album.id = r.get::<i64, _>("album_id");
                        album.release_date = r
                            .get::<Option<String>, _>("release_date")
                            .and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok());
                        Some(album)
                    }
                    None => None,
                };
                track.genres = genres_by_track
                    .get(&id)
                    .map(|names| names.iter().map(|n| Genre::new(n.clone())).collect())
                    .unwrap_or_default();
                track
            })
            .collect())
    }

    // ------------------------------------------------------------- playlists

    /// Stream de audio persistido (URL de googlevideo) para `video_id`, si
    /// existe y no supera `max_age_secs` de antigüedad. La URL la re-verifica
    /// quien la consuma (GET rápido) antes de usarla: el TTL solo poda.
    pub async fn cached_stream_url(
        &self,
        video_id: &str,
        max_age_secs: i64,
    ) -> Result<Option<String>> {
        let url = sqlx::query_scalar::<_, String>(
            "SELECT url FROM stream_cache \
             WHERE video_id = ?1 AND created_at > datetime('now', '-' || ?2 || ' seconds')",
        )
        .bind(video_id)
        .bind(max_age_secs)
        .fetch_optional(self.pool())
        .await?;
        Ok(url)
    }

    /// Guarda (o refresca) el stream resuelto de un video para las siguientes
    /// reproducciones. Fallos de escritura no son críticos: la resolución
    /// normal sigue funcionando.
    pub async fn cache_stream(&self, video_id: &str, url: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO stream_cache (video_id, url, created_at) \
             VALUES (?1, ?2, datetime('now')) \
             ON CONFLICT(video_id) DO UPDATE \
             SET url = excluded.url, created_at = excluded.created_at",
        )
        .bind(video_id)
        .bind(url)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Elimina el stream persistido de un video (invalidación puntual).
    pub async fn delete_cached_stream(&self, video_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM stream_cache WHERE video_id = ?1")
            .bind(video_id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    /// Vacía la caché de streams (invalidación por proveedor).
    pub async fn clear_stream_cache(&self) -> Result<()> {
        sqlx::query("DELETE FROM stream_cache")
            .execute(self.pool())
            .await?;
        Ok(())
    }

    /// Crea una playlist local. `Err` si ya existe un nombre igual (el nombre
    /// reservado de L1K3D nunca se puede crear: desencadena el conflicto de
    /// unicidad porque L1K3D ya existe).
    pub async fn create_playlist(&self, name: &str) -> Result<i64> {
        if name.eq_ignore_ascii_case(Playlist::LIKED_NAME) {
            anyhow::bail!("«{name}» es un nombre reservado de la app");
        }
        let result = sqlx::query(
            "INSERT INTO playlists (name, kind, created_at, updated_at) \
             VALUES (?1, 0, datetime('now'), datetime('now')) \
             ON CONFLICT(name) DO NOTHING",
        )
        .bind(name)
        .execute(self.pool())
        .await?;
        if result.rows_affected() == 0 {
            anyhow::bail!("ya existe una playlist llamada «{name}»");
        }
        Ok(result.last_insert_rowid())
    }

    /// La identidad de un playlist de sistema es inmutable (invariante del
    /// dominio): renombrar L1K3D es un error, no un no-op silencioso.
    pub async fn rename_playlist(&self, id: i64, name: &str) -> Result<()> {
        if self.is_system_playlist(id).await? {
            anyhow::bail!("no se puede renombrar una playlist de sistema (L1K3D)");
        }
        sqlx::query("UPDATE playlists SET name = ?2, updated_at = datetime('now') WHERE id = ?1")
            .bind(id)
            .bind(name)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    pub async fn delete_playlist(&self, id: i64) -> Result<()> {
        if self.is_system_playlist(id).await? {
            anyhow::bail!("no se puede eliminar una playlist de sistema (L1K3D)");
        }
        sqlx::query("DELETE FROM playlists WHERE id = ?1")
            .bind(id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    /// `true` si la playlist es de sistema (L1K3D). Guarda de la capa de
    /// aplicación; la BD la duplica con triggers `BEFORE DELETE/UPDATE`.
    pub async fn is_system_playlist(&self, id: i64) -> Result<bool> {
        let row = sqlx::query("SELECT kind FROM playlists WHERE id = ?1")
            .bind(id)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.map(|r| r.get::<i64, _>("kind") == 1).unwrap_or(false))
    }

    pub async fn list_playlists(&self) -> Result<Vec<PlaylistRow>> {
        let rows = sqlx::query(
            "SELECT p.id, p.name, p.created_at, p.kind, p.updated_at, \
                    (SELECT COUNT(*) FROM playlist_tracks pt WHERE pt.playlist_id = p.id) AS track_count \
             FROM playlists p \
             ORDER BY p.kind DESC, p.name",
        )
        .fetch_all(self.pool())
        .await?;
        Ok(rows
            .iter()
            .map(|r| PlaylistRow {
                id: r.get("id"),
                name: r.get("name"),
                created_at: r.get("created_at"),
                track_count: r.get("track_count"),
                kind: PlaylistKind::from_i64(r.get("kind")),
            })
            .collect())
    }

    /// Metadata completa de una playlist (incluida la de sistema).
    pub async fn get_playlist(&self, playlist_id: i64) -> Result<Option<PlaylistRow>> {
        let row = sqlx::query(
            "SELECT p.id, p.name, p.created_at, p.kind, p.updated_at, \
                    (SELECT COUNT(*) FROM playlist_tracks pt WHERE pt.playlist_id = p.id) AS track_count \
             FROM playlists p WHERE p.id = ?1",
        )
        .bind(playlist_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|r| PlaylistRow {
            id: r.get("id"),
            name: r.get("name"),
            created_at: r.get("created_at"),
            track_count: r.get("track_count"),
            kind: PlaylistKind::from_i64(r.get("kind")),
        }))
    }

    /// Duración total de una playlist (suma de las duraciones conocidas), para
    /// mostrar ", N canciones · h:mm:ss" en el listado.
    pub async fn playlist_total_duration(&self, playlist_id: i64) -> Result<std::time::Duration> {
        let row = sqlx::query(
            "SELECT COALESCE(SUM(t.duration), 0) AS total FROM playlist_tracks pt \
             JOIN tracks t ON t.id = pt.track_id WHERE pt.playlist_id = ?1",
        )
        .bind(playlist_id)
        .fetch_one(self.pool())
        .await?;
        Ok(std::time::Duration::from_millis(
            row.get::<i64, _>("total").max(0) as u64,
        ))
    }

    /// Ordena la membership de una playlist en orden explícito (`tracks` con
    /// los `track_id` en el orden deseado). Se usa al reordenar desde la UI.
    pub async fn reorder_playlist(&self, playlist_id: i64, order: &[i64]) -> Result<()> {
        let mut tx = self.pool().begin().await?;
        // El track pedido DEBE estar en la playlist: se valida contra las filas
        // existentes ANTES de reescribir (no se inventan posiciones nuevas).
        let existing: Vec<i64> =
            sqlx::query_scalar("SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1")
                .bind(playlist_id)
                .fetch_all(&mut *tx)
                .await?;
        let existing_set: std::collections::HashSet<i64> = existing.iter().copied().collect();
        if order.iter().any(|t| !existing_set.contains(t)) {
            anyhow::bail!("el reorden contiene tracks ajenos a la playlist");
        }
        for (pos, track_id) in order.iter().enumerate() {
            sqlx::query(
                "UPDATE playlist_tracks SET position = ?3 WHERE playlist_id = ?1 AND track_id = ?2",
            )
            .bind(playlist_id)
            .bind(track_id)
            .bind(pos as i64)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Mueve UN track a otra posición (el resto se reordena consecutivamente).
    /// Si `to` está fuera de rango, se satura al último.
    pub async fn move_playlist_track(
        &self,
        playlist_id: i64,
        track_id: i64,
        to_position: usize,
    ) -> Result<()> {
        let mut current: Vec<i64> = sqlx::query_scalar(
            "SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position",
        )
        .bind(playlist_id)
        .fetch_all(self.pool())
        .await?;
        let from = current
            .iter()
            .position(|t| *t == track_id)
            .ok_or_else(|| anyhow::anyhow!("el track no está en esta playlist"))?;
        let track = current.remove(from);
        let to = to_position.min(current.len());
        current.insert(to, track);
        self.reorder_playlist(playlist_id, &current).await
    }

    pub async fn playlist_tracks(&self, playlist_id: i64) -> Result<Vec<Track>> {
        let rows = sqlx::query(
            "SELECT t.id, t.title, t.duration, t.isrc, a.name AS artist_name, \
                    al.title AS album_title, p.youtube_id AS youtube_id \
             FROM playlist_tracks pt \
             JOIN tracks t ON t.id = pt.track_id \
             LEFT JOIN track_artists ta ON ta.track_id = t.id AND ta.position = 0 \
             LEFT JOIN artists a ON a.id = ta.artist_id \
             LEFT JOIN albums al ON al.id = t.album_id \
             LEFT JOIN providers p ON p.track_id = t.id \
             WHERE pt.playlist_id = ?1 \
             ORDER BY pt.position",
        )
        .bind(playlist_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(row_to_track).collect())
    }

    /// ¿Contiene la playlist este track? (lookup puntual, sin cargar la lista).
    pub async fn playlist_contains(&self, playlist_id: i64, track_id: i64) -> Result<bool> {
        let row =
            sqlx::query("SELECT 1 FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = ?2")
                .bind(playlist_id)
                .bind(track_id)
                .fetch_optional(self.pool())
                .await?;
        Ok(row.is_some())
    }

    pub async fn add_to_playlist(&self, playlist_id: i64, track_id: i64) -> Result<()> {
        sqlx::query(
            "INSERT OR IGNORE INTO playlist_tracks (playlist_id, track_id, position, added_at) \
             VALUES (?1, ?2, \
                (SELECT COALESCE(MAX(position), 0) + 1 FROM playlist_tracks WHERE playlist_id = ?1), \
                datetime('now'))",
        )
        .bind(playlist_id)
        .bind(track_id)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn remove_from_playlist(&self, playlist_id: i64, track_id: i64) -> Result<()> {
        sqlx::query("DELETE FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = ?2")
            .bind(playlist_id)
            .bind(track_id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    // ---------------------------------------------------------- L1K3D
    // L1K3D es una playlist normal de sistema (no una excepción por track):
    // "liked" == pertenece a L1K3D. Todo se resuelve con las operaciones de
    // membership de arriba; estos helpers solo dan azúcar semántica.

    fn l1k3d_query() -> &'static str {
        "SELECT id FROM playlists WHERE kind = 1 LIMIT 1"
    }

    /// Id de la playlist de sistema L1K3D (creada por la migración 0009).
    /// `None` si la base aún no migró (no debería ocurrir en producción).
    pub async fn l1k3d_id(&self) -> Result<Option<i64>> {
        let row = sqlx::query(Self::l1k3d_query())
            .fetch_optional(self.pool())
            .await?;
        Ok(row.map(|r| r.get("id")))
    }

    /// `true` si el track está en L1K3D.
    pub async fn is_liked(&self, track_id: i64) -> Result<bool> {
        let Some(id) = self.l1k3d_id().await? else {
            return Ok(false);
        };
        self.playlist_contains(id, track_id).await
    }

    /// Marca al track como liked (`liked=true` añade a L1K3D; `false` quita).
    /// La operación activa los triggers de touch (actualiza `updated_at`).
    pub async fn set_liked(&self, track_id: i64, liked: bool) -> Result<()> {
        let id = self
            .l1k3d_id()
            .await?
            .ok_or_else(|| anyhow::anyhow!("L1K3D no existe: aplica la migración 0009"))?;
        if liked {
            self.add_to_playlist(id, track_id).await
        } else {
            self.remove_from_playlist(id, track_id).await
        }
    }

    /// Todos los `track_id` actualmente en L1K3D (estado "liked" de la app).
    pub async fn liked_track_ids(&self) -> Result<Vec<i64>> {
        let Some(id) = self.l1k3d_id().await? else {
            return Ok(Vec::new());
        };
        let rows = sqlx::query_scalar(
            "SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position",
        )
        .bind(id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    /// Tracks completos de L1K3D (la propia playlist, editable como cualquiera
    /// salvo identidad).
    pub async fn liked_tracks(&self) -> Result<Vec<Track>> {
        let Some(id) = self.l1k3d_id().await? else {
            return Ok(Vec::new());
        };
        self.playlist_tracks(id).await
    }

    // ------------------------------------------------- membresía por track
    /// Devuelve, de TODOS los tracks dados, en qué playlists están (y de qué
    /// tipo). UNA consulta para `track_ids` enteros: evita el N+1 de preguntar
    /// por cada canción si está en playlists al pintar listas.
    pub async fn get_playlist_memberships(
        &self,
        track_ids: &[i64],
    ) -> Result<Vec<PlaylistMembership>> {
        if track_ids.is_empty() {
            return Ok(Vec::new());
        }
        // SQLite limita bindings a 999: se trocea en lotes seguros.
        const CHUNK: usize = 450;
        let mut out = Vec::new();
        for chunk in track_ids.chunks(CHUNK) {
            let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "SELECT pt.playlist_id AS pid, p.name AS pl_name, p.kind AS pl_kind, \
                        pt.track_id AS tid \
                 FROM playlist_tracks pt \
                 JOIN playlists p ON p.id = pt.playlist_id \
                 WHERE pt.track_id IN ({placeholders})"
            );
            let mut q = sqlx::query(&sql);
            for t in chunk {
                q = q.bind(t);
            }
            let rows = q.fetch_all(self.pool()).await?;
            for r in rows {
                out.push(PlaylistMembership {
                    playlist_id: r.get("pid"),
                    playlist_name: r.get("pl_name"),
                    kind: PlaylistKind::from_i64(r.get("pl_kind")),
                    track_id: r.get("tid"),
                });
            }
        }
        Ok(out)
    }

    // ------------------------------------------------- membership ordenada
    /// Elementos (track_id + added_at) de una playlist en orden explícito.
    pub async fn playlist_items(&self, playlist_id: i64) -> Result<Vec<PlaylistItem>> {
        let rows = sqlx::query(
            "SELECT playlist_id, track_id, position, added_at \
             FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position",
        )
        .bind(playlist_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows
            .iter()
            .map(|r| PlaylistItem {
                playlist_id: r.get("playlist_id"),
                track_id: r.get("track_id"),
                position: r.get("position"),
                added_at: r.get("added_at"),
            })
            .collect())
    }

    // ------------------------------------------------- artwork palette
    /// Persiste la paleta de tres colores dominantes del artwork de un track.
    /// Se escribe UNA vez al decodificar la miniatura (no durante el render).
    pub async fn set_track_palette(&self, track_id: i64, palette: [[u8; 3]; 3]) -> Result<()> {
        let hex = |c: [u8; 3]| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
        sqlx::query(
            "INSERT INTO artwork_palettes (track_id, primary_hex, secondary_hex, accent_hex, updated_at) \
             VALUES (?1, ?2, ?3, ?4, datetime('now')) \
             ON CONFLICT(track_id) DO UPDATE SET primary_hex = excluded.primary_hex, \
                secondary_hex = excluded.secondary_hex, accent_hex = excluded.accent_hex, \
                updated_at = excluded.updated_at",
        )
        .bind(track_id)
        .bind(hex(palette[0]))
        .bind(hex(palette[1]))
        .bind(hex(palette[2]))
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Paleta persistida de un track, si existe.
    pub async fn get_track_palette(&self, track_id: i64) -> Result<Option<ArtworkPalette>> {
        let row = sqlx::query(
            "SELECT track_id, primary_hex, secondary_hex, accent_hex \
             FROM artwork_palettes WHERE track_id = ?1",
        )
        .bind(track_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|r| ArtworkPalette {
            track_id: r.get("track_id"),
            primary: parse_hex(r.get::<String, _>("primary_hex")),
            secondary: parse_hex(r.get::<String, _>("secondary_hex")),
            accent: parse_hex(r.get::<String, _>("accent_hex")),
        }))
    }

    // ------------------------------------------------------- portada override

    /// Guarda una imagen de portada local que reemplaza a la de YouTube.
    pub async fn set_artwork_override(&self, track_id: i64, image: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO artwork_overrides (track_id, image, updated_at) \
             VALUES (?1, ?2, datetime('now')) \
             ON CONFLICT(track_id) DO UPDATE SET image = excluded.image, \
               updated_at = excluded.updated_at",
        )
        .bind(track_id)
        .bind(image)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn get_artwork_override(&self, track_id: i64) -> Result<Option<String>> {
        let row = sqlx::query("SELECT image FROM artwork_overrides WHERE track_id = ?1")
            .bind(track_id)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.map(|r| r.get("image")))
    }

    // ------------------------------------------------------------- letras

    /// Id interno canónico de la canción que tiene este `external_id`
    /// (`youtube_id`). `None` si la canción aún no se guardó. Permite buscar en
    /// la caché de letras aunque el track en mano llegara sin su `id` interno
    /// (p. ej. recién buscado y reproducido por primera vez).
    pub async fn internal_id_for_external(&self, external_id: &str) -> Result<Option<i64>> {
        let row = sqlx::query("SELECT track_id FROM providers WHERE youtube_id = ?1")
            .bind(external_id)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.map(|r| r.get("track_id")))
    }

    /// Guarda una letra sincronizada (LRC) cacheada de un track (LRCLIB). La
    /// caché de letras solo guarda sincronizadas: la letra plana heredada no
    /// tiene consumidores y no se escribe (ni se sirve) como karaoke.
    pub async fn cache_synced_lyrics(&self, track_id: i64, body: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO lyrics_cache (track_id, body, footer, synced, fetched_at) \
             VALUES (?1, ?2, NULL, 1, datetime('now')) \
             ON CONFLICT(track_id) DO UPDATE SET body = excluded.body, \
               footer = NULL, synced = 1, fetched_at = excluded.fetched_at",
        )
        .bind(track_id)
        .bind(body)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Letra sincronizada (LRC) cacheada de un track, si existe. Las filas
    /// planas (legado de la implementación antigua) no se devuelven.
    pub async fn get_synced_lyrics(&self, track_id: i64) -> Result<Option<String>> {
        let row = sqlx::query("SELECT body FROM lyrics_cache WHERE track_id = ?1 AND synced = 1")
            .bind(track_id)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.map(|r| r.get("body")))
    }

    // -------------------------------------------------------- play signals

    /// Registra una señal de interacción (play, completed, skip, like, ...).
    /// Es el modelo de métricas local (FASE 11): cada fila es un evento
    /// autónomo con su contexto para poder construir el perfil sin mezclar
    /// señales.
    pub async fn record_signal(
        &self,
        track_id: i64,
        signal: SignalKind,
        context: PlayContext,
        duration_ms: Option<i64>,
        recomm_id: Option<i64>,
        track_duration_ms: Option<i64>,
    ) -> Result<i64> {
        let result = sqlx::query(
            "INSERT INTO play_signals \
                (track_id, signal, context, at, duration_ms, recomm_id, track_duration_ms) \
             VALUES (?1, ?2, ?3, datetime('now'), ?4, ?5, ?6)",
        )
        .bind(track_id)
        .bind(signal.as_str())
        .bind(context.as_str())
        .bind(duration_ms)
        .bind(recomm_id)
        .bind(track_duration_ms)
        .execute(self.pool())
        .await?;
        Ok(result.last_insert_rowid())
    }

    /// Todas las señales de interacción registradas, más recientes primero.
    pub async fn all_signals(&self, limit: i64) -> Result<Vec<PlaySignal>> {
        let rows = sqlx::query(
            "SELECT id, track_id, signal, context, at, duration_ms, recomm_id, track_duration_ms \
             FROM play_signals ORDER BY at DESC, id DESC LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(signal_to_playsignal).collect())
    }

    /// Señales de un track concreto (para evaluar su afinidad negativa/skip).
    pub async fn signals_for_track(&self, track_id: i64) -> Result<Vec<PlaySignal>> {
        let rows = sqlx::query(
            "SELECT id, track_id, signal, context, at, duration_ms, recomm_id, track_duration_ms \
             FROM play_signals WHERE track_id = ?1 ORDER BY at DESC, id DESC",
        )
        .bind(track_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(signal_to_playsignal).collect())
    }

    // ------------------------------------------------------------- kv saludo
    /// Lee una clave del kv genérico (`0010`). `None` si no existe o está vacía.
    pub async fn kv_get(&self, key: &str) -> Result<Option<String>> {
        let row: Option<String> = sqlx::query_scalar("SELECT value FROM kv WHERE key = ?1")
            .bind(key)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.filter(|v| !v.is_empty()))
    }

    /// Escribe una clave del kv genérico (upsert).
    pub async fn kv_set(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO kv (key, value, updated_at) VALUES (?1, ?2, datetime('now')) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, \
                updated_at = excluded.updated_at",
        )
        .bind(key)
        .bind(value)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Nombre elegido por el usuario para el saludo (`display_name`).
    pub async fn display_name(&self) -> Result<Option<String>> {
        self.kv_get("display_name").await
    }

    /// Guarda el nombre del usuario (recortado a 32 chars, sin control).
    pub async fn set_display_name(&self, name: &str) -> Result<()> {
        let clean: String = name
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .take(32)
            .collect();
        self.kv_set("display_name", clean.trim()).await
    }

    /// Última fecha local con saludo mostrado (`YYYY-MM-DD`).
    pub async fn last_greeting_date(&self) -> Result<Option<String>> {
        self.kv_get("last_greeting_date").await
    }

    /// Marca el saludo de hoy como mostrado.
    pub async fn mark_greeting_shown(&self, today: &str) -> Result<()> {
        self.kv_set("last_greeting_date", today).await
    }
}

/// Parsea la representación textual "[r1,r2,r3,r4,r5]" del perfil de bandas a
/// un array `[f32; 5]`. Devuelve ceros si no se puede interpretar.
fn band_profile_from_str(s: &str) -> [f32; 5] {
    let s = s.trim();
    if !(s.starts_with('[') && s.ends_with(']')) {
        return [0.0f32; 5];
    }
    let inner = &s[1..s.len() - 1];
    let mut arr = [0.0f32; 5];
    let mut i = 0;
    for part in inner.split(',') {
        if i >= 5 {
            break;
        }
        if let Ok(val) = part.trim().parse::<f32>() {
            arr[i] = val;
            i += 1;
        }
    }
    arr
}

fn signal_to_playsignal(row: &sqlx::sqlite::SqliteRow) -> PlaySignal {
    let (signal, context) = {
        let s: String = row.get("signal");
        let c: Option<String> = row.get("context");
        (
            SignalKind::parse(&s).unwrap_or(SignalKind::Play),
            c.and_then(|x| PlayContext::parse(&x))
                .unwrap_or(PlayContext::Manual),
        )
    };
    PlaySignal {
        id: row.get("id"),
        track_id: row.get("track_id"),
        signal,
        context,
        at: row.get("at"),
        duration_ms: row.get("duration_ms"),
        recomm_id: row.get("recomm_id"),
        track_duration_ms: row.get("track_duration_ms"),
    }
}

async fn upsert_track_inner(
    tx: &mut Transaction<'_, Sqlite>,
    track: &Track,
    provider_ids: &HashMap<Source, String>,
) -> Result<i64> {
    // El `youtube_id` es la identidad canónica de una canción. Antes se
    // insertaba siempre y una segunda reproducción chocaba con el UNIQUE de
    // providers; por ello no había frecuencia persistente fiable.
    if let Some(youtube_id) = provider_ids.get(&Source::YouTube) {
        if let Some(row) = sqlx::query("SELECT track_id FROM providers WHERE youtube_id = ?1")
            .bind(youtube_id)
            .fetch_optional(&mut **tx)
            .await?
        {
            let track_id: i64 = row.get("track_id");
            sqlx::query(
                "UPDATE tracks SET duration = COALESCE(?2, duration), \
                 isrc = COALESCE(?3, isrc) WHERE id = ?1",
            )
            .bind(track_id)
            .bind(track.duration.map(|d| d.as_millis() as i64))
            .bind(&track.isrc)
            .execute(&mut **tx)
            .await?;
            return Ok(track_id);
        }
    }
    let album_id = match &track.album {
        Some(album) => Some(get_or_create_album(tx, album).await?),
        None => None,
    };

    let mut artist_ids = Vec::new();
    for artist in &track.artists {
        artist_ids.push(get_or_create_artist(tx, artist).await?);
    }

    let result =
        sqlx::query("INSERT INTO tracks (title, duration, isrc, album_id) VALUES (?1, ?2, ?3, ?4)")
            .bind(&track.title)
            .bind(track.duration.map(|d| d.as_millis() as i64))
            .bind(&track.isrc)
            .bind(album_id)
            .execute(&mut **tx)
            .await?;
    let track_id = result.last_insert_rowid();

    for (position, artist_id) in artist_ids.into_iter().enumerate() {
        sqlx::query(
            "INSERT OR IGNORE INTO track_artists (track_id, artist_id, position) \
             VALUES (?1, ?2, ?3)",
        )
        .bind(track_id)
        .bind(artist_id)
        .bind(position as i64)
        .execute(&mut **tx)
        .await?;
    }

    for genre in &track.genres {
        let genre_id = get_or_create_genre(tx, &genre.name).await?;
        sqlx::query("INSERT OR IGNORE INTO track_genres (track_id, genre_id) VALUES (?1, ?2)")
            .bind(track_id)
            .bind(genre_id)
            .execute(&mut **tx)
            .await?;
        sqlx::query("INSERT OR IGNORE INTO tags (track_id, name, source) VALUES (?1, ?2, ?3)")
            .bind(track_id)
            .bind(&genre.name)
            .bind(track.source.as_str())
            .execute(&mut **tx)
            .await?;
    }

    upsert_provider(tx, track_id, provider_ids).await?;

    Ok(track_id)
}

async fn get_or_create_album(tx: &mut Transaction<'_, Sqlite>, album: &Album) -> Result<i64> {
    let release_date = album.release_date.map(|d| d.format("%Y-%m-%d").to_string());

    if let Some(row) = sqlx::query(
        "SELECT id FROM albums \
         WHERE title = ?1 AND COALESCE(release_date, '') = COALESCE(?2, '') LIMIT 1",
    )
    .bind(&album.title)
    .bind(release_date.as_deref())
    .fetch_optional(&mut **tx)
    .await?
    {
        return Ok(row.get("id"));
    }

    let result: SqliteQueryResult = sqlx::query(
        "INSERT INTO albums (title, release_date, cover, label) VALUES (?1, ?2, ?3, ?4)",
    )
    .bind(&album.title)
    .bind(release_date.as_deref())
    .bind(&album.cover)
    .bind(&album.label)
    .execute(&mut **tx)
    .await?;
    Ok(result.last_insert_rowid())
}

async fn get_or_create_artist(tx: &mut Transaction<'_, Sqlite>, artist: &Artist) -> Result<i64> {
    if let Some(row) = sqlx::query("SELECT id FROM artists WHERE name = ?1 LIMIT 1")
        .bind(&artist.name)
        .fetch_optional(&mut **tx)
        .await?
    {
        return Ok(row.get("id"));
    }

    let result: SqliteQueryResult = sqlx::query(
        "INSERT INTO artists (name, country, biography, image) VALUES (?1, ?2, ?3, ?4)",
    )
    .bind(&artist.name)
    .bind(&artist.country)
    .bind(&artist.biography)
    .bind(&artist.image)
    .execute(&mut **tx)
    .await?;
    Ok(result.last_insert_rowid())
}

async fn get_or_create_genre(tx: &mut Transaction<'_, Sqlite>, name: &str) -> Result<i64> {
    let normalized = name.trim().to_lowercase();
    if let Some(row) = sqlx::query("SELECT id FROM genres WHERE name = ?1 LIMIT 1")
        .bind(&normalized)
        .fetch_optional(&mut **tx)
        .await?
    {
        return Ok(row.get("id"));
    }

    let result: SqliteQueryResult = sqlx::query("INSERT INTO genres (name) VALUES (?1)")
        .bind(&normalized)
        .execute(&mut **tx)
        .await?;
    Ok(result.last_insert_rowid())
}

async fn upsert_provider(
    tx: &mut Transaction<'_, Sqlite>,
    track_id: i64,
    provider_ids: &HashMap<Source, String>,
) -> Result<()> {
    let youtube_id = provider_ids.get(&Source::YouTube).cloned();

    sqlx::query(
        "INSERT INTO providers (track_id, youtube_id) \
         VALUES (?1, ?2) \
         ON CONFLICT(track_id) DO UPDATE SET \
           youtube_id = COALESCE(excluded.youtube_id, providers.youtube_id)",
    )
    .bind(track_id)
    .bind(youtube_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn row_to_track(row: &sqlx::sqlite::SqliteRow) -> Track {
    let mut artist = Artist::new(
        row.get::<Option<String>, _>("artist_name")
            .unwrap_or_default(),
        None,
        None,
        None,
    );
    if artist.name.is_empty() {
        artist = Artist::new("Unknown".to_string(), None, None, None);
    }
    let mut track = Track::new(row.get("title"), vec![artist], Source::YouTube);
    track.id = row.get("id");
    track.duration = row
        .get::<Option<i64>, _>("duration")
        .map(|ms| std::time::Duration::from_millis(ms as u64));
    track.isrc = row.get("isrc");
    track.external_id = row.get::<Option<String>, _>("youtube_id");
    track.album = row
        .get::<Option<String>, _>("album_title")
        .map(|title| Album::new(title, None, None, None));
    track
}

// ---------------------------------------------------------------------
// Caché fría de resoluciones: adaptador SQLite del puerto
// `media::ResolutionCache` (DIP: Infraestructura implementa el puerto).
// ---------------------------------------------------------------------

/// Antigüedad máxima de una URL persistida. La URL se re-verifica en vivo
/// antes de usarse (validador del resolver); este TTL solo poda entradas
/// viejas: las URLs de googlevideo viven horas.
const STREAM_CACHE_MAX_AGE_SECS: i64 = 6 * 60 * 60;

/// Capa FRÍA de la caché de resoluciones sobre SQLite.
///
/// Solo persiste la URI: sus entradas nacen "desnudas" (sin cabeceras ni
/// metadatos) y el validador en vivo del resolver debe repararlas antes de
/// servirse. La poda es perezosa, por edad al leer; `expiring_within` devuelve
/// vacío por diseño.
#[derive(Debug, Clone)]
pub struct DbResolutionCache {
    db: Db,
}

impl DbResolutionCache {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait::async_trait]
impl crate::media::ResolutionCache for DbResolutionCache {
    async fn get(&self, key: &str) -> Option<crate::domain::stream::StreamResolution> {
        // Solo las claves tipo video_id tienen sentido aquí; el identificador
        // compuesto de fallback ("título|artista") nunca provino de la red.
        if key.contains('|') {
            return None;
        }
        let url = self
            .db
            .cached_stream_url(key, STREAM_CACHE_MAX_AGE_SECS)
            .await
            .ok()
            .flatten()?;
        Some(crate::domain::stream::StreamResolution::new(
            Source::YouTube,
            url,
        ))
    }

    async fn put(&self, key: &str, resolution: crate::domain::stream::StreamResolution) {
        if !key.contains('|') {
            let _ = self.db.cache_stream(key, &resolution.uri).await;
        }
    }

    async fn invalidate(&self, key: &str) {
        if !key.contains('|') {
            let _ = self.db.delete_cached_stream(key).await;
        }
    }

    async fn clear_provider(&self, _source: Source) {
        let _ = self.db.clear_stream_cache().await;
    }

    async fn expiring_within(
        &self,
        _window: std::time::Duration,
    ) -> Vec<(String, chrono::DateTime<chrono::Utc>)> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::domain::{artist::Artist, genre::Genre, source::Source, track::Track};
    use crate::recommendation::types::UserProfile;

    use super::*;

    #[tokio::test]
    async fn upsert_and_history_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("music.db");
        let db = Db::connect(db_path.to_str().unwrap()).await.unwrap();

        let mut track = Track::new(
            "Test Song".to_string(),
            vec![Artist::new("Test Artist".to_string(), None, None, None)],
            Source::YouTube,
        );
        track.duration = Some(std::time::Duration::from_secs(200));
        track.genres = vec![Genre::new("rock".to_string())];
        track.external_id = Some("dQw4w9WgXcQ".to_string());
        track.isrc = Some("USX000000000".to_string());

        let mut ids = HashMap::new();
        ids.insert(Source::YouTube, "dQw4w9WgXcQ".to_string());
        let id = db.upsert_track(&track, &ids).await.unwrap();
        assert!(id > 0);

        db.record_history(id, Source::YouTube, Some(200_000))
            .await
            .unwrap();
        // La misma canción conserva su id canónico y acumula frecuencia.
        assert_eq!(db.upsert_track(&track, &ids).await.unwrap(), id);
        db.record_history(id, Source::YouTube, Some(200_000))
            .await
            .unwrap();

        let history = db.recent_history(10).await.unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].title, "Test Song");
        assert_eq!(history[0].artist_name.as_deref(), Some("Test Artist"));
        assert_eq!(history[0].source, Source::YouTube);
        assert_eq!(history[0].play_count, 2);

        let stats = db.listening_stats().await.unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].key, "dQw4w9WgXcQ");
        assert_eq!(stats[0].play_count, 2);
        assert!(stats[0].recently_played);

        let local = db.search_local("Test", 10).await.unwrap();
        assert_eq!(local.len(), 1);
        assert_eq!(local[0].primary_artist_name(), Some("Test Artist"));

        let tags: Vec<(String, String)> =
            sqlx::query_as("SELECT name, source FROM tags WHERE track_id = ?1")
                .bind(id)
                .fetch_all(db.pool())
                .await
                .unwrap();
        assert_eq!(tags, vec![("rock".to_string(), "youtube".to_string())]);
    }

    #[tokio::test]
    async fn playlist_crud() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::connect(dir.path().join("music.db").to_str().unwrap())
            .await
            .unwrap();

        let mut track = Track::new(
            "A".to_string(),
            vec![Artist::new("X".to_string(), None, None, None)],
            Source::YouTube,
        );
        track.external_id = Some("vid-a".to_string());
        let id = db.upsert_track(&track, &HashMap::new()).await.unwrap();

        let pl = db.create_playlist("Mi lista").await.unwrap();
        db.add_to_playlist(pl, id).await.unwrap();
        db.add_to_playlist(pl, id).await.unwrap(); // dedupe

        // La migración 0009 siembra L1K3D: el listado empieza con la playlist
        // de sistema (kind DESC, nombre) y la del usuario después.
        let pls = db.list_playlists().await.unwrap();
        assert_eq!(pls.len(), 2);
        assert_eq!(pls[0].name, "L1K3D");
        assert!(pls[0].kind.is_system());
        assert_eq!(pls[1].name, "Mi lista");
        assert!(pls[1].kind.is_user());
        assert_eq!(pls[1].track_count, 1);

        let tracks = db.playlist_tracks(pl).await.unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title, "A");

        db.remove_from_playlist(pl, id).await.unwrap();
        assert_eq!(db.playlist_tracks(pl).await.unwrap().len(), 0);

        // Invariante: identidad de L1K3D protegida ante renombrado/eliminado y
        // el nombre reservado no puede reutilizarse en playlists de usuario.
        let l1k3d = db.l1k3d_id().await.unwrap().expect("L1K3D sembrada");
        assert!(db.rename_playlist(l1k3d, "Otro").await.is_err());
        assert!(db.delete_playlist(l1k3d).await.is_err());
        assert!(db.create_playlist("L1K3D").await.is_err());

        db.set_artwork_override(id, "file:///tmp/c.jpeg")
            .await
            .unwrap();
        assert_eq!(
            db.get_artwork_override(id).await.unwrap().as_deref(),
            Some("file:///tmp/c.jpeg")
        );

        // Las letras sincronizadas (LRC) se cachean con el flag de sincronía y
        // solo esas se sirven como fuente del karaoke.
        db.cache_synced_lyrics(id, "[00:01.00] letra\n[00:05.00] letra")
            .await
            .unwrap();
        assert_eq!(
            db.get_synced_lyrics(id).await.unwrap().as_deref(),
            Some("[00:01.00] letra\n[00:05.00] letra")
        );

        // Una letra plana heredada (legado sin consumidor) no se sirve como
        // sincronizada: la caché respeta la distinción por el flag `synced`.
        let legacy = Track::new(
            "C".to_string(),
            vec![Artist::new("Z".to_string(), None, None, None)],
            Source::YouTube,
        );
        let mut legacy_ids = HashMap::new();
        legacy_ids.insert(Source::YouTube, "vid-c".to_string());
        let legacy_id = db.upsert_track(&legacy, &legacy_ids).await.unwrap();
        sqlx::query(
            "INSERT INTO lyrics_cache (track_id, body, footer, synced, fetched_at) \
             VALUES (?1, ?2, NULL, 0, datetime('now'))",
        )
        .bind(legacy_id)
        .bind("letra plana vieja")
        .execute(db.pool())
        .await
        .unwrap();
        assert!(
            db.get_synced_lyrics(legacy_id).await.unwrap().is_none(),
            "la letra plana no se devuelve como sincronizada"
        );
    }

    #[tokio::test]
    async fn like_is_l1k3d_membership() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::connect(dir.path().join("music.db").to_str().unwrap())
            .await
            .unwrap();

        let mut track = Track::new(
            "Like".to_string(),
            vec![Artist::new("X".to_string(), None, None, None)],
            Source::YouTube,
        );
        track.external_id = Some("vid-like".to_string());
        let id = db.upsert_track(&track, &HashMap::new()).await.unwrap();
        let l1k3d = db.l1k3d_id().await.unwrap().expect("L1K3D sembrada");

        assert!(!db.is_liked(id).await.unwrap());
        db.set_liked(id, true).await.unwrap();
        assert!(db.is_liked(id).await.unwrap());
        assert!(db.playlist_contains(l1k3d, id).await.unwrap());
        assert_eq!(db.liked_track_ids().await.unwrap(), vec![id]);
        let liked = db.liked_tracks().await.unwrap();
        assert_eq!(liked.len(), 1);
        assert_eq!(liked[0].title, "Like");
        // El like es idempotente (dedupe por PK) y respeta el orden de alta.
        db.set_liked(id, true).await.unwrap();
        assert_eq!(db.liked_track_ids().await.unwrap(), vec![id]);

        db.set_liked(id, false).await.unwrap();
        assert!(!db.is_liked(id).await.unwrap());
        assert!(!db.playlist_contains(l1k3d, id).await.unwrap());
        assert!(db.liked_track_ids().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn reorder_and_move_playlist_tracks() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::connect(dir.path().join("music.db").to_str().unwrap())
            .await
            .unwrap();

        let mut ids = vec![];
        for (i, vid) in ["v1", "v2", "v3"].iter().enumerate() {
            let mut track = Track::new(
                format!("T{i}"),
                vec![Artist::new("X".to_string(), None, None, None)],
                Source::YouTube,
            );
            track.external_id = Some(vid.to_string());
            ids.push(db.upsert_track(&track, &HashMap::new()).await.unwrap());
        }

        let pl = db.create_playlist("Orden").await.unwrap();
        for id in &ids {
            db.add_to_playlist(pl, *id).await.unwrap();
        }

        let items = db.playlist_items(pl).await.unwrap();
        let order: Vec<i64> = items.iter().map(|i| i.track_id).collect();
        assert_eq!(order, ids);
        // `add_to_playlist` encola con MAX(position)+1 (empieza en 1).
        assert_eq!(
            items.iter().map(|i| i.position).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );

        // Reordenar: [T2, T0, T1].
        let new_order = vec![ids[2], ids[0], ids[1]];
        db.reorder_playlist(pl, &new_order).await.unwrap();
        let items = db.playlist_items(pl).await.unwrap();
        assert_eq!(
            items.iter().map(|i| i.track_id).collect::<Vec<_>>(),
            new_order
        );
        assert_eq!(
            items.iter().map(|i| i.position).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );

        // Mover T0 a la posición final satura en el último índice.
        db.move_playlist_track(pl, ids[0], 99).await.unwrap();
        let items = db.playlist_items(pl).await.unwrap();
        assert_eq!(
            items.iter().map(|i| i.track_id).collect::<Vec<_>>(),
            vec![ids[2], ids[1], ids[0]]
        );
        // Un track ajeno a la playlist no la desordena (guard de validación).
        let foreign = {
            let mut t = Track::new(
                "Foráneo".to_string(),
                vec![Artist::new("Z".to_string(), None, None, None)],
                Source::YouTube,
            );
            t.external_id = Some("v-for".to_string());
            db.upsert_track(&t, &HashMap::new()).await.unwrap()
        };
        assert!(db.move_playlist_track(pl, foreign, 0).await.is_err());
        assert_eq!(db.playlist_items(pl).await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn playlist_membership_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::connect(dir.path().join("music.db").to_str().unwrap())
            .await
            .unwrap();

        let mut track = Track::new(
            "M".to_string(),
            vec![Artist::new("X".to_string(), None, None, None)],
            Source::YouTube,
        );
        track.external_id = Some("vid-m".to_string());
        let tid = db.upsert_track(&track, &HashMap::new()).await.unwrap();

        let a = db.create_playlist("A").await.unwrap();
        let b = db.create_playlist("B").await.unwrap();
        let l1k3d = db.l1k3d_id().await.unwrap().expect("L1K3D sembrada");
        db.add_to_playlist(a, tid).await.unwrap();
        db.add_to_playlist(b, tid).await.unwrap();

        let memberships = db.get_playlist_memberships(&[tid]).await.unwrap();
        assert_eq!(memberships.len(), 2);
        assert!(memberships.iter().any(|m| m.playlist_id == a));
        assert!(memberships.iter().any(|m| m.playlist_id == b));
        assert!(!memberships.iter().any(|m| m.playlist_id == l1k3d));

        let none = db.get_playlist_memberships(&[-1]).await.unwrap();
        assert!(none.is_empty());
    }

    #[tokio::test]
    async fn artwork_palette_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::connect(dir.path().join("music.db").to_str().unwrap())
            .await
            .unwrap();

        let mut track = Track::new(
            "P".to_string(),
            vec![Artist::new("X".to_string(), None, None, None)],
            Source::YouTube,
        );
        track.external_id = Some("vid-p".to_string());
        let id = db.upsert_track(&track, &HashMap::new()).await.unwrap();

        assert!(db.get_track_palette(id).await.unwrap().is_none());
        let palette = [[226, 120, 224], [96, 168, 252], [250, 176, 96]];
        db.set_track_palette(id, palette).await.unwrap();
        let got = db.get_track_palette(id).await.unwrap().unwrap();
        assert_eq!(got.primary, palette[0]);
        assert_eq!(got.secondary, palette[1]);
        assert_eq!(got.accent, palette[2]);
        // Re-escribir (nueva portada) sustituye, no acumula.
        let palette2 = [[1, 2, 3], [4, 5, 6], [7, 8, 9]];
        db.set_track_palette(id, palette2).await.unwrap();
        let got = db.get_track_palette(id).await.unwrap().unwrap();
        assert_eq!(got.primary, palette2[0]);
        assert_eq!(got.secondary, palette2[1]);
        assert_eq!(got.accent, palette2[2]);
    }

    #[tokio::test]
    async fn internal_id_for_external() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::connect(dir.path().join("music.db").to_str().unwrap())
            .await
            .unwrap();

        let mut track = Track::new(
            "B".to_string(),
            vec![Artist::new("Y".to_string(), None, None, None)],
            Source::YouTube,
        );
        track.external_id = Some("vid-b".to_string());
        let mut ids = HashMap::new();
        ids.insert(Source::YouTube, "vid-b".to_string());
        let id = db.upsert_track(&track, &ids).await.unwrap();

        // Resuelve el id interno a partir del youtube_id, aunque no se tenga el
        // Track (p. ej. al reproducir en vivo un resultado de búsqueda).
        assert_eq!(
            db.internal_id_for_external("vid-b").await.unwrap(),
            Some(id)
        );
        assert_eq!(
            db.internal_id_for_external("no-existe").await.unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn stream_cache_roundtrip_and_expiry() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::connect(dir.path().join("music.db").to_str().unwrap())
            .await
            .unwrap();

        // Vacío al principio.
        assert_eq!(
            db.cached_stream_url("vid-a", 3600).await.unwrap(),
            None,
            "sin caché no hay stream"
        );

        // Guarda y recupera.
        db.cache_stream("vid-a", "https://googlevideo/stream-a")
            .await
            .unwrap();
        assert_eq!(
            db.cached_stream_url("vid-a", 3600).await.unwrap(),
            Some("https://googlevideo/stream-a".to_string())
        );

        // Actualiza la URL del mismo video (los streams expiran y se re-resuelven).
        db.cache_stream("vid-a", "https://googlevideo/stream-a2")
            .await
            .unwrap();
        assert_eq!(
            db.cached_stream_url("vid-a", 3600).await.unwrap(),
            Some("https://googlevideo/stream-a2".to_string())
        );

        // El TTL poda: 0 segundos de antigüedad máxima → nada.
        assert_eq!(
            db.cached_stream_url("vid-a", 0).await.unwrap(),
            None,
            "el TTL filtra entradas viejas"
        );
    }

    #[tokio::test]
    async fn play_signals_roundtrip_distinguishes_kinds_and_contexts() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::connect(dir.path().join("music.db").to_str().unwrap())
            .await
            .unwrap();

        let mut track = Track::new(
            "A".to_string(),
            vec![Artist::new("X".to_string(), None, None, None)],
            Source::YouTube,
        );
        track.external_id = Some("vid-a".to_string());
        let id = db.upsert_track(&track, &HashMap::new()).await.unwrap();

        db.record_signal(
            id,
            SignalKind::Play,
            PlayContext::Manual,
            Some(120_000),
            None,
            Some(200_000),
        )
        .await
        .unwrap();
        db.record_signal(
            id,
            SignalKind::Completed,
            PlayContext::Manual,
            Some(200_000),
            None,
            Some(200_000),
        )
        .await
        .unwrap();
        db.record_signal(
            id,
            SignalKind::Skip,
            PlayContext::Autoplay,
            Some(5_000),
            None,
            Some(200_000),
        )
        .await
        .unwrap();
        db.record_signal(
            id,
            SignalKind::Like,
            PlayContext::Manual,
            None,
            Some(42),
            Some(200_000),
        )
        .await
        .unwrap();

        let all = db.all_signals(100).await.unwrap();
        assert_eq!(all.len(), 4, "cuatro señales distintas persistidas");
        let kinds: Vec<SignalKind> = all.iter().map(|s| s.signal).collect();
        assert!(kinds.contains(&SignalKind::Play));
        assert!(kinds.contains(&SignalKind::Completed));
        assert!(kinds.contains(&SignalKind::Skip));
        assert!(kinds.contains(&SignalKind::Like));

        let by_track = db.signals_for_track(id).await.unwrap();
        assert_eq!(by_track.len(), 4);
        assert!(
            by_track
                .iter()
                .any(|s| s.signal == SignalKind::Like && s.recomm_id == Some(42)),
            "la señal Like conserva su recomm_id"
        );
    }

    #[tokio::test]
    async fn reloaded_signals_rebuild_the_same_user_profile() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::connect(dir.path().join("music.db").to_str().unwrap())
            .await
            .unwrap();

        let mut metal = Track::new(
            "Metal A".to_string(),
            vec![Artist::new("X".to_string(), None, None, None)],
            Source::YouTube,
        );
        metal.external_id = Some("vid-metal".to_string());
        metal.genres = vec![Genre::new("metal".to_string())];
        let metal_id = db.upsert_track(&metal, &HashMap::new()).await.unwrap();

        let mut classical = Track::new(
            "Clásica A".to_string(),
            vec![Artist::new("Y".to_string(), None, None, None)],
            Source::YouTube,
        );
        classical.external_id = Some("vid-classical".to_string());
        classical.genres = vec![Genre::new("classical".to_string())];
        let classical_id = db.upsert_track(&classical, &HashMap::new()).await.unwrap();

        // Metal fuerte (5 completas manuales), clásica débil (1 play autoplay).
        for _ in 0..5 {
            db.record_signal(
                metal_id,
                SignalKind::Completed,
                PlayContext::Manual,
                Some(200_000),
                None,
                Some(200_000),
            )
            .await
            .unwrap();
        }
        db.record_signal(
            classical_id,
            SignalKind::Play,
            PlayContext::Autoplay,
            Some(30_000),
            None,
            Some(200_000),
        )
        .await
        .unwrap();

        // Reconstrucción desde la BD, igual que hace el backend (`LoadRelated`).
        let loaded = db.all_signals(100).await.unwrap();
        assert_eq!(loaded.len(), 6, "las señales persistieron");
        let tracks: HashMap<i64, Track> = db
            .all_tracks()
            .await
            .unwrap()
            .into_iter()
            .map(|t| (t.id, t))
            .collect();
        let profiles = db.all_acoustic_profiles().await.unwrap();
        let rebuilt = UserProfile::from_signals(&loaded, &tracks, &profiles);

        assert_eq!(rebuilt.total_completions, 5);
        assert_eq!(rebuilt.total_plays, 1);
        assert!(
            rebuilt.favorite_genres.first().map(String::as_str) == Some("metal"),
            "el género dominante se reproduce tras recargar"
        );
    }

    #[tokio::test]
    async fn acoustic_profile_roundtrip_persists_means_and_band_profile() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::connect(dir.path().join("music.db").to_str().unwrap())
            .await
            .unwrap();

        let mut track = Track::new(
            "A".to_string(),
            vec![Artist::new("X".to_string(), None, None, None)],
            Source::YouTube,
        );
        track.external_id = Some("vid-a".to_string());
        let id = db.upsert_track(&track, &HashMap::new()).await.unwrap();

        let p = TrackAcousticProfile {
            track_id: id,
            rms_mean: 0.3,
            bass_mean: 0.8,
            low_mid_mean: 0.4,
            mid_mean: 0.5,
            high_mid_mean: 0.3,
            high_mean: 0.2,
            spectral_centroid_mean: 0.4,
            bpm_mean: 96.0,
            bpm_variance: 4.0,
            onset_mean: 0.1,
            band_profile: [0.8, 0.4, 0.5, 0.3, 0.2],
            frame_count: 150,
        };
        db.save_acoustic_profile(&p).await.unwrap();

        let got = db.acoustic_profile_for_track(id).await.unwrap().unwrap();
        assert_eq!(got.track_id, id);
        assert!((got.bass_mean - 0.8).abs() < 1e-4);
        assert!((got.bpm_mean - 96.0).abs() < 1e-3);
        assert_eq!(got.band_profile, [0.8, 0.4, 0.5, 0.3, 0.2]);
        assert_eq!(got.frame_count, 150);

        // Re-guardar con valores nuevos reemplaza el perfil (acumulación nueva).
        let p2 = TrackAcousticProfile {
            track_id: id,
            rms_mean: 0.1,
            ..p
        };
        db.save_acoustic_profile(&p2).await.unwrap();
        let got2 = db.acoustic_profile_for_track(id).await.unwrap().unwrap();
        assert!((got2.rms_mean - 0.1).abs() < 1e-4);
    }

    #[tokio::test]
    async fn all_tracks_catalog_includes_artists_genres_and_provider() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::connect(dir.path().join("music.db").to_str().unwrap())
            .await
            .unwrap();

        let mut track = Track::new(
            "Catalogo".to_string(),
            vec![Artist::new("El Artista".to_string(), None, None, None)],
            Source::YouTube,
        );
        track.duration = Some(std::time::Duration::from_secs(180));
        track.genres = vec![
            Genre::new("jazz".to_string()),
            Genre::new("ambient".to_string()),
        ];
        track.external_id = Some("vid-cat".to_string());
        let mut ids = HashMap::new();
        ids.insert(Source::YouTube, "vid-cat".to_string());
        let id = db.upsert_track(&track, &ids).await.unwrap();
        assert!(id > 0);

        let catalog = db.all_tracks().await.unwrap();
        assert_eq!(catalog.len(), 1, "el catálogo incluye el track guardado");
        let t = &catalog[0];
        assert_eq!(t.id, id);
        assert_eq!(t.title, "Catalogo");
        assert_eq!(t.primary_artist_name(), Some("El Artista"));
        assert_eq!(t.external_id.as_deref(), Some("vid-cat"));
        let mut genre_names: Vec<&str> = t.genres.iter().map(|g| g.name.as_str()).collect();
        genre_names.sort_unstable();
        assert_eq!(genre_names, vec!["ambient", "jazz"]);
    }

    #[tokio::test]
    async fn all_acoustic_profiles_loads_all_profiles_by_track_id() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::connect(dir.path().join("music.db").to_str().unwrap())
            .await
            .unwrap();

        let mut t1 = Track::new(
            "S1".to_string(),
            vec![Artist::new("A".to_string(), None, None, None)],
            Source::YouTube,
        );
        t1.external_id = Some("v1".to_string());
        let mut ids1 = HashMap::new();
        ids1.insert(Source::YouTube, "v1".to_string());
        let id1 = db.upsert_track(&t1, &ids1).await.unwrap();

        let mut t2 = Track::new(
            "S2".to_string(),
            vec![Artist::new("A".to_string(), None, None, None)],
            Source::YouTube,
        );
        t2.external_id = Some("v2".to_string());
        let mut ids2 = HashMap::new();
        ids2.insert(Source::YouTube, "v2".to_string());
        let id2 = db.upsert_track(&t2, &ids2).await.unwrap();

        for id in [id1, id2] {
            db.save_acoustic_profile(&TrackAcousticProfile {
                track_id: id,
                rms_mean: 0.2,
                bass_mean: 0.6,
                low_mid_mean: 0.3,
                mid_mean: 0.4,
                high_mid_mean: 0.2,
                high_mean: 0.1,
                spectral_centroid_mean: 0.35,
                bpm_mean: 100.0,
                bpm_variance: 2.0,
                onset_mean: 0.05,
                band_profile: [0.6, 0.3, 0.4, 0.2, 0.1],
                frame_count: 60,
            })
            .await
            .unwrap();
        }

        let all = db.all_acoustic_profiles().await.unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.contains_key(&id1));
        assert!(all.contains_key(&id2));
        assert!((all[&id1].bpm_mean - 100.0).abs() < 1e-3);
        assert_eq!(all[&id2].band_profile, [0.6, 0.3, 0.4, 0.2, 0.1]);
    }
}
