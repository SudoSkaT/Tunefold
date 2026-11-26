//! Modelo de dominio: Álbum.

use chrono::NaiveDate;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Album {
    pub id: i64,
    pub title: String,
    pub release_date: Option<NaiveDate>,
    pub cover: Option<String>,
    pub label: Option<String>,
    pub artist_ids: Vec<i64>,
    /// ID externo en la plataforma de origen (ej. YouTube album id).
    #[serde(default)]
    pub external_id: Option<String>,
}

impl Album {
    pub fn new(
        title: String,
        release_date: Option<NaiveDate>,
        cover: Option<String>,
        label: Option<String>,
    ) -> Self {
        Self {
            id: 0,
            title,
            release_date,
            cover,
            label,
            artist_ids: Vec::new(),
            external_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn album_external_id_roundtrips() {
        let mut album = Album::new("T".to_string(), None, Some("c.jpg".to_string()), None);
        album.external_id = Some("MPREb_1".to_string());
        let json = serde_json::to_string(&album).unwrap();
        let parsed: Album = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.external_id.as_deref(), Some("MPREb_1"));
    }

    #[test]
    fn old_album_without_external_id_deserializes() {
        let json =
            r#"{"id":0,"title":"T","release_date":null,"cover":null,"label":null,"artist_ids":[]}"#;
        let parsed: Album = serde_json::from_str(json).unwrap();
        assert!(parsed.external_id.is_none());
    }
}
