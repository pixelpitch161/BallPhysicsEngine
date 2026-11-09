//! The grade's masks (colour grading part 4, `docs/notes/grading.md`): after the Whole picture,
//! each mask works on part of the picture. A mask is a layer (`grade.mask`) whose sub-layers are
//! first its **parts** (`LayerSection::MaskPart`: Linear and Radial Gradients, Colour and
//! Luminance Ranges, joined in order by Add, Subtract or Intersect) and then its **panels**.
//!
//! A gradient sits on the screen, in the world, on a thing, on one ball or on every ball (a copy
//! round each); [`places`] works out where through the world's camera at the scene's draw, so the
//! picture's grade only ever sees the screen.

use bpe_core::{
    Choice, Color, EditError, KindId, Layer, LayerId, LayerKind, LayerSection, Params, Registry, ResolvedSlot, Scene,
    SettingDef, Settings, Slot, SlotId, Target, ThingId, Value, layered_key,
};
use bpe_gfx::{
    GradeArea, GradeColourRange, GradeCombine, GradeLinear, GradeLuminanceRange, GradePart, GradePlace, GradeRadial,
    GradeShape,
};
use bpe_runtime::World;
use glam::Vec2;

use super::{BASIC, GRADE, PANEL_ORDER, ensure_grade};

/// The mask kind.
pub const MASK: &str = "grade.mask";
/// The part kinds.
pub const LINEAR: &str = "grade.part.linear";
pub const RADIAL: &str = "grade.part.radial";
pub const COLOUR_RANGE: &str = "grade.part.colour_range";
pub const LUMINANCE_RANGE: &str = "grade.part.luminance_range";
/// Every part kind, in the order the editor offers them.
pub const PART_KINDS: [&str; 4] = [LINEAR, RADIAL, COLOUR_RANGE, LUMINANCE_RANGE];

/// How many colours a Colour Range can pick.
pub const RANGE_PICKS: usize = 5;

/// A ball of this radius (a new group's) has a gradient that Grows with it at the sizes set.
pub const GROW_RADIUS: f32 = 24.0;

/// How a part joins the parts before it (Lightroom's mask components).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Combine {
    #[default]
    Add,
    Subtract,
    Intersect,
}

impl Combine {
    pub const ALL: [Combine; 3] = [Combine::Add, Combine::Subtract, Combine::Intersect];

    pub fn gpu(self) -> GradeCombine {
        match self {
            Combine::Add => GradeCombine::Add,
            Combine::Subtract => GradeCombine::Subtract,
            Combine::Intersect => GradeCombine::Intersect,
        }
    }

    /// The word the editor shows.
    pub fn word(self) -> &'static str {
        match self {
            Combine::Add => "Add",
            Combine::Subtract => "Subtract",
            Combine::Intersect => "Intersect",
        }
    }
}

/// Where a gradient sits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Placed {
    /// Fixed on the picture, whatever the camera does.
    #[default]
    OnTheScreen,
    /// Fixed in the world: it moves with the camera.
    InTheWorld,
    /// On the middle of a thing (a group: its balls' middle).
    OnAThing,
    /// On one ball of a group or emitter, by its number.
    OnOneBall,
    /// A copy on every ball.
    OnEveryBall,
}

impl Placed {
    /// Whether it follows something.
    pub fn follows(self) -> bool {
        matches!(self, Placed::OnAThing | Placed::OnOneBall | Placed::OnEveryBall)
    }
}

/// A mask's own settings.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct MaskSettings {
    /// How strongly its panels work where it covers.
    #[setting(label = "Amount", range(0, 100), keyframe, pulse, group = "Mask")]
    pub amount: f32,
    /// Covers where its parts don't, and not where they do.
    #[setting(label = "Invert", group = "Mask")]
    pub invert: bool,
}

impl Default for MaskSettings {
    fn default() -> Self {
        Self { amount: 100.0, invert: false }
    }
}

fn balls() -> Target {
    Target::Kind(KindId::from("balls"))
}

