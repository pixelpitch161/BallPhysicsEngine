//! The bus Filter and Distortion editors: their settings, under the same pictures as the
//! chain's strips (the filter's real response; the distortion's curve).

use super::super::cx::MixCx;
use super::{controls, panel, params};
use crate::sound_tab::strip_views::{self, View};
use crate::style;
use bpe_core::{SettingDef, Settings};
use bpe_kit::sound::{DistortionSettings, FilterSettings, fx};
use eframe::egui::{self, Sense, Ui, Vec2};

pub fn show(ui: &mut Ui, cx: &mut MixCx, kind: &str, defs: &[SettingDef]) {
    let p = params(cx, defs);
    panel(ui, |ui| {
        let width = ui.available_width();
        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 90.0), Sense::hover());
        if kind == fx::FILTER {
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, "Filter response"));
            if ui.is_rect_visible(rect) {
                let set = FilterSettings::from_params(&p);
                strip_views::paint(&ui.painter_at(rect), rect, &View::Filter(&set), style::TEXT);
            }
        } else {
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, "Distortion curve"));
            if ui.is_rect_visible(rect) {
                let set = DistortionSettings::from_params(&p);
                strip_views::paint(&ui.painter_at(rect), rect, &View::Distortion(&set), style::TEXT);
            }
        }
        ui.add_space(4.0);
        controls(ui, cx, defs, &[]);
    });
}
