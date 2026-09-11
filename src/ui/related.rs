//! Vista Related: letras (modo karaoke con LRC de LRCLIB) + recomendaciones.
//!
//! Muestra las letras sincronizadas de la canción en curso con la línea activa
//! resaltada (recibe la posición de reproducción de la UI); si no hay LRC se
//! muestra un estado limpio — nunca letra plana. Debajo, una lista de `tracks`
//! relacionadas.
//!
//! [`render_tracks_list`] es un helper compartido: la vista Now Playing lo usa
//! para el panel de recomendaciones de su pantalla principal.

use std::time::Duration;

use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::domain::lyrics::SyncLyrics;
use crate::domain::track::Track;
use crate::infrastructure::storage::TrackListeningStats;

use super::widgets::karaoke::KaraokeScroller;
use super::VisualContent;
use crate::visualization::palette::VisualPalette;
use crate::visualization::render as visualizer;

use super::navigation::ListSelection;

use crate::playback::queue::QueueItemOrigin;

#[derive(Debug, Default)]
pub struct RelatedState {
    /// CONTENIDO MOSTRADO: la cola de autoplay (lo que realmente va a sonar).
    /// Crece con dedupe en cada `BackendEvent::Related` y nunca se vacía al
    /// cambiar de canción — la "buena lista" que el usuario construye persiste.
    pub tracks: Vec<Track>,
    /// Origen de cada elemento de [`Self::tracks`] (paralelo, índice a índice):
    /// etiqueta qué añadió el autoplay (`Recommendation`) y qué pidió el
    /// usuario (`User`/`Search`/`Playlist`).
    pub origins: Vec<QueueItemOrigin>,
    /// Longitud de `tracks` ANTES del último `set_queue`: permite anunciar
    /// cuántas recomendaciones nuevas entraron en la última carga.
    pub previous_len: usize,
    /// Conjunto de identificadores recién añadidos en la última carga
    /// (`set_queue`): la vista los marca de verde como "nuevas".
    pub new_ids: std::collections::HashSet<String>,
    /// Recomendaciones FRESCAS de la canción en curso (sin dedupes ni fondo
    /// acumulado): la acción `R` puede imponerlas como nueva cola.
    pub fresh: Vec<Track>,
    /// Letra sincronizada (LRC, LRCLIB) para el modo karaoke: única fuente.
    pub synced: Option<SyncLyrics>,
    /// `true` cuando ya se intentó cargar la letra sincronizada y no existe:
    /// se muestra un estado limpio en vez de la letra plana antigua.
    pub synced_unavailable: bool,
    /// Ventana del karaoke: buffer circular de índices con desplazamiento en
    /// cascada sobre [`Self::synced`]. Se reinicia al cambiar de letra.
    pub scroll: KaraokeScroller,
    pub list_state: ListState,
}

impl RelatedState {
    pub fn has_tracks(&self) -> bool {
        !self.tracks.is_empty()
    }

    /// Actualiza la cola mostrada y su paralelo de orígenes, y marca las filas
    /// recién añadidas (diferencia por identificador estable con la cola
    /// anterior) como "nuevas" para el feedback contextual.
    pub fn set_queue(&mut self, queue: Vec<Track>, origins: Vec<QueueItemOrigin>) {
        let before: std::collections::HashSet<String> =
            self.tracks.iter().map(|t| t.identifier()).collect();
        self.previous_len = self.tracks.len();
        self.tracks = queue;
        self.origins = origins;
        self.new_ids = self
            .tracks
            .iter()
            .filter(|t| !before.contains(&t.identifier()))
            .map(|t| t.identifier())
            .collect();
    }

    /// Nueva canción: se descartan las letras anteriores y se vuelve al estado
    /// "sin pedir" (la ventana del karaoke es de la canción vieja y no debe
    /// reutilizarse nunca).
    pub fn clear_lyrics(&mut self) {
        self.scroll.reset();
        self.synced = None;
        self.synced_unavailable = false;
    }

    /// Resultado de la carga de letras sincronizadas:
    /// `Some` → karaoke listo; `None` → no hay LRC (estado limpio de aviso).
    pub fn set_synced(&mut self, synced: Option<SyncLyrics>) {
        self.scroll.reset();
        match synced {
            Some(s) => {
                self.synced = Some(s);
                self.synced_unavailable = false;
            }
            None => {
                self.synced = None;
                self.synced_unavailable = true;
            }
        }
    }

    pub fn select_next(&mut self) {
        self.step(true);
    }

