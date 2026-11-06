//! Turning a shape's resolved stack of layers into the records the paths pipeline walks (see
//! `PathInstance::layers` and the shape records table on `bpe_gfx::LayerGpu`): written once per
//! shape per frame, bottom first, the Body's first.

use crate::modules::look::layers::gives_light::{Emit, gpu_of, unlit_if_lamp};
use std::borrow::Cow;

use bpe_core::{Blend, Gradient, LayerId, Params, ResolvedSlot, Settings};
use bpe_gfx::{GradientAtlas, LayerGpu, LookProgram, MAX_LAYERS, PaintGpu};
use glam::Vec2;

use super::subs::{ShapeShading, ShapeSubGlow, ShapeSubShadow};
use super::{BLEND, BODY, CARD, GLOW, OUTLINE, PAINT, SHADER};
use super::{
    PaintLayer, SUB_CUT, SUB_FLASH, SUB_GLOW, SUB_HUE, SUB_OUTLINE, SUB_RECOLOUR, SUB_SHADER, SUB_SHADING, SUB_SHADOW,
    SUB_SHINE, ShapeGlow, ShapeOutline,
};
use crate::modules::look::layers::gpu::{Sub, SubCx, SubLayer, shader_record, sub_record, written};
use crate::modules::look::layers::{
    BlendLayer, ColourFrom, CutLayer, OutlineSits, ShaderLayer, ShapeCut, ShineLayer, SubFlash, SubHue, SubOutline,
    SubRecolour,
};

/// How far past its edge a glow reaches, in its sizes: where its light has all but gone. Matches
/// the balls' HALO_REACH.
pub const GLOW_REACH: f32 = 2.2;

/// A drawn layer of a shape's stack with its settings.
#[derive(Clone, Debug, PartialEq)]
pub enum ShapeDrawn {
    Paint(PaintLayer),
    Outline(ShapeOutline),
    Glow(ShapeGlow),
    Shader(Box<ShaderLayer>),
}

impl ShapeDrawn {
    /// Whether it writes a record: a Shader whose own code doesn't read writes none.
    pub fn writes(&self) -> bool {
        !matches!(self, ShapeDrawn::Shader(sh) if sh.header.is_err())
    }
}

/// A shape's sub-layer of `kind` with its settings `p`, read into a ball's sub-layer settings
/// (sizes in pixels); `None` for a kind that isn't one.
pub fn shape_sub(kind: &str, p: &Params) -> Option<SubLayer> {
    Some(match kind {
        SUB_CUT => SubLayer::Cut(CutLayer::from(ShapeCut::from_params(p))),
        SUB_GLOW => SubLayer::Glow(ShapeSubGlow::from_params(p).into()),
        SUB_OUTLINE => SubLayer::Outline(SubOutline::from_params(p)),
        SUB_SHADOW => SubLayer::Shadow(ShapeSubShadow::from_params(p).into()),
        SUB_SHADING => SubLayer::Shading(ShapeShading::from_params(p).into()),
        SUB_SHINE => SubLayer::Shine(ShineLayer::from_params(p)),
        SUB_SHADER => SubLayer::Shader(Box::new(ShaderLayer::from_params(p))),
        SUB_RECOLOUR => SubLayer::Recolour(SubRecolour::from_params(p)),
        SUB_FLASH => SubLayer::Flash(SubFlash::from_params(p)),
        SUB_HUE => SubLayer::Hue(SubHue::from_params(p)),
        super::SUB_LIGHT => SubLayer::Light(crate::modules::look::layers::gives_light::GivesLight::from_params(p)),
        crate::modules::look::layers::heat::SHAPE_SUB_HEAT => {
            SubLayer::Heat(crate::modules::look::layers::heat::SubHeat::from_params(p))
        }
        _ => return None,
    })
}

