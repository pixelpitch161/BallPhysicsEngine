//! A Text's Font row (Text part 1): a menu of the built-in fonts by group, each name drawn in its
//! own font, the fonts the scene keeps (each with Remove), a search box and Load font….
//!
//! Loading and removing a font change the scene, not the setting, so the picker leaves a
//! [`FontRequest`] in egui's memory and the inspector takes it once the panel is drawn.

use bpe_kit::text::fonts::{BUILT_IN, FontGroup, problem_of};
use eframe::egui::{self, FontId, Response, RichText, Ui};

use crate::style;

/// What the font picker asks the inspector to do to the scene.
#[derive(Clone, Debug, PartialEq)]
pub enum FontRequest {
    /// A font file's bytes and the file's name without its extension (the name when the font
    /// gives none).
    Load(Vec<u8>, String),
    /// Forget the scene's font of this name.
    Remove(String),
}

/// What the Font row needs from the scene.
pub struct FontCx<'a> {
    /// The fonts the scene keeps, by name.
    pub scene_fonts: &'a [String],
}

/// What the inspector says when a file given to Load font isn't a font.
pub const NOT_A_FONT: &str = "That file isn't a font the program can read (.ttf or .otf).";

fn request_id() -> egui::Id {
    egui::Id::unique("font request")
}

fn note_id() -> egui::Id {
    egui::Id::unique("font note")
}

/// Leaves `request` for the inspector.
pub fn request(ctx: &egui::Context, request: FontRequest) {
    ctx.data_mut(|d| d.insert_temp(request_id(), request));
}

/// The request the picker left this frame, taken.
pub fn take_request(ctx: &egui::Context) -> Option<FontRequest> {
    let id = request_id();
    let taken = ctx.data(|d| d.get_temp::<FontRequest>(id));
    if taken.is_some() {
        ctx.data_mut(|d| d.remove::<FontRequest>(id));
    }
    taken
}

/// Shows `note` under the Font row until a font is picked or loaded (none clears it).
pub fn set_note(ctx: &egui::Context, note: Option<&str>) {
    ctx.request_repaint();
    ctx.data_mut(|d| match note {
        Some(n) => {
            d.insert_temp(note_id(), n.to_string());
        }
        None => d.remove::<String>(note_id()),
    });
}

/// The Font row's widget: the current font's name in its own font, opening the picker. Changed
/// when a font is picked; `name` labels the button.
pub fn font_row(ui: &mut Ui, value: &mut String, cx: &FontCx, name: egui::Id) -> Response {
    let ctx = ui.ctx().clone();
    let in_scene = |n: &str| cx.scene_fonts.iter().any(|f| f == n);
    let problem = problem_of(in_scene(value), value);
    let note: Option<String> = ctx.data(|d| d.get_temp(note_id()));
    let mut picked: Option<String> = None;
    let r = ui.vertical(|ui| {
        let family = style::text_family(value);
        let shown = if problem.is_none() && style::family_ready(&ctx, &family) {
            RichText::new(value.as_str()).font(FontId::new(15.0, family))
        } else if problem.is_some() {
            RichText::new(format!("{value} (missing)"))
        } else {
            RichText::new(value.as_str())
        };
        // It stays open while the search box is typed in; picking a font closes it.
        let stay =
            egui::containers::menu::MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
        let menu = egui::containers::menu::MenuButton::new(shown).config(stay).ui(ui, |ui| {
            ui.set_min_width(240.0);
            menu(ui, value, cx, &mut picked);
        });
        let button = menu.0.labelled_by(name);
        for words in problem.iter().chain(note.iter()) {
            ui.add(egui::Label::new(RichText::new(words).small().color(style::PROBLEM)).wrap());
        }
        button
    });
    let mut button = r.inner;
    if let Some(font) = picked {
        set_note(&ctx, None);
        if *value != font {
            *value = font;
            button.mark_changed();
        }
    }
    button
}

/// The picker's contents.
fn menu(ui: &mut Ui, value: &str, cx: &FontCx, picked: &mut Option<String>) {
    let search_id = ui.scope_id().with("font search");
    let mut search: String = ui.data(|d| d.get_temp(search_id)).unwrap_or_default();
    let edit = ui.add(egui::TextEdit::singleline(&mut search).hint_text("Search fonts").desired_width(228.0));
    edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Search fonts"));
    ui.data_mut(|d| d.insert_temp(search_id, search.clone()));
    let wanted = search.trim().to_lowercase();
    let shows = |name: &str| wanted.is_empty() || name.to_lowercase().contains(&wanted);
    let row = |ui: &mut Ui, name: &str, picked: &mut Option<String>| {
        let family = style::text_family(name);
        let text = if style::family_ready(ui.ctx(), &family) {
            RichText::new(name).font(FontId::new(18.0, family))
        } else {
            RichText::new(name)
        };
        let r = ui.selectable_label(value == name, text);
        r.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, value == name, name));
        if r.clicked() {
            *picked = Some(name.to_string());
            ui.close();
        }
    };
    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
        let loaded: Vec<&String> = cx.scene_fonts.iter().filter(|n| shows(n)).collect();
        if !loaded.is_empty() {
            heading(ui, "Loaded");
            for name in loaded {
                ui.horizontal(|ui| {
                    row(ui, name, picked);
                    let remove = ui.small_button("Remove").on_hover_text(
                        "Takes the font out of the scene; words set in it show in IBM Plex Sans until another \
                         font is picked",
                    );
                    let label = format!("Remove {name}");
                    remove.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                    if remove.clicked() {
                        request(ui.ctx(), FontRequest::Remove(name.clone()));
                        ui.close();
                    }
                });
            }
        }
        for group in FontGroup::ALL {
            let names: Vec<&str> =
                BUILT_IN.iter().filter(|b| b.group == group && shows(b.name)).map(|b| b.name).collect();
            if names.is_empty() {
                continue;
            }
            heading(ui, group.name());
            for name in names {
                row(ui, name, picked);
            }
        }
    });
    ui.separator();
    let load = ui.button("Load font…").on_hover_text("Adds a .ttf or .otf font file to the scene, saved inside it");
    if load.clicked() {
        ui.close();
        if let Some(path) = rfd::FileDialog::new().add_filter("Fonts", &["ttf", "otf"]).pick_file() {
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            match std::fs::read(&path) {
                Ok(bytes) => request(ui.ctx(), FontRequest::Load(bytes, stem)),
                Err(_) => set_note(ui.ctx(), Some("That file couldn't be opened.")),
            }
        }
    }
}

fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(text).small().color(style::MUTED));
}