/// A Linear Gradient's settings.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct LinearPart {
    /// How it joins the parts above it (the first part is always added).
    #[setting(label = "Combine", group = "Part")]
    pub combine: Combine,
    /// Covers the other side instead.
    #[setting(label = "Invert", group = "Part")]
    pub invert: bool,
    /// Across: where its middle line's middle is (from the picture's middle, or from what it follows).
    #[setting(label = "X", span(-1500, 1500), unit = "px", keyframe, pulse, group = "Linear gradient")]
    pub x: f32,
    /// Down: where its middle line's middle is.
    #[setting(label = "Y", span(-1500, 1500), unit = "px", keyframe, pulse, group = "Linear gradient")]
    pub y: f32,
    /// The way the effect fades out, clockwise from the right: 90 fades downwards.
    #[setting(label = "Angle", range(-360, 360), unit = "°", keyframe, pulse, group = "Linear gradient")]
    pub angle: f32,
    /// How far the full effect is from none: the distance between the outer lines.
    #[setting(
        label = "Feather",
        range(1, 1e6),
        span(10, 2000),
        unit = "px",
        keyframe,
        pulse,
        group = "Linear gradient"
    )]
    pub feather: f32,
    /// Fixed on the screen, in the world, or following a thing, one ball or every ball.
    #[setting(label = "Placed", group = "Moving")]
    pub place: Placed,
    /// What it follows.
    #[setting(label = "Follow what", when = "place=on_a_thing|on_one_ball|on_every_ball", group = "Moving")]
    pub target: Target,
    /// Which ball (its number in its group).
    #[setting(label = "Ball number", when = "place=on_one_ball", range(1, 1e9), span(1, 100), group = "Moving")]
    pub ball: u32,
    /// Turns as the thing turns (a ball: the way it is going).
    #[setting(label = "Turns with it", when = "place=on_a_thing|on_one_ball|on_every_ball", group = "Moving")]
    pub turns: bool,
    /// Bigger on a bigger ball (the sizes are for a ball of 24 px).
    #[setting(label = "Grows with it", when = "place=on_a_thing|on_one_ball|on_every_ball", group = "Moving")]
    pub grows: bool,
}

impl Default for LinearPart {
    fn default() -> Self {
        Self {
            combine: Combine::Add,
            invert: false,
            x: 0.0,
            y: -400.0,
            angle: 90.0,
            feather: 400.0,
            place: Placed::OnTheScreen,
            target: balls(),
            ball: 1,
            turns: false,
            grows: false,
        }
    }
}

/// A Radial Gradient's settings.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct RadialPart {
    /// How it joins the parts above it (the first part is always added).
    #[setting(label = "Combine", group = "Part")]
    pub combine: Combine,
    /// Covers outside instead of inside.
    #[setting(label = "Invert", group = "Part")]
    pub invert: bool,
    /// Across: where its middle is (from the picture's middle, or from what it follows).
    #[setting(label = "X", span(-1500, 1500), unit = "px", keyframe, pulse, group = "Radial gradient")]
    pub x: f32,
    /// Down: where its middle is.
    #[setting(label = "Y", span(-1500, 1500), unit = "px", keyframe, pulse, group = "Radial gradient")]
    pub y: f32,
    /// The whole ellipse's width.
    #[setting(label = "Width", range(1, 1e6), span(10, 3000), unit = "px", keyframe, pulse, group = "Radial gradient")]
    pub width: f32,
    /// The whole ellipse's height.
    #[setting(
        label = "Height",
        range(1, 1e6),
        span(10, 3000),
        unit = "px",
        keyframe,
        pulse,
        group = "Radial gradient"
    )]
    pub height: f32,
    /// How far it is turned, clockwise.
    #[setting(label = "Angle", range(-360, 360), unit = "°", keyframe, pulse, group = "Radial gradient")]
    pub angle: f32,
    /// Down a rounder rectangle, up a circle on its short side.
    #[setting(label = "Roundness", range(-100, 100), keyframe, pulse, group = "Radial gradient")]
    pub roundness: f32,
    /// How soft its edge is: how far in from the edge the fade starts.
    #[setting(label = "Feather", range(0, 100), keyframe, pulse, group = "Radial gradient")]
    pub feather: f32,
    /// Fixed on the screen, in the world, or following a thing, one ball or every ball.
    #[setting(label = "Placed", group = "Moving")]
    pub place: Placed,
    /// What it follows.
    #[setting(label = "Follow what", when = "place=on_a_thing|on_one_ball|on_every_ball", group = "Moving")]
    pub target: Target,
    /// Which ball (its number in its group).
    #[setting(label = "Ball number", when = "place=on_one_ball", range(1, 1e9), span(1, 100), group = "Moving")]
    pub ball: u32,
    /// Turns as the thing turns (a ball: the way it is going).
    #[setting(label = "Turns with it", when = "place=on_a_thing|on_one_ball|on_every_ball", group = "Moving")]
    pub turns: bool,
    /// Bigger on a bigger ball (the sizes are for a ball of 24 px).
    #[setting(label = "Grows with it", when = "place=on_a_thing|on_one_ball|on_every_ball", group = "Moving")]
    pub grows: bool,
}