/// A shape's shown stack, read from its resolved Look card.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeStack {
    pub body_id: LayerId,
    /// The layers above the Body, bottom first, with their ids and shown sub-layers.
    pub drawn: Vec<(LayerId, ShapeDrawn, Vec<Sub>)>,
    pub blend: Option<BlendLayer>,
    /// Where the Blend layer sits: how many of `drawn` are under it.
    pub blend_at: usize,
    /// How each layer (the Body's first) takes the scene's light ([`crate::modules::look::layers::lighting::of`]).
    pub light: Vec<(LayerId, [f32; 4])>,
    /// Each layer's UV ([`crate::modules::look::layers::lighting::extra_of`]), where it has any.
    pub more: Vec<(LayerId, [f32; 4])>,
    /// The light the Body gives off (its Gives off light sub-layers), when it gives any.
    pub body_emit: Option<Emit>,
    /// The Body's Heat glow sub-layers: their records follow the Body's.
    pub body_heat: Vec<crate::modules::look::layers::heat::SubHeat>,
}

/// The shape as it is drawn, in its own units (see `things::shape::draw::wall_form`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Geometry {
    /// The outline's points; `None` for a circle of radius 1 or an ellipse.
    pub points: Option<Vec<Vec2>>,
    /// An exact ellipse's half-axes (the larger one 1).
    pub ellipse: Option<Vec2>,
    /// Half the band's thickness in shape units (0 filled).
    pub half: f32,
}

impl Geometry {
    /// Where the shape starts and ends along `dir` (a unit vector), band included.
    pub fn extent(&self, dir: Vec2) -> (f32, f32) {
        let (lo, hi) = match (&self.points, self.ellipse) {
            (Some(ps), _) if !ps.is_empty() => ps.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
                let d = p.dot(dir);
                (lo.min(d), hi.max(d))
            }),
            (_, Some(axes)) => {
                let far = (axes * dir).length();
                (-far, far)
            }
            _ => (-1.0, 1.0),
        };
        (lo - self.half, hi + self.half)
    }
}

impl ShapeStack {
    /// How layer `id` takes the scene's light.
    pub fn light_of(&self, id: LayerId) -> [f32; 4] {
        self.light.iter().find(|(l, _)| *l == id).map_or([0.0; 4], |(_, light)| *light)
    }

    /// Layer `id`'s UV ([`crate::modules::look::layers::lighting::extra_of`]).
    pub fn more_of(&self, id: LayerId) -> [f32; 4] {
        self.more.iter().find(|(l, _)| *l == id).map_or([0.0; 4], |(_, more)| *more)
    }

    /// The stack of a `look.shape` card; `None` for any other card, or one with no Body.
    pub fn of(slot: &ResolvedSlot) -> Option<ShapeStack> {
        if slot.kind != CARD {
            return None;
        }
        let body = slot.first_layer(BODY)?;
        let mut stack = ShapeStack {
            body_id: body.id,
            drawn: Vec::new(),
            blend: None,
            blend_at: 0,
            light: Vec::new(),
            more: Vec::new(),
            body_emit: None,
            body_heat: Vec::new(),
        };
        for layer in &slot.layers {
            let p = slot.layer_params(layer.id);
            let light = crate::modules::look::layers::lighting::of(&p);
            if light != [0.0; 4] {
                stack.light.push((layer.id, light));
            }
            let more = crate::modules::look::layers::lighting::extra_of(&p, 0.0, true);
            if more != [0.0; 4] {
                stack.more.push((layer.id, more));
            }
            let drawn = match layer.kind.as_str() {
                PAINT => ShapeDrawn::Paint(PaintLayer::from_params(&p)),
                OUTLINE => ShapeDrawn::Outline(ShapeOutline::from_params(&p)),
                GLOW => ShapeDrawn::Glow(ShapeGlow::from_params(&p)),
                SHADER => ShapeDrawn::Shader(Box::new(ShaderLayer::from_params(&p))),
                // A sub-layer goes with its layer; a hidden layer's don't matter.
                kind if layer.parent.is_some() => {
                    if let Some(sub) = shape_sub(kind, &p) {
                        if let Some((_, _, subs)) = stack.drawn.iter_mut().find(|(id, ..)| Some(*id) == layer.parent) {
                            subs.push(Sub { id: layer.id, sub });
                        } else if layer.parent == Some(stack.body_id) {
                            // The Body takes Gives off light and Heat glow only.
                            match sub {
                                SubLayer::Light(g) => stack.body_emit.get_or_insert_with(Default::default).add(&g),
                                SubLayer::Heat(h) => stack.body_heat.push(h),
                                _ => {}
                            }
                        }
                    }
                    continue;
                }
                // The first Blend counts.
                BLEND => {
                    if stack.blend.is_none() {
                        stack.blend = Some(BlendLayer::from_params(&p));
                        stack.blend_at = stack.drawn.len();
                    }
                    continue;
                }
                _ => continue,
            };
            stack.drawn.push((layer.id, drawn, Vec::new()));
        }
        Some(stack)
    }

