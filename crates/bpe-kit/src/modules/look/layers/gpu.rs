//! Turning a thing's resolved stack of layers into what the renderer draws: the layer records,
//! written once per thing per frame, and one instance per ball naming them. Only the body's
//! colour, the ball's own variation and its motion differ from ball to ball.

use std::borrow::Cow;
use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;

use bpe_core::{Blend, Choice, GradientForm, LayerId, ResolvedSlot, Settings};
use bpe_gfx::{GradientAtlas, LayerGpu, LayeredInstance, LookProgram};
use glam::Vec2;

use super::ShadingKind;
use super::{
    BLEND, BODY, BlendLayer, BodyLayer, Changes, ColourFrom, ColourOnHit, ColourOverTime, CutFrom, CutLayer, FLASH,
    FlashOnHit, GLOW, GROUP_GRADIENT, GROUP_SHADER, GlowLayer, GlowWhere, GroupGradient, GroupMix, GroupShader, HUE,
    OUTLINE, OutlineLayer, OutlineSits, PATTERN, PatternLayer, RAINBOW, RecolourMode, SHADER, SHADING, SHINE, STRETCH,
    SUB_CUT, SUB_FLASH, SUB_GLOW, SUB_HUE, SUB_OUTLINE, SUB_RECOLOUR, SUB_SHADER, SUB_SHADING, SUB_SHADOW, SUB_SHINE,
    ShaderLayer, ShadingLayer, ShineLayer, ShineLight, StretchWhenFast, SubColourFrom, SubFlash, SubGlow, SubHue,
    SubOutline, SubRecolour, SubShadow, TURNING, Turning,
};
use crate::modules::look::BallShape;
use crate::modules::look::basic::{STRETCH_SPEED, facing_degrees, heading, shift_linear_hue};
use crate::paint::{Follow, InsideFrom, Placing, form_gpu, inside_gpu};

/// How far a ball's shapes reach from its centre, in radii (a triangle's corners, a heart's
/// lobes).
const SHAPE_REACH: f32 = 1.45;
/// How far past the edge a glow reaches, in its sizes.
const HALO_REACH: f32 = 2.2;
/// The furthest a stack may reach, in radii, however big its outline: enough for the biggest
/// glow (Size 4). The shader fades a glow out before the quad's edge.
const MAX_REACH: f32 = SHAPE_REACH + HALO_REACH * 4.0 + 0.25;

/// One drawn layer of a stack with its settings.
#[derive(Clone, Debug, PartialEq)]
pub enum Drawn {
    Pattern(PatternLayer),
    Outline(OutlineLayer),
    Shine(ShineLayer),
    Glow(GlowLayer),
    Shading(ShadingLayer),
    Shader(Box<ShaderLayer>),
}

/// A sub-layer of a drawn layer (a ball's or a shape's), with its settings: it changes only that
/// layer (see sub_layers.wgsl). A shape's are read into the same settings, sizes in pixels.
#[derive(Clone, Debug, PartialEq)]
pub enum SubLayer {
    Cut(CutLayer),
    Glow(SubGlow),
    Outline(SubOutline),
    Shadow(SubShadow),
    Shading(ShadingLayer),
    Shine(ShineLayer),
    Shader(Box<ShaderLayer>),
    Recolour(SubRecolour),
    Flash(SubFlash),
    Hue(SubHue),
    /// Gives off light: folded into its layer's record, it writes none of its own.
    Light(super::gives_light::GivesLight),
    /// Heat glow: a Flash record that reads the heat.
    Heat(super::heat::SubHeat),
}

impl SubLayer {
    /// A ball's sub-layer of `kind` with its settings `p`; `None` for a kind that isn't one.
    pub fn of_ball(kind: &str, p: &bpe_core::Params) -> Option<SubLayer> {
        Some(match kind {
            SUB_CUT => SubLayer::Cut(CutLayer::from_params(p)),
            SUB_GLOW => SubLayer::Glow(SubGlow::from_params(p)),
            SUB_OUTLINE => SubLayer::Outline(SubOutline::from_params(p)),
            SUB_SHADOW => SubLayer::Shadow(SubShadow::from_params(p)),
            SUB_SHADING => SubLayer::Shading(ShadingLayer::from_params(p)),
            SUB_SHINE => SubLayer::Shine(ShineLayer::from_params(p)),
            SUB_SHADER => SubLayer::Shader(Box::new(ShaderLayer::from_params(p))),
            SUB_RECOLOUR => SubLayer::Recolour(SubRecolour::from_params(p)),
            SUB_FLASH => SubLayer::Flash(SubFlash::from_params(p)),
            SUB_HUE => SubLayer::Hue(SubHue::from_params(p)),
            super::gives_light::SUB_LIGHT => SubLayer::Light(super::gives_light::GivesLight::from_params(p)),
            super::heat::SUB_HEAT => SubLayer::Heat(super::heat::SubHeat::from_params(p)),
            _ => return None,
        })
    }

    /// Whether it writes a record: all do but a Shader whose own code doesn't read, and Gives off
    /// light, whose light its layer's record carries.
    pub fn writes(&self) -> bool {
        !matches!(self, SubLayer::Shader(sh) if sh.header.is_err()) && !matches!(self, SubLayer::Light(_))
    }

    /// Whether it gives off light that flares on hits (so the balls' hits are needed).
    pub fn flares(&self) -> bool {
        matches!(self, SubLayer::Light(g) if g.flare != 0.0)
    }

    /// Whether it reads the thing's hits (Flash on hit, Colour on hit, Heat glow).
    pub fn reads_hits(&self) -> bool {
        matches!(self, SubLayer::Flash(_) | SubLayer::Hue(_) | SubLayer::Heat(_))
    }

    /// How much further than its layer it reaches past the thing's edge, in its own units (ball
    /// radii, or a shape's pixels for `px` true; `radius` turns an outline's pixels into radii).
    pub fn reach(&self, radius: f32, px: bool) -> f32 {
        let unit = if px { 1.0 } else { radius.max(1e-3) };
        let r = match self {
            SubLayer::Glow(g) if g.strength > 0.0 && g.opacity > 0.0 && g.side != GlowWhere::Inside => {
                // A ball's glow is in radii, a shape's in pixels: both reach the same sizes out.
                HALO_REACH * g.size.max(0.0)
            }
            SubLayer::Outline(o) => {
                let w = o.width.max(0.0) / unit;
                match o.sits {
                    OutlineSits::Inside => 0.0,
                    OutlineSits::Centre => 0.5 * w,
                    OutlineSits::Outside => w,
                }
            }
            SubLayer::Shadow(s) if s.opacity > 0.0 => s.distance.max(0.0) + s.softness.max(0.0),
            _ => 0.0,
        };
        if r.is_finite() { r } else { 0.0 }
    }
}

/// A shown sub-layer, with its id.
#[derive(Clone, Debug, PartialEq)]
pub struct Sub {
    pub id: LayerId,
    pub sub: SubLayer,
}

/// The sub-layers of a layer that write records.
pub fn written(subs: &[Sub]) -> impl Iterator<Item = &Sub> {
    subs.iter().filter(|s| s.sub.writes())
}

/// What a sub-layer record needs besides the sub-layer: where the layers under it are, by id
/// (for a cut that names a layer), how far its layer's pattern has spun (for a cut moving with
/// the layer), the stack's effects (for a shader) and where gradients and brush tiles go.
pub struct SubCx<'a> {
    pub placed: &'a [(LayerId, usize)],
    pub angle: f32,
    pub sources: &'a [Cow<'static, str>],
    pub atlas: &'a mut GradientAtlas,
    pub tiles: &'a mut Vec<(u64, Arc<[u8]>)>,
}

