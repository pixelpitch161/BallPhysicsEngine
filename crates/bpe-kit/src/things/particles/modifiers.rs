//! The experimental modifiers every drawn Look layer of a Particles thing has (particles part 6),
//! in its **Experimental** fold: **Mirror** (turned or mirrored copies round a middle, a
//! kaleidoscope), **Colour split** (red, green and blue copies pulled apart along the way each
//! particle goes) and **Glitch** (bands of the picture torn sideways at random moments). They work
//! on what the layer drew, on the CPU, so they work for every layer kind and every sub-layer.

use std::sync::OnceLock;

use bpe_core::{Choice, SettingDef, Settings, Value};
use bpe_gfx::{LiquidBlob, Particle, Splat, TrailVertex};
use bpe_runtime::CameraView;
use glam::Vec2;

use super::births::{signed, unit};
use crate::modules::look::layers::around_subs::{OWN, Pieces, SHADOW};
use crate::modules::look::layers::particles::emit::hash;

/// How many copies Mirror makes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Mirror {
    #[default]
    Off,
    TwoWays,
    ThreeWays,
    FourWays,
    SixWays,
    EightWays,
    TwelveWays,
}

impl Mirror {
    /// How many copies there are in all (1 when off).
    pub fn ways(self) -> u32 {
        match self {
            Mirror::Off => 1,
            Mirror::TwoWays => 2,
            Mirror::ThreeWays => 3,
            Mirror::FourWays => 4,
            Mirror::SixWays => 6,
            Mirror::EightWays => 8,
            Mirror::TwelveWays => 12,
        }
    }
}

/// What Mirror turns its copies round.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum MirrorRound {
    /// The middle of what the camera sees.
    #[default]
    TheMiddleOfThePicture,
    /// The thing the particles sit in: its balls' middle, or its own place.
    TheThingTheyRide,
    /// Mirror centre.
    APoint,
}

/// A layer's modifiers.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct Modifiers {
    /// Copies of the layer turned round a middle: 2 to 12 ways.
    #[setting(label = "Mirror", group = "Experimental")]
    pub mirror: Mirror,
    /// What the copies turn round.
    #[setting(label = "Mirror round", group = "Experimental", when = "mirror!=off")]
    pub round: MirrorRound,
    /// The point the copies turn round, for A point.
    #[setting(label = "Mirror centre", group = "Experimental", when = "mirror!=off and round=a_point", unit = "px")]
    pub centre: Vec2,
    /// Every second copy also flipped over, as in a kaleidoscope.
    #[setting(label = "Mirrored copies", group = "Experimental", when = "mirror!=off")]
    pub flipped: bool,
    /// How far the red, green and blue copies are pulled apart: the way each particle goes in this
    /// many seconds (red ahead, blue behind).
    #[setting(label = "Colour split", group = "Experimental", span(0, 0.1), unit = "s", keyframe, pulse)]
    pub split: f32,
    /// How far apart the copies are pulled however fast a particle goes.
    #[setting(label = "Split offset", group = "Experimental", span(0, 20), unit = "px", keyframe, pulse)]
    pub offset: f32,
    /// The chance a band of the picture is torn at a moment.
    #[setting(label = "Glitch", group = "Experimental", range(0, 1), keyframe, pulse)]
    pub glitch: f32,
    /// How tall each band is.
    #[setting(label = "Band height", group = "Experimental", when = "glitch", span(2, 200), unit = "px", keyframe)]
    pub band: f32,
    /// The furthest a torn band moves sideways.
    #[setting(label = "Tear", group = "Experimental", when = "glitch", span(0, 300), unit = "px", keyframe, pulse)]
    pub tear: f32,
    /// How often the tears change.
    #[setting(label = "Changes a second", group = "Experimental", when = "glitch", span(0, 60), keyframe)]
    pub rate: f32,
    /// How far red and blue part in a torn band.
    #[setting(
        label = "Tear colour split",
        group = "Experimental",
        when = "glitch",
        span(0, 30),
        unit = "px",
        keyframe
    )]
    pub tear_split: f32,
    /// Another set of tears.
    #[setting(label = "Glitch seed", group = "Experimental", when = "glitch")]
    pub seed: u32,
}

impl Default for Modifiers {
    fn default() -> Self {
        Self {
            mirror: Mirror::Off,
            round: MirrorRound::TheMiddleOfThePicture,
            centre: Vec2::ZERO,
            flipped: false,
            split: 0.0,
            offset: 0.0,
            glitch: 0.0,
            band: 24.0,
            tear: 40.0,
            rate: 8.0,
            tear_split: 4.0,
            seed: 0,
        }
    }
}

