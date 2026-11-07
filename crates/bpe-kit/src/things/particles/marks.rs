//! The **Marks that stay** Look layer (particles part 6): particles that touch a shape (or an
//! edge of the picture, or die) leave a mark where it happened, and it stays: paint drips, splats,
//! scorch marks. A draw is worked out from the state alone, so the state keeps the marks
//! ([`Mark`]), only while a Marks layer asks; each particle step's touches are noted by the move
//! in the particle's flags and made into marks after it ([`collect`]).

use bpe_core::{Blend, Color, Settings};
use bpe_gfx::Particle;
use glam::Vec2;

use super::births::{signed, unit};
use super::look::{ColourFrom, Common, FineDetail, MOST_COPIES, Only, look_settings};
use super::state::{DEAD, ParticlesState, Speck};
use super::step::DT;
use crate::modules::look::layers::particles::{ParticleFade, ShapeMadeOf, SizeOverLife};
use crate::modules::look::layers::trail::AroundPlace;
use crate::paint::{Follow, Paint};

/// The layer kind.
pub const MARKS: &str = "particles.marks";

/// Flags the move sets on a particle this step: it touched a shape.
pub const TOUCHED_SHAPE: u32 = 1 << 16;
/// It met an edge of the picture (vanished, wrapped or bounced there).
pub const TOUCHED_EDGE: u32 = 1 << 17;
/// It has left its shape mark (a particle leaves one).
pub const MARKED_SHAPE: u32 = 1 << 18;
/// It has left its edge mark.
pub const MARKED_EDGE: u32 = 1 << 19;

/// Why a mark was made: on a shape.
pub const ON_SHAPE: u8 = 1;
/// At an edge of the picture.
pub const ON_EDGE: u8 = 2;
/// Where a particle died.
pub const ON_DEATH: u8 = 4;

/// The most marks ever kept.
pub const MOST_MARKS_EVER: u32 = 1_000_000;

/// One mark a particle left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    pub place: Vec2,
    /// How the particle was going as it made it (from where it was a step before).
    pub velocity: Vec2,
    /// The particle's half size.
    pub size: f32,
    /// The particle's dice.
    pub seed: u32,
    /// The parent ball the particle came from (body row + 1), 0 when none.
    pub ball: u32,
    /// The particle step it was made on.
    pub at: u32,
    /// Why it was made: [`ON_SHAPE`], [`ON_EDGE`] or [`ON_DEATH`].
    pub why: u8,
    /// Where the particle was born (Colours following Place from start).
    pub home: Vec2,
}

impl Mark {
    fn of(s: &Speck, at: u32, why: u8) -> Mark {
        Mark {
            place: s.place,
            velocity: Some((s.place - s.previous) / DT).filter(|v| v.is_finite()).unwrap_or(Vec2::ZERO),
            size: s.size,
            seed: s.seed,
            ball: s.ball,
            at,
            why,
            home: s.home,
        }
    }

    /// How old it is at particle step `now`, in seconds.
    pub fn age(&self, now: u32) -> f32 {
        now.saturating_sub(self.at) as f32 * DT
    }
}

/// Makes this step's marks from the flags the move set (and from the dead, when asked), then
/// drops the oldest past Most marks and those every layer has finished fading. Clears the
/// touch flags. Runs after the move and before the dead are removed.
pub fn collect(st: &mut ParticlesState) {
    let why = st.wants.marks_why;
    let now = st.step;
    for s in &mut st.specks {
        let touched = s.flags & (TOUCHED_SHAPE | TOUCHED_EDGE);
        s.flags &= !(TOUCHED_SHAPE | TOUCHED_EDGE);
        // A particle that went wild (dropped for a place that isn't a number) leaves no mark.
        if why == 0 || !s.place.is_finite() {
            continue;
        }
        if touched & TOUCHED_SHAPE != 0 && why & ON_SHAPE != 0 && s.flags & MARKED_SHAPE == 0 {
            s.flags |= MARKED_SHAPE;
            st.marks.push(Mark::of(s, now, ON_SHAPE));
        }
        if touched & TOUCHED_EDGE != 0 && why & ON_EDGE != 0 && s.flags & MARKED_EDGE == 0 {
            s.flags |= MARKED_EDGE;
            st.marks.push(Mark::of(s, now, ON_EDGE));
        }
        if s.flags & DEAD != 0 && why & ON_DEATH != 0 {
            st.marks.push(Mark::of(s, now, ON_DEATH));
        }
    }
    prune(st);
}

/// Drops the oldest marks past Most marks, and those every layer has finished fading.
pub fn prune(st: &mut ParticlesState) {
    if st.wants.marks_why == 0 {
        st.marks = Vec::new();
        return;
    }
    let now = st.step;
    let mut gone = st.marks.len().saturating_sub(st.wants.most_marks.max(1));
    if let Some(last) = st.wants.marks_last {
        gone = gone.max(st.marks.iter().take_while(|m| m.age(now) > last).count());
    }
    if gone > 0 {
        st.marks.drain(..gone);
    }
}