/// The record of sub-layer `sub` (codes 20 to 29, see `LayerGpu` in bpe-gfx): its fields as
/// sub_layers.wgsl and the walks read them. Sizes are in the walk's units (a ball's radii, a
/// shape's pixels) but for an outline's width, in pixels for both.
///
/// | code | head.z | amount | p0 | p1 | colour / paint |
/// | --- | --- | --- | --- | --- | --- |
/// | 20 Cut | | 1, target record, pattern spin, shape | from, invert, softness, moves | angle, size, threshold, tile | |
/// | 21 Glow | FLAG same as layer | opacity, strength | size, where | | its own |
/// | 22 Outline | FLAG same as layer | opacity | width px, sits | | its own |
/// | 23 Shadow | | opacity | offset x, y, softness, moves | | its colour |
/// | 24 Shading | INVERT | as a Shading layer's | | | |
/// | 25 Shine | FLAG own light | as a Shine layer's | | | |
/// | 26 Shader | | as a Shader layer's | | | |
/// | 27 Recolour | | opacity, amount | mode, turn (turns), brightness, contrast | saturation | tint / gradient |
/// | 28 Flash | | strength, length, follow impact | | | its colour |
/// | 29 Colour on hit | | turn per hit (turns) | | | |
pub fn sub_record(sub: &SubLayer, cx: &mut SubCx) -> LayerGpu {
    let flag = |from: SubColourFrom| if from == SubColourFrom::SameAsLayer { LayerGpu::FLAG } else { 0 };
    let r = match sub {
        SubLayer::Cut(m) => {
            // A cut naming a layer that is missing, hidden, not under it or not drawn passes nothing.
            let target =
                cx.placed.iter().find(|(id, _)| id.0 as i64 == m.target as i64).map_or(-1.0, |&(_, i)| i as f32);
            let tile = if m.from == CutFrom::Brush { tile_index(cx.tiles, &m.strokes) } else { -1.0 };
            LayerGpu {
                head: [LayerGpu::SUB_CUT, 0, 0, 0],
                amount: [1.0, target, cx.angle, m.shape.code()],
                p0: [m.from.code(), if m.invert { 1.0 } else { 0.0 }, m.softness, m.moves.code()],
                p1: [m.angle.to_radians(), m.size, m.threshold, tile],
                ..LayerGpu::ZERO
            }
        }
        SubLayer::Glow(g) => LayerGpu {
            head: [LayerGpu::SUB_GLOW, 0, flag(g.colour_from), 0],
            amount: [g.opacity, g.strength, 0.0, 0.0],
            p0: [g.size.max(1e-3), g.side.code(), 0.0, 0.0],
            colour: g.colour.sample(0.5),
            paint: own_paint(&g.colour, cx.atlas),
            ..LayerGpu::ZERO
        },
        SubLayer::Outline(o) => LayerGpu {
            head: [LayerGpu::SUB_OUTLINE, 0, flag(o.colour_from), 0],
            amount: [o.opacity, 0.0, 0.0, 0.0],
            p0: [o.width.max(0.0), o.sits.code(), 0.0, 0.0],
            colour: o.colour.sample(0.5),
            paint: own_paint(&o.colour, cx.atlas),
            ..LayerGpu::ZERO
        },
        SubLayer::Shadow(s) => {
            let a = if s.angle.is_finite() { s.angle } else { 0.0 }.to_radians();
            let d = s.distance.max(0.0);
            LayerGpu {
                head: [LayerGpu::SUB_SHADOW, 0, 0, 0],
                amount: [s.opacity, 0.0, 0.0, 0.0],
                p0: [d * a.cos(), d * a.sin(), s.softness.max(0.0), s.moves.code()],
                colour: s.colour.to_linear(),
                ..LayerGpu::ZERO
            }
        }
        // Shading keeps its INVERT and Shine its FLAG (own light) from the shared records.
        SubLayer::Shading(s) => {
            let r = shading_record(s, cx.atlas, cx.tiles);
            LayerGpu { head: [LayerGpu::SUB_SHADING, 0, r.head[2], 0], ..r }
        }
        SubLayer::Shine(s) => {
            let r = shine_record(s, cx.atlas);
            LayerGpu { head: [LayerGpu::SUB_SHINE, 0, r.head[2], 0], ..r }
        }
        SubLayer::Shader(sh) => LayerGpu { head: [LayerGpu::SUB_SHADER, 0, 0, 0], ..shader_record(sh, cx.sources) },
        SubLayer::Recolour(c) => {
            let gradient = c.mode == RecolourMode::Gradient;
            LayerGpu {
                head: [LayerGpu::SUB_RECOLOUR, 0, 0, 0],
                amount: [c.opacity, c.amount, 0.0, 0.0],
                p0: [c.mode.code(), c.turn / 360.0, c.brightness, c.contrast],
                p1: [c.saturation, 0.0, 0.0, 0.0],
                colour: if gradient { c.colours.sample(0.5) } else { c.tint.to_linear() },
                // The gradient's colours, darkest first, read by how light each colour is.
                paint: if gradient {
                    bpe_gfx::PaintGpu {
                        row: cx.atlas.row(&c.colours) as f32,
                        grain: c.colours.grain,
                        ..bpe_gfx::PaintGpu::default()
                    }
                } else {
                    bpe_gfx::PaintGpu::default()
                },
                ..LayerGpu::ZERO
            }
        }
        SubLayer::Flash(f) => LayerGpu {
            head: [LayerGpu::SUB_FLASH, 0, 0, 0],
            amount: [f.strength, f.length, if f.follow_impact { 1.0 } else { 0.0 }, 0.0],
            colour: f.colour.to_linear(),
            ..LayerGpu::ZERO
        },
        SubLayer::Hue(h) => LayerGpu {
            head: [LayerGpu::SUB_HUE, 0, 0, 0],
            amount: [h.turn_per_hit / 360.0, 0.0, 0.0, 0.0],
            ..LayerGpu::ZERO
        },
        // Never written (see `SubLayer::writes`): its light rides on its layer's record.
        SubLayer::Light(_) => LayerGpu::ZERO,
        SubLayer::Heat(h) => h.record(),
    };
    // The codes are fixed above; the shared records keep their own flags.
    LayerGpu { head: [r.head[0], 0, r.head[2], 0], ..r }
}

/// A Shading layer's record (top level or sub-layer), its code and blend left to the caller.
fn shading_record(s: &ShadingLayer, atlas: &mut GradientAtlas, tiles: &mut Vec<(u64, Arc<[u8]>)>) -> LayerGpu {
    let p1 = match s.kind {
        ShadingKind::Linear => [s.from.x, s.from.y, s.to.x, s.to.y],
        ShadingKind::Radial => [s.centre.x, s.centre.y, s.size, 0.0],
        ShadingKind::Brush => [0.0, 0.0, 0.0, tile_index(tiles, &s.strokes)],
    };
    LayerGpu {
        head: [LayerGpu::SHADING, 0, if s.invert { LayerGpu::INVERT } else { 0 }, 0],
        amount: [s.opacity, 0.0, 0.0, 0.0],
        p0: [s.kind.code(), s.softness, s.space.code(), s.amount],
        p1,
        colour: s.colour.sample(0.5),
        paint: own_paint(&s.colour, atlas),
        ..LayerGpu::ZERO
    }
}

/// A Shine layer's record (top level or sub-layer): white light is the classic shine; own
/// colours tint its light.
fn shine_record(s: &ShineLayer, atlas: &mut GradientAtlas) -> LayerGpu {
    let own = s.light == ShineLight::Own;
    LayerGpu {
        head: [LayerGpu::SHINE, 0, if own { LayerGpu::FLAG } else { 0 }, 0],
        amount: [s.opacity, s.strength, 0.0, 0.0],
        p0: [s.spot.x, s.spot.y, s.size, s.material.code()],
        p1: [s.sharpness(), 0.0, 0.0, 0.0],
        colour: if own { s.colour.sample(0.5) } else { [1.0; 4] },
        paint: if own { own_paint(&s.colour, atlas) } else { bpe_gfx::PaintGpu::default() },
        ..LayerGpu::ZERO
    }
}

/// A Shader layer's record (top level or sub-layer): its effect's slot in `sources` and its
/// settings in `extra`.
pub fn shader_record(sh: &ShaderLayer, sources: &[Cow<'static, str>]) -> LayerGpu {
    let slot = sources.iter().position(|src| *src == sh.source).unwrap_or(0);
    let mut extra = [[0.0; 4]; 4];
    for (i, v) in sh.params.iter().enumerate().take(16) {
        extra[i / 4][i % 4] = *v;
    }
    LayerGpu {
        head: [LayerGpu::SHADER, 0, 0, 0],
        amount: [sh.opacity, 0.0, 0.0, 0.0],
        p0: [sh.speed, sh.scale, sh.space.code(), slot as f32],
        extra,
        ..LayerGpu::ZERO
    }
}

/// A drawn layer above the body, with its id and its shown sub-layers.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawnLayer {
    pub layer: Drawn,
    pub id: LayerId,
    /// Its shown sub-layers, in order, each over the layer and the ones before it.
    pub subs: Vec<Sub>,
    /// The balls of a group it is given to (empty: every ball), from [`bpe_core::Layer::only`].
    pub only: Vec<u32>,
    /// How it takes the scene's light ([`super::lighting::of`]).
    pub light: [f32; 4],
    /// Its UV and beams ([`super::lighting::extra_of`], no link number yet).
    pub more: [f32; 4],
}

impl DrawnLayer {
    /// Whether it writes a record: all do but a Shader layer whose own code doesn't read.
    pub fn writes(&self) -> bool {
        !matches!(&self.layer, Drawn::Shader(sh) if sh.header.is_err())
    }

    /// How many records it writes: its own and its sub-layers' (none when it writes none).
    pub fn records(&self) -> usize {
        if self.writes() { 1 + written(&self.subs).count() } else { 0 }
    }

    /// The light its Gives off light sub-layers give off, when it has any.
    pub fn emit(&self) -> Option<super::gives_light::Emit> {
        super::gives_light::Emit::of_subs(self.subs.iter().map(|s| &s.sub))
    }
}

/// A group layer that writes a record after the ball's own layers, in the group list's order.
#[derive(Clone, Debug, PartialEq)]
pub enum GroupEnd {
    /// The group gradient's record, when it changes every layer.
    Gradient,
    /// A group shader, with its id.
    Shader(LayerId, Box<GroupShader>),
}

/// A thing's shown stack, read from its resolved Look card.
#[derive(Clone, Debug, PartialEq)]
pub struct Stack {
    pub body_id: LayerId,
    pub body: BodyLayer,
    /// How the Body takes the scene's light ([`super::lighting::of`]).
    pub body_light: [f32; 4],
    /// The Body's UV ([`super::lighting::extra_of`], no link number yet).
    pub body_more: [f32; 4],
    /// The light the Body gives off (its Gives off light sub-layers), when it gives any.
    pub body_emit: Option<super::gives_light::Emit>,
    /// The Body's Heat glow sub-layers: their records follow the Body's.
    pub body_heat: Vec<super::heat::SubHeat>,
    /// The drawn layers above the body, bottom first.
    pub drawn: Vec<DrawnLayer>,
    /// The group gradient, when the look has one shown and it mixes in.
    pub group: Option<GroupGradient>,
    /// The group layers that write records after the ball's own, in order.
    pub ends: Vec<GroupEnd>,
    pub blend: Option<BlendLayer>,
    /// How many drawn layers sit under the Blend layer (all of them without one).
    pub blend_at: usize,
    pub flash: Option<FlashOnHit>,
    pub hue: Option<ColourOnHit>,
    pub stretch: Option<StretchWhenFast>,
    pub rainbow: Option<ColourOverTime>,
    pub turning: Option<Turning>,
    /// How long after a hit a gradient of the card still shows its Hit wave, 0 when none has
    /// one ([`bpe_core::gradient_extras::wave_memory`]).
    pub wave_memory: f32,
}

