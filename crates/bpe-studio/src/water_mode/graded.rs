//! A Water scene's stage in Grade mode (Water mode part 6d): the camera's shot at the playhead,
//! fitted to the room and drawn with its whole finish as the video shows it (the draft, or the
//! final where it is made), under Grade mode's own tools: After | Before | Split and the Split
//! line, the clipping picture (J, Alt), Point Colour and Colour Range picks from the picture, the
//! targeted tool, the selected mask's handles and brush, and the scopes counted from the picture
//! shown. There is no tool rail: the course is built in Build mode.

use bpe_gfx::grade::{ClipView, Clipping, Scopes};
use bpe_gfx::wgpu;
use eframe::egui::{self, Rect, Sense, Ui};

use super::course_view::CourseView;
use super::picture::Shown;
use super::stage::course_size;
use crate::app::Studio;
use crate::style;

/// What the graded stage keeps between frames: its scopes and clipping picture, and the drawing
/// each last worked from.
#[derive(Default)]
pub struct Graded {
    scopes: Option<Scopes>,
    /// The drawing the scopes last counted.
    counted: Option<u64>,
    clipping: Option<(Clipping, egui::TextureId)>,
    /// The drawing and the view the clipping picture was last made from.
    clipped: Option<(u64, ClipView)>,
}

impl Graded {
    /// Lets the clipping picture's texture go.
    pub fn free(&mut self, render: Option<&egui_wgpu::RenderState>) {
        if let (Some((_, id)), Some(render)) = (self.clipping.take(), render) {
            render.renderer.write().free_texture(&id);
        }
        *self = Graded::default();
    }
}

/// The camera's shot at the playhead, fitted into `room`.
pub fn shot_view(studio: &Studio, room: Rect) -> CourseView {
    let (centre, shot) = bpe_cli::water::shot_at(&studio.scene, &studio.kit.registry, studio.time);
    CourseView::fit_shot(room, course_size(studio), centre, shot)
}

/// Draws the graded stage and applies what was done on the picture.
pub fn stage(ui: &mut Ui, studio: &mut Studio) {
    crate::preview::compare_switch(ui, studio);
    let room = ui.available_rect_before_wrap();
    let view = shot_view(studio, room);
    studio.water_stage.view = Some(view);
    let rect = view.seen_rect();
    let response = ui.allocate_rect(rect, Sense::click_and_drag());

    // While a colour is picked from the picture the overlay stays off, so the pick and the scopes
    // read the picture itself.
    studio.grade.picking = crate::eyedropper::target(ui.ctx()).is_some() || crate::grade_mode::target::active(studio);
    let shown = super::stage::water(ui, studio, &view, room);
    if shown.is_none() {
        ui.painter_at(room).rect_filled(rect, 0.0, style::GROUND);
    }
    let render = studio.panels.inspector.render.clone();
    if let (Some(shown), Some(render)) = (&shown, &render) {
        count_scope(studio, shown, render, ui.ctx());
        clip_picture(studio, shown, render);
        if studio.grade.clip_view() != ClipView::Off
            && let Some((_, texture)) = &studio.water_stage.graded.clipping
        {
            let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter_at(room).image(*texture, rect, uv, style::UNTINTED);
        }
    }
    if studio.grade.clipping {
        crate::preview::corner_pill(&ui.painter_at(rect), rect, "Clipping (J)");
    }
    // (Of the picture shown, read back when asked: a pick, or the targeted tool's press.)
    let read = || {
        let (shown, render) = (shown.as_ref()?, render.as_ref()?);
        let pixels =
            bpe_gfx::read_rgba8(&render.device, &render.queue, &shown.texture, shown.size.0, shown.size.1).ok()?;
        Some((pixels, shown.size))
    };
    crate::preview::split_line(ui, &response, rect, studio);
    if crate::eyedropper::target(ui.ctx()).is_some() {
        crate::preview::pick_colour(ui, &response, rect, read);
        return;
    }
    if studio.grade.split_drag {
        return;
    }
    if crate::grade_mode::target::active(studio) {
        let at = |p: egui::Pos2| {
            let uv = (p - rect.min) / rect.size();
            let (pixels, size) = read()?;
            let c = crate::eyedropper::pixel_at(&pixels, size, egui::vec2(uv.x, uv.y))?;
            Some([c.r, c.g, c.b])
        };
        crate::grade_mode::target::preview(ui, &response, rect, studio, at);
    } else {
        // Screen points per pixel of the video: the masks are placed in the video's pixels.
        let video = studio.settings().width.max(1) as f32;
        crate::grade_mode::handles::preview(ui, &response, rect, rect.width() / video, studio);
    }
}

/// Counts the picture shown into the scope Grade mode shows, after each new drawing (or until it
/// has counts of that scope), and takes the counts once the card has them.
fn count_scope(studio: &mut Studio, shown: &Shown, render: &egui_wgpu::RenderState, ctx: &egui::Context) {
    let (device, queue) = (&render.device, &render.queue);
    let kind = studio.grade.scope;
    let graded = &mut studio.water_stage.graded;
    let scopes = graded.scopes.get_or_insert_with(|| Scopes::new(device));
    if scopes.busy() {
        let _ = device.poll(wgpu::PollType::Poll);
    }
    if let Some(counts) = scopes.take()
        && counts.kind == kind
    {
        studio.grade.counts = Some(counts);
        studio.grade.counted += 1;
    }
    let have = studio.grade.counts.as_ref().is_some_and(|c| c.kind == kind);
    studio.grade.recount |= graded.counted != Some(shown.drawing) || !have;
    if studio.grade.recount && !scopes.busy() {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("scopes") });
        let view = shown.texture.create_view(&wgpu::TextureViewDescriptor::default());
        if scopes.encode(device, &mut encoder, &view, shown.size, kind) {
            queue.submit([encoder.finish()]);
            scopes.after_submit();
        }
        graded.counted = Some(shown.drawing);
        studio.grade.recount = false;
    }
    if scopes.busy() {
        // Looked at again soon: the counts come back without anything else asking for a frame.
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}

/// Makes Grade mode's clipping picture (J, Alt) from the picture shown when a view is on and the
/// drawing or the view changed, and points its egui texture at it.
fn clip_picture(studio: &mut Studio, shown: &Shown, render: &egui_wgpu::RenderState) {
    let view = studio.grade.clip_view();
    let graded = &mut studio.water_stage.graded;
    if view == ClipView::Off {
        graded.clipped = None;
        return;
    }
    if graded.clipped == Some((shown.drawing, view)) {
        return;
    }
    let device = &render.device;
    let fresh = graded.clipping.is_none();
    let (clipping, texture) =
        graded.clipping.get_or_insert_with(|| (Clipping::new(device), egui::TextureId::default()));
    let picture = shown.texture.create_view(&wgpu::TextureViewDescriptor::default());
    let made = clipping.run(device, &render.queue, &picture, shown.size, view);
    graded.clipped = Some((shown.drawing, view));
    let Some(target) = clipping.view() else { return };
    if fresh {
        *texture = render.renderer.write().register_native_texture(device, target, wgpu::FilterMode::Linear);
    } else if made {
        render.renderer.write().update_egui_texture_from_wgpu_texture(
            device,
            target,
            wgpu::FilterMode::Linear,
            *texture,
        );
    }
}