/// The modifiers' settings, as every drawn layer kind adds them (`Kind.dynamic`).
pub fn defs(_: &dyn Fn(&str) -> Option<Value>) -> &'static [SettingDef] {
    Modifiers::defs()
}

/// The modifiers' settings but Colour split's, for a Liquid layer (a surface doesn't split).
pub fn defs_without_split(_: &dyn Fn(&str) -> Option<Value>) -> &'static [SettingDef] {
    static DEFS: OnceLock<Vec<SettingDef>> = OnceLock::new();
    DEFS.get_or_init(|| Modifiers::defs().iter().filter(|d| !matches!(d.key, "split" | "offset")).cloned().collect())
}

impl Modifiers {
    /// Whether Colour split moves anything.
    pub fn splits(&self) -> bool {
        self.split > 0.0 || self.offset > 0.0
    }

    /// Whether Glitch tears anything.
    pub fn glitches(&self) -> bool {
        self.glitch > 0.0 && (self.tear > 0.0 || self.tear_split > 0.0)
    }

    /// How far red is moved ahead of green for a particle going at `velocity` (blue as far
    /// behind).
    pub fn split_of(&self, velocity: Vec2) -> Vec2 {
        let way = velocity.try_normalize().unwrap_or(Vec2::X);
        velocity * self.split.max(0.0) + way * self.offset.max(0.0)
    }
}

/// One channel of `c` as added light: `ch` 0 red, 1 green, 2 blue. A sprite's covering paint
/// turns into its light.
fn channel_sprite(c: [f32; 4], ch: usize) -> [f32; 4] {
    let mut out = [0.0, 0.0, 0.0, -c[3].abs()];
    out[ch] = c[ch];
    out
}

/// A strip corner's colour, one channel as added light (alpha -1, the light faded in).
fn channel_strip(c: [f32; 4], ch: usize) -> [f32; 4] {
    let k = if c[3] < 0.0 { 1.0 } else { c[3] };
    let mut out = [0.0, 0.0, 0.0, -1.0];
    out[ch] = c[ch] * k;
    out
}

/// Puts what one particle drew (`scratch`, emptied) into `out` as three copies, red moved by
/// `ahead`, green in place, blue moved back as far. Shadows go across as they are.
pub fn split_into(scratch: &mut Pieces, out: &mut Pieces, ahead: Vec2) {
    let moves = [ahead, Vec2::ZERO, -ahead];
    for k in 0..4 {
        let sprites = std::mem::take(&mut scratch.particles[k]);
        let strips = std::mem::take(&mut scratch.strips[k]);
        if k == SHADOW {
            out.particles[k].extend(sprites);
            out.strips[k].extend(strips);
            continue;
        }
        for (ch, by) in moves.iter().enumerate() {
            out.particles[k].extend(sprites.iter().map(|p| {
                let mut q = *p;
                q.place[0] += by.x;
                q.place[1] += by.y;
                q.color = channel_sprite(p.color, ch);
                q
            }));
            out.strips[k].extend(strips.iter().map(|v| TrailVertex {
                position: [v.position[0] + by.x, v.position[1] + by.y],
                color: channel_strip(v.color, ch),
                ..*v
            }));
        }
    }
}

/// Copy `k` of `ways`: how it turns a place round the middle, and whether it is flipped.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Turned {
    angle: f32,
    flip: bool,
}

fn copies(ways: u32, flipped: bool) -> Vec<Turned> {
    (1..ways)
        .map(|k| Turned { angle: k as f32 * std::f32::consts::TAU / ways as f32, flip: flipped && k % 2 == 1 })
        .collect()
}

impl Turned {
    /// Where `p` lands, turned (flipped first, across the line through `c` along x) round `c`.
    fn place(self, p: Vec2, c: Vec2) -> Vec2 {
        let mut d = p - c;
        if self.flip {
            d.y = -d.y;
        }
        c + Vec2::from_angle(self.angle).rotate(d)
    }

    fn turn(self, t: f32) -> f32 {
        if self.flip { self.angle - t } else { self.angle + t }
    }
}

/// Adds Mirror's copies of everything in `pieces` round `centre`: each list keeps its own
/// copies, so shadows stay under glows under the layer under its outlines.
pub fn mirror(pieces: &mut Pieces, m: &Modifiers, centre: Vec2) {
    let copies = copies(m.mirror.ways(), m.flipped);
    if copies.is_empty() {
        return;
    }
    for k in 0..4 {
        let sprites = pieces.particles[k].clone();
        let strips = pieces.strips[k].clone();
        for c in &copies {
            pieces.particles[k].extend(sprites.iter().map(|p| {
                let at = c.place(p.at(), centre);
                Particle { place: [at.x, at.y, c.turn(p.place[2]), p.place[3]], ..*p }
            }));
            pieces.strips[k].extend(strips.iter().map(|v| {
                let at = c.place(Vec2::from(v.position), centre);
                TrailVertex { position: at.into(), ..*v }
            }));
        }
    }
}

