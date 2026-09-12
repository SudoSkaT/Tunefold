//! Vista Playlists (Shift+8): listado de playlists y detalle de una playlist.
//!
//! El listado muestra metadata persistente (L1K3D primero, protegida) y el
//! detalle reutiliza [`super::related::render_tracks_list`] para la lista de
//! tracks — así las filas, los orígenes y el feedback de reproducción son los
//! mismos que en la vista Related/Now Playing.

use crossterm::event::KeyCode;
use ratatui::prelude::*;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};

use crate::domain::track::Track;
use crate::infrastructure::storage::{PlaylistRow, TrackListeningStats};

use super::liked::Liked;
use super::navigation::ListSelection;
use super::related::{render_tracks_list, RelatedState};

/// Sub-estado de presentación de la vista Playlists.
///
/// `None` en [`Self::detail`] → listado; `Some` → detalle de una playlist
/// (los tracks se guardan en [`Detail::tracks`], reusando `RelatedState`
/// para reaprovechar la lista de canciones de toda la app).
#[derive(Debug, Default)]
pub struct PlaylistState {
    pub detail: Option<Detail>,
    pub listing: ListState,
    /// `true` mientras se escribe el nombre de una playlist nueva (input en
    /// línea sobre el listado).
    pub creating: bool,
    /// Buffer del nombre en edición.
    pub draft: String,
}

/// Detalle abierto: identidad de la playlist y sus tracks.
#[derive(Debug)]
pub struct Detail {
    pub id: i64,
    pub name: String,
    pub tracks: RelatedState,
}

impl Detail {
    pub fn open(id: i64, name: String) -> Self {
        Self {
            id,
            name,
            tracks: RelatedState::default(),
        }
    }
}

/// Acción que la vista delega de vuelta al `App` (envío de comandos al
/// backend / navegación). La vista NO toca el backend directamente.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)] // PlayTrack arrastra el Track completo
pub enum PlaylistAction {
    None,
    /// Abrir el detalle de una playlist (el `App` pedirá sus tracks).
    Open(i64, String),
    /// Reproducir una playlist completa como cola explícita.
    Play(i64),
    /// Reproducir un track concreto del detalle.
    PlayTrack(Track),
    /// Crear una playlist con el nombre recién escrito.
    Create(String),
    /// Borrar una playlist (válido solo si es de usuario).
    Delete(i64),
    /// Quitar un track de la playlist abierta.
    RemoveTrack(i64, i64),
    /// Mover un track dentro de la playlist abierta.
    MoveTrack(i64, i64, usize),
    /// Cerrar el detalle y volver al listado.
    CloseDetail,
    /// Salir de la vista Playlists (a Now Playing).
    LeaveView,
}

/// Teclas de la vista Playlists (delegado desde `App::on_key`).
///
/// - Listado: `n` crea (input en línea), `Enter` abre, `p` reproduce completa,
///   `d` borra (solo playlists de usuario), `Esc` sale.
/// - Detalle: `Enter` reproduce el track, `p` reproduce la lista, `x` quita,
///   `u`/`D` mueven arriba/abajo, `Esc` vuelve al listado.
pub fn handle_key(
    state: &mut PlaylistState,
    key: KeyCode,
    playlists: &[PlaylistRow],
) -> PlaylistAction {
    // Input en línea: consume todo salvo confirmar/cancelar.
    if state.creating {
        return match key {
            KeyCode::Esc => {
                state.creating = false;
                state.draft.clear();
                PlaylistAction::None
            }
            KeyCode::Enter => {
                let name = state.draft.trim().to_string();
                state.creating = false;
                state.draft.clear();
                if name.is_empty() {
                    PlaylistAction::None
                } else {
                    PlaylistAction::Create(name)
                }
            }
            KeyCode::Backspace => {
                state.draft.pop();
                PlaylistAction::None
            }
            KeyCode::Char(c) => {
                state.draft.push(c);
                PlaylistAction::None
            }
            _ => PlaylistAction::None,
        };
    }

    let Some(detail) = state.detail.as_mut() else {
        return handle_listing_key(state, key, playlists);
    };
    handle_detail_key(detail, key)
}

