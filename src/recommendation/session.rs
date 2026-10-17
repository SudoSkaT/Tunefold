//! Sesión de recomendaciones (spec §13/§26).
//!
//! El backend responde por un canal asíncrono: mientras una petición está en
//! vuelo el usuario puede reproducir otra canción (o la misma de nuevo). La
//! sesión es la dueña de QUÉ recomendaciones se muestran y CUÁLES son válidas:
//! cada petición consume una `generation` creciente y una respuesta solo se
//! acepta si su `(track, generation)` coincide con la carga EN VUELO. Así una
//! respuesta tardía de una sesión anterior jamás puebla la lista nueva ni
//! libera la petición en curso.

/// Carga de recomendaciones pedida al backend y aún sin respuesta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveLoad {
    /// Identificador estable del track del que se pidieron recomendaciones.
    pub track_key: String,
    /// Generación de la petición (identidad dentro de la sesión).
    pub generation: u64,
}

/// Dueña de qué recomendaciones se muestran y cuáles son válidas.
#[derive(Debug, Default)]
pub struct RecommendationSession {
    /// Track cuyas recomendaciones se están mostrando (`None` si aún no hay).
    source_track_key: Option<String>,
    /// Generación de la carga que pobló `source_track_key` (para validar
    /// actualizaciones tardías del MISMO ciclo — p. ej. el re-rank tras el
    /// pre-análisis acústico en background).
    source_generation: Option<u64>,
    /// Próxima generación a asignar (crece con cada petición, nunca decrece).
    generation: u64,
    /// Carga en vuelo (`track_key` + generación) o `None`.
    loading: Option<ActiveLoad>,
}

impl RecommendationSession {
    pub fn new() -> Self {
        Self::default()
    }

    /// Track cuyas recomendaciones se muestran actualmente.
    pub fn source_track_key(&self) -> Option<&str> {
        self.source_track_key.as_deref()
    }

    /// ¿Hay una carga de recomendaciones todavía sin responder?
    pub fn is_loading(&self) -> bool {
        self.loading.is_some()
    }

    /// Carga en vuelo, si la hay.
    pub fn loading(&self) -> Option<&ActiveLoad> {
        self.loading.as_ref()
    }

    /// Pide recomendaciones para `track_key`.
    ///
    /// Devuelve `None` y no envía nada si ya están cargadas para esa canción
    /// o ya hay una carga en vuelo para ella: es el guard que evita la
    /// regeneración por redibujado, tick o cambio de vista. Si la pide, marca
    /// la carga en vuelo y devuelve la generación que viajará con la petición
    /// (y que la respuesta deberá repetir).
    pub fn request(&mut self, track_key: &str) -> Option<u64> {
        if self.source_track_key.as_deref() == Some(track_key)
            || self
                .loading
                .as_ref()
                .is_some_and(|l| l.track_key == track_key)
        {
            return None;
        }
        self.generation += 1;
        self.loading = Some(ActiveLoad {
            track_key: track_key.to_string(),
            generation: self.generation,
        });
        Some(self.generation)
    }

    /// Completa la sesión con la respuesta del backend.
    ///
    /// Solo aplica si `(track_key, generation)` coincide con la carga EN
    /// VUELO: una respuesta obsoleta (de otra canción o de una sesión anterior
    /// para esta) se descarta sin tocar nada. Devuelve `true` si se aplicó.
    pub fn complete(&mut self, track_key: &str, generation: u64) -> bool {
        if !self
            .loading
            .as_ref()
            .is_some_and(|l| l.track_key == track_key && l.generation == generation)
        {
            return false;
        }
        self.loading = None;
        self.source_track_key = Some(track_key.to_string());
        self.source_generation = Some(generation);
        true
    }

    /// ¿Es válida una ACTUALIZACIÓN tardía del mismo ciclo de carga?
    ///
    /// A diferencia de [`Self::complete`] (que exige la carga EN VUELO), esto
    /// valida respuestas que llegan DESPUÉS de completarla con la MISMA
    /// generación: el re-rank del pre-análisis acústico de candidatos (que
    /// tarda segundos en background) reutiliza la generación original. Se
    /// rechaza si ya hay otra carga en vuelo o si la sesión cambió de track.
    pub fn accepts_update(&self, track_key: &str, generation: u64) -> bool {
        self.loading.is_none()
            && self.source_track_key.as_deref() == Some(track_key)
            && self.source_generation == Some(generation)
    }

    /// Cancela la carga en vuelo (p. ej. un error del backend).
    ///
    /// No invalida lo ya cargado: las recomendaciones mostradas siguen siendo
    /// las de `source_track_key`. Para forzar una recarga explícita usar
    /// [`Self::reset`].
    pub fn abort(&mut self) {
        self.loading = None;
    }

    /// El track en curso cambió de canción: ni lo cargado ni lo en vuelo
    /// pertenecen ya a la sesión.
    pub fn on_track_changed(&mut self) {
        self.source_track_key = None;
        self.source_generation = None;
        self.loading = None;
    }

