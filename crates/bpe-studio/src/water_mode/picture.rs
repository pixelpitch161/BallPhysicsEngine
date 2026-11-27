//! The water on the Water stage (Water mode part 2b): the draft's drops drawn in the scene's look
//! (part 6) straight from the card, at the course's size on screen, into a texture egui shows.

use std::time::{Duration, Instant};

use bpe_gfx::wgpu;
use bpe_water::SolidField;
use bpe_water::draw::{Look, View};
use eframe::egui::{self, Painter, Rect, pos2};
use glam::Vec2;

use super::course_view::CourseView;
use super::draft::DraftJob;
use crate::style;

/// The most a side of the picture has, in pixels.
const MOST_SIDE: f32 = 2048.0;
/// How long the stage goes without the draft's body before it waits for the frame the draft is
/// on, so a draft catching up still shows how far it has got.
const LONGEST_WAIT: Duration = Duration::from_millis(100);

/// The picture for the part of the world the view shows (the whole course, or the camera's shot)
/// at `pixels_per_point`: one picture pixel a screen pixel, at most [`MOST_SIDE`] a side and never
/// finer than two a world pixel.
pub fn picture_view(view: &CourseView, pixels_per_point: f32) -> View {
    let (lo, size) = view.seen();
    let size = size.max(Vec2::ONE);
    let scale = (view.scale() * pixels_per_point).clamp(1e-4, 2.0).min(MOST_SIDE / size.x).min(MOST_SIDE / size.y);
    View {
        lo,
        scale,
        width: (size.x * scale).round().max(1.0) as u32,
        height: (size.y * scale).round().max(1.0) as u32,
    }
}

/// The solids the picture shows.
pub struct Rock<'a> {
    /// Goes up whenever `preview` changes.
    pub version: u64,
    /// The course as it will be (a carve stroke being painted, a piece being moved or moulded;
    /// `None`: the body's own field).
    pub preview: Option<&'a SolidField>,
}

/// What a drawn picture showed.
#[derive(Clone, Debug, PartialEq)]
struct Drawn {
    generation: u64,
    frame: u32,
    size: (u32, u32),
    /// The part of the world shown (its corner and scale, as bits): the Shot view moves with the
    /// camera at the same size.
    place: [u32; 3],
    look: Look,
    rock: u64,
}

/// A view's corner and scale as bits, to tell views apart.
pub fn place_of(v: &View) -> [u32; 3] {
    [v.lo.x.to_bits(), v.lo.y.to_bits(), v.scale.to_bits()]
}

/// A picture of the water on the card, as last drawn: what Grade mode's scopes, clipping and
/// colour picks read (`graded`).
#[derive(Clone, Debug)]
pub struct Shown {
    pub texture: wgpu::Texture,
    pub size: (u32, u32),
    /// Different for every drawing, the draft's and the final's.
    pub drawing: u64,
}

impl Shown {
    /// The drawing's picture just drawn from `texture`.
    pub fn new(texture: &wgpu::Texture) -> Shown {
        static DRAWINGS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let drawing = DRAWINGS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Shown { texture: texture.clone(), size: (texture.width(), texture.height()), drawing }
    }
}

/// The stage's picture of the water.
#[derive(Default)]
pub struct StagePicture {
    /// egui's texture, and the body and size it was made for.
    texture: Option<(egui::TextureId, u64, (u32, u32))>,
    drawn: Option<Drawn>,
    last: Option<Instant>,
    /// The body and rock version the drawing's field was last set for, and whether it shows a
    /// stroke being painted.
    rock: Option<(u64, u64)>,
    previewing: bool,
    /// What the drawing told about the last look drawn (a back picture that can't be read).
    notes: Vec<String>,
    shown: Option<Shown>,
}

impl StagePicture {
    /// Draws the draft's water if anything it shows changed, and shows it over the course's rect,
    /// with the `rock` asked for. Returns whether a picture is on the stage.
    pub fn show(
        &mut self,
        painter: &Painter,
        render: &egui_wgpu::RenderState,
        job: &DraftJob,
        look: &Look,
        view: &CourseView,
        rock: Rock,
    ) -> bool {
        let v = picture_view(view, painter.ctx().pixels_per_point());
        let waited_long = self.last.is_none_or(|t| t.elapsed() > LONGEST_WAIT);
        if let Some(mut guard) = job.body(waited_long)
            && let Some(live) = guard.as_mut()
        {
            self.set_rock(live, &rock);
            let want = Drawn {
                generation: live.generation,
                frame: live.water.frames(),
                size: (v.width, v.height),
                place: place_of(&v),
                look: look.clone(),
                rock: rock.version,
            };
            if self.drawn.as_ref() != Some(&want) {
                live.look.set_wet(Some(&live.wet), live.water.frames());
                live.look.draw(&live.water.drop_buffers(), &v, look);
                self.notes = live.look.notes();
                self.shown = live.look.output().map(Shown::new);
                self.drawn = Some(want);
            }
            self.last = Some(Instant::now());
            let made_for = (live.generation, (v.width, v.height));
            if self.texture.is_none_or(|t| (t.1, t.2) != made_for)
                && let Some(output) = live.look.output()
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
        painter.image(id, view.seen_rect(), uv, style::UNTINTED);
        true
    }

    /// Gives the drawing the field of `rock` if it hasn't it yet: the course as it will be while a
    /// stroke is painted or a piece moved, or the body's own again once it is let go or dropped.
    /// (Set here, under the body's lock, so a preview dropped while the draft is busy is never left
    /// on the stage.)
    fn set_rock(&mut self, live: &mut super::draft::Live, rock: &Rock) {
        let now = (live.generation, rock.version);
        if self.rock == Some(now) {
            return;
        }
        if self.rock.is_none_or(|(generation, _)| generation != live.generation) {
            // (A new body's drawing shows its own field.)
            self.previewing = false;
        }
        match rock.preview {
            Some(field) => {
                if field.layout == live.field.layout {
                    live.look.set_field(field);
                    self.previewing = true;
                }
            }
            None if self.previewing => {
                live.look.set_field(&live.field);
                self.previewing = false;
            }
            None => {}
        }
        self.rock = Some(now);
    }

    /// The picture last drawn.
    pub fn shown(&self) -> Option<&Shown> {
        self.shown.as_ref()
    }

    /// The look the picture was last drawn in.
    pub fn last_look(&self) -> Option<&Look> {
        self.drawn.as_ref().map(|d| &d.look)
    }

    /// What the drawing told about the last look drawn, in plain words.
    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    /// Lets the texture go (the scene is no Water scene any more).
    pub fn free(&mut self, render: Option<&egui_wgpu::RenderState>) {
        if let (Some((id, ..)), Some(render)) = (self.texture.take(), render) {
            render.renderer.write().free_texture(&id);
        }
        *self = StagePicture::default();
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