impl Default for RadialPart {
    fn default() -> Self {
        Self {
            combine: Combine::Add,
            invert: false,
            x: 0.0,
            y: 0.0,
            width: 500.0,
            height: 500.0,
            angle: 0.0,
            roundness: 0.0,
            feather: 50.0,
            place: Placed::OnTheScreen,
            target: balls(),
            ball: 1,
            turns: false,
            grows: false,
        }
    }
}

/// A Colour Range's settings: up to five picked colours in fixed slots (`used1`/`colour1` ..).
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ColourRangePart {
    /// How it joins the parts above it (the first part is always added).
    #[setting(label = "Combine", group = "Part")]
    pub combine: Combine,
    /// Covers every colour but the picked ones.
    #[setting(label = "Invert", group = "Part")]
    pub invert: bool,
    #[setting(label = "Colour 1 picked", group = "Colours")]
    pub used1: bool,
    #[setting(label = "Colour 1", group = "Colours")]
    pub colour1: Color,
    #[setting(label = "Colour 2 picked", group = "Colours")]
    pub used2: bool,
    #[setting(label = "Colour 2", group = "Colours")]
    pub colour2: Color,
    #[setting(label = "Colour 3 picked", group = "Colours")]
    pub used3: bool,
    #[setting(label = "Colour 3", group = "Colours")]
    pub colour3: Color,
    #[setting(label = "Colour 4 picked", group = "Colours")]
    pub used4: bool,
    #[setting(label = "Colour 4", group = "Colours")]
    pub colour4: Color,
    #[setting(label = "Colour 5 picked", group = "Colours")]
    pub used5: bool,
    #[setting(label = "Colour 5", group = "Colours")]
    pub colour5: Color,
    /// How far from a picked colour a colour may be and still be covered.
    #[setting(label = "Refine", range(0, 100), keyframe, pulse, group = "Colour range")]
    pub refine: f32,
}

impl Default for ColourRangePart {
    fn default() -> Self {
        let red = Color::rgb(255, 0, 0);
        Self {
            combine: Combine::Add,
            invert: false,
            used1: false,
            colour1: red,
            used2: false,
            colour2: red,
            used3: false,
            colour3: red,
            used4: false,
            colour4: red,
            used5: false,
            colour5: red,
            refine: 50.0,
        }
    }
}

/// Slot `slot`'s (0..5) setting `"used"` or `"colour"`: `"used1"`, `"colour1"` for the first.
pub fn range_key(slot: usize, what: &str) -> String {
    format!("{what}{}", slot + 1)
}

impl ColourRangePart {
    /// The five slots: whether each is picked, and its colour.
    pub fn slots(&self) -> [(bool, Color); RANGE_PICKS] {
        [
            (self.used1, self.colour1),
            (self.used2, self.colour2),
            (self.used3, self.colour3),
            (self.used4, self.colour4),
            (self.used5, self.colour5),
        ]
    }

    pub fn gpu(&self) -> GradeColourRange {
        let linear = |c: Color| [c.r, c.g, c.b].map(|v| bpe_gfx::grade::maths::untone(v as f64 / 255.0) as f32);
        GradeColourRange {
            picks: self.slots().iter().filter(|(on, _)| *on).map(|(_, c)| linear(*c)).collect(),
            refine: self.refine,
        }
    }
}

/// A Luminance Range's settings.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct LuminanceRangePart {
    /// How it joins the parts above it (the first part is always added).
    #[setting(label = "Combine", group = "Part")]
    pub combine: Combine,
    /// Covers the brightnesses outside the range.
    #[setting(label = "Invert", group = "Part")]
    pub invert: bool,
    /// The darkest brightness covered (0 black, 100 white).
    #[setting(label = "Low", range(0, 100), keyframe, pulse, group = "Luminance range")]
    pub low: f32,
    /// The brightest brightness covered.
    #[setting(label = "High", range(0, 100), keyframe, pulse, group = "Luminance range")]
    pub high: f32,
    /// How gently the coverage fades out past the ends.
    #[setting(label = "Smoothness", range(0, 100), keyframe, pulse, group = "Luminance range")]
    pub smoothness: f32,
}