impl Stack {
    /// The stack of a `look.layers` card; `None` for any other card, or one with no Body.
    pub fn of(slot: &ResolvedSlot) -> Option<Stack> {
        if slot.kind != super::CARD {
            return None;
        }
        let body = slot.first_layer(BODY)?;
        let mut stack = Stack {
            body_id: body.id,
            body: BodyLayer::from_params(&slot.layer_params(body.id)),
            body_light: {
                // The Body carries the Look's Bulge (its Surface section) for the ball's height.
                let mut light = super::lighting::of(&slot.layer_params(body.id));
                light[2] = crate::modules::look::surface::of_card(slot).bulge;
                light
            },
            body_more: super::lighting::extra_of(&slot.layer_params(body.id), 0.0, true),
            body_emit: None,
            body_heat: Vec::new(),
            drawn: Vec::new(),
            group: None,
            ends: Vec::new(),
            blend: None,
            blend_at: 0,
            flash: None,
            hue: None,
            stretch: None,
            rainbow: None,
            turning: None,
            wave_memory: bpe_core::gradient_extras::wave_memory(&slot.params),
        };
        for layer in &slot.layers {
            let p = slot.layer_params(layer.id);
            let mut push = |d: Drawn| {
                stack.drawn.push(DrawnLayer {
                    layer: d,
                    id: layer.id,
                    subs: Vec::new(),
                    only: layer.only.clone(),
                    light: super::lighting::of(&p),
                    more: super::lighting::extra_of(&p, 0.0, true),
                })
            };
            match layer.kind.as_str() {
                PATTERN => push(Drawn::Pattern(PatternLayer::from_params(&p))),
                OUTLINE => push(Drawn::Outline(OutlineLayer::from_params(&p))),
                SHINE => push(Drawn::Shine(ShineLayer::from_params(&p))),
                GLOW => push(Drawn::Glow(GlowLayer::from_params(&p))),
                SHADING => push(Drawn::Shading(ShadingLayer::from_params(&p))),
                SHADER => push(Drawn::Shader(Box::new(ShaderLayer::from_params(&p)))),
                // A sub-layer goes with its layer; a hidden layer's don't matter.
                kind if layer.parent.is_some() => {
                    if let Some(sub) = SubLayer::of_ball(kind, &p) {
                        if let Some(d) = stack.drawn.iter_mut().find(|d| Some(d.id) == layer.parent) {
                            d.subs.push(Sub { id: layer.id, sub });
                        } else if layer.parent == Some(stack.body_id) {
                            // The Body takes Gives off light and Heat glow only.
                            match sub {
                                SubLayer::Light(g) => stack.body_emit.get_or_insert_with(Default::default).add(&g),
                                SubLayer::Heat(h) => stack.body_heat.push(h),
                                _ => {}
                            }
                        }
                    }
                }
                // Blend and the reactions are there at most once; the first counts.
                BLEND => {
                    if stack.blend.is_none() {
                        stack.blend = Some(BlendLayer::from_params(&p));
                        stack.blend_at = stack.drawn.len();
                    }
                }
                FLASH => stack.flash = stack.flash.take().or(Some(FlashOnHit::from_params(&p))),
                HUE => stack.hue = stack.hue.take().or(Some(ColourOnHit::from_params(&p))),
                STRETCH => stack.stretch = stack.stretch.take().or(Some(StretchWhenFast::from_params(&p))),
                RAINBOW => stack.rainbow = stack.rainbow.take().or(Some(ColourOverTime::from_params(&p))),
                TURNING => stack.turning = stack.turning.take().or(Some(Turning::from_params(&p))),
                GROUP_GRADIENT => {
                    let g = GroupGradient::from_params(&p);
                    if stack.group.is_none() && g.mix != GroupMix::Off {
                        stack.group = Some(g);
                        stack.ends.push(GroupEnd::Gradient);
                    }
                }
                GROUP_SHADER => {
                    let gs = GroupShader::from_params(&p);
                    if gs.writes() {
                        stack.ends.push(GroupEnd::Shader(layer.id, Box::new(gs)));
                    }
                }
                _ => {}
            }
        }
        if stack.blend.is_none() {
            stack.blend_at = stack.drawn.len();
        }
        Some(stack)
    }

    /// Whether the Body's colours run across each ball on its own (Each ball, several colours).
    pub fn body_each_ball(&self) -> bool {
        self.body.fill.follow == Follow::EachBall && self.body.fill.colours.stops.len() > 1
    }

    /// Where the records of the layers above the Blend layer start, when the ball draws in two
    /// parts: the layers under the Blend layer mixed with what is behind by its blend, then the
    /// ones above it painted normally over them. `None` when it draws in one: no Blend layer, a
    /// Normal one at full amount, or nothing drawn above it.
    pub fn split(&self) -> Option<u32> {
        let b = self.blend.as_ref()?;
        if b.blend == Blend::Normal && b.blend_amount >= 1.0 {
            return None;
        }
        let drawn = &self.drawn[..self.kept()];
        let at = self.blend_at.min(drawn.len());
        if !drawn[at..].iter().any(|d| d.writes()) {
            return None;
        }
        let under: usize = drawn[..at].iter().map(|d| d.records()).sum();
        Some((self.body_records() + under) as u32)
    }

    /// How many records the group layers write after the ball's own.
    fn group_records(&self) -> usize {
        let gradient = self.group_on_gpu().is_some_and(|g| g.changes == Changes::EveryLayer);
        self.ends.iter().filter(|e| matches!(e, GroupEnd::Shader(..)) || gradient).count()
    }

    /// How many of `drawn` (from the bottom) have their records written: the walk reads at most
    /// [`bpe_gfx::MAX_LAYERS`] records, so past that whole layers are left out from the top,
    /// never part of one, keeping room for the group records at the end.
    pub fn kept(&self) -> usize {
        let room = (bpe_gfx::MAX_LAYERS as usize).saturating_sub(self.body_records() + self.group_records());
        let mut used = 0;
        for (i, d) in self.drawn.iter().enumerate() {
            used += d.records();
            if used > room {
                return i;
            }
        }
        self.drawn.len()
    }

    /// How the whole ball mixes with what is behind it: the Blend layer's, Normal without one.
    pub fn whole_blend(&self) -> (Blend, f32, bool) {
        self.blend.as_ref().map_or((Blend::Normal, 1.0, false), |b| (b.blend, b.blend_amount, b.blend_each))
    }

    /// Hue shift per hit, in degrees: 0 without Colour on hit.
    pub fn hue_per_hit(&self) -> f32 {
        self.hue.as_ref().map_or(0.0, |h| h.hue_per_hit)
    }

    /// Turns of the colour wheel per second: 0 without Colour over time.
    pub fn rainbow(&self) -> f32 {
        self.rainbow.as_ref().map_or(0.0, |r| r.rainbow)
    }

    /// The body colour `base` (linear light) of a ball with `hits` hits at `time` seconds, after
    /// the colour reactions: turned per hit, then round the rainbow.
    pub fn body_colour(&self, base: [f32; 4], hits: u32, time: f32) -> [f32; 4] {
        recoloured(base, self.hue_per_hit(), self.rainbow() * time, hits)
    }

    /// Whether any pattern paints from its own gradient, or the body's colours run across each
    /// ball, so balls need their own variation.
    pub fn varies(&self) -> bool {
        self.body_each_ball()
            || self
                .drawn
                .iter()
                .any(|d| matches!(&d.layer, Drawn::Pattern(p) if p.inside.from == InsideFrom::OwnGradient))
    }

    /// Whether the shape turns to point the way the ball goes (never a circle, which has no point).
    pub fn faces_motion(&self) -> bool {
        self.turning.as_ref().is_some_and(|t| t.face_motion) && self.body.ball_shape != BallShape::Circle
    }

    /// How the group gradient reaches the GPU: `None` when it doesn't (no gradient, or it
    /// changes only the body, which is mixed on the CPU).
    /// A gradient body is changed on the GPU too, as the CPU can't reach its colours.
    pub fn group_on_gpu(&self) -> Option<&GroupGradient> {
        self.group.as_ref().filter(|g| g.changes != Changes::Body || self.body_each_ball())
    }

    /// The group gradient when it changes the body colour on the CPU (a plain body).
    pub fn group_on_body(&self) -> Option<&GroupGradient> {
        self.group.as_ref().filter(|g| g.changes == Changes::Body && !self.body_each_ball())
    }

    /// Whether some drawn layer is given to some balls only.
    pub fn has_only(&self) -> bool {
        self.drawn.iter().any(|d| !d.only.is_empty())
    }

    /// Whether drawn layer `index` (in `drawn`) applies to ball `number`.
    pub fn applies(&self, index: usize, number: u32) -> bool {
        self.drawn.get(index).is_none_or(|d| d.only.is_empty() || d.only.binary_search(&number).is_ok())
    }