    pub fn select_prev(&mut self) {
        self.step(false);
    }

    pub fn selected(&self) -> Option<&Track> {
        self.list_state.selected().and_then(|i| self.tracks.get(i))
    }
}

impl super::navigation::ListSelection for RelatedState {
    fn list_len(&self) -> usize {
        self.tracks.len()
    }

    fn cursor(&self) -> Option<usize> {
        self.list_state.selected()
    }

    fn set_cursor(&mut self, index: Option<usize>) {
        self.list_state.select(index);
    }
}

/// Contenido efectivo de la banda superior, resuelto desde [`VisualContent`].
enum BandContent {
    /// El visualizador (lava + barras) ocupa la banda.
    Visual,
    /// Letras karaoke sobre la capa ambiental.
    Lyrics,
    /// Las letras no existen (LRCLIB no las tiene): aviso sobre el ambient.
    Unavailable,
    /// Las letras aún no se pidieron: aviso neutro sobre el ambient.
    Waiting,
}

impl BandContent {
    fn resolve(mode: VisualContent, has_lyrics: bool, unavailable: bool) -> Self {
        match mode {
            VisualContent::Visual => Self::Visual,
            VisualContent::Lyrics => {
                if has_lyrics {
                    Self::Lyrics
                } else if unavailable {
                    Self::Unavailable
                } else {
                    Self::Waiting
                }
            }
            VisualContent::Auto => {
                if has_lyrics {
                    Self::Lyrics
                } else {
                    Self::Visual
                }
            }
        }
    }
}

/// Renderiza la vista Related: banda superior (letras/visual, capa ambiental
/// compartida) + lista de recomendados.
///
/// `position` es el reloj del karaoke y `finished` si la reproducción ya
/// terminó de verdad (estado `Stopped`): al terminar se limpia el panel de
/// letras. `mode` es el contenido elegido por el usuario (`v`): Auto/Letras/
/// Visual; cambiar de modo nunca regenera recomendaciones ni toca la
/// reproducción (§17).
#[allow(clippy::too_many_arguments)] // posición, fin, modo y paleta son datos del render
pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &mut RelatedState,
    position: Option<Duration>,
    finished: bool,
    mode: VisualContent,
    current_key: Option<&str>,
    visual: &crate::visualization::VisualState,
    mouse: &Option<(u16, u16)>,
    click: &mut bool,
    stats: &std::collections::HashMap<String, TrackListeningStats>,
) {
    let has_lyrics = state.synced.as_ref().filter(|s| !s.is_empty()).is_some();
    // El visual necesita al menos un poco de altura; si el terminal es bajo se
    // prescinde de la banda superior y todo el espacio va a la lista.
    let can_visual = area.height >= 24;
    // La banda superior se reserva si hay letras (karaoke), el visual puede
    // ocupar su lugar, o hay que avisar de que las letras no están disponibles.
    let reserve_band = has_lyrics || can_visual || state.synced_unavailable;

    let mut chunks = vec![];
    if reserve_band {
        let band_height = area.height.saturating_sub(7).clamp(6, 18);
        chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(band_height), Constraint::Min(0)])
            .split(area)
            .to_vec();
    } else {
        chunks = vec![area];
    }

    if reserve_band {
        let top_area = chunks[0];
        match BandContent::resolve(mode, has_lyrics, state.synced_unavailable) {
            BandContent::Visual => {
                visualizer::render(
                    frame,
                    top_area,
                    visual,
                    position.unwrap_or(Duration::ZERO).as_secs_f32(),
                );
            }
            BandContent::Lyrics => {
                visualizer::render_ambient(frame, top_area, visual, true);
                render_karaoke_over_scene(
                    frame,
                    top_area,
                    state,
                    position,
                    finished,
                    &visual.scene.palette,
                );
            }
            BandContent::Unavailable => {
                visualizer::render_ambient(frame, top_area, visual, true);
                render_message_over_scene(
                    frame,
                    top_area,
                    " Letras ",
                    "Letras sincronizadas no disponibles",
                );
            }
            BandContent::Waiting => {
                visualizer::render_ambient(frame, top_area, visual, true);
                render_message_over_scene(
                    frame,
                    top_area,
                    " Letras ",
                    "Sin letras sincronizadas todavía. Reproduce y vuelve aquí.",
                );
            }
        }
    }

    let tracks_area = chunks.last().copied().unwrap_or(area);
    render_tracks_list(
        frame,
        tracks_area,
        state,
        " Recomendaciones ".to_string(),
        "Sin recomendaciones todavía. Pulsa Enter en esta vista para pedirlas a YouTube.",
        current_key,
        mouse,
        click,
        stats,
    );
}