fn handle_listing_key(
    state: &mut PlaylistState,
    key: KeyCode,
    playlists: &[PlaylistRow],
) -> PlaylistAction {
    match key {
        KeyCode::Char('n') => {
            state.creating = true;
            state.draft.clear();
            PlaylistAction::None
        }
        KeyCode::Char('w') | KeyCode::Up => {
            let next =
                super::navigation::step_selection(playlists.len(), state.listing.selected(), false);
            state.listing.select(next);
            PlaylistAction::None
        }
        KeyCode::Char('s') | KeyCode::Down => {
            let next =
                super::navigation::step_selection(playlists.len(), state.listing.selected(), true);
            state.listing.select(next);
            PlaylistAction::None
        }
        KeyCode::Enter => {
            let Some(pl) = state.listing.selected().and_then(|i| playlists.get(i)) else {
                return PlaylistAction::None;
            };
            PlaylistAction::Open(pl.id, pl.name.clone())
        }
        KeyCode::Char('p') => {
            let Some(pl) = state.listing.selected().and_then(|i| playlists.get(i)) else {
                return PlaylistAction::None;
            };
            PlaylistAction::Play(pl.id)
        }
        KeyCode::Char('d') => {
            let Some(pl) = state.listing.selected().and_then(|i| playlists.get(i)) else {
                return PlaylistAction::None;
            };
            if !pl.kind.is_user() {
                return PlaylistAction::None;
            }
            PlaylistAction::Delete(pl.id)
        }
        KeyCode::Esc => PlaylistAction::LeaveView,
        _ => PlaylistAction::None,
    }
}

fn handle_detail_key(detail: &mut Detail, key: KeyCode) -> PlaylistAction {
    match key {
        KeyCode::Char('w') | KeyCode::Up => {
            detail.tracks.step(false);
            PlaylistAction::None
        }
        KeyCode::Char('s') | KeyCode::Down => {
            detail.tracks.step(true);
            PlaylistAction::None
        }
        KeyCode::Enter => match detail.tracks.selected() {
            Some(t) => PlaylistAction::PlayTrack(t.clone()),
            None if detail.tracks.tracks.is_empty() => PlaylistAction::Play(detail.id),
            _ => PlaylistAction::None,
        },
        KeyCode::Char('p') => PlaylistAction::Play(detail.id),
        KeyCode::Char('x') => match detail.tracks.selected() {
            Some(t) if t.id > 0 => PlaylistAction::RemoveTrack(detail.id, t.id),
            _ => PlaylistAction::None,
        },
        KeyCode::Char('u') => match detail.tracks.selected() {
            Some(t) if t.id > 0 => PlaylistAction::MoveTrack(detail.id, t.id, move_to(detail, -1)),
            _ => PlaylistAction::None,
        },
        KeyCode::Char('D') => match detail.tracks.selected() {
            Some(t) if t.id > 0 => PlaylistAction::MoveTrack(detail.id, t.id, move_to(detail, 1)),
            _ => PlaylistAction::None,
        },
        KeyCode::Esc => PlaylistAction::CloseDetail,
        _ => PlaylistAction::None,
    }
}

/// Destino del movimiento (índice nuevo): satura en los extremos de la lista.
fn move_to(detail: &Detail, delta: isize) -> usize {
    let pos = detail.tracks.cursor().unwrap_or(0) as isize;
    let last = detail.tracks.tracks.len().saturating_sub(1);
    (pos + delta).clamp(0, last as isize) as usize
}

impl ListSelection for PlaylistState {
    fn list_len(&self) -> usize {
        if let Some(detail) = &self.detail {
            detail.tracks.list_len()
        } else {
            0
        }
    }

    fn cursor(&self) -> Option<usize> {
        if let Some(detail) = &self.detail {
            detail.tracks.cursor()
        } else {
            self.listing.selected()
        }
    }

    fn set_cursor(&mut self, index: Option<usize>) {
        if let Some(detail) = &mut self.detail {
            detail.tracks.set_cursor(index);
        } else {
            self.listing.select(index);
        }
    }
}

