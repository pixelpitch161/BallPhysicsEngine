//! What to draw for one frame, built on the CPU by each thing's drawing part.
//!
//! This file holds the [`Frame`] itself, its blend layers' records and what every frame does
//! (clear, what it gives off). The rest, by topic:
//!
//! - `build.rs`: building a frame from a scene ([`ThingDraw`], [`ModuleDraw`], [`build_frame`]).
//! - `instances.rs`: the GPU records of balls, layers and path pieces; `trails.rs`, `particles.rs`
//!   and `paths.rs`: trails and streaks, sprites and fields, shapes and text.
//! - `layers.rs`: blend layers and the overlay; `cull.rs`: the view, list limits and culling.
//! - `light.rs`: what the lighting reads ([`LightScene`]); `effects.rs`: effect calls, the
//!   background and the grade; `ripples.rs`: ripple sources.

use std::borrow::Cow;
use std::collections::BTreeMap;

use bpe_core::{Animation, Blend, Inputs, Resolved, ResolvedThing, Scene, SlotId, Thing, ThingId};
use bpe_runtime::{CameraView, LevelsAt, LightAt, LightLevelsAt, Log, SoundAt, World};
use bytemuck::{Pod, Zeroable};
use glam::Vec2;

use crate::gradients::{GradientAtlas, PaintGpu};

pub use bpe_runtime::hit_strength;

mod build;
mod cull;
mod effects;
mod instances;
mod layers;
mod light;
mod particles;
mod paths;
mod ripples;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tint_tests;
mod trails;

pub use build::*;
pub use effects::*;
pub use instances::*;
use layers::*;
pub use light::*;
pub use particles::*;
pub use ripples::*;
pub use trails::*;

/// Everything the renderer needs for one frame: the picture's globals and the instance lists.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// The video's size in pixels; world coordinates are these pixels, origin at the centre, y
    /// down.
    pub width: u32,
    pub height: u32,
    pub time: f32,
    /// The colours behind everything, painted by the background pass.
    pub background: BackgroundPaint,
    pub bloom: f32,
    pub exposure: f32,
    /// Shapes, and the points (xy, two spare) their outlines run through.
    pub paths: Vec<PathInstance>,
    pub path_points: Vec<[f32; 4]>,
    pub flashes: Vec<FlashInstance>,
    /// Balls drawn with a stack of layers, and the stacks' records they name.
    pub layered: Vec<LayeredInstance>,
    pub look_layers: Vec<LayerGpu>,
    /// The brush tiles the records name (`MASK_TILE`² bytes each) with a hash to tell them apart.
    pub mask_tiles: Vec<(u64, std::sync::Arc<[u8]>)>,
    /// The in-ball programs stacks with shader layers draw with, and which instances of
    /// `layered` draw with which (in order). No runs draws every instance with the plain
    /// pipeline.
    pub programs: Vec<crate::LookProgram>,
    pub layered_runs: Vec<crate::LayeredRun>,
    pub trails: Vec<TrailVertex>,
    /// Particles and trail bits behind the balls, drawn right after `trails`.
    pub particles: Vec<Particle>,
    /// Trails and particles in front of the balls, drawn after them (and after the effects on
    /// things), trails first.
    pub trails_front: Vec<TrailVertex>,
    pub particles_front: Vec<Particle>,
    /// Soft fields of particles behind the balls and in front of them, each drawn among the
    /// sprites of `particles` / `particles_front` where its `at` says.
    pub fields: Vec<Field>,
    pub fields_front: Vec<Field>,
    /// Huge swarms (particles part 8) behind the balls and in front of them, each drawn from its
    /// particle buffers among the sprites of `particles` / `particles_front` where its `at` says.
    pub swarms: Vec<crate::SwarmRun>,
    pub swarms_front: Vec<crate::SwarmRun>,
    /// Recent hits, filled by the things' drawing parts for the modules' ones.
    pub hits: Vec<HitMark>,
    pub effects: Vec<EffectCall>,
    /// The gradients the instances paint with, one atlas row each.
    pub gradients: GradientAtlas,
    /// What the camera shows: everything above is drawn through it.
    pub camera: CameraView,
    /// The background's place is measured on the screen, still while the camera moves; off, it
    /// is measured in the world and moves with the camera.
    pub background_on_screen: bool,
    /// Things stuck to the screen, drawn after the main lists with no camera. Its own camera,
    /// background and overlay are unused.
    pub overlay: Option<Box<Frame>>,
    /// The scene's video size. Usually `width` by `height`; the editor's World view draws a
    /// picture of another shape, and the GPU layers keep stepping in the video's.
    pub video: (u32, u32),
    /// Things drawn with a blend other than Normal at full amount, one layer per blend and
    /// amount, in the order they were first drawn into. Each is mixed into the picture after
    /// this frame's own lists.
    pub layers: Vec<BlendLayer>,
    /// The colour grade run over the finished picture. Only the top frame's counts.
    pub grade: Grade,
    /// The scene's lights, when it has a Lighting card (only the top frame's counts). `None`
    /// draws the picture exactly as it was before lighting.
    pub light: Option<LightScene>,
    /// Every live ball, for the picture effects' ball list (filled only when the scene has an
    /// Effects card). Blend layers' frames hold the balls drawn into them.
    pub ball_marks: Vec<BallMark>,
    /// Whether some paint in the frame lays its colours out as Ripples: things then push their
    /// [`RippleSource`]s and the renderer builds the distance field Ripples read. Blend layers'
    /// frames take it from the frame they are made in.
    pub ripples: bool,
    /// What Ripples measure from (balls and shape outlines), in this frame and its layers' frames.
    pub ripple_sources: Vec<RippleSource>,
    /// The last owner number given out by [`Frame::ripple_owner`].
    pub ripple_owners: u32,
    /// The beat at this moment: its phase 0 to 1, the bar's phase 0 to 1, beats per second, and
    /// 1 when the scene has a tempo (else all 0). Only the top frame's counts.
    pub beat: [f32; 4],
    /// Which sample of a baked frame this is (lighting part 3), `None` drawn live.
    pub bake: Option<crate::bake::BakeSample>,
    /// Liquids (particles part 6): particles run together into a surface, each drawn behind the
    /// balls (after the trails and sprites behind them) or in front of them (after the flashes).
    pub liquids: Vec<crate::liquid::Liquid>,
}

