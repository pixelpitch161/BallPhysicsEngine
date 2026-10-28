//! Ctrl+K: type what you want (a tab, a workspace, a setting, an action) and press Enter.
//!
//! Settings are found by the names the settings panel shows. A setting's row tells the palette
//! its name and tab as it is drawn, so every setting on a tab that has been opened once can be
//! found; animatable settings are listed from the start.

use crate::inspector::Tab;
use crate::theme;
use crate::workspace::Workspace;
use bpe_scene::{Property, Scene};
use eframe::egui::{self, Id, Key, Modifiers, RichText, vec2};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    New,
    Open,
    Save,
    SaveAs,
    Undo,
    Redo,
    Export,
    Explore,
    Randomize,
    PlayPause,
    Restart,
    SpeedMeter,
    TogglePreview,
}

impl Action {
    const ALL: [Action; 13] = [
        Action::PlayPause,
        Action::Restart,
        Action::Export,
        Action::New,
        Action::Open,
        Action::Save,
        Action::SaveAs,
        Action::Undo,
        Action::Redo,
        Action::Explore,
        Action::Randomize,
        Action::SpeedMeter,
        Action::TogglePreview,
    ];

    fn label(self) -> (&'static str, &'static str) {
        match self {
            Action::New => ("New scene from a preset", ""),
            Action::Open => ("Open a scene…", "Ctrl+O"),
            Action::Save => ("Save", "Ctrl+S"),
            Action::SaveAs => ("Save as…", ""),
            Action::Undo => ("Undo", "Ctrl+Z"),
            Action::Redo => ("Redo", "Ctrl+Y"),
            Action::Export => ("Export video…", ""),
            Action::Explore => ("Explore random variations…", ""),
            Action::Randomize => ("Randomize with locks…", ""),
            Action::PlayPause => ("Play or pause", "Space"),
            Action::Restart => ("Back to the start", "Home"),
            Action::SpeedMeter => ("Show or hide the speed meter", "F3"),
            Action::TogglePreview => ("Show or hide the floating preview", ""),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Workspace(Workspace),
    Tab(Tab),
    /// Open the setting's tab and point at it: at the row for `property` when there is one,
    /// otherwise at the row named `label`.
    Setting {
        label: String,
        tab: Tab,
        property: Option<Property>,
    },
    Action(Action),
}

struct Entry {
    command: Command,
    title: String,
    kind: &'static str,
    keys: String,
}

#[derive(Default)]
pub struct CommandPalette {
    pub open: bool,
    query: String,
    highlighted: usize,
    frames: u32,
}

/// Setting names seen in the settings panel, with their tab's place in [`Tab::ALL`].
type Seen = Arc<BTreeSet<(String, usize)>>;

/// The row the palette asked to point at, and when.
#[derive(Clone)]
struct Highlight {
    label: String,
    tab: Tab,
    property: Option<Property>,
    since: f64,
}

const HIGHLIGHT_SECONDS: f64 = 1.6;

fn tab_index(tab: Tab) -> usize {
    Tab::ALL.iter().position(|t| *t == tab).unwrap_or(0)
}

fn seen_id() -> Id {
    Id::unique("command palette settings")
}

fn tab_id() -> Id {
    Id::unique("command palette current tab")
}

fn highlight_id() -> Id {
    Id::unique("command palette highlight")
}

/// Called by the settings panel before it draws a tab.
pub fn drawing_tab(ctx: &egui::Context, tab: Tab) {
    let now = ctx.input(|i| i.time);
    ctx.data_mut(|d| {
        d.insert_temp(tab_id(), tab);
        if d.get_temp::<Highlight>(highlight_id()).is_some_and(|h| now - h.since > HIGHLIGHT_SECONDS) {
            d.remove::<Highlight>(highlight_id());
        }
    });
}

/// Called by each setting row: remembers the setting so the palette can find it, and scrolls
/// to and lights up the row the palette asked for.
pub fn setting_row(response: &egui::Response, label: &str, property: Option<&Property>) {
    let ctx = &response.ctx;
    let Some(tab) = ctx.data(|d| d.get_temp::<Tab>(tab_id())) else { return };
    let key = (label.to_string(), tab_index(tab));
    let known = ctx.data(|d| d.get_temp::<Seen>(seen_id())).is_some_and(|s| s.contains(&key));
    if !known {
        ctx.data_mut(|d| {
            let seen = d.get_temp_mut_or_default::<Seen>(seen_id());
            Arc::make_mut(seen).insert(key);
        });
    }
    let Some(wanted) = ctx.data(|d| d.get_temp::<Highlight>(highlight_id())) else { return };
    let matches = match &wanted.property {
        Some(wanted) => property == Some(wanted),
        None => wanted.label == label && wanted.tab == tab,
    };
    if !matches {
        return;
    }
    let age = ctx.input(|i| i.time) - wanted.since;
    if age < 0.1 {
        response.scroll_to_me(Some(egui::Align::Center));
    }
    if age < HIGHLIGHT_SECONDS {
        let alpha = (1.0 - age / HIGHLIGHT_SECONDS) as f32;
        let rect = response.rect.expand2(vec2(6.0, 3.0));
        ctx.layer_painter(response.layer_id).rect_filled(rect, 6.0, theme::ACCENT.gamma_multiply(0.45 * alpha));
        ctx.request_repaint();
    }
}

/// Lights up a setting row the next time it is drawn.
pub fn point_at(ctx: &egui::Context, label: &str, tab: Tab, property: Option<Property>) {
    let since = ctx.input(|i| i.time);
    let highlight = Highlight { label: label.to_string(), tab, property, since };
    ctx.data_mut(|d| d.insert_temp(highlight_id(), highlight));
}

const ANIMATABLE: [Property; 20] = [
    Property::Gravity,
    Property::SidewaysGravity,
    Property::Bounciness,
    Property::AirDrag,
    Property::SpeedGain,
    Property::ArenaSize,
    Property::ArenaSpin,
    Property::ShrinkSpeed,
    Property::ArenaColor,
    Property::BackgroundTop,
    Property::BackgroundBottom,
    Property::TrailWidth,
    Property::TrailOpacity,
    Property::BallGlow,
    Property::FlashStrength,
    Property::FlashSize,
    Property::Bloom,
    Property::Exposure,
    Property::StringOpacity,
    Property::StringWidth,
];

impl CommandPalette {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.query.clear();
        self.highlighted = 0;
        self.frames = 0;
    }

    fn entries(&self, ctx: &egui::Context, scene: &Scene) -> Vec<Entry> {
        let mut out = Vec::new();
        for (i, workspace) in Workspace::ALL.iter().enumerate() {
            out.push(Entry {
                command: Command::Workspace(*workspace),
                title: format!("{} workspace", workspace.name()),
                kind: "Workspace",
                keys: format!("Ctrl+{}", i + 1),
            });
        }
        for tab in Tab::ALL {
            out.push(Entry {
                command: Command::Tab(tab),
                title: format!("{} tab", tab.name()),
                kind: "Tab",
                keys: String::new(),
            });
        }
        for action in Action::ALL {
            let (title, keys) = action.label();
            out.push(Entry {
                command: Command::Action(action),
                title: title.into(),
                kind: "Action",
                keys: keys.into(),
            });
        }
        // Animatable settings first, under the timeline's names, then every row seen so far.
        let mut settings: BTreeMap<(String, usize), Option<Property>> = ANIMATABLE
            .iter()
            .map(|p| ((crate::keys::name(p, scene), tab_index(crate::keys::tab(p))), Some(p.clone())))
            .collect();
        if let Some(seen) = ctx.data(|d| d.get_temp::<Seen>(seen_id())) {
            for key in seen.iter() {
                settings.entry(key.clone()).or_insert(None);
            }
        }
        for ((label, tab), property) in settings {
            let tab = Tab::ALL[tab];
            out.push(Entry {
                title: label.clone(),
                command: Command::Setting { label, tab, property },
                kind: "Setting",
                keys: tab.name().to_string(),
            });
        }
        out
    }

    fn matches(&self, entries: Vec<Entry>) -> Vec<Entry> {
        let words: Vec<String> = self.query.to_lowercase().split_whitespace().map(str::to_string).collect();
        let mut found: Vec<(u8, Entry)> = entries
            .into_iter()
            .filter_map(|e| {
                let title = e.title.to_lowercase();
                let haystack = format!("{title} {}", e.kind.to_lowercase());
                if !words.iter().all(|w| haystack.contains(w.as_str())) {
                    return None;
                }
                let rank = match words.first() {
                    Some(w) if title.starts_with(w.as_str()) => 0,
                    Some(w) if title.split_whitespace().any(|t| t.starts_with(w.as_str())) => 1,
                    Some(_) => 2,
                    None => 0,
                };
                Some((rank, e))
            })
            .collect();
        found.sort_by_key(|(rank, _)| *rank);
        found.into_iter().map(|(_, e)| e).collect()
    }

    /// Draws the palette while open and returns the command picked this frame.
    pub fn show(&mut self, ctx: &egui::Context, scene: &Scene) -> Option<Command> {
        if !self.open {
            return None;
        }
        self.frames += 1;
        let mut found = self.matches(self.entries(ctx, scene));
        found.truncate(200);
        self.highlighted = self.highlighted.min(found.len().saturating_sub(1));
        let (up, down, enter, escape) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if escape {
            self.open = false;
            return None;
        }
        if !found.is_empty() {
            if down {
                self.highlighted = (self.highlighted + 1) % found.len();
            }
            if up {
                self.highlighted = (self.highlighted + found.len() - 1) % found.len();
            }
        }
        let mut picked = enter.then(|| found.get(self.highlighted).map(|e| e.command.clone())).flatten();
        let modal = egui::Modal::new(Id::unique("command palette")).show(ctx, |ui| {
            ui.set_width(520.0);
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Type a tab, setting or action…")
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Heading),
            );
            if self.frames <= 2 {
                edit.request_focus();
            }
            if edit.changed() {
                self.highlighted = 0;
            }
            ui.add_space(6.0);
            if found.is_empty() {
                ui.label(RichText::new("Nothing matches.").color(theme::MUTED));
            }
            egui::ScrollArea::vertical().max_height(380.0).auto_shrink([false, true]).show(ui, |ui| {
                for (index, entry) in found.iter().enumerate() {
                    let selected = index == self.highlighted;
                    let hint = if entry.keys.is_empty() {
                        entry.kind.to_string()
                    } else {
                        format!("{}  ·  {}", entry.kind, entry.keys)
                    };
                    let button = egui::Button::selectable(selected, entry.title.as_str())
                        .shortcut_text(RichText::new(hint).small())
                        .min_size(vec2(ui.available_width(), 26.0));
                    let row = ui.add(button);
                    if selected && (up || down) {
                        row.scroll_to_me(None);
                    }
                    if row.clicked() {
                        picked = Some(entry.command.clone());
                    }
                }
            });
        });
        if modal.should_close() && self.frames > 1 {
            self.open = false;
        }
        if picked.is_some() {
            self.open = false;
        }
        picked
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;