/// Renderiza la vista: detalle (lista de tracks vía `render_tracks_list`) o
/// listado de playlists con su metadata persistente.
#[allow(clippy::too_many_arguments)] // frame, área, estado, listado, claves y ratón son datos del render
pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &mut PlaylistState,
    playlists: &[PlaylistRow],
    current_key: Option<&str>,
    mouse: &Option<(u16, u16)>,
    click: &mut bool,
    stats: &std::collections::HashMap<String, TrackListeningStats>,
    liked: &Liked,
) {
    if let Some(detail) = state.detail.as_mut() {
        let count = detail.tracks.tracks.len();
        let title = format!(" {}  ·  {count} tracks  ", detail.name);
        let empty = if count == 0 {
            "Playlist vacía. Esc para volver · p para reproducirla."
        } else {
            "Enter reproduce · x quita · u/D mueve · p reproduce toda."
        };
        render_tracks_list(
            frame,
            area,
            &mut detail.tracks,
            title,
            empty,
            current_key,
            mouse,
            click,
            stats,
            liked,
        );
        return;
    }

    if state.creating {
        render_create_overlay(frame, area, state);
        return;
    }

    render_listing(frame, area, state, playlists);
}

fn render_listing(
    frame: &mut Frame,
    area: Rect,
    state: &mut PlaylistState,
    playlists: &[PlaylistRow],
) {
    let items: Vec<ListItem> = playlists
        .iter()
        .map(|pl| {
            let system = pl.kind.is_system();
            let g = &crate::ui::glyphs::GLYPHS;
            let line = Line::from(vec![
                Span::styled(
                    if system {
                        g.playlist_system()
                    } else {
                        g.playlist_user()
                    },
                    Style::new()
                        .fg(if system { Color::Magenta } else { Color::Cyan })
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    pl.name.clone(),
                    Style::new()
                        .fg(if system { Color::Magenta } else { Color::White })
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  ·  {} tracks", pl.track_count),
                    Style::new().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("  ·  {}", pl.kind.label()),
                    Style::new().fg(if system {
                        Color::Yellow
                    } else {
                        Color::DarkGray
                    }),
                ),
                if system {
                    Span::styled("  (protegida)", Style::new().fg(Color::DarkGray))
                } else {
                    Span::raw("")
                },
            ]);
            ListItem::new(line)
        })
        .collect();

    let hint = "n crear · Enter abrir · p reproducir · d borrar · w/s navegar · Esc salir";
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Playlists ")
        .title_bottom(Line::from(hint).right_aligned());
    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::new()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, area, &mut state.listing);
}

/// Input en línea sobre el listado para nombrar la playlist nueva.
fn render_create_overlay(frame: &mut Frame, area: Rect, state: &mut PlaylistState) {
    frame.render_widget(Clear, area);
    let dim = Paragraph::new(Line::from(""))
        .block(Block::default().borders(Borders::ALL).title(" Playlists "))
        .style(Style::new().fg(Color::DarkGray));
    frame.render_widget(dim, area);

    let box_area = focused_rect(area, 3);
    let prompt = "Nombre de la playlist (Enter=crear, Esc=cancelar):";
    let g = &crate::ui::glyphs::GLYPHS;
    let input = Paragraph::new(Line::from(vec![
        Span::styled(g.prompt(), Style::new().fg(Color::Green)),
        Span::raw(state.draft.clone()),
        Span::styled(g.edit_cursor(), Style::new().fg(Color::Yellow)),
    ]))
    .block(Block::default().borders(Borders::ALL).title(prompt))
    .style(Style::new());
    frame.render_widget(input, box_area);
}

/// Caja centrada horizontalmente, a un tercio de la altura, para el input.
fn focused_rect(area: Rect, height: u16) -> Rect {
    let w = area.width.saturating_sub(4).clamp(20, 80);
    let h = height.clamp(3, area.height);
    Rect {
        x: area.x + area.width.saturating_sub(w) / 2,
        y: area.y + area.height.saturating_sub(h) / 2,
        width: w,
        height: h,
    }
}