impl Default for LuminanceRangePart {
    fn default() -> Self {
        Self { combine: Combine::Add, invert: false, low: 50.0, high: 100.0, smoothness: 50.0 }
    }
}

/// A mask kind on the Grade card.
pub struct MaskKind;

impl LayerKind for MaskKind {
    fn id(&self) -> &'static str {
        MASK
    }
    fn name(&self) -> &'static str {
        "Mask"
    }
    fn help(&self) -> &'static str {
        "Its panels grade only where its parts cover, after the Whole picture and the masks above it."
    }
    fn settings(&self) -> &'static [SettingDef] {
        MaskSettings::defs()
    }
    fn section(&self) -> LayerSection {
        LayerSection::Mask
    }
    fn repeats(&self) -> bool {
        true
    }
    fn card(&self) -> &'static str {
        GRADE
    }
    fn takes_sub(&self, sub: &str) -> bool {
        PANEL_ORDER.contains(&sub) || PART_KINDS.contains(&sub)
    }
    fn per_ball(&self) -> bool {
        false
    }
}

/// A mask part kind.
pub struct PartKind {
    pub id: &'static str,
    pub name: &'static str,
    pub help: &'static str,
    pub defs: fn() -> &'static [SettingDef],
}

impl LayerKind for PartKind {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        self.name
    }
    fn help(&self) -> &'static str {
        self.help
    }
    fn settings(&self) -> &'static [SettingDef] {
        (self.defs)()
    }
    fn section(&self) -> LayerSection {
        LayerSection::MaskPart
    }
    fn repeats(&self) -> bool {
        true
    }
    fn card(&self) -> &'static str {
        GRADE
    }
    fn takes_subs(&self) -> bool {
        false
    }
    fn per_ball(&self) -> bool {
        false
    }
}

/// Every part kind, in the order the editor offers them.
pub fn part_kinds() -> [PartKind; 4] {
    [
        PartKind {
            id: LINEAR,
            name: "Linear Gradient",
            help: "Full effect on one side of a line, none on the other, fading in between.",
            defs: LinearPart::defs,
        },
        PartKind {
            id: RADIAL,
            name: "Radial Gradient",
            help: "An ellipse: full effect inside, fading out towards its edge.",
            defs: RadialPart::defs,
        },
        PartKind {
            id: COLOUR_RANGE,
            name: "Colour Range",
            help: "The colours near up to five colours picked from the picture.",
            defs: ColourRangePart::defs,
        },
        PartKind {
            id: LUMINANCE_RANGE,
            name: "Luminance Range",
            help: "The parts of the picture between a low and a high brightness.",
            defs: LuminanceRangePart::defs,
        },
    ]
}

/// The masks of `card` after the Whole picture, in the order they work.
pub fn masks_of(card: &Slot) -> Vec<&Layer> {
    card.layers.iter().filter(|l| l.parent.is_none() && l.kind == MASK).collect()
}

/// The parts of mask `mask` in `card`, in order.
pub fn parts_of(card: &Slot, mask: LayerId) -> Vec<&Layer> {
    card.layers.iter().filter(|l| l.parent == Some(mask) && PART_KINDS.contains(&l.kind.as_str())).collect()
}

/// Adds a mask (after the others) with one part of `part` and one Basic panel, giving the scene
/// its Grade card first. Returns the mask.
pub fn add_mask(scene: &mut Scene, reg: &Registry, part: &str) -> Result<LayerId, EditError> {
    ensure_grade(scene, reg);
    let (root, slot) = (ThingId::ROOT, SlotId::Grade);
    let mask = scene.add_layer(reg, root, slot, MASK)?;
    let part = scene.add_sub_layer(reg, root, slot, mask, part)?;
    super::name_among_siblings(scene, reg, part);
    let basic = scene.add_sub_layer(reg, root, slot, mask, BASIC)?;
    super::name_among_siblings(scene, reg, basic);
    Ok(mask)
}