    struct State {
        palette: CommandPalette,
        scene: Scene,
        picked: Option<Command>,
    }

    fn pick(query: &str) -> Option<Command> {
        let mut palette = CommandPalette::default();
        palette.toggle();
        let state = State { palette, scene: Scene::default(), picked: None };
        let mut harness = Harness::builder().with_size(vec2(900.0, 700.0)).build_ui_state(
            |ui, state: &mut State| {
                if let Some(command) = state.palette.show(ui.ctx(), &state.scene) {
                    state.picked = Some(command);
                }
            },
            state,
        );
        harness.run();
        harness.event(egui::Event::Text(query.into()));
        harness.run();
        harness.key_press(Key::Enter);
        harness.run();
        assert!(!harness.state().palette.open);
        harness.state().picked.clone()
    }

    #[test]
    fn typing_a_setting_finds_its_tab() {
        assert_eq!(
            pick("gravity"),
            Some(Command::Setting { label: "Gravity".into(), tab: Tab::Arena, property: Some(Property::Gravity) })
        );
    }

    #[test]
    fn typing_a_workspace_switches_to_it() {
        assert_eq!(pick("rules work"), Some(Command::Workspace(Workspace::Rules)));
    }

    #[test]
    fn typing_an_action_runs_it() {
        assert_eq!(pick("export"), Some(Command::Action(Action::Export)));
    }
}