    /// The records the stack draws with at `time` seconds, bottom first, its gradients baked into
    /// `atlas` and its brush strokes added to `tiles` (a frame's [`bpe_gfx::Frame::mask_tiles`]).
    /// Hidden layers, Blend and the reactions have none; each drawn layer's sub-layers follow it, and
    /// the group colour's record comes last when the group gradient changes every layer. Layers
    /// the group gradient's Choose picks are flagged [`LayerGpu::GROUPED`]. At most
    /// [`bpe_gfx::MAX_LAYERS`]: the layers past [`Stack::kept`] are left out.
    pub fn records(&self, atlas: &mut GradientAtlas, tiles: &mut Vec<(u64, Arc<[u8]>)>, time: f32) -> Vec<LayerGpu> {
        let blend = |b: Blend| b.shader_code();
        let flag = |on: bool| if on { LayerGpu::FLAG } else { 0 };
        let chosen = self.group_on_gpu().and_then(|g| match g.changes {
            Changes::Choose => Some(g.chosen_ids()),
            // A gradient body takes the group's colour on the GPU.
            Changes::Body => Some(vec![self.body_id.0]),
            Changes::EveryLayer => None,
        });
        let grouped =
            |id: LayerId| if chosen.as_ref().is_some_and(|c| c.contains(&id.0)) { LayerGpu::GROUPED } else { 0 };
        let turn = self.rainbow() * time;
        let body_paint = if self.body_each_ball() {
            let f = &self.body.fill;
            let mut colours = f.colours.clone();
            if turn != 0.0 {
                for stop in &mut colours.stops {
                    stop.color = stop.color.shift_hue(turn);
                }
            }
            // Across each ball by the paint's own form, placed by its Direction.
            let place = Placing { turn: f.angle, centre: [0.0; 2], k: 1.0, repeat: f.repeat, scroll: f.scroll };
            form_gpu(&colours, GradientForm::Straight, place, atlas)
        } else {
            bpe_gfx::PaintGpu::default()
        };
        let mut out = vec![LayerGpu {
            head: [LayerGpu::BODY, 0, grouped(self.body_id), self.body_heat.len() as u32],
            amount: [self.body.opacity.clamp(0.0, 1.0), 0.0, 0.0, 0.0],
            paint: body_paint,
            light: super::gives_light::unlit_if_lamp(self.body_light, self.body_emit),
            emit: super::gives_light::gpu_of(self.body_emit),
            more: self.body_more,
            ..LayerGpu::ZERO
        }];
        out.extend(self.body_heat.iter().map(super::heat::SubHeat::record));
        // Where each layer's record is, by id, for cuts that name a layer.
        let mut placed: Vec<(LayerId, usize)> = vec![(self.body_id, 0)];
        let sources = self.effect_sources();
        for d in &self.drawn[..self.kept()] {
            let r = match &d.layer {
                Drawn::Pattern(p) => {
                    let mut inside = p.inside.clone();
                    if turn != 0.0 && inside.from == InsideFrom::OwnGradient {
                        for stop in &mut inside.colours.stops {
                            stop.color = stop.color.shift_hue(turn);
                        }
                    }
                    LayerGpu {
                        head: [LayerGpu::PATTERN, blend(p.blend), flag(inside.turns), 0],
                        amount: [p.opacity, 0.0, 0.0, 0.0],
                        paint: inside_gpu(&inside, bpe_gfx::NO_VARY, atlas, time),
                        ..LayerGpu::ZERO
                    }
                }
                Drawn::Outline(o) => LayerGpu {
                    head: [LayerGpu::OUTLINE, blend(o.blend), flag(o.colour_from == ColourFrom::SameAsBody), 0],
                    amount: [o.opacity, 0.0, 0.0, 0.0],
                    p0: [o.width.max(0.0), o.sits.code(), 0.0, 0.0],
                    colour: o.colour.sample(0.5),
                    paint: own_paint(&o.colour, atlas),
                    ..LayerGpu::ZERO
                },
                Drawn::Shine(s) => {
                    let r = shine_record(s, atlas);
                    LayerGpu { head: [LayerGpu::SHINE, blend(s.blend), r.head[2], 0], ..r }
                }
                Drawn::Glow(g) => LayerGpu {
                    head: [LayerGpu::GLOW, blend(g.blend), flag(g.colour_from == ColourFrom::SameAsBody), 0],
                    amount: [g.opacity, g.strength, 0.0, 0.0],
                    p0: [g.size.max(0.05), 0.0, 0.0, 0.0],
                    colour: g.colour.sample(0.5),
                    paint: own_paint(&g.colour, atlas),
                    ..LayerGpu::ZERO
                },
                Drawn::Shading(s) => {
                    let r = shading_record(s, atlas, tiles);
                    LayerGpu { head: [LayerGpu::SHADING, blend(s.blend), r.head[2], 0], ..r }
                }
                // Own code that doesn't read draws nothing; the editor says why.
                Drawn::Shader(_) if !d.writes() => continue,
                Drawn::Shader(sh) => {
                    LayerGpu { head: [LayerGpu::SHADER, blend(sh.blend), 0, 0], ..shader_record(sh, &sources) }
                }
            };
            let at = out.len();
            // "Moves with the layer": a pattern's cut turns as the pattern spins, from where the
            // cut's own angle puts it, not by the pattern's own angle.
            let angle = match &d.layer {
                Drawn::Pattern(p) => (r.paint.angle - p.inside.angle).to_radians(),
                _ => 0.0,
            };
            let subs = written(&d.subs).count() as u32;
            let emit = d.emit();
            out.push(LayerGpu {
                head: [r.head[0], r.head[1], r.head[2] | grouped(d.id), subs],
                light: super::gives_light::unlit_if_lamp(d.light, emit),
                emit: super::gives_light::gpu_of(emit),
                more: d.more,
                ..r
            });
            let mut cx = SubCx { placed: &placed, angle, sources: &sources, atlas, tiles };
            for s in written(&d.subs) {
                out.push(sub_record(&s.sub, &mut cx));
            }
            placed.push((d.id, at));
        }
        // The group records, in the group list's order. A group shader that changes some layers
        // only is named by them: their `amount.z` is the first group record and `amount.w` a bit
        // mask of the group records after it.
        let ends_at = out.len();
        for end in &self.ends {
            match end {
                GroupEnd::Gradient => {
                    if self.group_on_gpu().is_some_and(|g| g.changes == Changes::EveryLayer) {
                        out.push(LayerGpu { head: [LayerGpu::GROUP, 0, 0, 0], ..LayerGpu::ZERO });
                    }
                }
                GroupEnd::Shader(_, gs) => {
                    let sh = &gs.shader;
                    let slot = sources.iter().position(|src| *src == sh.source).unwrap_or(0);
                    let extra = shader_record(sh, &sources).extra;
                    let whole = gs.changes == Changes::EveryLayer;
                    let bit = out.len() - ends_at;
                    out.push(LayerGpu {
                        head: [LayerGpu::GROUP_SHADER, blend(gs.mix.blend()), 0, 0],
                        amount: [gs.amount, 0.0, 0.0, 0.0],
                        p0: [sh.speed, sh.scale, if whole { 1.0 } else { 0.0 }, slot as f32],
                        // How much it leaves uncovered (0: the whole group), from where, how soft.
                        p1: [gs.around.unwrap_or(0.0), 1.0 - gs.covers, gs.from.to_radians(), gs.edge],
                        extra,
                        ..LayerGpu::ZERO
                    });
                    if whole || bit >= 24 {
                        continue;
                    }
                    let targets: Vec<usize> = match gs.changes {
                        Changes::Body => vec![0],
                        _ => {
                            let chosen = gs.chosen_ids();
                            placed.iter().filter(|(id, _)| chosen.contains(&id.0)).map(|&(_, i)| i).collect()
                        }
                    };
                    for t in targets {
                        let r = &mut out[t];
                        r.amount[2] = ends_at as f32;
                        r.amount[3] = (r.amount[3] as u32 | 1 << bit) as f32;
                    }
                }
            }
        }
        out
    }

    /// The layer a click at ball point `q` (in radii, y down, before the ball turns) picks: the
    /// topmost shown drawn layer covering it, else the Body inside the shape, else nothing.
    /// Patterns, shine, shading and shaders cover the shape; an outline its band (`radius` is the
    /// ball's, in pixels); a glow the ring just outside. Sub-layers don't change what a click picks.
    pub fn topmost_at(&self, q: Vec2, radius: f32) -> Option<LayerId> {
        let d = shape_distance(self.body.ball_shape, self.body.roundness, q);
        // Thin bands still take a click a few pixels wide.
        let slack = (3.0 / radius.max(1.0)).max(0.03);
        for layer in self.drawn.iter().rev().filter(|l| l.writes()) {
            let covers = match &layer.layer {
                Drawn::Outline(o) => {
                    let w = o.width.max(0.0) / radius.max(1.0);
                    let (lo, hi) = match o.sits {
                        OutlineSits::Inside => (-w, 0.0),
                        OutlineSits::Centre => (-0.5 * w, 0.5 * w),
                        OutlineSits::Outside => (0.0, w),
                    };
                    w > 0.0 && d >= lo - slack && d <= hi + slack
                }
                Drawn::Glow(g) => d > slack && d <= g.size.max(0.05),
                _ => d <= 0.0,
            };
            if covers {
                return Some(layer.id);
            }
        }
        (d <= 0.0).then_some(self.body_id)
    }

    /// Where drawn layer `id`'s record is in [`Stack::records`] (the body's is 0); `None` for a
    /// layer that writes none, or is left out past [`Stack::kept`].
    pub fn record_of(&self, id: LayerId) -> Option<usize> {
        if id == self.body_id {
            return Some(0);
        }
        let mut at = self.body_records();
        for d in self.drawn[..self.kept()].iter().filter(|d| d.writes()) {
            if d.id == id {
                return Some(at);
            }
            at += d.records();
        }
        None
    }

    /// The effects the stack's shader layers and shader sub-layers run, each once, in order: a
    /// shader record's effect slot is its place here. Own code that doesn't read is left out.
    pub fn effect_sources(&self) -> Vec<Cow<'static, str>> {
        let mut out: Vec<Cow<'static, str>> = Vec::new();
        let mut add = |sh: &ShaderLayer| {
            if sh.header.is_ok() && !out.contains(&sh.source) {
                out.push(sh.source.clone());
            }
        };
        for d in &self.drawn {
            if let Drawn::Shader(sh) = &d.layer {
                add(sh);
            }
            // A layer that writes no record writes none of its sub-layers'.
            if d.writes() {
                for s in &d.subs {
                    if let SubLayer::Shader(sh) = &s.sub {
                        add(sh);
                    }
                }
            }
        }
        for end in &self.ends {
            if let GroupEnd::Shader(_, gs) = end
                && !out.contains(&gs.shader.source)
            {
                out.push(gs.shader.source.clone());
            }
        }
        out
    }

    /// Whether a shown sub-layer reads the balls' hits (Flash on hit, Colour on hit, Heat glow),
    /// so each ball's instance needs its hit data.
    pub fn needs_hits(&self) -> bool {
        self.drawn.iter().any(|d| d.writes() && d.subs.iter().any(|s| s.sub.reads_hits() || s.sub.flares()))
            || self.body_emit.is_some_and(|e| e.flare != 0.0)
            || !self.body_heat.is_empty()
            || self.wave_memory > 0.0
    }

    /// How many records the Body writes: its own and its Heat glows'.
    pub fn body_records(&self) -> usize {
        1 + self.body_heat.len()
    }

    /// The Heat glow whose heat its balls carry (`hit[3]`): the Body's first, else the first on a
    /// drawn layer. One heat a ball; each Heat glow shows it by its own Glow and Hottest.
    pub fn heat(&self) -> Option<&super::heat::SubHeat> {
        self.body_heat.first().or_else(|| {
            self.drawn.iter().filter(|d| d.writes()).flat_map(|d| &d.subs).find_map(|s| match &s.sub {
                SubLayer::Heat(h) => Some(h),
                _ => None,
            })
        })
    }

    /// Whether any of its shown layers gives off light (then a lit scene draws what it gives).
    pub fn gives_light(&self) -> bool {
        self.body_emit.is_some() || self.drawn.iter().any(|d| d.writes() && d.emit().is_some())
    }

    /// How long ago a hit can still show in a Flash sub-layer, in seconds (its light has all but
    /// gone five lengths on); 0 without one.
    pub fn flash_memory(&self) -> f32 {
        let lengths = self.drawn.iter().filter(|d| d.writes()).flat_map(|d| &d.subs);
        lengths
            .filter_map(|s| match &s.sub {
                SubLayer::Flash(f) if f.strength > 0.0 => Some(5.0 * f.length.max(0.0)),
                SubLayer::Light(g) if g.flare != 0.0 => Some(5.0 * g.flare_length.max(0.0)),
                _ => None,
            })
            .fold(0.0, f32::max)
            .max(self.body_emit.filter(|e| e.flare != 0.0).map_or(0.0, |e| 5.0 * e.length))
            .max(self.wave_memory)
    }

    /// The group shaders, in the group list's order.
    pub fn group_shaders(&self) -> impl Iterator<Item = &GroupShader> {
        self.ends.iter().filter_map(|e| match e {
            GroupEnd::Shader(_, gs) => Some(gs.as_ref()),
            GroupEnd::Gradient => None,
        })
    }

    /// The in-ball program the stack draws with: `None` without shader layers or group shaders.
    pub fn program(&self) -> Option<LookProgram> {
        let sources = self.effect_sources();
        (!sources.is_empty()).then(|| LookProgram::new(sources))
    }

    /// The records for a ball that wears only the drawn layers `wears` says (by index in
    /// `drawn`): `all` (from [`Stack::records`]) with the others marked [`LayerGpu::UNWORN`] (and
    /// at opacity 0), so every record stays where cuts and the group records expect it. Their sub-layers' records
    /// stay: what they add fades with the layer's opacity.
    pub fn records_for(&self, all: &[LayerGpu], wears: impl Fn(usize) -> bool) -> Vec<LayerGpu> {
        let mut out = all.to_vec();
        for (i, d) in self.drawn.iter().enumerate() {
            if !wears(i)
                && let Some(k) = self.record_of(d.id)
            {
                out[k].amount[0] = 0.0;
                out[k].head[2] |= LayerGpu::UNWORN;
            }
        }
        out
    }

    /// How far past the centre a ball of `radius` with this stack reaches, in radii.
    pub fn reach(&self, radius: f32) -> f32 {
        let mut past_edge: f32 = 0.0;
        for d in &self.drawn {
            let own = match &d.layer {
                Drawn::Glow(g) if g.strength > 0.0 && g.opacity > 0.0 => HALO_REACH * g.size,
                Drawn::Outline(o) => {
                    let out = match o.sits {
                        OutlineSits::Inside => 0.0,
                        OutlineSits::Centre => 0.5 * o.width,
                        OutlineSits::Outside => o.width,
                    };
                    out.max(0.0) / radius.max(1e-3)
                }
                _ => 0.0,
            };
            // Each sub-layer that reaches out does so from where the piece reaches so far.
            let subs: f32 = d.subs.iter().map(|s| s.sub.reach(radius, false)).sum();
            past_edge = past_edge.max(own + subs);
        }
        // A group shader's ring fades out a little past its size.
        for gs in self.group_shaders() {
            if let Some(around) = gs.around.filter(|_| gs.amount > 0.0) {
                past_edge = past_edge.max(1.25 * around);
            }
        }
        (SHAPE_REACH + past_edge).min(MAX_REACH)
    }

    /// The instance for one ball at `center` moving at `velocity`, of `radius`, at `time`, its body
    /// in `body` (after the colour reactions, [`bpe_gfx::blend_alpha`]ed), its thing code and seed,
    /// its own variation and its records at `first`.
    #[allow(clippy::too_many_arguments)]
    pub fn instance(
        &self,
        body: [f32; 4],
        center: Vec2,
        velocity: Vec2,
        radius: f32,
        time: f32,
        code: f32,
        seed: f32,
        vary: [f32; 4],
        first: u32,
        count: u32,
    ) -> LayeredInstance {
        let speed = velocity.length();
        let way = heading(velocity);
        // Fast balls stretch along their path and thin out across it, keeping about the same area.
        let stretch = self.stretch.as_ref().map_or(0.0, |s| s.stretch);
        let amount = stretch.clamp(0.0, 1.0) * (speed / STRETCH_SPEED).min(1.5);
        let along = 1.0 + amount;
        let across = 1.0 / along.sqrt();
        let faces = self.faces_motion();
        let stretches = amount > 1e-4 || faces;
        let dir = if stretches { way } else { Vec2::X };
        let (turn, spin) = self.turning.as_ref().map_or((0.0, 0.0), |t| (t.turn, t.spin));
        let mut angle = (turn + spin * time).to_radians();
        if faces {
            // Shapes point up at no turn; a quarter turn points them along the motion.
            angle += FRAC_PI_2;
        } else if stretches {
            // Keep the shape and pattern still on screen while the quad lines up with the motion.
            angle -= way.to_angle();
        }
        LayeredInstance {
            center: center.into(),
            radius,
            seed,
            body,
            stretch: [dir.x, dir.y, along, across],
            form: [self.body.ball_shape.code(), self.body.roundness.clamp(0.0, 1.0), angle, self.reach(radius)],
            misc: [1.0, code, facing_degrees(velocity), if faces { 1.0 } else { 0.0 }],
            vary,
            range: [first, count, 0, 0],
            group: [0.0; 4],
            place: [0.0; 4],
            hit: bpe_gfx::NO_HIT,
            clip: [0.0; 4],
        }
    }
}