/// Adds a part of `kind` after mask `mask`'s other parts, joining them by `combine`. Returns it.
pub fn add_part(
    scene: &mut Scene,
    reg: &Registry,
    mask: LayerId,
    kind: &str,
    combine: Combine,
) -> Result<LayerId, EditError> {
    ensure_grade(scene, reg);
    let id = scene.add_sub_layer(reg, ThingId::ROOT, SlotId::Grade, mask, kind)?;
    super::name_among_siblings(scene, reg, id);
    if combine != Combine::Add
        && let Some(card) = scene.card_mut(ThingId::ROOT, SlotId::Grade)
    {
        card.params.set(&layered_key(id, "combine"), Value::Choice(combine.name().into()));
    }
    Ok(id)
}

/// Where a gradient placed by `place` sits this moment: one place, or one a ball for every ball
/// (at most [`bpe_gfx::grade::MAX_BALLS`]), or none when what it follows isn't there. Without a
/// world (nothing has run) only the screen and the world at the camera's start have a place.
pub fn places(
    world: Option<&World>,
    place: Placed,
    target: &Target,
    ball: u32,
    turns: bool,
    grows: bool,
) -> Vec<GradePlace> {
    let camera = world.map(|w| w.camera).unwrap_or_default();
    let on = |at: Vec2, turn: f32, scale: f32| {
        let s = camera.to_screen(at);
        GradePlace { at: [s.x, s.y], turn: turn - camera.turn, scale: camera.zoom * scale }
    };
    let grow = |radius: f32| if grows && radius.is_finite() && radius > 0.0 { radius / GROW_RADIUS } else { 1.0 };
    let heading = |v: Vec2| if turns && v.is_finite() && v.length() >= 1.0 { v.y.atan2(v.x) } else { 0.0 };
    match place {
        Placed::OnTheScreen => vec![GradePlace::SCREEN],
        Placed::InTheWorld => vec![on(Vec2::ZERO, 0.0, 1.0)],
        _ => {
            let Some(world) = world else { return Vec::new() };
            match place {
                Placed::OnAThing => on_a_thing(world, target, turns, grows)
                    .map_or_else(Vec::new, |(at, turn, r)| vec![on(at, turn, if r > 0.0 { grow(r) } else { 1.0 })]),
                Placed::OnOneBall => {
                    let Some((p, v, r)) = one_ball(world, target, ball) else { return Vec::new() };
                    vec![on(p, heading(v), grow(r))]
                }
                _ => every_ball(world, target)
                    .into_iter()
                    .take(bpe_gfx::grade::MAX_BALLS)
                    .map(|(p, v, r)| on(p, heading(v), grow(r)))
                    .collect(),
            }
        }
    }
}

/// The things `target` names that the grade's places look at: following Balls follows every
/// thing's balls, emitters' too (as the camera does).
fn named(world: &World, target: &Target) -> Vec<ThingId> {
    let mut ids = world.matching(target, ThingId::ROOT);
    if *target == balls() {
        let emitters = Target::Kind(KindId::from(crate::things::emitter::KIND));
        ids.extend(world.matching(&emitters, ThingId::ROOT));
        ids.sort();
        ids.dedup();
    }
    ids
}

/// Every live ball of what `target` names: place, velocity, radius.
fn every_ball(world: &World, target: &Target) -> Vec<(Vec2, Vec2, f32)> {
    let mut out = Vec::new();
    for id in named(world, target) {
        let Some(b) = world.bodies.get(&id) else { continue };
        for i in 0..b.len() {
            if b.alive[i] && b.position[i].is_finite() {
                out.push((b.position[i], b.velocity[i], b.radius[i]));
            }
        }
    }
    out
}

/// Ball number `number` (from 1) of the first thing `target` names that has balls.
fn one_ball(world: &World, target: &Target, number: u32) -> Option<(Vec2, Vec2, f32)> {
    let id = named(world, target).into_iter().find(|id| world.bodies.contains_key(id))?;
    let b = world.bodies.get(&id)?;
    let row = b.row_of(number.max(1) - 1).filter(|&r| b.alive[r] && b.position[r].is_finite())?;
    Some((b.position[row], b.velocity[row], b.radius[row]))
}