/// Prepara el estado del karaoke (línea activa, fin de letra, paleta) y delega
/// el render en el overlay que preserva la capa ambiental.
///
/// Los tres estados (ya leído / en lectura / no leído) adoptan los tres colores
/// de la paleta fundida de la portada (que el motor visual entrega en
/// `visual.scene.palette`). El panel se limpia cuando la reproducción terminó
/// de verdad (`finished` = estado `Stopped` del motor).
fn render_karaoke_over_scene(
    frame: &mut Frame,
    area: Rect,
    state: &mut RelatedState,
    position: Option<Duration>,
    finished: bool,
    palette: &VisualPalette,
) {
    let Some(sync) = state.synced.as_ref().filter(|s| !s.is_empty()) else {
        return;
    };
    let pos = position.unwrap_or(Duration::ZERO);
    let active = if finished {
        None
    } else {
        sync.active_index(pos)
    };
    let colors = karaoke_colors(palette);
    super::widgets::karaoke::render_over_scene(
        frame,
        area,
        &mut state.scroll,
        sync,
        active,
        finished,
        colors,
    );
}

/// Mapea los tres estados del karaoke a los tres colores de la paleta fundida:
/// `(ya leído, en lectura, no leído)`. La línea activa usa el dominante.
fn karaoke_colors(p: &VisualPalette) -> (Color, Color, Color) {
    (
        Color::Rgb(p.secondary[0], p.secondary[1], p.secondary[2]),
        Color::Rgb(p.primary[0], p.primary[1], p.primary[2]),
        Color::Rgb(p.accent[0], p.accent[1], p.accent[2]),
    )
}

/// Panel de aviso CON fondo transparente (escritura directa): no borra la capa
/// ambiental que hay detrás.
fn render_message_over_scene(frame: &mut Frame, area: Rect, title: &str, text: &str) {
    super::widgets::karaoke::paint_frame(frame, area, title, Color::DarkGray);
    let inner = area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    if inner.width < 4 || inner.height == 0 {
        return;
    }
    let y = inner.y + inner.height.saturating_sub(1) / 2;
    let pad = inner.width.saturating_sub(text.chars().count() as u16) / 2;
    frame
        .buffer_mut()
        .set_string(inner.x + pad, y, text, Style::new().fg(Color::DarkGray));
}