/// How a layer's own `colours` paint across each ball: nothing (row -1, the record's plain colour)
/// for one colour, else laid out by the gradient's Shape tab (Straight top to bottom when nothing
/// is stored; Round is measured by layers.wgsl for the layer: an outline across its ring, a glow
/// out from the edge).
fn own_paint(colours: &bpe_core::Gradient, atlas: &mut GradientAtlas) -> bpe_gfx::PaintGpu {
    if colours.stops.len() <= 1 {
        return bpe_gfx::PaintGpu::default();
    }
    form_gpu(colours, GradientForm::Straight, Placing::of_layout(&colours.layout), atlas)
}

/// How far ball point `q` is from the edge of `shape`, in radii; negative inside. The circle,
/// square and donut are exact; the other shapes are taken as a circle.
pub fn shape_distance(shape: BallShape, roundness: f32, q: Vec2) -> f32 {
    let k = roundness.clamp(0.0, 1.0) * 0.35;
    match shape {
        BallShape::Square => {
            let d = q.abs() - Vec2::splat(0.84 - k);
            d.max(Vec2::ZERO).length() + d.x.max(d.y).min(0.0) - k
        }
        BallShape::Donut => (q.length() - 0.72).abs() - 0.28,
        _ => q.length() - 1.0,
    }
}

/// Where `strokes`' tile is in `tiles`, adding it if it isn't there yet; -1 for no strokes or
/// past the most a frame holds.
fn tile_index(tiles: &mut Vec<(u64, Arc<[u8]>)>, strokes: &bpe_core::Strokes) -> f32 {
    if strokes.list.is_empty() {
        return -1.0;
    }
    let hash = strokes.hash64();
    if let Some(i) = tiles.iter().position(|(h, _)| *h == hash) {
        return i as f32;
    }
    if tiles.len() >= bpe_gfx::MAX_MASK_TILES {
        return -1.0;
    }
    tiles.push(crate::paint::brush::tile(strokes));
    (tiles.len() - 1) as f32
}

/// `base` (linear light) turned `hue_per_hit` degrees for each of `hits`, then `turn` turns round
/// the colour wheel.
pub fn recoloured(base: [f32; 4], hue_per_hit: f32, turn: f32, hits: u32) -> [f32; 4] {
    let c = crate::modules::look::draw::hue_shifted(base, hue_per_hit, hits);
    if turn != 0.0 { shift_linear_hue(c, turn) } else { c }
}

