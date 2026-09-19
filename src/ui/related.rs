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

use super::liked::Liked;
use super::widgets::karaoke::KaraokeScroller;
use super::VisualContent;
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
    /// El visualizador (osciloscopio + barras) ocupa la banda.
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
    liked: &Liked,
) {
    let has_lyrics = state.synced.as_ref().filter(|s| !s.is_empty()).is_some();
    // La banda superior es una sección permanente de la vista (relacionadas,
    // letras o visual) y se conserva en TODO perfil: solo cambia su altura
    // (`layout::related_band_height`), nunca su presencia. En perfiles bajos
    // se reduce hasta 3 filas y el resto del espacio va a la lista.
    let profile = super::layout::TerminalProfile::from_rect(area);
    let band_height = super::layout::related_band_height(area.height, profile);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(band_height), Constraint::Min(0)])
        .split(area)
        .to_vec();

    {
        let top_area = chunks[0];
        // El osciloscopio vive tras el texto: fondo plano + trazo atenuado de
        // puntos (nunca bloques), y encima el contenido (letras o mensaje).
        let trace_inner = top_area.inner(Margin {
            horizontal: 1,
            vertical: 1,
        });
        let paint_ambient = |frame: &mut Frame| {
            visualizer::render_backdrop(frame, top_area, visual, true);
            if trace_inner.width > 0 && trace_inner.height > 0 {
                visualizer::render_trace_subdued(frame, trace_inner, visual);
            }
        };
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
                paint_ambient(frame);
                let colors = visual.scene.palette.karaoke_colors();
                render_karaoke_over_scene(frame, top_area, state, position, finished, colors);
            }
            BandContent::Unavailable => {
                paint_ambient(frame);
                render_message_over_scene(
                    frame,
                    top_area,
                    " Letras ",
                    "Letras sincronizadas no disponibles",
                );
            }
            BandContent::Waiting => {
                paint_ambient(frame);
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
        liked,
    );
}