/// Renderiza la lista de recomendaciones (o un aviso si está vacía).
///
/// La comparten la vista Related (lista completa) y el panel de la vista
/// Now Playing. Acepta ratón para seleccionar la fila bajo el cursor; el
/// scroll de la lista lo gestiona `ListState` automáticamente.
///
/// `current_key` identifica el track en curso (doblete contextual `▶`); el
/// origen de cada fila y el juego de "nuevas" (green badge) viven en el estado.
#[allow(clippy::too_many_arguments)] // ratón/estadísticas son datos del render
pub fn render_tracks_list(
    frame: &mut Frame,
    area: Rect,
    state: &mut RelatedState,
    title: String,
    empty: &str,
    current_key: Option<&str>,
    mouse: &Option<(u16, u16)>,
    click: &mut bool,
    stats: &std::collections::HashMap<String, TrackListeningStats>,
) {
    if state.tracks.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(empty))
                .block(Block::default().borders(Borders::ALL).title(title)),
            area,
        );
        return;
    }

    let hovered = super::widgets::hover_index(*mouse, area);
    if *click {
        if let Some(i) = hovered {
            state.list_state.select(Some(i));
        }
        *click = false;
    }
    let items: Vec<ListItem> = state
        .tracks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let artist = t.primary_artist_name().unwrap_or("Desconocido");
            let duration = t
                .duration
                .map(super::widgets::format_duration)
                .unwrap_or_else(|| "duración pendiente".to_string());
            let listened = stats.get(&t.identifier());
            let is_current = current_key == Some(t.identifier().as_str());
            let origin = state
                .origins
                .get(i)
                .copied()
                .unwrap_or(crate::playback::queue::QueueItemOrigin::Recommendation);
            let is_new = state.new_ids.contains(&t.identifier());
            // Doblete discreto: el track en curso lleva un `▶`; lo añadido por
            // el autoplay se marca `↻` tenue; lo recién añadido, verde.
            let marker = if is_current {
                Span::styled(
                    "▶ ",
                    Style::new().fg(Color::Green).add_modifier(Modifier::BOLD),
                )
            } else if is_new {
                Span::styled("✚ ", Style::new().fg(Color::Green))
            } else if origin.is_auto() {
                Span::styled("↻ ", Style::new().fg(Color::DarkGray))
            } else {
                Span::styled("· ", Style::new().fg(Color::DarkGray))
            };
            let line = Line::from(vec![
                marker,
                Span::styled(
                    format!("[{}] ", t.source.label()),
                    Style::new().fg(Color::Cyan),
                ),
                Span::raw(format!("{artist} - {}", t.title)),
                Span::styled(format!("  ({duration})"), Style::new().fg(Color::DarkGray)),
                listened
                    .map(|s| {
                        Span::styled(
                            if s.recently_played {
                                format!("  ● reciente · {}×", s.play_count)
                            } else {
                                format!("  ↻ {}×", s.play_count)
                            },
                            Style::new().fg(Color::Green),
                        )
                    })
                    .unwrap_or_else(|| Span::raw("")),
            ]);
            let style = if Some(i) == hovered {
                Style::new().fg(Color::Gray)
            } else {
                Style::new()
            };
            ListItem::new(line).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(
            Style::new()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, area, &mut state.list_state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::visualization::VisualState;
    use ratatui::backend::TestBackend;

    fn visual_with_palette(cover: Option<[[u8; 3]; 3]>) -> VisualState {
        let mut s = VisualState::inactive();
        s.scene.palette = VisualPalette::from_cover(cover);
        s
    }

    fn render_state(
        synced: Option<SyncLyrics>,
        position: Duration,
        finished: bool,
        mode: VisualContent,
        cover: Option<[[u8; 3]; 3]>,
    ) -> ratatui::buffer::Buffer {
        let mut state = RelatedState {
            synced,
            ..RelatedState::default()
        };
        let backend = TestBackend::new(60, 16);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render(
                    f,
                    f.area(),
                    &mut state,
                    Some(position),
                    finished,
                    mode,
                    None,
                    &visual_with_palette(cover),
                    &None,
                    &mut false,
                    &std::collections::HashMap::new(),
                )
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn buffer_text(buf: &ratatui::buffer::Buffer) -> String {
        buf.content().iter().map(|c| c.symbol()).collect::<String>()
    }

    #[test]
    fn last_line_stays_visible_during_outro_and_clears_at_real_end() {
        let sync = SyncLyrics::parse("[00:05] uno\n[00:10] dos\n");

        // En mitad de la canción la línea activa se muestra.
        let mid = render_state(
            Some(sync.clone()),
            Duration::from_secs(7),
            false,
            VisualContent::Auto,
            None,
        );
        assert!(buffer_text(&mid).contains("dos"), "línea activa visible");

        // Superada la última marca, durante el outro la última línea se queda
        // (el karaoke no se limpia antes del fin real).
        let outro = render_state(
            Some(sync.clone()),
            Duration::from_secs(11),
            false,
            VisualContent::Auto,
            None,
        );
        assert!(
            buffer_text(&outro).contains("dos"),
            "durante el outro la última línea sigue visible"
        );

        // Al terminar la reproducción (fin real) sí se limpia todo.
        let done = render_state(
            Some(sync),
            Duration::from_secs(11),
            true,
            VisualContent::Auto,
            None,
        );
        let text = buffer_text(&done);
        assert!(
            !text.contains("dos") && !text.contains("uno"),
            "al fin real se limpia el panel"
        );
    }

    #[test]
    fn outro_does_not_clear_before_real_end() {
        // La canción dura 180s pero el LRC termina a los 10s (outro largo):
        // la última línea sigue visible mientras suena el outro.
        let sync = SyncLyrics::parse("[00:05] uno\n[00:10] dos\n");
        let buf = render_state(
            Some(sync.clone()),
            Duration::from_secs(60),
            false,
            VisualContent::Auto,
            None,
        );
        assert!(
            buffer_text(&buf).contains("dos"),
            "durante el outro la última línea se queda (no se limpia a los 10s)"
        );

        // Durante la última línea aún se ve.
        let singing = render_state(
            Some(sync),
            Duration::from_secs(10),
            false,
            VisualContent::Auto,
            None,
        );
        assert!(
            buffer_text(&singing).contains("dos"),
            "durante la última línea aún se ve"
        );
    }

    #[test]
    fn karaoke_clears_at_real_end_when_lrc_lasts_longer() {
        // El LRC marca la última línea a los 200s pero la canción acaba a los
        // 180s: al llegar al fin real sí se limpia (no se queda colgado).
        let sync = SyncLyrics::parse("[00:05] uno\n[03:20] dos\n");
        let buf = render_state(
            Some(sync),
            Duration::from_secs(180),
            true,
            VisualContent::Auto,
            None,
        );
        let text = buffer_text(&buf);
        assert!(
            !text.contains("dos"),
            "al terminar la canción se limpia aunque el LRC no haya acabado"
        );
    }

    #[test]
    fn karaoke_uses_cover_palette() {
        let sync = SyncLyrics::parse("[00:05] uno\n[00:10] dos\n[00:15] tres\n");
        // En posición 12s: "uno" (ya leído), "dos" (en lectura), "tres" (no leído).
        let buf = render_state(
            Some(sync),
            Duration::from_secs(12),
            false,
            VisualContent::Auto,
            Some([[255, 0, 0], [0, 255, 0], [0, 0, 255]]),
        );
        let cells: Vec<(&str, Color)> = buf.content().iter().map(|c| (c.symbol(), c.fg)).collect();
        assert!(
            cells
                .iter()
                .any(|(s, fg)| *s == "d" && *fg == Color::Rgb(255, 0, 0)),
            "en lectura = color dominante"
        );
        assert!(
            cells
                .iter()
                .any(|(s, fg)| *s == "u" && *fg == Color::Rgb(0, 255, 0)),
            "ya leído = segundo color"
        );
        assert!(
            cells
                .iter()
                .any(|(s, fg)| *s == "t" && *fg == Color::Rgb(0, 0, 255)),
            "no leído = tercer color"
        );
    }

    #[test]
    fn unavailable_lyrics_show_clean_state_without_plain_fallback() {
        // LRCLIB no devolvió `syncedLyrics`: en modo Letras (explícito) debe
        // verse el estado limpio y jamás la letra plana amarilla antigua.
        let mut state = RelatedState::default();
        state.set_synced(None);

        let backend = TestBackend::new(60, 16);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render(
                    f,
                    f.area(),
                    &mut state,
                    Some(Duration::from_secs(5)),
                    false,
                    VisualContent::Lyrics,
                    None,
                    &visual_with_palette(None),
                    &None,
                    &mut false,
                    &std::collections::HashMap::new(),
                )
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(
            text.contains("Letras sincronizadas no disponibles"),
            "estado limpio de no disponible"
        );
        assert!(
            !buf.content().iter().any(|c| c.fg == Color::Yellow),
            "la letra plana amarilla antigua no debe aparecer"
        );
    }

    #[test]
    fn explicit_visual_mode_overrides_lyrics() {
        // El usuario eligió "visual" (`v`): aunque haya letras, la banda se
        // dedica al visualizador y no se pisan letras.
        let sync = SyncLyrics::parse("[00:05] una linea oculta\n");
        let buf = render_state(
            Some(sync),
            Duration::from_secs(10),
            false,
            VisualContent::Visual,
            None,
        );
        let text = buffer_text(&buf);
        assert!(
            !text.contains("una linea oculta"),
            "en modo visual no se pintan las letras"
        );
    }

    #[test]
    fn karaoke_colors_map_from_palette() {
        let p = VisualPalette::fallback();
        let (read, cur, unread) = karaoke_colors(&p);
        assert_eq!(
            read,
            Color::Rgb(p.secondary[0], p.secondary[1], p.secondary[2])
        );
        assert_eq!(cur, Color::Rgb(p.primary[0], p.primary[1], p.primary[2]));
        assert_eq!(unread, Color::Rgb(p.accent[0], p.accent[1], p.accent[2]));
    }

    #[test]
    fn band_content_resolves_from_mode() {
        assert!(matches!(
            BandContent::resolve(VisualContent::Auto, true, false),
            BandContent::Lyrics
        ));
        assert!(matches!(
            BandContent::resolve(VisualContent::Auto, false, false),
            BandContent::Visual
        ));
        assert!(matches!(
            BandContent::resolve(VisualContent::Lyrics, false, true),
            BandContent::Unavailable
        ));
        assert!(matches!(
            BandContent::resolve(VisualContent::Lyrics, false, false),
            BandContent::Waiting
        ));
        assert!(matches!(
            BandContent::resolve(VisualContent::Lyrics, true, false),
            BandContent::Lyrics
        ));
        assert!(matches!(
            BandContent::resolve(VisualContent::Visual, true, false),
            BandContent::Visual
        ));
    }
}