/// Where a layer's things draw among the others of their z-order, in drawing order. At z-order 0
/// the frame's own lists are its plain things: trails and shapes, then the `Behind` and
/// `BehindOver` layers, then its balls and flashes, then the `Blend` and `Over` layers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Place {
    /// Plain shapes of a z-order other than 0 (drawn straight in, not mixed).
    Back,
    /// Blended things that sit behind the balls (shapes): mixed with what is behind them only.
    Behind,
    /// What a split shape paints normally over its blended part, still behind the balls.
    BehindOver,
    /// Plain balls, trails and particles of a z-order other than 0 (drawn straight in).
    Front,
    /// Other blended things.
    #[default]
    Blend,
    /// What a split ball paints normally over its blended part.
    Over,
}

/// What things with one blend and amount drew, mixed into the picture under them as one.
#[derive(Clone, Debug, PartialEq)]
pub struct BlendLayer {
    pub blend: Blend,
    /// 0 to 1.
    pub amount: f32,
    /// Its things blend with each other by `blend` too, not only with the picture under them:
    /// the layer starts as the blend's identity, each thing is mixed into it by the blend, and
    /// the layer then counts as covering everywhere one drew. Only for a blend that
    /// [combines with itself](Blend::combines_with_itself).
    pub each: bool,
    /// The z-order of its things: layers below 0 are drawn before the frame's own lists, above
    /// 0 after everything at 0.
    pub z: i32,
    /// Where it draws among the layers of its z-order.
    pub place: Place,
    /// The layer's own lists. Its globals follow the frame it belongs to; it paints from that
    /// frame's gradient atlas, so its own `gradients` stay empty, and the hits and effects drawn
    /// into it are handed to that frame.
    pub frame: Frame,
}

impl BlendLayer {
    /// Its things sit behind the balls of its z-order (shapes).
    pub fn behind(&self) -> bool {
        matches!(self.place, Place::Back | Place::Behind | Place::BehindOver)
    }

    /// Whether it is drawn after the frame's own things and the Things-stage effects on them:
    /// a higher z-order, or at 0 not behind the balls.
    pub fn above(&self) -> bool {
        self.z > 0 || (self.z == 0 && !self.behind())
    }