/// The name a choice shows as, for summaries: "Same as body".
pub fn choice_label<C: Choice>(c: C) -> String {
    let name = c.name().replace('_', " ");
    let mut chars = name.chars();
    match chars.next() {
        Some(f) => f.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::GROUP_SHADER;
    use super::*;
    use crate::registry;
    use bpe_core::{Scene, SlotId, ThingId, Value, layered_key, resolve};

    /// A new ball's resolved Look after `f` changes its scene.
    fn stack_after(f: impl FnOnce(&bpe_core::Registry, &mut Scene, ThingId)) -> Stack {
        let reg = registry();
        let mut s = Scene::empty(&reg);
        let b = crate::add_thing(&mut s, &reg, "balls", ThingId::ROOT).unwrap();
        f(&reg, &mut s, b);
        let mut resolved = resolve(&s, &reg);
        bpe_core::gradient_extras::apply_moves(&mut resolved, 0.0);
        Stack::of(&resolved.things[&b].slots[&SlotId::Look]).unwrap()
    }

    #[test]
    fn a_new_ball_draws_one_body_record_and_reaches_just_past_its_shape() {
        let stack = stack_after(|_, _, _| {});
        let mut atlas = GradientAtlas::default();
        let records = stack.records(&mut atlas, &mut Vec::new(), 0.0);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].head[0], LayerGpu::BODY);
        assert_eq!(stack.reach(24.0), SHAPE_REACH);
        assert_eq!(stack.whole_blend(), (Blend::Normal, 1.0, false));
        assert!(stack.flash.is_none() && stack.hue.is_none() && stack.turning.is_none());
    }

    #[test]
    fn each_drawn_layer_writes_its_code_and_settings_in_order() {
        let stack = stack_after(|reg, s, b| {
            let o = s.add_layer(reg, b, SlotId::Look, OUTLINE).unwrap();
            s.set_at(reg, &bpe_core::Address::on(b, SlotId::Look, &layered_key(o, "width")), Value::Number(9.0))
                .unwrap();
            s.set_at(
                reg,
                &bpe_core::Address::on(b, SlotId::Look, &layered_key(o, "sits")),
                Value::Choice("outside".into()),
            )
            .unwrap();
            s.add_layer(reg, b, SlotId::Look, GLOW).unwrap();
            let hidden = s.add_layer(reg, b, SlotId::Look, SHINE).unwrap();
            s.set_layer_enabled(reg, b, SlotId::Look, hidden, false).unwrap();
            s.add_layer(reg, b, SlotId::Look, PATTERN).unwrap();
        });
        let records = stack.records(&mut GradientAtlas::default(), &mut Vec::new(), 0.0);
        let codes: Vec<u32> = records.iter().map(|r| r.head[0]).collect();
        assert_eq!(
            codes,
            [LayerGpu::BODY, LayerGpu::OUTLINE, LayerGpu::GLOW, LayerGpu::PATTERN],
            "the hidden shine is left out"
        );
        assert_eq!(records[1].p0[..2], [9.0, 2.0]);
        assert_eq!(records[1].head[2], LayerGpu::FLAG, "same as body by default");
        assert_eq!(records[2].amount[..2], [1.0, 0.8]);
        assert!(records[3].paint.row < -1.5, "a new pattern is a shade of the body");
        assert!((stack.reach(10.0) - (SHAPE_REACH + HALO_REACH * 0.8)).abs() < 1e-5, "the glow reaches furthest");
    }

    #[test]
    fn reactions_stretch_turn_and_recolour_the_ball() {
        let stack = stack_after(|reg, s, b| {
            for k in [STRETCH, HUE, RAINBOW, TURNING] {
                s.add_layer(reg, b, SlotId::Look, k).unwrap();
            }
        });
        let fast =
            stack.instance([1.0; 4], Vec2::ZERO, Vec2::new(0.0, STRETCH_SPEED), 20.0, 0.0, 3.0, 0.5, [0.0; 4], 0, 1);
        assert_eq!(fast.stretch[..2], [0.0, 1.0]);
        assert!((fast.stretch[2] - 1.3).abs() < 1e-5, "{:?}", fast.stretch);
        let still = stack.instance([1.0; 4], Vec2::ZERO, Vec2::ZERO, 20.0, 1.0, 3.0, 0.5, [0.0; 4], 0, 1);
        assert_eq!(still.stretch, [1.0, 0.0, 1.0, 1.0]);
        assert!((still.form[2] - 90f32.to_radians()).abs() < 1e-5, "spun 90° in a second");
        let red = bpe_core::Color::rgb(255, 0, 0).to_linear();
        let shifted = stack.body_colour(red, 4, 0.0);
        assert_eq!(
            bpe_core::Color::from_linear(shifted),
            bpe_core::Color::from_linear(shift_linear_hue(red, 120.0 / 360.0))
        );
        let later = stack.body_colour(red, 0, 2.0);
        assert_eq!(bpe_core::Color::from_linear(later), bpe_core::Color::from_linear(shift_linear_hue(red, 0.5)));
    }

    #[test]
    fn sub_layers_stay_with_their_shown_layer() {
        let stack = stack_after(|reg, s, b| {
            let pat = s.add_layer(reg, b, SlotId::Look, PATTERN).unwrap();
            s.add_sub_layer(reg, b, SlotId::Look, pat, SUB_CUT).unwrap();
            let off = s.add_sub_layer(reg, b, SlotId::Look, pat, SUB_GLOW).unwrap();
            s.set_layer_enabled(reg, b, SlotId::Look, off, false).unwrap();
            let hidden = s.add_layer(reg, b, SlotId::Look, OUTLINE).unwrap();
            s.add_sub_layer(reg, b, SlotId::Look, hidden, SUB_CUT).unwrap();
            s.set_layer_enabled(reg, b, SlotId::Look, hidden, false).unwrap();
            s.add_layer(reg, b, SlotId::Look, SHADING).unwrap();
        });
        assert_eq!(stack.drawn.len(), 2, "the hidden outline is left out");
        assert_eq!(stack.drawn[0].subs.len(), 1, "the hidden glow is left out");
        assert!(matches!(stack.drawn[0].subs[0].sub, SubLayer::Cut(_)));
        assert!(matches!(stack.drawn[1].layer, Drawn::Shading(_)) && stack.drawn[1].subs.is_empty());
    }

    #[test]
    fn shine_and_shading_sub_layers_keep_their_flags() {
        let mut atlas = GradientAtlas::default();
        let mut tiles = Vec::new();
        let mut cx = SubCx { placed: &[], angle: 0.0, sources: &[], atlas: &mut atlas, tiles: &mut tiles };
        let shine = ShineLayer { light: ShineLight::Own, ..ShineLayer::default() };
        let r = sub_record(&SubLayer::Shine(shine), &mut cx);
        assert_eq!((r.head[0], r.head[2]), (LayerGpu::SUB_SHINE, LayerGpu::FLAG), "own light");
        let white = sub_record(&SubLayer::Shine(ShineLayer::default()), &mut cx);
        assert_eq!(white.head[2], 0, "white light");
        let shading = ShadingLayer { invert: true, ..ShadingLayer::default() };
        let r = sub_record(&SubLayer::Shading(shading), &mut cx);
        assert_eq!((r.head[0], r.head[2]), (LayerGpu::SUB_SHADING, LayerGpu::INVERT), "darken the middle");
    }

    #[test]
    fn each_sub_layer_writes_its_code_after_its_layer_in_order() {
        let stack = stack_after(|reg, s, b| {
            let pat = s.add_layer(reg, b, SlotId::Look, PATTERN).unwrap();
            for kind in [SUB_SHADOW, SUB_GLOW, SUB_RECOLOUR] {
                s.add_sub_layer(reg, b, SlotId::Look, pat, kind).unwrap();
            }
            let outline = s.add_layer(reg, b, SlotId::Look, OUTLINE).unwrap();
            for kind in [SUB_OUTLINE, SUB_SHADING, SUB_SHINE, SUB_FLASH, SUB_HUE, SUB_CUT] {
                s.add_sub_layer(reg, b, SlotId::Look, outline, kind).unwrap();
            }
        });
        let records = stack.records(&mut GradientAtlas::default(), &mut Vec::new(), 0.0);
        let codes: Vec<u32> = records.iter().map(|r| r.head[0]).collect();
        assert_eq!(
            codes,
            [
                LayerGpu::BODY,
                LayerGpu::PATTERN,
                LayerGpu::SUB_SHADOW,
                LayerGpu::SUB_GLOW,
                LayerGpu::SUB_RECOLOUR,
                LayerGpu::OUTLINE,
                LayerGpu::SUB_OUTLINE,
                LayerGpu::SUB_SHADING,
                LayerGpu::SUB_SHINE,
                LayerGpu::SUB_FLASH,
                LayerGpu::SUB_HUE,
                LayerGpu::SUB_CUT,
            ]
        );
        assert_eq!((records[1].head[3], records[5].head[3]), (3, 6), "each layer counts its sub-layers");
        assert!(records[2..5].iter().all(|r| r.head[1] == 0 && r.head[3] == 0), "sub-layers have no blend");
        // The shadow falls 45° down and right, 0.15 radii.
        let off = 0.15 * 45f32.to_radians().cos();
        assert!((records[2].p0[0] - off).abs() < 1e-5 && (records[2].p0[1] - off).abs() < 1e-5);
        assert_eq!(records[3].head[2], LayerGpu::FLAG, "the glow takes its layer's colour");
        assert_eq!(records[3].amount[..2], [1.0, 0.8]);
        assert_eq!(records[9].amount[..3], [0.8, 0.3, 1.0], "flash strength, length, follow impact");
        assert!((records[10].amount[0] - 30.0 / 360.0).abs() < 1e-6, "turns per hit");
        assert!(stack.needs_hits());
        assert_eq!(stack.record_of(stack.drawn[1].id), Some(5));
        assert_eq!(stack.split(), None);
        assert!(stack.reach(10.0) > SHAPE_REACH + HALO_REACH * 0.4, "the glow on the pattern reaches out");
    }

    #[test]
    fn a_hit_wave_on_a_gradient_needs_the_balls_hits() {
        let reg = registry();
        let mut s = Scene::empty(&reg);
        let b = crate::add_thing(&mut s, &reg, "balls", ThingId::ROOT).unwrap();
        let body = s.things[&b].slots[&SlotId::Look].layers[0].id;
        let wave = bpe_core::gradient_extras::key(&layered_key(body, "colours"), "wave");
        s.set_at(&reg, &bpe_core::Address::on(b, SlotId::Look, &wave), Value::Number(1.0)).unwrap();
        let mut resolved = resolve(&s, &reg);
        let still = Stack::of(&resolved.things[&b].slots[&SlotId::Look]).unwrap();
        assert!(!still.needs_hits(), "not until the moves are on the gradients");
        bpe_core::gradient_extras::apply_moves(&mut resolved, 0.0);
        let stack = Stack::of(&resolved.things[&b].slots[&SlotId::Look]).unwrap();
        assert!(stack.needs_hits());
        assert!((stack.flash_memory() - 1.15 / 2.0).abs() < 1e-6, "as long as the wave takes to pass");
    }

    #[test]
    fn a_split_never_lands_on_a_sub_layer_and_records_for_keeps_sub_records() {
        let stack = stack_after(|reg, s, b| {
            let o = s.add_layer(reg, b, SlotId::Look, OUTLINE).unwrap();
            s.add_sub_layer(reg, b, SlotId::Look, o, SUB_GLOW).unwrap();
            s.add_sub_layer(reg, b, SlotId::Look, o, SUB_SHADOW).unwrap();
            s.add_layer(reg, b, SlotId::Look, BLEND).unwrap();
            s.add_layer(reg, b, SlotId::Look, PATTERN).unwrap();
        });
        assert_eq!(stack.split(), Some(4), "the pattern's record, after the body, the outline and its two");
        let all = stack.records(&mut GradientAtlas::default(), &mut Vec::new(), 0.0);
        assert_eq!(all[4].head[0], LayerGpu::PATTERN);
        let some = stack.records_for(&all, |k| k != 0);
        assert_eq!(some.len(), all.len());
        assert_eq!(some[1].head[2] & LayerGpu::UNWORN, LayerGpu::UNWORN);
        assert_eq!(some[1].amount[0], 0.0);
        assert_eq!(some[2], all[2], "its sub-layers stay where they are");
        assert!(!stack.needs_hits());
    }

    #[test]
    fn a_sub_layer_shader_joins_the_effects_once() {
        let stack = stack_after(|reg, s, b| {
            let lava = s.add_layer(reg, b, SlotId::Look, SHADER).unwrap();
            assert!(s.add_sub_layer(reg, b, SlotId::Look, lava, SUB_SHADER).is_err(), "no shader on a shader");
            let p = s.add_layer(reg, b, SlotId::Look, PATTERN).unwrap();
            let sub = s.add_sub_layer(reg, b, SlotId::Look, p, SUB_SHADER).unwrap();
            s.set_at(
                reg,
                &bpe_core::Address::on(b, SlotId::Look, &layered_key(sub, "effect")),
                Value::Choice("glitch".into()),
            )
            .unwrap();
            let o = s.add_layer(reg, b, SlotId::Look, OUTLINE).unwrap();
            s.add_sub_layer(reg, b, SlotId::Look, o, SUB_SHADER).unwrap();
        });
        assert_eq!(stack.effect_sources().len(), 2, "lava and glitch, each once");
        let records = stack.records(&mut GradientAtlas::default(), &mut Vec::new(), 0.0);
        let slots: Vec<f32> = records.iter().filter(|r| r.head[0] == LayerGpu::SUB_SHADER).map(|r| r.p0[3]).collect();
        assert_eq!(slots, [1.0, 0.0]);
    }

    #[test]
    fn cuts_follow_their_layer_and_name_their_target_by_record() {
        let set = |reg: &bpe_core::Registry, s: &mut Scene, b, id, key: &str, v: Value| {
            s.set_at(reg, &bpe_core::Address::on(b, SlotId::Look, &layered_key(id, key)), v).unwrap();
        };
        let mut ids = Vec::new();
        let stack = stack_after(|reg, s, b| {
            let hidden = s.add_layer(reg, b, SlotId::Look, SHINE).unwrap();
            let shown = s.add_layer(reg, b, SlotId::Look, GLOW).unwrap();
            let pat = s.add_layer(reg, b, SlotId::Look, PATTERN).unwrap();
            let m1 = s.add_sub_layer(reg, b, SlotId::Look, pat, SUB_CUT).unwrap();
            let m2 = s.add_sub_layer(reg, b, SlotId::Look, pat, SUB_CUT).unwrap();
            let off = s.add_sub_layer(reg, b, SlotId::Look, pat, SUB_CUT).unwrap();
            s.set_layer_enabled(reg, b, SlotId::Look, off, false).unwrap();
            s.set_layer_enabled(reg, b, SlotId::Look, hidden, false).unwrap();
            set(reg, s, b, m1, "from", Value::Choice("layer".into()));
            set(reg, s, b, m1, "target", Value::Int(hidden.0 as i64));
            set(reg, s, b, m2, "from", Value::Choice("layer".into()));
            set(reg, s, b, m2, "target", Value::Int(shown.0 as i64));
            let outline = s.add_layer(reg, b, SlotId::Look, OUTLINE).unwrap();
            let shading = s.add_layer(reg, b, SlotId::Look, SHADING).unwrap();
            set(reg, s, b, shading, "kind", Value::Choice("brush".into()));
            let stroke =
                bpe_core::Stroke { points: vec![[0.0, 0.0]], size: 0.2, softness: 0.0, strength: 1.0, erase: false };
            set(reg, s, b, shading, "strokes", Value::Strokes(bpe_core::Strokes { list: vec![stroke] }));
            ids.extend([pat, outline]);
        });
        let mut tiles = Vec::new();
        let records = stack.records(&mut GradientAtlas::default(), &mut tiles, 0.0);
        let codes: Vec<u32> = records.iter().map(|r| r.head[0]).collect();
        assert_eq!(
            codes,
            [
                LayerGpu::BODY,
                LayerGpu::GLOW,
                LayerGpu::PATTERN,
                LayerGpu::SUB_CUT,
                LayerGpu::SUB_CUT,
                LayerGpu::OUTLINE,
                LayerGpu::SHADING
            ]
        );
        assert_eq!(records[2].head[3], 2, "the hidden cut is left out of the count");
        assert_eq!(records[3].amount[1], -1.0, "a hidden target passes nothing");
        assert_eq!(records[4].amount[1], 1.0, "the glow is record 1");
        assert_eq!(records[5].head[3], 0);
        assert_eq!(tiles.len(), 1);
        assert_eq!(records[6].p1[3], 0.0, "the brush names its tile");
        assert_eq!(stack.record_of(stack.body_id), Some(0));
        assert_eq!(stack.record_of(ids[0]), Some(2), "the pattern");
        assert_eq!(stack.record_of(ids[1]), Some(5), "the outline, after the pattern's cuts");
        assert_eq!(stack.record_of(LayerId(999)), None);
        let middle = (bpe_gfx::MASK_TILE / 2) * bpe_gfx::MASK_TILE + bpe_gfx::MASK_TILE / 2;
        assert_eq!(tiles[0].1[2 * middle], 255, "the tile holds the stroke");
        assert_eq!(stack.reach(10.0), stack.reach(10.0));
        let _ = ids;
    }

    #[test]
    fn a_cut_moving_with_a_pattern_turns_only_as_the_pattern_spins() {
        let set = |reg: &bpe_core::Registry, s: &mut Scene, b, id, key: &str, v: Value| {
            s.set_at(reg, &bpe_core::Address::on(b, SlotId::Look, &layered_key(id, key)), v).unwrap();
        };
        let stack = stack_after(|reg, s, b| {
            let pat = s.add_layer(reg, b, SlotId::Look, PATTERN).unwrap();
            let m = s.add_sub_layer(reg, b, SlotId::Look, pat, SUB_CUT).unwrap();
            set(reg, s, b, m, "from", Value::Choice("shape".into()));
            set(reg, s, b, pat, "in_spin", Value::Number(90.0));
        });
        let at = |time: f32| stack.records(&mut GradientAtlas::default(), &mut Vec::new(), time)[2].amount[2];
        assert_eq!(at(0.0), 0.0, "a still pattern's cut sits where its handles and summary say");
        assert!((at(1.0) - 90f32.to_radians()).abs() < 1e-5, "and turns as the pattern spins: {}", at(1.0));
    }

    #[test]
    fn a_click_picks_the_topmost_layer_covering_the_point() {
        let mut ids = Vec::new();
        let stack = stack_after(|reg, s, b| {
            let pat = s.add_layer(reg, b, SlotId::Look, PATTERN).unwrap();
            let outline = s.add_layer(reg, b, SlotId::Look, OUTLINE).unwrap();
            s.set_at(reg, &bpe_core::Address::on(b, SlotId::Look, &layered_key(outline, "width")), Value::Number(6.0))
                .unwrap();
            let glow = s.add_layer(reg, b, SlotId::Look, GLOW).unwrap();
            ids.extend([pat, outline, glow]);
        });
        let r = 60.0;
        assert_eq!(stack.topmost_at(Vec2::new(0.97, 0.0), r), Some(ids[1]), "the outline on the rim");
        assert_eq!(stack.topmost_at(Vec2::new(0.0, 0.2), r), Some(ids[0]), "the pattern in the middle");
        assert_eq!(stack.topmost_at(Vec2::new(0.0, -1.3), r), Some(ids[2]), "the glow just outside");
        assert_eq!(stack.topmost_at(Vec2::new(2.5, 2.5), r), None, "nothing far outside");
        let plain = stack_after(|_, _, _| {});
        assert_eq!(plain.topmost_at(Vec2::new(0.3, 0.3), r), Some(plain.body_id), "the Body with nothing over it");
    }

    /// A ball's stack with shader layers running `effects` (an id, or own code).
    fn shaders(effects: &[&str]) -> Stack {
        let effects: Vec<String> = effects.iter().map(|e| (*e).to_owned()).collect();
        stack_after(move |reg, s, b| {
            for e in &effects {
                let id = s.add_layer(reg, b, SlotId::Look, SHADER).unwrap();
                let at = |k: &str| bpe_core::Address::on(b, SlotId::Look, &layered_key(id, k));
                if e.contains("fn ") {
                    s.set_at(reg, &at("effect"), Value::Choice("own".into())).unwrap();
                    s.set_at(reg, &at("code"), Value::Text(e.clone())).unwrap();
                } else {
                    s.set_at(reg, &at("effect"), Value::Choice(e.clone())).unwrap();
                }
            }
        })
    }

    #[test]
    fn shader_layers_share_a_slot_per_distinct_effect() {
        let lava = shaders(&["lava", "lava"]);
        let records = lava.records(&mut GradientAtlas::default(), &mut Vec::new(), 0.0);
        let slots: Vec<f32> = records.iter().filter(|r| r.head[0] == LayerGpu::SHADER).map(|r| r.p0[3]).collect();
        assert_eq!(slots, [0.0, 0.0]);
        assert_eq!(lava.program().unwrap().sources.len(), 1);
        let two = shaders(&["lava", "glitch"]);
        let records = two.records(&mut GradientAtlas::default(), &mut Vec::new(), 0.0);
        let slots: Vec<f32> = records.iter().filter(|r| r.head[0] == LayerGpu::SHADER).map(|r| r.p0[3]).collect();
        assert_eq!(slots, [0.0, 1.0]);
        assert_ne!(two.program().unwrap().key, lava.program().unwrap().key);
        let lava_heat = records[1].extra;
        assert!(lava_heat.iter().flatten().any(|v| *v != 0.0), "the effect's settings ride in extra");
        assert!(stack_after(|_, _, _| {}).program().is_none());
    }

    #[test]
    fn own_code_that_does_not_read_writes_no_record() {
        let broken = shaders(&["fn main() {}"]);
        let records = broken.records(&mut GradientAtlas::default(), &mut Vec::new(), 0.0);
        assert_eq!(records.len(), 1, "the body alone");
        assert!(broken.program().is_none());
    }

    #[test]
    fn own_code_that_does_not_compile_leaves_the_other_shader_layers_drawing() {
        let broken = "// name: Mine\nfn effect(uv: vec2<f32>, p: Params) -> vec3<f32> { return nope(uv); }\n";
        let stack = shaders(&["lava", broken]);
        let records = stack.records(&mut GradientAtlas::default(), &mut Vec::new(), 0.0);
        let shader_records = records.iter().filter(|r| r.head[0] == LayerGpu::SHADER).count();
        assert_eq!(shader_records, 1, "the lava alone");
        assert_eq!(stack.program().unwrap().sources.len(), 1, "a program of the lava alone, which builds");
    }

    #[test]
    fn the_biggest_glow_fits_its_quad() {
        let stack = stack_after(|reg, s, b| {
            let g = s.add_layer(reg, b, SlotId::Look, GLOW).unwrap();
            s.set_at(reg, &bpe_core::Address::on(b, SlotId::Look, &layered_key(g, "size")), Value::Number(4.0))
                .unwrap();
        });
        assert!(stack.reach(24.0) >= SHAPE_REACH + HALO_REACH * 4.0);
    }

    #[test]
    fn group_shaders_write_records_after_the_balls_own_and_join_the_program() {
        let set = |reg: &bpe_core::Registry, s: &mut Scene, b: ThingId, id: LayerId, key: &str, v: Value| {
            s.set_at(reg, &bpe_core::Address::on(b, SlotId::Look, &layered_key(id, key)), v).unwrap();
        };
        let stack = stack_after(|reg, s, b| {
            let outline = s.add_layer(reg, b, SlotId::Look, OUTLINE).unwrap();
            let whole = s.add_layer(reg, b, SlotId::Look, GROUP_SHADER).unwrap();
            set(reg, s, b, whole, "effect", Value::Choice("rainbow_flow".into()));
            set(reg, s, b, whole, "shows", Value::Choice("around".into()));
            set(reg, s, b, whole, "around", Value::Number(1.0));
            let body = s.add_layer(reg, b, SlotId::Look, GROUP_SHADER).unwrap();
            set(reg, s, b, body, "changes", Value::Choice("body".into()));
            let chosen = s.add_layer(reg, b, SlotId::Look, GROUP_SHADER).unwrap();
            set(reg, s, b, chosen, "changes", Value::Choice("choose".into()));
            set(reg, s, b, chosen, "chosen", Value::Text(outline.0.to_string()));
        });
        let records = stack.records(&mut GradientAtlas::default(), &mut Vec::new(), 0.0);
        let codes: Vec<u32> = records.iter().map(|r| r.head[0]).collect();
        let gs = LayerGpu::GROUP_SHADER;
        assert_eq!(codes, [LayerGpu::BODY, LayerGpu::OUTLINE, gs, gs, gs]);
        let sources = stack.effect_sources();
        assert_eq!(sources.len(), 2, "rainbow flow and lava, each once");
        assert_eq!(records[2].p0[2..], [1.0, 0.0], "over the whole ball, effect slot 0");
        assert_eq!(records[2].p1[0], 1.0);
        assert_eq!(records[3].p0[2..], [0.0, 1.0]);
        // The body names the second group record (bit 1), the outline the third (bit 2).
        assert_eq!(records[0].amount[2..], [2.0, 2.0]);
        assert_eq!(records[1].amount[2..], [2.0, 4.0]);
        assert!(stack.program().is_some());
        assert_eq!(stack.reach(24.0), SHAPE_REACH + 1.25, "the ring raises the reach");
    }

    #[test]
    fn too_many_records_cut_whole_layers_and_keep_the_group_records() {
        use bpe_core::SettingValue;
        let max = bpe_gfx::MAX_LAYERS as usize;
        let stack = stack_after(|reg, s, b| {
            // Ten patterns of thirteen records each, and a Blend layer after the eighth.
            for i in 0..10 {
                let pat = s.add_layer(reg, b, SlotId::Look, PATTERN).unwrap();
                for _ in 0..bpe_core::MAX_SUBS {
                    s.add_sub_layer(reg, b, SlotId::Look, pat, SUB_GLOW).unwrap();
                }
                if i == 7 {
                    let blend = s.add_layer(reg, b, SlotId::Look, BLEND).unwrap();
                    let at = bpe_core::Address::on(b, SlotId::Look, &layered_key(blend, "blend"));
                    s.set_at(reg, &at, Blend::Screen.to_value()).unwrap();
                }
            }
            s.add_layer(reg, b, SlotId::Look, GROUP_SHADER).unwrap();
        });
        let records = stack.records(&mut GradientAtlas::default(), &mut Vec::new(), 0.0);
        assert!(records.len() <= max, "{} records", records.len());
        assert_eq!(records.last().unwrap().head[0], LayerGpu::GROUP_SHADER, "the group shader is kept");
        // Every layer kept has all its sub-layers after it.
        let mut k = 1;
        while k < records.len() - 1 {
            assert_eq!(records[k].head[0], LayerGpu::PATTERN, "record {k}");
            let subs = records[k].head[3] as usize;
            assert_eq!(subs, bpe_core::MAX_SUBS);
            assert!(records[k + 1..=k + subs].iter().all(|r| r.head[0] == LayerGpu::SUB_GLOW));
            k += 1 + subs;
        }
        assert_eq!(k, records.len() - 1);
        // Nine patterns fit (1 + 9 × 13 + 1 = 119): the tenth is left out, and nothing names it.
        assert_eq!(records.len(), 1 + 9 * 13 + 1);
        assert_eq!(stack.record_of(stack.drawn[9].id), None);
        assert_eq!(stack.record_of(stack.drawn[8].id), Some(1 + 8 * 13));
        assert_eq!(stack.records_for(&records, |i| i != 9).len(), records.len());
        assert_eq!(stack.split(), Some(1 + 8 * 13), "the ninth pattern is over the Blend layer");
    }

    #[test]
    fn choice_labels_read_as_words() {
        assert_eq!(choice_label(ColourFrom::SameAsBody), "Same as body");
        assert_eq!(choice_label(OutlineSits::Inside), "Inside");
    }
}

