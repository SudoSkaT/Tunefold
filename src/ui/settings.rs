//! Vista de ajustes: formulario editable mínimo (política de reproducción).
//!
//! Con YouTube como única fuente no hay credenciales que configurar; el único
//! campo es `PLAYBACK_POLICY`. Se guarda en `.env` vía el backend.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState};
use ratatui::Frame;

use crate::infrastructure::config::ConfigForm;

/// Descriptor de un campo editable del formulario.
struct FieldDef {
    label: &'static str,
    hint: &'static str,
    /// Acceso al valor dentro de un `ConfigForm`.
    get: fn(&ConfigForm) -> &str,
    set: fn(&mut ConfigForm, String),
}

const FIELDS: [FieldDef; 2] = [
    FieldDef {
        label: "Backend de reproducción (auto/rodio)",
        hint: "auto enruta por fuente; 'rodio' fuerza el motor local.",
        get: |f| &f.playback_policy,
        set: |f, v| f.playback_policy = v,
    },
    FieldDef {
        label: "DSP (JSON)",
        hint: "Parámetros del pipeline DSP serializados; Enter guarda y aplica en vivo.",
        get: |f| &f.dsp_json,
        set: |f, v| f.dsp_json = v,
    },
];

/// Estado del editor de ajustes.
pub struct SettingsForm {
    pub form: ConfigForm,
    pub focus: usize,
    pub list_state: ListState,
}

impl SettingsForm {
    pub fn new(form: ConfigForm) -> Self {
        // Valor por defecto motivo: los campos nuevos llegan vacíos.
        let mut form = form;
        if form.playback_policy.is_empty() {
            form.playback_policy = "auto".to_string();
        }
        Self {
            form,
            focus: 0,
            list_state: ListState::default(),
        }
    }

    pub fn move_focus(&mut self, delta: isize) {
        let len = FIELDS.len() as isize;
        self.focus = ((self.focus as isize + delta).rem_euclid(len)) as usize;
        self.list_state.select(Some(self.focus));
    }

    pub fn focus_index(&self) -> usize {
        self.focus
    }

    fn current_value(&self) -> &str {
        (FIELDS[self.focus].get)(&self.form)
    }

    pub fn insert_char(&mut self, c: char) {
        let (set, get) = (FIELDS[self.focus].set, FIELDS[self.focus].get);
        let mut s = get(&self.form).to_string();
        s.push(c);
        set(&mut self.form, s);
    }

    pub fn backspace(&mut self) {
        let set = FIELDS[self.focus].set;
        let mut s = self.current_value().to_string();
        s.pop();
        set(&mut self.form, s);
    }
}

/// Estado del proxy para el pie del formulario: verdura o guía si no hay.
fn proxy_line(form: &ConfigForm) -> String {
    if form.proxy.is_empty() {
        "Proxy: no configurado (HTTP_PROXY evita cortes de YouTube por IP)".to_string()
    } else {
        format!("Proxy activo: {}", form.proxy)
    }
}

/// Renderiza el formulario de ajustes.
pub fn render(frame: &mut Frame, area: Rect, settings: &mut SettingsForm) {
    let items: Vec<ListItem> = FIELDS
        .iter()
        .enumerate()
        .map(|(i, field)| {
            let raw = (field.get)(&settings.form);
            let line = Line::from(vec![
                Span::styled(
                    format!(" {} ", field.label),
                    Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::raw(raw.to_string()),
            ]);
            let style = if i == settings.focus {
                Style::new().bg(Color::DarkGray)
            } else {
                Style::new()
            };
            ListItem::new(line).style(style)
        })
        .collect();

    let block = Block::default()
        .title(" Settings · Enter guarda en .env · Esc vuelve ")
        .borders(ratatui::widgets::Borders::ALL)
        .title_bottom(Line::from(vec![
            Span::styled(
                format!("{}  ", FIELDS[settings.focus].hint),
                Style::new().fg(Color::DarkGray),
            ),
            Span::styled(
                proxy_line(&settings.form),
                Style::new().fg(if settings.form.proxy.is_empty() {
                    Color::DarkGray
                } else {
                    Color::Green
                }),
            ),
        ]))
        // Resumen de feature flags de proveedores (solo lectura; se controlan
        // desde el entorno/.env, apagables sin recompilar).
        .title_top(Line::from(Span::styled(
            format!(" {} ", settings.form.providers),
            Style::new().fg(Color::DarkGray),
        )));
    let list = List::new(items).block(block).highlight_symbol("> ");
    frame.render_stateful_widget(list, area, &mut settings.list_state);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_auto_policy() {
        let form = SettingsForm::new(ConfigForm::default());
        assert_eq!(form.current_value(), "auto");
    }

    #[test]
    fn editing_appends_and_pops() {
        let mut form = SettingsForm::new(ConfigForm::default());
        let base = form.current_value().to_string();
        form.insert_char('r');
        form.insert_char('o');
        assert_eq!(form.current_value(), format!("{base}ro"));
        form.backspace();
        assert_eq!(form.current_value(), format!("{base}r"));
    }
}