    /// A layer of things drawn straight into the picture, not mixed (things at a z-order
    /// other than 0 that don't blend).
    pub fn plain(&self) -> bool {
        matches!(self.place, Place::Back | Place::Front)
    }
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            width: 1080,
            height: 1920,
            time: 0.0,
            background: BackgroundPaint::default(),
            bloom: 0.0,
            exposure: 1.0,
            paths: Vec::new(),
            path_points: Vec::new(),
            flashes: Vec::new(),
            layered: Vec::new(),
            look_layers: Vec::new(),
            mask_tiles: Vec::new(),
            programs: Vec::new(),
            layered_runs: Vec::new(),
            trails: Vec::new(),
            particles: Vec::new(),
            trails_front: Vec::new(),
            particles_front: Vec::new(),
            fields: Vec::new(),
            fields_front: Vec::new(),
            swarms: Vec::new(),
            swarms_front: Vec::new(),
            hits: Vec::new(),
            effects: Vec::new(),
            gradients: GradientAtlas::default(),
            camera: CameraView::default(),
            background_on_screen: true,
            overlay: None,
            video: (1080, 1920),
            layers: Vec::new(),
            grade: Grade::default(),
            light: None,
            ball_marks: Vec::new(),
            ripples: false,
            ripple_sources: Vec::new(),
            ripple_owners: 0,
            beat: [0.0; 4],
            bake: None,
            liquids: Vec::new(),
        }
    }
}

impl Frame {
    /// Empties the instance lists and the gradients and keeps the globals.
    pub fn clear(&mut self) {
        self.paths.clear();
        self.path_points.clear();
        self.flashes.clear();
        self.layered.clear();
        self.look_layers.clear();
        self.mask_tiles.clear();
        self.programs.clear();
        self.layered_runs.clear();
        self.trails.clear();
        self.particles.clear();
        self.trails_front.clear();
        self.particles_front.clear();
        self.fields.clear();
        self.fields_front.clear();
        self.liquids.clear();
        self.swarms.clear();
        self.swarms_front.clear();
        self.hits.clear();
        self.effects.clear();
        self.gradients.clear();
        self.background = BackgroundPaint::default();
        self.camera = CameraView::default();
        self.layers.clear();
        self.grade = Grade::default();
        self.light = None;
        self.ball_marks.clear();
        self.ripples = false;
        self.ripple_sources.clear();
        self.ripple_owners = 0;
        self.beat = [0.0; 4];
        self.bake = None;
        if let Some(overlay) = &mut self.overlay {
            overlay.clear();
        }
    }

    /// The longest memory (seconds) of this frame's effects and its glow in the dark: how far
    /// back the editor replays after a seek so the picture matches a straight run (0 when
    /// nothing remembers). Remembered light fades by e^(-t / fade), under 1/4096 after about
    /// 8.3 fades.
    pub fn memory(&self) -> f32 {
        let glow = self
            .light
            .as_ref()
            .filter(|l| l.settings.remembers())
            .map_or(0.0, |l| (l.settings.memory[1].max(1e-3) * 4096f32.ln()).min(crate::effects::MAX_MEMORY));
        self.effects.iter().filter(|e| e.blend != 0.0).map(|e| e.memory).fold(glow, f32::max)
    }

    /// Whether this frame's own lists (not its layers') draw the thing with `code` (see
    /// [`thing_code`]) into the object picture: a ball or a shape of it.
    pub(crate) fn draws_thing(&self, code: f32) -> bool {
        let is = |c: f32| (c.floor() - code).abs() < 0.5;
        self.layered.iter().any(|l| is(l.misc[1])) || self.paths.iter().any(|p| is(p.c[0]))
    }

    /// Whether anything it (or a layer or its overlay) draws gives off light: a layer record, a
    /// trail vertex or a particle with an emission.
    pub fn gives_light(&self) -> bool {
        let on = |e: &[f32; 4]| e[..3].iter().any(|c| *c != 0.0);
        // A Heat glow that lights the room counts too (lighting part 4).
        let heat = |r: &LayerGpu| r.head[0] == LayerGpu::SUB_FLASH && r.amount[3] > 0.5 && r.amount[2] > 0.0;
        self.look_layers.iter().any(|r| r.emit.iter().any(on) || heat(r))
            || self.trails.iter().chain(&self.trails_front).any(|v| on(&v.emit))
            || self.particles.iter().chain(&self.particles_front).any(|p| on(&p.emit))
            || self.layers.iter().any(|l| l.frame.gives_light())
            || self.overlay.as_deref().is_some_and(Frame::gives_light)
    }