/// Mirror's copies of a liquid's blobs.
pub fn mirror_blobs(blobs: &mut Vec<LiquidBlob>, m: &Modifiers, centre: Vec2) {
    let copies = copies(m.mirror.ways(), m.flipped);
    let own = blobs.clone();
    for c in &copies {
        blobs.extend(own.iter().map(|b| {
            let at = c.place(Vec2::new(b.place[0], b.place[1]), centre);
            LiquidBlob { place: [at.x, at.y, b.place[2], b.place[3]], ..*b }
        }));
    }
}

/// Mirror's copies of a Soft smoke or Flame field's splats round `centre`.
pub fn mirror_splats(splats: &mut Vec<Splat>, m: &Modifiers, centre: Vec2) {
    let copies = copies(m.mirror.ways(), m.flipped);
    let own = splats.clone();
    for c in &copies {
        splats.extend(own.iter().map(|b| {
            let at = c.place(Vec2::new(b.place[0], b.place[1]), centre);
            Splat { place: [at.x, at.y, b.place[2], b.place[3]], ..*b }
        }));
    }
}

/// Glitch at a moment: which bands are torn and how far.
pub struct Tears<'a> {
    m: &'a Modifiers,
    camera: &'a CameraView,
    /// The moment's slot: tears change from one to the next.
    slot: u32,
    /// The layer's own dice.
    layer: u32,
}

impl<'a> Tears<'a> {
    pub fn new(m: &'a Modifiers, camera: &'a CameraView, time: f64, layer: u32) -> Self {
        let slot = (time.max(0.0) * f64::from(m.rate.max(0.0))).floor().min(u32::MAX as f64) as u32;
        Self { m, camera, slot, layer }
    }

    /// How far (world units, along the screen's across) the band at world place `p` is torn,
    /// and its red/blue split, both zero when it isn't.
    pub fn at(&self, p: Vec2) -> (Vec2, Vec2) {
        let screen = self.camera.to_screen(p);
        let band = (screen.y / self.m.band.max(1.0)).floor();
        if !band.is_finite() {
            return (Vec2::ZERO, Vec2::ZERO);
        }
        let dice = hash(&[self.m.seed, self.layer, band as i32 as u32, self.slot]);
        if unit(dice, 1) >= self.m.glitch.clamp(0.0, 1.0) {
            return (Vec2::ZERO, Vec2::ZERO);
        }
        let zoom = if self.camera.zoom.is_finite() && self.camera.zoom > 1e-6 { self.camera.zoom } else { 1.0 };
        let across = Vec2::from_angle(self.camera.turn).rotate(Vec2::X) / zoom;
        (across * signed(dice, 2) * self.m.tear.max(0.0), across * self.m.tear_split.max(0.0))
    }
}

/// Tears everything in `pieces`: each sprite and strip corner moves with its band; a torn
/// band's own sprites part into red and blue too.
pub fn glitch(pieces: &mut Pieces, tears: &Tears) {
    for k in 0..4 {
        let mut parted = Vec::new();
        for p in &mut pieces.particles[k] {
            let (by, split) = tears.at(p.at());
            if by == Vec2::ZERO && split == Vec2::ZERO {
                continue;
            }
            p.place[0] += by.x;
            p.place[1] += by.y;
            if k == OWN && split != Vec2::ZERO {
                for (ch, s) in [(0, split), (2, -split)] {
                    let mut q = *p;
                    q.place[0] += s.x;
                    q.place[1] += s.y;
                    q.color = channel_sprite(p.color, ch);
                    parted.push(q);
                }
            }
        }
        pieces.particles[k].extend(parted);
        for v in &mut pieces.strips[k] {
            let (by, _) = tears.at(Vec2::from(v.position));
            v.position = [v.position[0] + by.x, v.position[1] + by.y];
        }
    }
}

/// Tears a liquid's blobs.
pub fn glitch_blobs(blobs: &mut [LiquidBlob], tears: &Tears) {
    for b in blobs {
        let (by, _) = tears.at(Vec2::new(b.place[0], b.place[1]));
        b.place[0] += by.x;
        b.place[1] += by.y;
    }
}

/// Tears a field's splats as [`glitch`] tears sprites (no colour split: a field is one colour).
pub fn glitch_splats(splats: &mut [Splat], tears: &Tears) {
    for b in splats {
        let (by, _) = tears.at(Vec2::new(b.place[0], b.place[1]));
        b.place[0] += by.x;
        b.place[1] += by.y;
    }
}
