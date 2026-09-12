//! Estado "me gusta" (L1K3D) compartido por todas las vistas de la TUI.
//!
//! La pertenencia a L1K3D vive en la BD por `track_id` interno, pero en la UI
//! casi todos los tracks llegan sin id interno (`id == 0`, resultados de
//! recomendaciones/búsqueda no persistidos todavía). Por eso este estado se
//! indexa por los DOS universos de claves que usa la app:
//!
//! - `keys`: el identificador estable [`Track::identifier`] (id externo, o
//!   firma `título|artista` como respaldo). Sirve para los tracks sin id de
//!   BD — recomendaciones, búsquedas, Now Playing.
//! - `ids`: el `track_id` interno de BD (`> 0`). Sirve para las vistas que
//!   solo exponen ese id (p. ej. Historial) y como guarda cuando el
//!   identificador de un track recién creado aún no coincide.
//!
//! Indexar por id interno únicamente causaba que, tras dar "me gusta" a una
//! canción aún no persistida (`id == 0`), el corazón se quedara activo en TODA
//! canción nueva que tampoco tuviera id interno.

use std::collections::HashSet;

use crate::domain::track::Track;

#[derive(Debug, Clone, Default)]
pub struct Liked {
    /// Identificadores estables de los tracks en L1K3D.
    keys: HashSet<String>,
    /// `track_id` internos de BD (`> 0`) de los tracks en L1K3D.
    ids: HashSet<i64>,
}

impl Liked {
    /// Registra un track como liked (p. ej. al confirmar `L1K3DChanged`).
    pub fn add(&mut self, track: &Track) {
        self.keys.insert(track.identifier());
        if track.id > 0 {
            self.ids.insert(track.id);
        }
    }

    /// Quita un track de L1K3D.
    pub fn remove(&mut self, track: &Track) {
        self.keys.remove(&track.identifier());
        self.ids.remove(&track.id);
    }

    /// Rellena el estado completo desde los tracks liked cargados de la BD.
    pub fn set_tracks<I: IntoIterator<Item = Track>>(&mut self, tracks: I) {
        self.keys.clear();
        self.ids.clear();
        for t in tracks {
            self.add(&t);
        }
    }

    /// ¿El track está en L1K3D? Se consultan las dos claves: el identificador
    /// (válido para tracks sin id de BD) y el id interno cuando existe.
    pub fn contains(&self, track: &Track) -> bool {
        self.keys.contains(&track.identifier()) || (track.id > 0 && self.ids.contains(&track.id))
    }

    /// ¿El `track_id` interno está en L1K3D? Para vistas que solo expongan el
    /// id de BD (p. ej. Historial) sin el track completo.
    pub fn contains_id(&self, track_id: i64) -> bool {
        track_id > 0 && self.ids.contains(&track_id)
    }

    pub fn len(&self) -> usize {
        self.ids.len().max(self.keys.len())
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty() && self.keys.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{artist::Artist, source::Source};

    fn track(id: i64, ext: Option<&str>) -> Track {
        let mut t = Track::new(
            "Título".to_string(),
            vec![Artist::new("Artista".to_string(), None, None, None)],
            Source::YouTube,
        );
        t.id = id;
        t.external_id = ext.map(|s| s.to_string());
        t
    }

    #[test]
    fn matches_by_identifier_even_without_internal_id() {
        let mut liked = Liked::default();
        // Track sin persistir (id 0) pero con id externo: se "likea".
        liked.add(&track(0, Some("video-1")));
        // Otra copia del mismo video (id 0) debe verse como liked.
        assert!(liked.contains(&track(0, Some("video-1"))));
        // Un video distinto, no.
        assert!(!liked.contains(&track(0, Some("video-2"))));
    }

    #[test]
    fn matches_by_internal_id_for_unidentified_tracks() {
        let mut liked = Liked::default();
        liked.add(&track(42, Some("video-1")));
        // La vista Historial solo tiene el track_id 42 (sin identificador).
        assert!(liked.contains_id(42));
        assert!(!liked.contains_id(43));
        // Un track reconstruido con el mismo id también encaja.
        assert!(liked.contains(&track(42, None)));
    }

    #[test]
    fn remove_cleans_both_key_spaces() {
        let mut liked = Liked::default();
        liked.add(&track(42, Some("video-1")));
        liked.remove(&track(42, Some("video-1")));
        assert!(!liked.contains(&track(42, Some("video-1"))));
        assert!(!liked.contains_id(42));
        assert!(liked.is_empty());
    }

    #[test]
    fn set_tracks_replaces_the_whole_state() {
        let mut liked = Liked::default();
        liked.add(&track(1, Some("a")));
        liked.set_tracks([track(2, Some("b")), track(3, Some("c"))]);
        assert!(liked.contains(&track(2, Some("b"))));
        assert!(!liked.contains(&track(1, Some("a"))));
        assert_eq!(liked.len(), 2);
    }
}