/// The middle of what `target` names (as the camera follows it), the first named thing's turn
/// when `turns`, and its balls' mean radius (0 for no balls) when `grows`.
fn on_a_thing(world: &World, target: &Target, turns: bool, grows: bool) -> Option<(Vec2, f32, f32)> {
    let settings = crate::things::camera::CameraSettings { follow_target: target.clone(), ..Default::default() };
    let (middle, _, _) = crate::things::camera::sim::follow_point(world, &settings, ThingId::ROOT)?;
    let ids = named(world, target);
    let turn = if turns {
        ids.iter().find_map(|id| world.poses.get(id)).map_or(0.0, |p| if p.angle.is_finite() { p.angle } else { 0.0 })
    } else {
        0.0
    };
    let radius = if grows {
        let balls = ids
            .iter()
            .filter_map(|id| world.bodies.get(id))
            .flat_map(|b| (0..b.len()).filter(|&i| b.alive[i]).map(|i| b.radius[i]).collect::<Vec<_>>());
        let (sum, n) = balls.fold((0.0f32, 0u32), |(s, n), r| (s + r, n + 1));
        if n > 0 { sum / n as f32 } else { 0.0 }
    } else {
        0.0
    };
    Some((middle, turn, radius))
}

/// What a part's row says about what it follows: "Thing not found" when it follows nothing that
/// is there, or that only the first balls are covered.
pub fn follow_note(world: &World, kind: &str, params: &Params) -> Option<String> {
    let (place, target, ball) = match kind {
        LINEAR => {
            let s = LinearPart::from_params(params);
            (s.place, s.target, s.ball)
        }
        RADIAL => {
            let s = RadialPart::from_params(params);
            (s.place, s.target, s.ball)
        }
        _ => return None,
    };
    match place {
        Placed::OnAThing if on_a_thing(world, &target, false, false).is_none() => Some("Thing not found".into()),
        Placed::OnOneBall if one_ball(world, &target, ball).is_none() => Some("Thing not found".into()),
        Placed::OnEveryBall => {
            let n = every_ball(world, &target).len();
            if named(world, &target).is_empty() {
                Some("Thing not found".into())
            } else if n > bpe_gfx::grade::MAX_BALLS {
                Some(format!("First {} of {n} balls only", bpe_gfx::grade::MAX_BALLS))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Mask `mask`'s area in the resolved card: its shown parts in order, placed through `world`. A
/// gradient with nowhere to be (what it follows isn't there, or no balls yet) is left out, as a
/// hidden part is, so it covers nothing even inverted; a mask left with no parts covers nothing.
pub fn area(card: &ResolvedSlot, mask: LayerId, world: Option<&World>, invert: bool) -> GradeArea {
    let parts = card
        .layers
        .iter()
        .filter(|l| l.parent == Some(mask))
        .filter_map(|l| {
            let params = card.layer_params(l.id);
            let (combine, invert, shape) = match l.kind.as_str() {
                LINEAR => {
                    let s = LinearPart::from_params(&params);
                    let places = places(world, s.place, &s.target, s.ball, s.turns, s.grows);
                    if places.is_empty() {
                        return None;
                    }
                    let shape =
                        GradeShape::Linear(GradeLinear { x: s.x, y: s.y, angle: s.angle, feather: s.feather, places });
                    (s.combine, s.invert, shape)
                }
                RADIAL => {
                    let s = RadialPart::from_params(&params);
                    let places = places(world, s.place, &s.target, s.ball, s.turns, s.grows);
                    if places.is_empty() {
                        return None;
                    }
                    let shape = GradeShape::Radial(GradeRadial {
                        x: s.x,
                        y: s.y,
                        width: s.width,
                        height: s.height,
                        angle: s.angle,
                        roundness: s.roundness,
                        feather: s.feather,
                        places,
                    });
                    (s.combine, s.invert, shape)
                }
                COLOUR_RANGE => {
                    let s = ColourRangePart::from_params(&params);
                    (s.combine, s.invert, GradeShape::ColourRange(s.gpu()))
                }
                LUMINANCE_RANGE => {
                    let s = LuminanceRangePart::from_params(&params);
                    let shape = GradeShape::LuminanceRange(GradeLuminanceRange {
                        low: s.low,
                        high: s.high,
                        smoothness: s.smoothness,
                    });
                    (s.combine, s.invert, shape)
                }
                _ => return None,
            };
            Some(GradePart { combine: combine.gpu(), invert, shape, id: l.id.0 })
        })
        .collect();
    GradeArea { parts, invert }
}