    /// Whether anything it (or a layer or its overlay) draws blocks light but lets some through
    /// (glass and light part 1): a record that blocks light at less than 1 or with an opacity
    /// under 1, a layered ball at an opacity under 1, or any trail, particle or field that blocks
    /// light (their soft edges let light through). Only then is the tint picture drawn. It may
    /// answer yes for a frame whose see-through parts end up hidden: that costs only the pass.
    pub fn see_through_blockers(&self) -> bool {
        let part = |b: f32| b > 0.0 && b < 1.0;
        self.look_layers.iter().any(|r| part(r.light[1]) || (r.light[1] > 0.0 && r.amount[0] < 1.0))
            || self.layered.iter().any(|l| l.misc[0] < 1.0)
            || self.trails.iter().chain(&self.trails_front).any(|v| v.light[1] > 0.0)
            || self.particles.iter().chain(&self.particles_front).any(|p| p.light[1] > 0.0)
            || self.fields.iter().chain(&self.fields_front).any(|f| f.look.light[1] > 0.0)
            || self.liquids.iter().any(|l| l.style.real() && part(l.style.d[2]))
            || self.layers.iter().any(|l| l.frame.see_through_blockers())
            || self.overlay.as_deref().is_some_and(Frame::see_through_blockers)
    }

    /// Glass and light part 3: the box round everything drawn as free glass (particles whose
    /// layer bends light, `Particle::light[2]` above 1, and Glass liquids that bend light,
    /// `LiquidStyle::c[2]`), in the frame and its blend layers' frames (not the overlay: what is
    /// stuck to the screen bends no lamp light), as a [`FREE_BENDER`]; `None` when nothing is.
    pub fn free_glass_box(&self) -> Option<Bender> {
        let sprites =
            self.particles.iter().chain(&self.particles_front).filter(|p| p.light[2] > BENDS_FROM).filter_map(|p| {
                let reach = p.place[3].abs().max(p.style[0].abs()) * 1.5 + 2.0;
                Bender::round([p.at()], reach, FREE_BENDER)
            });
        let blobs = self.liquids.iter().filter(|l| l.style.c[2] > BENDS_FROM).flat_map(|l| {
            l.blobs.iter().filter_map(|b| {
                Bender::round([Vec2::new(b.place[0], b.place[1])], b.place[2].abs() * 2.0 + 2.0, FREE_BENDER)
            })
        });
        let inner = self.layers.iter().filter_map(|l| l.frame.free_glass_box());
        sprites.chain(blobs).chain(inner).reduce(Bender::union)
    }

    /// Whether none of the lists the picture is drawn from has anything in them.
    fn draws_nothing(&self) -> bool {
        self.paths.is_empty()
            && self.flashes.is_empty()
            && self.layered.is_empty()
            && self.trails.is_empty()
            && self.particles.is_empty()
            && self.trails_front.is_empty()
            && self.particles_front.is_empty()
            && self.fields.is_empty()
            && self.fields_front.is_empty()
            && self.swarms.is_empty()
            && self.swarms_front.is_empty()
            && self.layers.is_empty()
            && self.liquids.is_empty()
    }

    /// The moved Stage camera this frame is seen through (lighting part 5): a lit frame in Stage
    /// whose light scene has a camera. `None` leaves the picture as the front shot.
    pub fn stage_view(&self) -> Option<crate::StageView> {
        let light = self.light.as_ref()?;
        let cam = light.camera.as_ref()?;
        let s = &light.settings;
        if s.stage[0] < 0.5 {
            return None;
        }
        let size = glam::Vec2::new(self.width.max(1) as f32, self.height.max(1) as f32);
        let floor = match s.floor[0] {
            k if k > 1.5 => Some(self.camera.to_world(glam::Vec2::new(0.0, size.y * 0.5)).y),
            k if k > 0.5 => Some(s.floor[1]),
            _ => None,
        };
        let layout = crate::StageLayout { back: s.stage[1], k: s.stage[3], floor };
        Some(crate::StageView::solve(cam, &self.camera, size, layout))
    }
}

/// The most trail vertices one list of a frame holds (about 115 MB).
pub const MOST_TRAIL_VERTICES: usize = 3_000_000;
/// The most particles one list of a frame holds (about 61 MB).
pub const MOST_SPRITES: usize = 1_000_000;