look_settings! {
    /// Marks that stay: a mark where each particle landed, staying there.
    MarksLayer {
        /// Marks where the particles touch a shape (At shapes must not be Pass).
        #[setting(label = "On shapes", group = "Marks", simple)]
        on_shapes: bool = true,
        /// Marks where they meet an edge of the picture (At edges must not be Pass).
        #[setting(label = "On edges", group = "Marks")]
        on_edges: bool = false,
        /// Marks where they die or vanish.
        #[setting(label = "Where they die", group = "Marks")]
        on_death: bool = false,
        /// What each mark is drawn as.
        #[setting(label = "Made of", group = "Marks")]
        made_of: ShapeMadeOf = ShapeMadeOf::Dots,
        /// How soft a mark's edge is.
        #[setting(label = "Softness", group = "Marks", range(0, 1), keyframe)]
        softness: f32 = 0.4,
        /// Small drops round each mark, flung the way the particle was going.
        #[setting(label = "Splats", group = "Marks", range(0, 12), simple)]
        splats: u32 = 4,
        /// How far the splats spread, in the particle's sizes.
        #[setting(label = "Splat spread", group = "Marks", when = "splats", span(0, 10), keyframe)]
        splat_spread: f32 = 3.0,
        /// How far a drip runs down from each mark.
        #[setting(label = "Drip", group = "Marks", span(0, 200), unit = "px", simple, keyframe)]
        drip: f32 = 0.0,
        /// How long a drip takes to run its length.
        #[setting(label = "Drip time", group = "Marks", when = "drip", span(0.1, 10), unit = "s")]
        drip_time: f32 = 2.0,
        /// How long a mark stays before it fades (0: for good).
        #[setting(label = "Fade after", group = "Marks", span(0, 30), unit = "s")]
        fade_after: f32 = 0.0,
        /// How long it takes to fade.
        #[setting(label = "Fade over", group = "Marks", when = "fade_after", span(0, 10), unit = "s")]
        fade_over: f32 = 1.0,
        /// The most marks kept: past it the oldest go.
        #[setting(label = "Most marks", group = "Marks", range(1, 1000000), span(100, 100000))]
        most: u32 = 5000,
    }
}

impl MarksLayer {
    /// Why it draws marks: the [`ON_SHAPE`], [`ON_EDGE`] and [`ON_DEATH`] it asks for.
    pub fn why(&self) -> u8 {
        (if self.on_shapes { ON_SHAPE } else { 0 })
            | (if self.on_edges { ON_EDGE } else { 0 })
            | (if self.on_death { ON_DEATH } else { 0 })
    }

    /// How long a mark lasts in it, when it fades.
    pub fn lasts(&self) -> Option<f32> {
        (self.fade_after > 0.0).then(|| self.fade_after + self.fade_over.max(0.0))
    }

    /// How much of a mark of age `age` shows, 0 to 1.
    pub fn shows(&self, age: f32) -> f32 {
        if self.fade_after <= 0.0 || age <= self.fade_after {
            return 1.0;
        }
        if self.fade_over <= 0.0 {
            return 0.0;
        }
        (1.0 - (age - self.fade_after) / self.fade_over).clamp(0.0, 1.0)
    }

    /// How far through its life a mark of age `age` is, 0 to 1 (0 for good when it never fades).
    pub fn life(&self, age: f32) -> f32 {
        self.lasts().map_or(0.0, |l| (age / l.max(1e-6)).clamp(0.0, 1.0))
    }
}

/// The sprites of mark `m` (age `age`) for `layer`: the mark itself, its splats and its drip,
/// each coloured `colour` at `size` (its half size), its sprite code `code`. `down` is the
/// screen's down in world units.
pub fn sprites_of(layer: &MarksLayer, m: &Mark, age: f32, size: f32, colour: [f32; 4], down: Vec2) -> Vec<Particle> {
    let code = layer.made_of.as_ball().code();
    let soft = layer.softness.clamp(0.0, 1.0);
    let mut out = Vec::with_capacity(2 + layer.splats as usize);
    let turn = unit(m.seed, 71) * std::f32::consts::TAU;
    let mut own = Particle::new(code, m.place, turn, size, colour);
    own.style[2] = soft;
    out.push(own);
    let speed = m.velocity.length();
    let way = m.velocity.try_normalize().unwrap_or(-down);
    for k in 0..layer.splats.min(12) {
        let spread = layer.splat_spread.max(0.0) * size;
        // Flung mostly the way it was going, some round about.
        let angle = signed(m.seed, 100 + k) * 1.4;
        let reach = spread * (0.35 + 0.65 * unit(m.seed, 120 + k)) * (0.6 + (speed / 600.0).min(1.4));
        let at = m.place + Vec2::from_angle(angle).rotate(way) * reach;
        let small = size * (0.18 + 0.32 * unit(m.seed, 140 + k));
        let mut p = Particle::new(code, at, turn, small, colour);
        p.style[2] = soft;
        out.push(p);
    }
    if layer.drip > 0.0 && layer.drip_time > 0.0 {
        let t = (age / layer.drip_time).clamp(0.0, 1.0);
        // Runs fast, then slows: ease out, each drip a little longer or shorter.
        let length = layer.drip * (1.0 - (1.0 - t) * (1.0 - t)) * (0.6 + 0.8 * unit(m.seed, 160));
        if length > 0.5 {
            let width = size * 0.45;
            let tip = m.place + down * length;
            let angle = down.y.atan2(down.x);
            let mut p =
                Particle::new(ShapeMadeOf::Streaks.as_ball().code(), (m.place + tip) * 0.5, angle, width, colour);
            p.style[0] = (length * 0.5).max(width);
            p.style[2] = soft;
            out.push(p);
            let mut drop = Particle::new(code, tip, turn, width * 1.3, colour);
            drop.style[2] = soft;
            out.push(drop);
        }
    }
    out
}

impl MarksLayer {
    fn starting(s: Self) -> Self {
        s
    }
}
