//! The left column in Video mode: the scenes kept in the video (part 2 plan, decisions 8 and 9).
//! A row picks its scene; dragged onto the Scenes lane it makes a clip there; its menu adds it at
//! the end or takes it out. **+ Add scene** keeps a copy of the scene in Build mode, a scene file or
//! a built-in scene.

use super::{VideoPick, arrange};
use crate::app::{ProblemKind, Studio};
use crate::style;
use bpe_core::Scene;
use bpe_core::video::KeptId;
use eframe::egui::{self, RichText, Sense, Ui};

/// "0:12".
pub fn short_length(seconds: f64) -> String {
    let s = seconds.max(0.0).round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Keeps `scene` in the video as one undo step, and picks it.
pub fn keep(studio: &mut Studio, mut scene: Scene) -> KeptId {
    bpe_kit::ensure_camera(&mut scene, &studio.kit.registry);
    bpe_kit::ensure_mixer(&mut scene, &studio.kit.registry);
    let name = scene.name.clone();
    let mut kept = KeptId(0);
    studio.video.change(false, |video| kept = arrange::keep_scene(video, scene, &name));
    studio.video.pick = VideoPick::Scene(kept);
    let shown = &studio.video.video.scenes[&kept].name;
    studio.status = format!("Added {shown} to the video. Drag it onto the Scenes lane to play it.");
    kept
}

/// Adds kept scene `kept` as a clip at the video's end, picking the clip.
pub fn add_to_end(studio: &mut Studio, kept: KeptId) {
    let kit = studio.kit.clone();
    let mut made = None;
    studio.video.change(false, |video| made = arrange::add_at_end(video, &kit.registry, kept));
    match made {
        Some(id) => studio.video.pick = VideoPick::Clip(id),
        None => studio.status = "The video is full: it can't be longer than 30 minutes.".into(),
    }
}

/// The + Add scene menu.
fn add_menu(ui: &mut Ui, studio: &mut Studio) {
    ui.menu_button("+ Add scene", |ui| {
        if ui
            .button("The scene in Build mode")
            .on_hover_text("A copy: editing it in the video never changes your scene")
            .clicked()
        {
            let scene = studio.scene.clone();
            keep(studio, scene);
            ui.close();
        }
        if ui.button("From a file…").clicked() {
            ui.close();
            let mut dialog = rfd::FileDialog::new().add_filter("Ball Studio scene", &["ron"]);
            if let Some(dir) = studio.video_dir().or_else(|| studio.base_dir()) {
                dialog = dialog.set_directory(dir);
            }
            if let Some(path) = dialog.pick_file() {
                add_file(studio, &path);
            }
        }
        ui.menu_button("Built-in scene", |ui| {
            egui::ScrollArea::vertical().max_height(ui.ctx().content_rect().height() * 0.6).show(ui, |ui| {
                for entry in bpe_kit::library::all() {
                    if ui.button(entry.info.name).clicked() {
                        let scene = entry.scene(&studio.kit.registry);
                        keep(studio, scene);
                        ui.close();
                    }
                }
            });
        });
    })
    .response
    .on_hover_text("Keep a scene in the video: the one in Build mode, a scene file, or a built-in scene");
}

/// Keeps the scene file at `path` in the video.
pub fn add_file(studio: &mut Studio, path: &std::path::Path) -> bool {
    let loaded = std::fs::read_to_string(path)
        .map_err(|e| e.to_string())
        .and_then(|text| bpe_core::load(&text, &studio.kit.registry).map_err(|e| e.to_string()));
    match loaded {
        Ok(mut scene) => {
            if scene.name.trim().is_empty() {
                scene.name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            }
            keep(studio, scene);
            true
        }
        Err(error) => {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            studio.report(ProblemKind::File, format!("Couldn't add {name} to the video: {error}"));
            false
        }
    }
}

/// The column.
pub fn show(ui: &mut Ui, studio: &mut Studio) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("SCENES IN THIS VIDEO").size(12.0).color(style::MUTED));
    });
    ui.add_space(4.0);
    add_menu(ui, studio);
    ui.add_space(6.0);
    if studio.video.video.scenes.is_empty() {
        ui.label(
            RichText::new("No scenes yet. Add one, then drag it onto the Scenes lane.").small().color(style::MUTED),
        );
        return;
    }
    let kit = studio.kit.clone();
    let reg = &kit.registry;
    let rows: Vec<(KeptId, String, bpe_core::TrackColour, f64, usize)> = studio
        .video
        .video
        .scenes
        .iter()
        .map(|(id, k)| {
            let length = bpe_kit::video::scene_length(&k.scene, reg);
            (*id, k.name.clone(), k.colour, length, arrange::clips_of(&studio.video.video, *id))
        })
        .collect();
    let picked = studio.video.pick();
    let mut add_end = None;
    let mut remove = None;
    egui::ScrollArea::vertical().id_salt("video scenes list").show(ui, |ui| {
        for (id, name, colour, length, clips) in rows {
            let height = 40.0;
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::click_and_drag());
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Scene {name}")));
            let on = picked == VideoPick::Scene(id);
            let painter = ui.painter_at(rect);
            let fill = if on {
                style::SELECTED
            } else if response.hovered() {
                style::HOVER
            } else {
                style::PANEL
            };
            painter.rect_filled(rect, style::RADIUS, fill);
            let swatch =
                egui::Rect::from_min_size(rect.left_top() + egui::vec2(6.0, 8.0), egui::vec2(6.0, height - 16.0));
            painter.rect_filled(swatch, 2.0, style::track_color(colour));
            let x = swatch.right() + 8.0;
            painter.text(
                egui::pos2(x, rect.top() + 12.0),
                egui::Align2::LEFT_CENTER,
                &name,
                egui::FontId::proportional(13.0),
                style::TEXT,
            );
            let used = match clips {
                0 => "Not in the video yet: left out when saved".to_owned(),
                1 => "1 clip".to_owned(),
                n => format!("{n} clips"),
            };
            painter.text(
                egui::pos2(x, rect.top() + 28.0),
                egui::Align2::LEFT_CENTER,
                format!("{} · {used}", short_length(length)),
                egui::FontId::proportional(11.0),
                style::MUTED,
            );
            if response.clicked() {
                studio.video.pick = VideoPick::Scene(id);
            }
            if response.double_clicked() {
                add_end = Some(id);
            }
            if response.drag_started() {
                egui::DragAndDrop::set_payload(ui.ctx(), id);
            }
            if response.dragged() {
                // The scene's name follows the pointer while it is carried to the lane.
                if let Some(at) = ui.ctx().pointer_latest_pos() {
                    let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("carried scene"));
                    let painter = ui.ctx().layer_painter(layer);
                    let r = egui::Rect::from_min_size(at + egui::vec2(10.0, 6.0), egui::vec2(140.0, 22.0));
                    let (fill, edge) = style::clip_colours(Some(style::track_color(colour)));
                    painter.rect_filled(r, style::RADIUS, fill);
                    painter.rect_stroke(r, style::RADIUS, egui::Stroke::new(1.0, edge), egui::StrokeKind::Inside);
                    painter.text(
                        r.left_center() + egui::vec2(6.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        &name,
                        egui::FontId::proportional(12.0),
                        style::TEXT,
                    );
                }
            }
            response.context_menu(|ui| {
                if ui.button("Add to the end").clicked() {
                    add_end = Some(id);
                    ui.close();
                }
                if ui.button("Remove from the video").clicked() {
                    remove = Some(id);
                    ui.close();
                }
            });
            ui.add_space(2.0);
        }
    });
    if let Some(id) = add_end {
        add_to_end(studio, id);
    }
    if let Some(id) = remove {
        studio.video.change(false, |video| arrange::remove_scene(video, id));
        studio.status = "Removed the scene and its clips from the video.".into();
    }
}