#[cfg(test)]
mod fixes_tests {
    use super::*;
    use crate::registry;
    use bpe_core::{Address, Color, Gradient, Scene, SlotId, ThingId, Value, layered_key, resolve};

    fn stack_after(f: impl FnOnce(&bpe_core::Registry, &mut Scene, ThingId)) -> Stack {
        let reg = registry();
        let mut s = Scene::empty(&reg);
        let b = crate::add_thing(&mut s, &reg, "balls", ThingId::ROOT).unwrap();
        f(&reg, &mut s, b);
        let mut resolved = resolve(&s, &reg);
        bpe_core::gradient_extras::apply_moves(&mut resolved, 0.0);
        Stack::of(&resolved.things[&b].slots[&SlotId::Look]).unwrap()
    }

    fn set(reg: &bpe_core::Registry, s: &mut Scene, b: ThingId, layer: LayerId, key: &str, v: Value) {
        s.set_at(reg, &Address::on(b, SlotId::Look, &layered_key(layer, key)), v).unwrap();
    }

    #[test]
    fn a_blend_layer_splits_the_ball_where_it_sits_unless_nothing_draws_above_it() {
        // Outline, Blend, Pattern: the body and the outline blend, the pattern paints over them.
        let stack = stack_after(|reg, s, b| {
            s.add_layer(reg, b, SlotId::Look, OUTLINE).unwrap();
            s.add_layer(reg, b, SlotId::Look, BLEND).unwrap();
            s.add_layer(reg, b, SlotId::Look, PATTERN).unwrap();
        });
        assert_eq!(stack.blend_at, 1);
        assert_eq!(stack.split(), Some(2), "the pattern's record, after the body's and the outline's");
        // At the top it blends the whole ball, as before.
        let top = stack_after(|reg, s, b| {
            s.add_layer(reg, b, SlotId::Look, OUTLINE).unwrap();
            s.add_layer(reg, b, SlotId::Look, BLEND).unwrap();
        });
        assert_eq!(top.split(), None);
        assert_eq!(top.whole_blend().0, Blend::Add);
        // A Normal blend at full amount needs no split.
        let normal = stack_after(|reg, s, b| {
            let blend = s.add_layer(reg, b, SlotId::Look, BLEND).unwrap();
            set(reg, s, b, blend, "blend", Value::Choice("normal".into()));
            s.add_layer(reg, b, SlotId::Look, PATTERN).unwrap();
        });
        assert_eq!(normal.split(), None);
    }

