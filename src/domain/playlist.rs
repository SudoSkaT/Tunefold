//! Modelo de dominio: Playlist (metadata + membership).
//!
//! Base de toda la funcionalidad de playlists: una playlists es METADATA
//! (identidad, nombre, tipo, fechas, artwork opcional) que NO contiene tracks
//! (elegantemente se evita el modelo `tracks: Vec<Track>` que duplicaba el
//! catálogo); la relación obra-vía-playlist es la membership [`PlaylistItem`],
//! persistida con `playlist_id` + `track_id` (id interno canónico) + orden
//! explícito + fecha de alta.

/// Tipo de playlist. La diferenciación es un invariante de la capa de
/// aplicación y de la BD (ver migración 0009): una playlist de sistema (L1K3D)
/// no puede renombrarse, eliminarse ni editarse su tipo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum PlaylistKind {
    /// Creada por el usuario: libre de renombrar, eliminar y reordenar.
    #[default]
    User,
    /// Playlist autogenerada por el producto (L1K3D): protegida.
    System,
}

impl PlaylistKind {
    /// Valor persistido en `playlists.kind`.
    pub fn as_i64(self) -> i64 {
        match self {
            Self::User => 0,
            Self::System => 1,
        }
    }

    /// Restaura el tipo desde el valor persistido.
    pub fn from_i64(value: i64) -> Self {
        match value {
            1 => Self::System,
            _ => Self::User,
        }
    }

    pub fn is_user(self) -> bool {
        self == Self::User
    }

    pub fn is_system(self) -> bool {
        self == Self::System
    }

    /// `true` solo si la playlist es de sistema (L1K3D).
    pub fn protected(self) -> bool {
        self.is_system()
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::User => "playlist",
            Self::System => "L1K3D",
        }
    }
}

/// Metadata de una playlist persistente. No contiene los tracks: la
/// membership se modela con [`PlaylistItem`] (evita la doble fuente de verdad
/// y permite consultas de pertenencia por track sin cargar la lista entera).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub kind: PlaylistKind,
    pub created_at: String,
    pub updated_at: Option<String>,
    /// Portada opcional (p. ej. portada elegida por el usuario); las canciones
    /// conservan la suya propia.
    pub artwork: Option<String>,
}

/// Elemento de la membership playlists-tracks: el vínculo persistente con el
/// `track_id` interno canónico del track, su posición explícita y la fecha de
/// alta (para preservar el orden de inserción y el historial de "likes").
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PlaylistItem {
    pub playlist_id: i64,
    pub track_id: i64,
    pub position: i64,
    pub added_at: String,
}

impl Playlist {
    /// Nombre reservado de la playlist de sistema que materializa el estado
    /// "me gusta" del producto (no es una excepción: es una playlist normal con
    /// `kind = System`).
    pub const LIKED_NAME: &'static str = "L1K3D";

    /// La playlist de sistema existe siempre y es inmutable en identidad.
    pub fn liked(id: i64) -> Self {
        Self {
            id,
            name: Self::LIKED_NAME.to_string(),
            kind: PlaylistKind::System,
            created_at: String::new(),
            updated_at: None,
            artwork: None,
        }
    }

    /// Invariante central: una playlist de sistema jamás se renombra ni se
    /// elimina (L1K3D resuelve la pertenencia de "like" en toda la app).
    pub fn invariant_update_allowed(&self) -> bool {
        !self.kind.protected()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_maps_to_persisted_values() {
        assert_eq!(PlaylistKind::User.as_i64(), 0);
        assert_eq!(PlaylistKind::System.as_i64(), 1);
        assert_eq!(PlaylistKind::from_i64(0), PlaylistKind::User);
        assert_eq!(PlaylistKind::from_i64(1), PlaylistKind::System);
        assert_eq!(PlaylistKind::from_i64(42), PlaylistKind::User);
    }

    #[test]
    fn system_playlist_is_protected() {
        assert!(PlaylistKind::System.protected());
        assert!(!PlaylistKind::User.protected());
        assert!(PlaylistKind::System.is_system());
        assert!(PlaylistKind::User.is_user());
    }

    #[test]
    fn liked_factory_is_system_and_carries_reserved_name() {
        let p = Playlist::liked(7);
        assert_eq!(p.id, 7);
        assert_eq!(p.name, Playlist::LIKED_NAME);
        assert!(p.kind.is_system());
        assert!(!p.invariant_update_allowed());
    }

    #[test]
    fn user_playlist_can_be_updated() {
        let p = Playlist {
            id: 3,
            name: "Mi lista".to_string(),
            kind: PlaylistKind::User,
            created_at: "2026-01-01".to_string(),
            updated_at: None,
            artwork: None,
        };
        assert!(p.invariant_update_allowed());
    }

    #[test]
    fn item_carries_position_and_added_at() {
        let item = PlaylistItem {
            playlist_id: 1,
            track_id: 9,
            position: 0,
            added_at: "2026-01-01".to_string(),
        };
        assert_eq!(item.track_id, 9);
        assert_eq!(item.position, 0);
        assert!(!item.added_at.is_empty());
    }

    #[test]
    fn playlists_are_metadata_not_track_containers() {
        // El modelo no expone `tracks: Vec<Track>`: la membership vive aparte.
        let p = Playlist::liked(1);
        assert!(std::mem::size_of_val(&p) > 0);
        let _ = p;
    }
}
