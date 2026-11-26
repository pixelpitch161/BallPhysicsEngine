//! The water on the Water stage (Water mode part 2b): the draft's drops drawn in the plain look
//! straight from the card, at the course's size on screen, into a texture egui shows.

use std::time::{Duration, Instant};

use bpe_gfx::wgpu;
use bpe_water::draw::{PlainLook, View};
use eframe::egui::{self, Painter, Rect, pos2};
use glam::Vec2;

use super::course_view::CourseView;
use super::draft::DraftJob;
use crate::style;

/// The most a side of the picture has, in pixels.
const MOST_SIDE: f32 = 2048.0;
/// How long the stage waits for the draft's lock before taking it outright, so a draft catching up
/// still shows how far it has got.
const LONGEST_WAIT: Duration = Duration::from_millis(250);

/// The picture for the whole course on screen at `pixels_per_point`: one picture pixel a screen
/// pixel, at most [`MOST_SIDE`] a side and never finer than two a world pixel.
pub fn picture_view(view: &CourseView, pixels_per_point: f32) -> View {
    let size = view.size.max(Vec2::ONE);
    let scale = (view.scale() * pixels_per_point).clamp(1e-4, 2.0).min(MOST_SIDE / size.x).min(MOST_SIDE / size.y);
    View {
        lo: -size / 2.0,
        scale,
        width: (size.x * scale).round().max(1.0) as u32,
        height: (size.y * scale).round().max(1.0) as u32,
    }
}

/// What a drawn picture showed.
#[derive(Clone, Debug, PartialEq)]
struct Drawn {
    generation: u64,
    frame: u32,
    size: (u32, u32),
    look: PlainLook,
    /// Goes up whenever the field the drawing shows changes (a carve stroke being painted).
    field: u64,
}

/// The stage's picture of the water.
#[derive(Default)]
pub struct StagePicture {
    /// egui's texture, and the body and size it was made for.
    texture: Option<(egui::TextureId, u64, (u32, u32))>,
    drawn: Option<Drawn>,
    last: Option<Instant>,
}

impl StagePicture {
    /// Draws the draft's water if anything it shows changed, and shows it over the course's rect.
    /// `field` goes up when the drawing's field was changed under it. Returns whether a picture
    /// is on the stage.
    pub fn show(
        &mut self,
        painter: &Painter,
        render: &egui_wgpu::RenderState,
        job: &DraftJob,
        look: &PlainLook,
        view: &CourseView,
        field: u64,
    ) -> bool {
        let v = picture_view(view, painter.ctx().pixels_per_point());
        let waited_long = self.last.is_none_or(|t| t.elapsed() > LONGEST_WAIT);
        let guard = if waited_long { job.live().lock().ok() } else { job.live().try_lock().ok() };
        if let Some(mut guard) = guard
            && let Some(live) = guard.as_mut()
        {
            let want = Drawn {
                generation: live.generation,
                frame: live.water.frames(),
                size: (v.width, v.height),
                look: look.clone(),
                field,
            };
            if self.drawn.as_ref() != Some(&want) {
                live.plain.draw(&live.water.drop_buffers(), &v, look);
                self.drawn = Some(want);
            }
            self.last = Some(Instant::now());
            let made_for = (live.generation, (v.width, v.height));
            if self.texture.is_none_or(|t| (t.1, t.2) != made_for)
                && let Some(output) = live.plain.output()
            {
                let output = output.create_view(&wgpu::TextureViewDescriptor::default());
                let mut renderer = render.renderer.write();
                let id = match self.texture {
                    Some((id, ..)) => {
                        renderer.update_egui_texture_from_wgpu_texture(
                            &render.device,
                            &output,
                            wgpu::FilterMode::Linear,
                            id,
                        );
                        id
                    }
                    None => renderer.register_native_texture(&render.device, &output, wgpu::FilterMode::Linear),
                };
                self.texture = Some((id, made_for.0, made_for.1));
            }
        }
        let Some((id, ..)) = self.texture else { return false };
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        painter.image(id, view.rect, uv, style::UNTINTED);
        true
    }

    /// Lets the texture go (the scene is no Water scene any more).
    pub fn free(&mut self, render: Option<&egui_wgpu::RenderState>) {
        if let (Some((id, ..)), Some(render)) = (self.texture.take(), render) {
            render.renderer.write().free_texture(&id);
        }
        self.drawn = None;
        self.last = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_kit::modules::water::SCREEN;
    use eframe::egui::vec2;

    #[test]
    fn the_picture_covers_the_course_view() {
        let room = Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 900.0));
        let view = CourseView::fit(room, SCREEN);
        let v = picture_view(&view, 2.0);
        assert_eq!(v.lo, Vec2::new(-540.0, -960.0));
        assert_eq!(v.width, (view.rect.width() * 2.0).round() as u32);
        assert!((v.scale - view.rect.width() * 2.0 / 1080.0).abs() < 1e-5);
        assert_eq!(v.height, (1920.0 * v.scale).round() as u32);
        // A course of four screens in a huge room: no side past 2048.
        let big = CourseView::fit(Rect::from_min_size(pos2(0.0, 0.0), vec2(9000.0, 9000.0)), SCREEN * 2.0);
        let v = picture_view(&big, 2.0);
        assert_eq!((v.width, v.height), (1152, 2048));
        // Never finer than two picture pixels a world pixel.
        let tiny = CourseView::fit(Rect::from_min_size(pos2(0.0, 0.0), vec2(9000.0, 9000.0)), Vec2::new(100.0, 100.0));
        assert_eq!(picture_view(&tiny, 2.0).width, 200);
    }
}