    /// Where the records of the layers above the Blend layer start, when the shape draws in two
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
        if !drawn[at..].iter().any(|(_, d, _)| d.writes()) {
            return None;
        }
        let under: usize = drawn[..at].iter().map(|(_, d, subs)| records_of(d, subs)).sum();
        Some((1 + self.body_heat.len() + under) as u32)
    }

    /// How many of `drawn` (from the bottom) have their records written: the walk reads at most
    /// [`MAX_LAYERS`] records, so past that whole layers are left out from the top, never part
    /// of one.
    pub fn kept(&self) -> usize {
        let room = (MAX_LAYERS as usize).saturating_sub(1 + self.body_heat.len());
        let mut used = 0;
        for (i, (_, d, subs)) in self.drawn.iter().enumerate() {
            used += records_of(d, subs);
            if used > room {
                return i;
            }
        }
        self.drawn.len()
    }

    /// Where layer `id`'s record is among [`ShapeStack::records`] (its sub-layers' follow it):
    /// `None` for a layer that writes none or is past [`ShapeStack::kept`].
    pub fn record_of(&self, id: LayerId) -> Option<usize> {
        if id == self.body_id {
            return Some(0);
        }
        let mut at = 1;
        for (layer, d, subs) in &self.drawn[..self.kept()] {
            let n = records_of(d, subs);
            if n > 0 && *layer == id {
                return Some(at);
            }
            at += n;
        }
        None
    }

    /// How the whole shape mixes with what is behind it: its Blend layer's, Normal without one.
    pub fn whole_blend(&self) -> (Blend, f32, bool) {
        self.blend.as_ref().map_or((Blend::Normal, 1.0, false), |b| (b.blend, b.blend_amount, b.blend_each))
    }

    /// The effects its Shader layers and shader sub-layers run, each once, in order: a shader
    /// record's effect slot is its place here. Own code that doesn't read is left out.
    pub fn effect_sources(&self) -> Vec<Cow<'static, str>> {
        let mut out: Vec<Cow<'static, str>> = Vec::new();
        let mut add = |sh: &ShaderLayer| {
            if sh.header.is_ok() && !out.contains(&sh.source) {
                out.push(sh.source.clone());
            }
        };
        for (_, d, subs) in &self.drawn {
            if let ShapeDrawn::Shader(sh) = d {
                add(sh);
            }
            if d.writes() {
                for s in subs {
                    if let SubLayer::Shader(sh) = &s.sub {
                        add(sh);
                    }
                }
            }
        }
        out
    }

    /// The in-ball program the shape draws with: `None` without Shader layers that run.
    pub fn program(&self) -> Option<LookProgram> {
        let sources = self.effect_sources();
        (!sources.is_empty()).then(|| LookProgram::new(sources))
    }

    /// How far past the shape's edge its layers reach, in world pixels: an outline outside it
    /// (half of one centred on it), a glow until its light has all but gone, and each sub-layer
    /// that reaches further (a glow, an outline outside, a shadow) from there.
    pub fn reach_past(&self) -> f32 {
        self.drawn
            .iter()
            .map(|(_, d, subs)| {
                let own = match d {
                    ShapeDrawn::Outline(o) => match o.sits {
                        OutlineSits::Inside => 0.0,
                        OutlineSits::Centre => 0.5 * o.width.max(0.0),
                        OutlineSits::Outside => o.width.max(0.0),
                    },
                    ShapeDrawn::Glow(g) => GLOW_REACH * g.size.max(1.0),
                    _ => 0.0,
                };
                own + subs.iter().map(|s| s.sub.reach(1.0, true)).sum::<f32>()
            })
            .filter(|r| r.is_finite())
            .fold(0.0, f32::max)
    }

    /// Whether a shown sub-layer reads the shape's hits (Flash on hit, Colour on hit).
    pub fn needs_hits(&self) -> bool {
        self.drawn.iter().any(|(_, d, subs)| d.writes() && subs.iter().any(|s| s.sub.reads_hits() || s.sub.flares()))
            || self.body_emit.is_some_and(|e| e.flare != 0.0)
            || !self.body_heat.is_empty()
    }

    /// The Heat glow whose heat the shape carries (`hit[3]`): the Body's first, else the first
    /// on a drawn layer.
    pub fn heat(&self) -> Option<&crate::modules::look::layers::heat::SubHeat> {
        self.body_heat.first().or_else(|| {
            self.drawn.iter().filter(|(_, d, _)| d.writes()).flat_map(|(_, _, subs)| subs).find_map(|s| match &s.sub {
                SubLayer::Heat(h) => Some(h),
                _ => None,
            })
        })
    }

    /// Whether any of its shown layers gives off light.
    pub fn gives_light(&self) -> bool {
        self.body_emit.is_some()
            || self.drawn.iter().any(|(_, d, subs)| d.writes() && Emit::of_subs(subs.iter().map(|s| &s.sub)).is_some())
    }

    /// The records the stack draws with, bottom first, its gradients baked into `atlas`. The
    /// Body's is first; each layer's sub-layers follow it; a Shader whose own code doesn't read
    /// writes none. At most [`MAX_LAYERS`]: the layers past [`ShapeStack::kept`] are left out.
    pub fn records(&self, geometry: &Geometry, atlas: &mut GradientAtlas) -> Vec<LayerGpu> {
        let blend = |b: Blend| b.shader_code();
        let flag = |from: ColourFrom| if from == ColourFrom::SameAsBody { LayerGpu::FLAG } else { 0 };
        let across = |direction: f32| {
            let a = if direction.is_finite() { direction } else { 90.0 }.to_radians();
            let dir = Vec2::new(a.cos(), a.sin());
            let (lo, hi) = geometry.extent(dir);
            [dir.x, dir.y, lo, hi]
        };
        let sources = self.effect_sources();
        let mut out = vec![LayerGpu {
            head: [LayerGpu::BODY, 0, 0, self.body_heat.len() as u32],
            amount: [1.0, 0.0, 0.0, 0.0],
            colour: [1.0, 1.0, 1.0, 1.0],
            light: unlit_if_lamp(self.light_of(self.body_id), self.body_emit),
            more: self.more_of(self.body_id),
            emit: gpu_of(self.body_emit),
            ..LayerGpu::ZERO
        }];
        out.extend(self.body_heat.iter().map(crate::modules::look::layers::heat::SubHeat::record));
        // Where each layer's record is, by id, for cuts that name a layer.
        let mut placed: Vec<(LayerId, usize)> = vec![(self.body_id, 0)];
        // Shapes have no brush; cuts and shading never add tiles.
        let mut tiles = Vec::new();
        for (id, d, subs) in &self.drawn[..self.kept()] {
            let r = match d {
                ShapeDrawn::Paint(p) => LayerGpu {
                    head: [LayerGpu::SHAPE_PAINT, blend(p.blend), 0, 0],
                    amount: [p.opacity, 0.0, 0.0, 0.0],
                    p0: [p.runs.code(), 0.0, 0.0, 0.0],
                    p1: across(p.direction),
                    colour: p.colours.sample(0.5),
                    paint: run_paint(&p.colours, p.flow, atlas),
                    ..LayerGpu::ZERO
                },
                ShapeDrawn::Outline(o) => LayerGpu {
                    head: [LayerGpu::OUTLINE, blend(o.blend), flag(o.colour_from), 0],
                    amount: [o.opacity, 0.0, 0.0, 0.0],
                    p0: [o.width.max(0.0), o.sits.code(), o.gradient.code(), 0.0],
                    p1: across(o.direction),
                    colour: o.colour.sample(0.5),
                    paint: run_paint(&o.colour, o.flow, atlas),
                    ..LayerGpu::ZERO
                },
                ShapeDrawn::Glow(g) => LayerGpu {
                    head: [LayerGpu::GLOW, blend(g.blend), flag(g.colour_from), 0],
                    amount: [g.opacity, g.strength, 0.0, 0.0],
                    p0: [g.size.max(1.0), 0.0, g.gradient.code(), 0.0],
                    p1: across(g.direction),
                    colour: g.colour.sample(0.5),
                    paint: run_paint(&g.colour, g.flow, atlas),
                    ..LayerGpu::ZERO
                },
                // Own code that doesn't read draws nothing; the editor says why.
                ShapeDrawn::Shader(sh) if sh.header.is_err() => continue,
                ShapeDrawn::Shader(sh) => {
                    LayerGpu { head: [LayerGpu::SHADER, blend(sh.blend), 0, 0], ..shader_record(sh, &sources) }
                }
            };
            let at = out.len();
            let count = written(subs).count() as u32;
            let emit = Emit::of_subs(subs.iter().map(|s| &s.sub));
            out.push(LayerGpu {
                head: [r.head[0], r.head[1], r.head[2], count],
                light: unlit_if_lamp(self.light_of(*id), emit),
                more: self.more_of(*id),
                emit: gpu_of(emit),
                ..r
            });
            let mut cx = SubCx { placed: &placed, angle: 0.0, sources: &sources, atlas, tiles: &mut tiles };
            for s in written(subs) {
                out.push(sub_record(&s.sub, &mut cx));
            }
            placed.push((*id, at));
        }
        out
    }
}

