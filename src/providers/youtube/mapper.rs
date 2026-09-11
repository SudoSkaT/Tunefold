//! Mapeo de modelos de rustypipe → dominio Tunefold.
//!
//! Frontera anti-filtración: los tipos `TrackItem`/`MusicArtist`/`MusicAlbum`
//! de rustypipe NO salen de este módulo; hacia dentro solo llegan modelos de
//! dominio ya firmados.

use rustypipe::model::TrackItem;

use crate::domain::source::Source;
use crate::domain::track::Thumbnail as TrackThumbnail;
use crate::domain::{album::Album, artist::Artist, track::Track};

/// Cadena de fallback de miniaturas `i.ytimg.com`, en orden descendente de
/// resolución. Se prueban hasta encontrar una que exista y se decodifique.
pub(crate) const THUMB_FALLBACK: [&str; 4] = ["maxresdefault", "hqdefault", "mqdefault", "default"];

/// Firma un `Track` de dominio a partir de un item de YouTube Music.
pub(crate) fn map_track(item: &TrackItem) -> Track {
    let artists: Vec<Artist> = item
        .artists
        .iter()
        .filter_map(|a| {
            let name = a.name.trim();
            if name.is_empty() {
                None
            } else {
                Some(Artist::new(name.to_string(), None, None, None))
            }
        })
        .collect();

    let mut track = Track::new(
        item.name.clone(),
        if artists.is_empty() {
            vec![Artist::new("Desconocido".to_string(), None, None, None)]
        } else {
            artists
        },
        Source::YouTube,
    );
    track.external_id = Some(item.id.clone());
    track.thumbnail = best_thumbnail(&item.cover).map(|url| TrackThumbnail { url });
    if let Some(secs) = item.duration {
        track.duration = Some(std::time::Duration::from_secs(secs as u64));
    }
    track.album = item
        .album
        .as_ref()
        .map(|a| Album::new(a.name.clone(), None, None, None));
    track.url = Some(format!("https://www.youtube.com/watch?v={}", item.id));
    track
}

/// Elige la portada de mayor resolución disponible.
pub(crate) fn best_thumbnail(thumbs: &[rustypipe::model::Thumbnail]) -> Option<String> {
    thumbs
        .iter()
        .max_by_key(|t| (t.width, t.height))
        .map(|t| t.url.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn best_thumbnail_picks_largest() {
        use rustypipe::model::Thumbnail;
        let thumbs: Vec<Thumbnail> = serde_json::from_str(
            r#"[
                {"url": "small", "width": 120, "height": 90},
                {"url": "big", "width": 720, "height": 540},
                {"url": "huge", "width": 1280, "height": 720}
            ]"#,
        )
        .expect("JSON válido");
        assert_eq!(best_thumbnail(&thumbs).as_deref(), Some("huge"));
    }
    #[test]
    fn map_track_populates_fields() {
        use rustypipe::model::TrackItem;
        let json = r#"{
            "id": "dQw4w9WgXcQ",
            "name": "Never Gonna Give You Up",
            "duration": 213,
            "track_type": "track",
            "by_va": false,
            "cover": [{"url": "cover.jpg", "width": 640, "height": 640}],
            "artists": [{"id": "UCx", "name": "Rick Astley"}],
            "album": {"id": "MPREb", "name": "Whenever You Need Somebody"}
        }"#;
        let item: TrackItem = serde_json::from_str(json).expect("deserializa TrackItem");
        let track = map_track(&item);
        assert_eq!(track.title, "Never Gonna Give You Up");
        assert_eq!(track.source, Source::YouTube);
        assert_eq!(track.external_id.as_deref(), Some("dQw4w9WgXcQ"));
        assert_eq!(track.primary_artist_name(), Some("Rick Astley"));
        assert_eq!(track.duration, Some(std::time::Duration::from_secs(213)));
        assert_eq!(
            track.thumbnail.as_ref().map(|t| t.url.as_str()),
            Some("cover.jpg")
        );
    }
}