    /// Reinicia por completo (recarga explícita del usuario): descarta lo
    /// cargado y lo en vuelo para que la siguiente petición sea sesión nueva.
    /// La generación NO se reinicia: las respuestas de sesiones anteriores
    /// jamás pueden volver a coincidir con una carga en vuelo futura.
    pub fn reset(&mut self) {
        self.source_track_key = None;
        self.source_generation = None;
        self.loading = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_starts_a_session_and_returns_the_generation() {
        let mut s = RecommendationSession::new();
        assert_eq!(s.request("a"), Some(1));
        assert!(s.is_loading());
        assert_eq!(s.source_track_key(), None, "aún no hay nada cargado");
    }

    #[test]
    fn complete_accepts_only_the_in_flight_request() {
        let mut s = RecommendationSession::new();
        let gen = s.request("a").unwrap();

        // Generación equivocada (respuesta anterior/ajena): se descarta.
        assert!(!s.complete("a", gen - 1));
        assert!(!s.complete("b", gen));

        // La correcta aplica y el track pasa a ser el origen.
        assert!(s.complete("a", gen));
        assert_eq!(s.source_track_key(), Some("a"));
        assert!(!s.is_loading());
    }

    #[test]
    fn already_loaded_track_is_not_requested_again() {
        let mut s = RecommendationSession::new();
        let gen = s.request("a").unwrap();
        s.complete("a", gen);

        assert_eq!(s.request("a"), None, "ya cargadas: no se re-piden");
        assert_eq!(
            s.request("b"),
            Some(2),
            "otra canción sí arranca sesión nueva"
        );
    }

    #[test]
    fn no_duplicate_request_while_in_flight() {
        let mut s = RecommendationSession::new();
        s.request("a");
        assert_eq!(s.request("a"), None, "carga en vuelo: no se duplica");
    }

    #[test]
    fn late_response_of_a_previous_session_is_rejected() {
        // El usuario reproduce A (gen 1), luego B (gen 2) y vuelve a A (gen 3).
        // La respuesta de la PRIMERA sesión de A llega tarde: la sesión nueva
        // (carga en vuelo A[3]) no debe recibir ese contenido.
        let mut s = RecommendationSession::new();
        s.request("a"); // gen 1
        s.on_track_changed();
        s.request("b"); // gen 2
        s.on_track_changed();
        s.request("a"); // gen 3

        assert!(!s.complete("a", 1), "sesión anterior rechazada");
        assert!(
            s.is_loading(),
            "y no libera la carga en vuelo de la sesión nueva"
        );
        assert!(s.complete("a", 3), "la sesión en curso sí aplica");
    }

    #[test]
    fn abort_clears_only_the_in_flight_request() {
        let mut s = RecommendationSession::new();
        let gen = s.request("a").unwrap();
        s.abort();
        assert!(!s.is_loading());
        assert!(
            !s.complete("a", gen),
            "tras abort no hay nada que completar"
        );
        assert_eq!(s.source_track_key(), None);
    }

    #[test]
    fn on_track_changed_invalidates_loaded_and_pending() {
        let mut s = RecommendationSession::new();
        let gen = s.request("a").unwrap();
        s.complete("a", gen);
        assert_eq!(s.source_track_key(), Some("a"));

        s.on_track_changed();
        assert_eq!(s.source_track_key(), None);
        assert_eq!(s.request("b"), Some(2), "canción nueva abre sesión");
    }

    #[test]
    fn reset_allows_requesting_the_same_track_again() {
        let mut s = RecommendationSession::new();
        let gen = s.request("a").unwrap();
        s.complete("a", gen);
        assert_eq!(s.request("a"), None);

        s.reset();
        assert_eq!(
            s.request("a"),
            Some(2),
            "tras reset la misma canción vuelve a pedirse"
        );
    }

    #[test]
    fn accepts_update_only_for_the_completed_generation_of_the_same_track() {
        let mut s = RecommendationSession::new();
        let gen = s.request("a").unwrap();
        // Antes de completar no hay sesión cargada: no se acepta actualización.
        assert!(!s.accepts_update("a", gen));

        s.complete("a", gen);
        assert!(
            s.accepts_update("a", gen),
            "re-rank tardío del mismo ciclo se acepta"
        );
        assert!(!s.accepts_update("a", gen - 1), "generación ajena");
        assert!(!s.accepts_update("b", gen), "track ajeno");
    }

    #[test]
    fn accepts_update_is_rejected_while_another_load_is_in_flight() {
        let mut s = RecommendationSession::new();
        let gen_a = s.request("a").unwrap();
        s.complete("a", gen_a);

        // El usuario cambió a B (carga en vuelo): el re-rank de A es tarde.
        s.request("b");
        assert!(
            !s.accepts_update("a", gen_a),
            "una carga en vuelo invalida actualizaciones de la sesión anterior"
        );
    }

    #[test]
    fn accepts_update_is_rejected_after_track_change() {
        let mut s = RecommendationSession::new();
        let gen = s.request("a").unwrap();
        s.complete("a", gen);
        s.on_track_changed();
        assert!(!s.accepts_update("a", gen));
    }
}
