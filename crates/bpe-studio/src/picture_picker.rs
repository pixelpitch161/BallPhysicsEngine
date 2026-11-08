//! A Particles thing's Picture row (editor workflow, W5): the "…" button reads the PNG and the
//! scene keeps its bytes ([`bpe_core::Scene::add_picture`]), so the picture lasts when the file
//! moves. The row names the picture, and says so when it names neither one the scene keeps nor
//! a file there is.

use std::sync::Arc;

use eframe::egui::{self, Response, Ui};

/// What the inspector says when a file chosen for a picture isn't a PNG it can read.
pub const NOT_A_PICTURE: &str = "That file isn't a PNG picture the program can read.";
/// The note under a Picture row naming nothing there is.
pub const NOT_FOUND: &str = "Picture not found: choose it again with …";

/// A picture file's bytes and the name to keep it under (the file's name without `.png`), left
/// for the inspector.
#[derive(Clone, Debug)]
pub struct PictureRequest {
    pub name: String,
    pub bytes: Arc<[u8]>,
}

fn request_id() -> egui::Id {
    egui::Id::unique("picture request")
}

fn note_id() -> egui::Id {
    egui::Id::unique("picture note")
}

/// Shows `note` under the Picture row until a picture is chosen (none clears it).
pub fn set_note(ctx: &egui::Context, note: Option<&str>) {
    ctx.request_repaint();
    ctx.data_mut(|d| match note {
        Some(n) => {
            d.insert_temp(note_id(), n.to_string());
        }
        None => d.remove::<String>(note_id()),
    });
}

/// Leaves `request` for the inspector.
pub fn request(ctx: &egui::Context, request: PictureRequest) {
    ctx.data_mut(|d| d.insert_temp(request_id(), request));
}

/// The request the row left this frame, taken.
pub fn take_request(ctx: &egui::Context) -> Option<PictureRequest> {
    let id = request_id();
    let taken = ctx.data(|d| d.get_temp::<PictureRequest>(id));
    if taken.is_some() {
        ctx.data_mut(|d| d.remove::<PictureRequest>(id));
    }
    taken
}

/// Whether `bytes` are a PNG (its first eight bytes).
pub fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n")
}

/// The name a picture file is kept under: its file name without the extension.
pub fn name_of(path: &std::path::Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Whether `text` names a picture there is: one the scene keeps, or a file (an older scene's
/// path). Empty names nothing and is fine.
pub fn found(text: &str, kept: &[String]) -> bool {
    text.is_empty() || kept.iter().any(|k| k == text) || std::path::Path::new(text).is_file()
}

/// The Picture row's widget: the picture's name, and "…" to choose a PNG. Changed when the name
/// is typed; a chosen file goes to the inspector as a [`PictureRequest`].
pub fn picture_row(ui: &mut Ui, text: &mut String, kept: &[String], label: &str, name: egui::Id) -> Response {
    ui.vertical(|ui| {
        let response = ui
            .horizontal(|ui| {
                let width = (ui.available_width() - 40.0).max(80.0);
                let response = ui.add(egui::TextEdit::singleline(text).desired_width(width)).labelled_by(name);
                let browse = ui.add(egui::Button::new("…").small()).on_hover_text(
                    "Choose a picture (PNG). The scene keeps a copy, so moving the file later doesn't lose it",
                );
                browse.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Browse {label}")));
                if browse.clicked()
                    && let Some(path) = rfd::FileDialog::new().add_filter("PNG pictures", &["png"]).pick_file()
                {
                    // An unreadable file leaves an empty request, which the inspector says isn't a picture.
                    let bytes = std::fs::read(&path).unwrap_or_default();
                    request(ui.ctx(), PictureRequest { name: name_of(&path), bytes: bytes.into() });
                }
                response
            })
            .inner;
        let note = ui.ctx().data(|d| d.get_temp::<String>(note_id()));
        if let Some(note) = note.as_deref().or((!found(text, kept)).then_some(NOT_FOUND)) {
            ui.label(egui::RichText::new(note).color(crate::style::PROBLEM).small());
        }
        response
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_the_scene_keeps_or_a_file_is_found_and_nothing_else_is() {
        let kept = vec!["Logo".to_owned()];
        assert!(found("", &kept));
        assert!(found("Logo", &kept));
        assert!(!found("Gone", &kept));
        let file = std::env::temp_dir().join("bpe-picture-found-test.png");
        std::fs::write(&file, b"x").unwrap();
        assert!(found(&file.to_string_lossy(), &kept));
        let _ = std::fs::remove_file(file);
    }

    #[test]
    fn a_png_is_known_by_its_first_bytes() {
        assert!(is_png(b"\x89PNG\r\n\x1a\nrest"));
        assert!(!is_png(b"GIF89a"));
        assert_eq!(name_of(std::path::Path::new("/a/b/Logo.png")), "Logo");
    }
}