/// Prepara el estado del karaoke (línea activa, fin de letra, paleta) y delega
/// el render en el overlay que preserva la capa ambiental.
///
/// Los tres estados (ya leído / en lectura / no leído) reciben los colores de
/// la paleta fundida de la portada con contraste garantizado contra la escena
/// aplacada (χ ≥ 4.5 activa, χ ≥ 3.0 históricas; ver `palette.rs`). El panel
/// se limpia cuando la reproducción terminó de verdad (`finished` = estado
/// `Stopped` del motor).
fn render_karaoke_over_scene(
    frame: &mut Frame,
    area: Rect,
    state: &mut RelatedState,
    position: Option<Duration>,
    finished: bool,
    colors: crate::visualization::palette::KaraokeColors,
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
    super::widgets::karaoke::render_over_scene(
        frame,
        area,
        &mut state.scroll,
        sync,
        active,
        finished,
        (
            Color::Rgb(colors.read[0], colors.read[1], colors.read[2]),
            Color::Rgb(colors.current[0], colors.current[1], colors.current[2]),
            Color::Rgb(colors.unread[0], colors.unread[1], colors.unread[2]),
        ),
    );
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
#[allow(clippy::too_many_arguments)] // ratón/estadísticas/liked son datos del render
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
    liked: &Liked,
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
            // Doblete funcional discreto: el track en curso lleva su marcador;
            // lo añadido por el autoplay se marca tenue; lo recién añadido,
            // verde. Los glifos vienen del sistema centralizado (con respaldo
            // ASCII).
            let g = &crate::ui::glyphs::GLYPHS;
            let marker = if is_current {
                Span::styled(
                    g.current(),
                    Style::new().fg(Color::Green).add_modifier(Modifier::BOLD),
                )
            } else if is_new {
                Span::styled(g.newly_added(), Style::new().fg(Color::Green))
            } else if origin.is_auto() {
                Span::styled(g.auto(), Style::new().fg(Color::DarkGray))
            } else {
                Span::styled(g.explicit(), Style::new().fg(Color::DarkGray))
            };
            let line = Line::from(vec![
                marker,
                Span::styled(
                    format!("[{}] ", t.source.label()),
                    Style::new().fg(Color::Cyan),
                ),
                Span::raw(format!("{artist} - {}", t.title)),
                // L1K3D: corazón lleno y cálido para las canciones que el
                // usuario ya marcó como "me gusta" (presentes en cualquier
                // lista de la app, no solo en la playlist propia).
                if liked.contains(t) {
                    Span::styled(
                        format!("   {}", g.heart_liked()),
                        Style::new()
                            .fg(Color::LightRed)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::raw("")
                },
                Span::styled(format!("  ({duration})"), Style::new().fg(Color::DarkGray)),
                listened
                    .map(|s| {
                        Span::styled(
                            if s.recently_played {
                                format!("  {} reciente · {}×", g.recent(), s.play_count)
                            } else {
                                format!("  {} {}×", g.listens(), s.play_count)
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
    use crate::visualization::palette::VisualPalette;
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
                    &Liked::default(),
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
        let cover = Some([[255, 0, 0], [0, 255, 0], [0, 0, 255]]);
        // Los estados adoptan la paleta de la portada YA resuelta por contraste
        // (la activa conserva el tinte cuando cumple; las demás se iluminan si
        // el tercer color azul puro no alcanzaba 3:1 sobre la escena aplacada).
        let colors = VisualPalette::from_cover(cover).karaoke_colors();
        // En posición 12s: "uno" (ya leído), "dos" (en lectura), "tres" (no leído).
        let buf = render_state(
            Some(sync),
            Duration::from_secs(12),
            false,
            VisualContent::Auto,
            cover,
        );
        let cells: Vec<(&str, Color)> = buf.content().iter().map(|c| (c.symbol(), c.fg)).collect();
        let active = Color::Rgb(colors.current[0], colors.current[1], colors.current[2]);
        let read = Color::Rgb(colors.read[0], colors.read[1], colors.read[2]);
        let unread = Color::Rgb(colors.unread[0], colors.unread[1], colors.unread[2]);
        assert!(
            cells.iter().any(|(s, fg)| *s == "d" && *fg == active),
            "en lectura = color dominante resuelto"
        );
        assert!(
            cells.iter().any(|(s, fg)| *s == "u" && *fg == read),
            "ya leído = segundo color resuelto"
        );
        assert!(
            cells.iter().any(|(s, fg)| *s == "t" && *fg == unread),
            "no leído = tercer color resuelto"
        );
        // La activa (rojo) conserva dominancia roja: el primer canal es el
        // mayor de los tres (puede ser ajustado hacia blanco por contraste
        // cuando el techo de brillo sube por un canal osciloscopio brillante,
        // pero la dominancia cromática se preserva).
        assert!(
            colors.current[0] >= colors.current[1] && colors.current[0] >= colors.current[2],
            "activa conserva dominancia roja: {:?}",
            colors.current
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
                    &Liked::default(),
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
    fn karaoke_colors_resolve_for_contrast() {
        // La resolución adapta los tres estados garantizando contraste contra
        // el techo del fondo (χ ≥ 4.5 activo, χ ≥ 3.0 históricos).
        let p = VisualPalette::fallback();
        let colors = p.karaoke_colors();
        let bg = p.karaoke_bg_ceiling();
        let ratio = crate::visualization::palette::contrast_ratio;
        assert!(ratio(colors.current, bg) >= 4.5, "activa legible");
        assert!(ratio(colors.read, bg) >= 3.0, "leída legible");
        assert!(ratio(colors.unread, bg) >= 3.0, "no leída legible");
    }

    #[test]
    fn loud_waveform_does_not_block_or_mottle_lyrics_band() {
        // Con señal fuerte Y letras: la banda superior muestra el texto sobre
        // un fondo plano del techo de contraste — sin puntos del trazo (que
        // aquí no se pintan), sin bloques de espectro y sin moteado del glow
        // por columna que se superponga a las letras.
        use crate::analysis::WaveformEnvelope;
        use crate::visualization::engine::{SceneState, WaveformView};

        let samples: Vec<f32> = (0..2048)
            .map(|i| 0.8 * ((i as f32 / 2048.0) * std::f32::consts::TAU * 6.0).sin())
            .collect();
        let env = WaveformEnvelope::from_window(&samples);
        let palette = VisualPalette::fallback();
        let visual = VisualState {
            bars: [0.9; crate::visualization::VISUAL_BARS],
            level: 0.9,
            intensity: 0.9,
            pulse: 0.5,
            phase: 0.25,
            active: true,
            scene: SceneState {
                waveform: WaveformView {
                    left: env,
                    right: env,
                    gain: 1.0,
                },
                energy: 0.8,
                brightness: 0.5,
                palette,
                active: true,
            },
        };
        let sync = SyncLyrics::parse("[00:05] primera linea\n[00:10] segunda linea\n");
        let mut state = RelatedState {
            synced: Some(sync),
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
                    Some(Duration::from_secs(7)),
                    false,
                    VisualContent::Auto,
                    None,
                    &visual,
                    &None,
                    &mut false,
                    &std::collections::HashMap::new(),
                    &Liked::default(),
                )
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();

        let profile = crate::ui::layout::TerminalProfile::from_rect(buf.area);
        let band_h = crate::ui::layout::related_band_height(buf.area.height, profile);
        assert!(band_h >= 5, "banda con sitio para letras en 60x16");

        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(
            text.contains("primera linea"),
            "la letra se ve con señal fuerte"
        );

        // Interior de la banda (sin bordes): letras + puntos atenuados del
        // osciloscopio; nada de bloques de espectro ni moteado. El fondo es
        // plano del techo en TODAS las celdas, también bajo el texto y los
        // puntos (el trazo no toca el fondo).
        const BLOCKS: [&str; 11] = ["█", "▇", "▆", "▅", "▄", "▃", "▂", "▁", "●", "░", "▒"];
        let ceiling = palette.karaoke_bg_ceiling();
        let expected = Color::Rgb(ceiling[0], ceiling[1], ceiling[2]);
        let mut points = 0usize;
        for y in 1..band_h.saturating_sub(1) {
            for x in 1..59u16 {
                let cell = buf.cell((x, y)).unwrap();
                assert!(
                    !BLOCKS.contains(&cell.symbol()),
                    "sin bloques sobre las letras ({x},{y}={:?})",
                    cell.symbol()
                );
                assert_eq!(
                    cell.bg, expected,
                    "fondo plano tras letras y puntos ({x},{y})"
                );
                if matches!(cell.symbol(), "•" | "*") {
                    points += 1;
                }
            }
        }
        assert!(
            points > 0,
            "el osciloscopio sigue vivo tras las letras (puntos atenuados)"
        );
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