/// How many records drawn layer `d` with sub-layers `subs` writes: its own and its shown
/// sub-layers', none when it writes none.
fn records_of(d: &ShapeDrawn, subs: &[Sub]) -> usize {
    if d.writes() { 1 + written(subs).count() } else { 0 }
}

/// What a layer's own `colours` paint: nothing for one colour (the record's `colour` is used),
/// else their gradient, moving along by `flow` times a second and wrapping round while it does.
fn run_paint(colours: &Gradient, flow: f32, atlas: &mut GradientAtlas) -> PaintGpu {
    if colours.stops.len() <= 1 {
        return PaintGpu::default();
    }
    let flow = if flow.is_finite() { flow } else { 0.0 };
    PaintGpu {
        row: atlas.row(colours) as f32,
        steps: if colours.transition == bpe_core::Transition::Steps { 1.0 } else { 0.0 },
        scroll: flow,
        repeat: if flow != 0.0 { 1.0 } else { 0.0 },
        ..PaintGpu::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry;
    use bpe_core::{Color, Scene, SettingValue, SlotId, ThingId, Value, layered_key, resolve};

    /// A new studio scene with a shape, its Look card, and `add` run on it first.
    fn shape_with(add: impl FnOnce(&mut Scene, &bpe_core::Registry, ThingId)) -> (ShapeStack, Scene) {
        let reg = registry();
        let mut scene = crate::new_studio_scene(&reg);
        let shape = crate::add_thing(&mut scene, &reg, "shape", ThingId::ROOT).unwrap();
        add(&mut scene, &reg, shape);
        let resolved = resolve(&scene, &reg);
        let stack = ShapeStack::of(&resolved.things[&shape].slots[&SlotId::Look]).unwrap();
        (stack, scene)
    }

    fn set(scene: &mut Scene, shape: ThingId, layer: LayerId, key: &str, v: Value) {
        let card = scene.things.get_mut(&shape).unwrap().slots.get_mut(&SlotId::Look).unwrap();
        card.params.set(&layered_key(layer, key), v);
    }

    #[test]
    fn a_stack_reads_its_shown_layers_in_order_and_the_first_blend() {
        let (stack, _) = shape_with(|s, reg, t| {
            s.add_layer(reg, t, SlotId::Look, PAINT).unwrap();
            let hidden = s.add_layer(reg, t, SlotId::Look, GLOW).unwrap();
            s.set_layer_enabled(reg, t, SlotId::Look, hidden, false).unwrap();
            s.add_layer(reg, t, SlotId::Look, OUTLINE).unwrap();
            s.add_layer(reg, t, SlotId::Look, BLEND).unwrap();
        });
        let kinds: Vec<&str> = stack
            .drawn
            .iter()
            .map(|(_, d, _)| match d {
                ShapeDrawn::Paint(_) => "paint",
                ShapeDrawn::Outline(_) => "outline",
                ShapeDrawn::Glow(_) => "glow",
                ShapeDrawn::Shader(_) => "shader",
            })
            .collect();
        assert_eq!(kinds, ["paint", "outline"], "the hidden glow is left out");
        assert_eq!(stack.whole_blend().0, Blend::Add, "a new Blend layer adds light");
        let (plain, _) = shape_with(|_, _, _| {});
        assert!(plain.drawn.is_empty());
        assert_eq!(plain.whole_blend(), (Blend::Normal, 1.0, false));
    }

    #[test]
    fn records_follow_the_shape_records_table() {
        let mut atlas = GradientAtlas::default();
        let (stack, _) = shape_with(|s, reg, t| {
            let paint = s.add_layer(reg, t, SlotId::Look, PAINT).unwrap();
            set(s, t, paint, "blend", Blend::Screen.to_value());
            let outline = s.add_layer(reg, t, SlotId::Look, OUTLINE).unwrap();
            set(s, t, outline, "sits", OutlineSits::Outside.to_value());
            let glow = s.add_layer(reg, t, SlotId::Look, GLOW).unwrap();
            set(s, t, glow, "colour_from", ColourFrom::Own.to_value());
        });
        let r = stack.records(&Geometry::default(), &mut atlas);
        let codes: Vec<u32> = r.iter().map(|l| l.head[0]).collect();
        assert_eq!(codes, [LayerGpu::BODY, LayerGpu::SHAPE_PAINT, LayerGpu::OUTLINE, LayerGpu::GLOW]);
        assert_eq!(r[0].colour, [1.0; 4], "the body is plain white");
        assert_eq!(r[1].head[1], Blend::Screen.shader_code());
        assert_eq!(r[1].paint.row, -1.0, "one colour paints from the record's colour");
        assert_eq!(r[1].colour, PaintLayer::default().colours.sample(0.5));
        assert_eq!(r[2].head[2], LayerGpu::FLAG, "the outline takes the colour under it");
        assert_eq!((r[2].p0[0], r[2].p0[1]), (6.0, 2.0));
        assert_eq!(r[3].head[2], 0, "the glow has its own colour");
        assert_eq!((r[3].amount[1], r[3].p0[0]), (0.8, 30.0));
        assert_eq!(stack.reach_past(), GLOW_REACH * 30.0, "the glow reaches further than the outline");
    }

    #[test]
    fn across_runs_from_edge_to_edge_of_the_shape_as_drawn() {
        let rect = Geometry {
            points: Some(vec![Vec2::new(-1.0, -0.5), Vec2::new(1.0, -0.5), Vec2::new(1.0, 0.5), Vec2::new(-1.0, 0.5)]),
            ellipse: None,
            half: 0.0,
        };
        let (lo, hi) = rect.extent(Vec2::X);
        assert_eq!((lo, hi), (-1.0, 1.0));
        let (lo, hi) = rect.extent(Vec2::Y);
        assert_eq!((lo, hi), (-0.5, 0.5));
        let band = Geometry { points: None, ellipse: None, half: 0.1 };
        assert_eq!(band.extent(Vec2::new(0.6, 0.8)), (-1.1, 1.1), "a circle, with its band");
        let oval = Geometry { points: None, ellipse: Some(Vec2::new(1.0, 0.5)), half: 0.0 };
        assert_eq!((oval.extent(Vec2::X), oval.extent(Vec2::Y)), ((-1.0, 1.0), (-0.5, 0.5)), "an ellipse");
        let mut atlas = GradientAtlas::default();
        let (stack, _) = shape_with(|s, reg, t| {
            let paint = s.add_layer(reg, t, SlotId::Look, PAINT).unwrap();
            set(s, t, paint, "direction", Value::Number(0.0));
            set(s, t, paint, "colours", Value::Gradient(Gradient::even(&[Color::WHITE, Color::rgb(255, 0, 0)])));
            set(s, t, paint, "flow", Value::Number(0.5));
        });
        let r = stack.records(&rect, &mut atlas)[1];
        assert!((r.p1[0] - 1.0).abs() < 1e-6 && r.p1[1].abs() < 1e-6);
        assert_eq!((r.p1[2], r.p1[3]), (-1.0, 1.0));
        assert!(r.paint.row >= 0.0, "several colours paint a gradient");
        assert_eq!((r.paint.scroll, r.paint.repeat), (0.5, 1.0), "flowing colours wrap round");
    }

    #[test]
    fn shader_layers_name_their_effects_once_and_broken_code_writes_nothing() {
        let mut atlas = GradientAtlas::default();
        let (stack, _) = shape_with(|s, reg, t| {
            s.add_layer(reg, t, SlotId::Look, SHADER).unwrap();
            s.add_layer(reg, t, SlotId::Look, SHADER).unwrap();
            let screen = s.add_layer(reg, t, SlotId::Look, SHADER).unwrap();
            set(s, t, screen, "space", Value::Choice("screen".into()));
        });
        assert_eq!(stack.effect_sources().len(), 1, "two lava layers run one effect");
        assert!(stack.program().is_some());
        let r = stack.records(&Geometry::default(), &mut atlas);
        assert_eq!(r.len(), 4);
        assert!(r[1..].iter().all(|l| l.head[0] == LayerGpu::SHADER && l.p0[3] == 0.0));
        assert_eq!((r[1].p0[2], r[3].p0[2]), (0.0, 1.0), "turning with the shape, then upright");
        let (broken, _) = shape_with(|s, reg, t| {
            let sh = s.add_layer(reg, t, SlotId::Look, SHADER).unwrap();
            set(s, t, sh, "effect", Value::Choice("own".into()));
            set(s, t, sh, "code", Value::Text("fn main() {}".into()));
        });
        assert!(broken.program().is_none());
        assert_eq!(broken.records(&Geometry::default(), &mut atlas).len(), 1, "the body alone");
    }

    #[test]
    fn sub_layers_follow_their_layer_in_pixels_and_cuts_name_layers_by_record() {
        use super::super::{SUB_CUT, SUB_GLOW, SUB_SHADOW};
        let mut atlas = GradientAtlas::default();
        let (stack, _) = shape_with(|s, reg, t| {
            let paint = s.add_layer(reg, t, SlotId::Look, PAINT).unwrap();
            let outline = s.add_layer(reg, t, SlotId::Look, OUTLINE).unwrap();
            let glow = s.add_sub_layer(reg, t, SlotId::Look, outline, SUB_GLOW).unwrap();
            set(s, t, glow, "size", Value::Number(20.0));
            let cut = s.add_sub_layer(reg, t, SlotId::Look, outline, SUB_CUT).unwrap();
            set(s, t, cut, "from", Value::Choice("layer".into()));
            set(s, t, cut, "target", Value::Int(paint.0 as i64));
            let hidden = s.add_sub_layer(reg, t, SlotId::Look, outline, SUB_SHADOW).unwrap();
            s.set_layer_enabled(reg, t, SlotId::Look, hidden, false).unwrap();
            s.add_layer(reg, t, SlotId::Look, BLEND).unwrap();
            s.add_layer(reg, t, SlotId::Look, GLOW).unwrap();
        });
        assert_eq!(stack.drawn[1].2.len(), 2, "the hidden shadow is left out");
        let r = stack.records(&Geometry::default(), &mut atlas);
        let codes: Vec<u32> = r.iter().map(|l| l.head[0]).collect();
        assert_eq!(
            codes,
            [
                LayerGpu::BODY,
                LayerGpu::SHAPE_PAINT,
                LayerGpu::OUTLINE,
                LayerGpu::SUB_GLOW,
                LayerGpu::SUB_CUT,
                LayerGpu::GLOW
            ]
        );
        assert_eq!(r[2].head[3], 2);
        assert_eq!(r[3].p0[0], 20.0, "a shape's glow size is in pixels");
        assert_eq!(r[4].amount[1], 1.0, "the cut names the paint layer's record");
        assert_eq!(stack.split(), Some(5), "the top glow, after the outline's sub-layers");
        // The outline sits inside: the glow on it reaches its own way out.
        assert_eq!(stack.reach_past(), (GLOW_REACH * 30.0).max(GLOW_REACH * 20.0));
        assert!(!stack.needs_hits());
    }

    #[test]
    fn too_many_records_cut_whole_layers() {
        use super::super::SUB_GLOW;
        let mut atlas = GradientAtlas::default();
        let (stack, _) = shape_with(|s, reg, t| {
            // Ten paint layers of thirteen records each, and a Blend layer after the ninth.
            for i in 0..10 {
                let paint = s.add_layer(reg, t, SlotId::Look, PAINT).unwrap();
                for _ in 0..bpe_core::MAX_SUBS {
                    s.add_sub_layer(reg, t, SlotId::Look, paint, SUB_GLOW).unwrap();
                }
                if i == 8 {
                    let blend = s.add_layer(reg, t, SlotId::Look, BLEND).unwrap();
                    set(s, t, blend, "blend", Blend::Screen.to_value());
                }
            }
        });
        let r = stack.records(&Geometry::default(), &mut atlas);
        // Nine fit (1 + 9 × 13 = 118): the tenth is left out whole, sub-layers and all.
        assert_eq!(r.len(), 1 + 9 * 13);
        assert_eq!(stack.kept(), 9);
        let last = 1 + 8 * 13;
        assert_eq!((r[last].head[0], r[last].head[3]), (LayerGpu::SHAPE_PAINT, bpe_core::MAX_SUBS as u32));
        assert!(r[last + 1..].iter().all(|l| l.head[0] == LayerGpu::SUB_GLOW));
        assert_eq!(stack.split(), None, "nothing kept is over the Blend layer");
    }

    #[test]
    fn reach_past_counts_outlines_outside_and_centred() {
        let (stack, _) = shape_with(|s, reg, t| {
            let a = s.add_layer(reg, t, SlotId::Look, OUTLINE).unwrap();
            set(s, t, a, "sits", OutlineSits::Centre.to_value());
            set(s, t, a, "width", Value::Number(20.0));
        });
        assert_eq!(stack.reach_past(), 10.0);
        let (inside, _) = shape_with(|s, reg, t| {
            s.add_layer(reg, t, SlotId::Look, OUTLINE).unwrap();
        });
        assert_eq!(inside.reach_past(), 0.0);
    }
}