    #[test]
    fn a_split_ball_draws_twice_the_upper_part_painted_normally() {
        use bpe_gfx::{Frame, build_frame};
        let kit = crate::kit();
        let reg = &kit.registry;
        let mut scene = crate::new_studio_scene(reg);
        let b = crate::add_thing(&mut scene, reg, "balls", ThingId::ROOT).unwrap();
        scene.add_layer(reg, b, SlotId::Look, BLEND).unwrap();
        scene.add_layer(reg, b, SlotId::Look, OUTLINE).unwrap();
        let resolved = resolve(&scene, reg);
        let world = bpe_runtime::World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &bpe_runtime::Log::default(), 0.0, &kit.draws, &mut frame);
        // The body under the Blend layer goes into the Add layer, the outline over it drawn normally.
        let added = frame.layers.iter().find(|l| l.blend == Blend::Add).expect("an Add layer");
        assert_eq!(added.frame.layered.len(), 1);
        assert_eq!(added.frame.layered[0].range[2], 1);
        assert!(frame.layered.is_empty(), "nothing of the ball in the frame's own lists");
        // The over layer comes last, so the outline is mixed in after the blended body.
        let over = frame.layers.last().unwrap();
        assert_eq!((over.blend, over.amount, over.each), (Blend::Normal, 1.0, false));
        assert_eq!(over.frame.layered.len(), 1);
        assert_eq!(over.frame.layered[0].range[2], 1 | 0x1_0000);
        assert_eq!(over.frame.look_layers.len(), 2, "the upper part walks the whole stack");
    }

    #[test]
    fn a_body_gradient_runs_across_each_ball_by_default_and_layers_take_their_own() {
        let two = Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)]);
        let stack = stack_after(|reg, s, b| {
            let body = s.card(b, SlotId::Look).unwrap().layers[0].id;
            set(reg, s, b, body, "colours", Value::Gradient(two.clone()));
            set(reg, s, b, body, "opacity", Value::Number(0.4));
            let o = s.add_layer(reg, b, SlotId::Look, OUTLINE).unwrap();
            set(reg, s, b, o, "colour_from", Value::Choice("own".into()));
            set(reg, s, b, o, "colour", Value::Gradient(two.clone()));
            set(reg, s, b, o, "colour~form", Value::Choice("round".into()));
            s.add_layer(reg, b, SlotId::Look, GLOW).unwrap();
        });
        assert!(stack.body_each_ball() && stack.varies());
        let mut atlas = GradientAtlas::default();
        let records = stack.records(&mut atlas, &mut Vec::new(), 0.0);
        assert!(records[0].paint.row >= 0.0, "the body paints its gradient");
        assert_eq!((records[0].paint.pattern, records[0].paint.angle), (1.0, 90.0), "straight, top to bottom");
        assert_eq!(records[0].amount[0], 0.4, "the body's opacity");
        assert!(records[1].paint.row >= 0.0 && records[1].paint.pattern == 2.0, "the outline's own, outward");
        assert!(records[2].paint.row < 0.0, "a one-colour glow paints its plain colour");
    }

    #[test]
    fn a_body_gradient_following_the_group_place_paints_one_colour_per_ball() {
        let stack = stack_after(|reg, s, b| {
            let body = s.card(b, SlotId::Look).unwrap().layers[0].id;
            let two = Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)]);
            set(reg, s, b, body, "colours", Value::Gradient(two));
            set(reg, s, b, body, "follow", Value::Choice("place".into()));
        });
        assert!(!stack.body_each_ball());
        let records = stack.records(&mut GradientAtlas::default(), &mut Vec::new(), 0.0);
        assert!(records[0].paint.row < 0.0);
    }

    #[test]
    fn shine_and_group_shader_settings_reach_their_records() {
        let stack = stack_after(|reg, s, b| {
            let shine = s.add_layer(reg, b, SlotId::Look, SHINE).unwrap();
            set(reg, s, b, shine, "material", Value::Choice("metal".into()));
            set(reg, s, b, shine, "softness", Value::Number(0.0));
            let gs = s.add_layer(reg, b, SlotId::Look, super::super::GROUP_SHADER).unwrap();
            set(reg, s, b, gs, "covers", Value::Number(0.25));
            set(reg, s, b, gs, "from", Value::Number(90.0));
        });
        let records = stack.records(&mut GradientAtlas::default(), &mut Vec::new(), 0.0);
        assert_eq!(records[1].p0[3], 2.0, "metal");
        assert!((records[1].p1[0] - 3.0).abs() < 1e-5, "softness 0 is four times sharper");
        let gs = records.iter().find(|r| r.head[0] == LayerGpu::GROUP_SHADER).unwrap();
        assert_eq!(gs.p1[1], 0.75, "three quarters uncovered");
        assert!((gs.p1[2] - 90f32.to_radians()).abs() < 1e-6);
    }

    #[test]
    fn a_shine_s_light_is_white_or_its_own_colours() {
        let white = stack_after(|reg, s, b| {
            s.add_layer(reg, b, SlotId::Look, SHINE).unwrap();
        });
        let mut atlas = GradientAtlas::default();
        let r = white.records(&mut atlas, &mut Vec::new(), 0.0)[1];
        assert_eq!((r.head[2], r.colour, r.paint.row), (0, [1.0; 4], -1.0), "white light, the classic shine");
        let own = stack_after(|reg, s, b| {
            let shine = s.add_layer(reg, b, SlotId::Look, SHINE).unwrap();
            set(reg, s, b, shine, "light", Value::Choice("own".into()));
            let colours = bpe_core::Gradient::even(&[bpe_core::Color::rgb(255, 0, 0), bpe_core::Color::rgb(0, 0, 255)]);
            set(reg, s, b, shine, "colour", Value::Gradient(colours));
        });
        let r = own.records(&mut atlas, &mut Vec::new(), 0.0)[1];
        assert_eq!(r.head[2], LayerGpu::FLAG, "its own colours");
        assert!(r.paint.row >= 0.0, "a gradient runs across the ball");
    }
}
