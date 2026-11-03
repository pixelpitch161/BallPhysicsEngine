//! Turning a thing's resolved stack of layers into what the renderer draws: the layer records,
//! written once per thing per frame, and one instance per ball naming them. Only the body's
//! colour, the ball's own variation and its motion differ from ball to ball.

use std::f32::consts::FRAC_PI_2;

use bpe_core::{Blend, Choice, LayerId, ResolvedSlot, Settings};
use bpe_gfx::{GradientAtlas, LayerGpu, LayeredInstance};
use glam::Vec2;

use super::{
    BLEND, BODY, BlendLayer, BodyLayer, ColourFrom, ColourOnHit, ColourOverTime, FLASH, FlashOnHit, GLOW, GlowLayer,
    HUE, MASK, MaskLayer, OUTLINE, OutlineLayer, OutlineSits, PATTERN, PatternLayer, RAINBOW, SHADER, SHADING, SHINE,
    STRETCH, ShaderLayer, ShadingLayer, ShineLayer, StretchWhenFast, TURNING, Turning,
};
use crate::modules::look::BallShape;
use crate::modules::look::basic::{STRETCH_SPEED, facing_degrees, heading, shift_linear_hue};
use crate::paint::{InsideFrom, inside_gpu};

/// How far a ball's shapes reach from its centre, in radii (a triangle's corners, a heart's
/// lobes); matches SHAPE_REACH in styled.wgsl.
const SHAPE_REACH: f32 = 1.45;
/// How far past the edge a glow reaches, in its sizes; matches HALO_REACH in styled.wgsl.
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

/// A drawn layer above the body, with its id and its shown masks.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawnLayer {
    pub layer: Drawn,
    pub id: LayerId,
    /// Its shown masks, in order; they multiply.
    pub masks: Vec<MaskLayer>,
}

/// A thing's shown stack, read from its resolved Look card.
#[derive(Clone, Debug, PartialEq)]
pub struct Stack {
    pub body_id: LayerId,
    pub body: BodyLayer,
    /// The drawn layers above the body, bottom first.
    pub drawn: Vec<DrawnLayer>,
    pub blend: Option<BlendLayer>,
    pub flash: Option<FlashOnHit>,
    pub hue: Option<ColourOnHit>,
    pub stretch: Option<StretchWhenFast>,
    pub rainbow: Option<ColourOverTime>,
    pub turning: Option<Turning>,
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
            drawn: Vec::new(),
            blend: None,
            flash: None,
            hue: None,
            stretch: None,
            rainbow: None,
            turning: None,
        };
        for layer in &slot.layers {
            let p = slot.layer_params(layer.id);
            let mut push = |d: Drawn| stack.drawn.push(DrawnLayer { layer: d, id: layer.id, masks: Vec::new() });
            match layer.kind.as_str() {
                PATTERN => push(Drawn::Pattern(PatternLayer::from_params(&p))),
                OUTLINE => push(Drawn::Outline(OutlineLayer::from_params(&p))),
                SHINE => push(Drawn::Shine(ShineLayer::from_params(&p))),
                GLOW => push(Drawn::Glow(GlowLayer::from_params(&p))),
                SHADING => push(Drawn::Shading(ShadingLayer::from_params(&p))),
                SHADER => push(Drawn::Shader(Box::new(ShaderLayer::from_params(&p)))),
                // A mask goes with its layer; a hidden layer's masks don't matter.
                MASK => {
                    if let Some(d) = stack.drawn.iter_mut().find(|d| Some(d.id) == layer.parent) {
                        d.masks.push(MaskLayer::from_params(&p));
                    }
                }
                // Blend and the reactions are there at most once; the first counts.
                BLEND => stack.blend = stack.blend.take().or(Some(BlendLayer::from_params(&p))),
                FLASH => stack.flash = stack.flash.take().or(Some(FlashOnHit::from_params(&p))),
                HUE => stack.hue = stack.hue.take().or(Some(ColourOnHit::from_params(&p))),
                STRETCH => stack.stretch = stack.stretch.take().or(Some(StretchWhenFast::from_params(&p))),
                RAINBOW => stack.rainbow = stack.rainbow.take().or(Some(ColourOverTime::from_params(&p))),
                TURNING => stack.turning = stack.turning.take().or(Some(Turning::from_params(&p))),
                _ => {}
            }
        }
        Some(stack)
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

    /// Whether any pattern paints from its own gradient, so balls need their own variation.
    pub fn varies(&self) -> bool {
        self.drawn.iter().any(|d| matches!(&d.layer, Drawn::Pattern(p) if p.inside.from == InsideFrom::OwnGradient))
    }

    /// Whether the shape turns to point the way the ball goes (never a circle, which has no point).
    pub fn faces_motion(&self) -> bool {
        self.turning.as_ref().is_some_and(|t| t.face_motion) && self.body.ball_shape != BallShape::Circle
    }

    /// The records the stack draws with at `time` seconds, bottom first, its gradients baked into
    /// `atlas`. Hidden layers, Blend and the reactions have none.
    pub fn records(&self, atlas: &mut GradientAtlas, time: f32) -> Vec<LayerGpu> {
        let blend = |b: Blend| b.shader_code();
        let flag = |on: bool| if on { LayerGpu::FLAG } else { 0 };
        let mut out = vec![LayerGpu { amount: [1.0, 0.0, 0.0, 0.0], ..LayerGpu::ZERO }];
        let turn = self.rainbow() * time;
        for d in &self.drawn {
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
                    colour: o.colour.to_linear(),
                    ..LayerGpu::ZERO
                },
                Drawn::Shine(s) => LayerGpu {
                    head: [LayerGpu::SHINE, blend(s.blend), 0, 0],
                    amount: [s.opacity, s.strength, 0.0, 0.0],
                    p0: [s.spot.x, s.spot.y, s.size, 0.0],
                    ..LayerGpu::ZERO
                },
                Drawn::Glow(g) => LayerGpu {
                    head: [LayerGpu::GLOW, blend(g.blend), flag(g.colour_from == ColourFrom::SameAsBody), 0],
                    amount: [g.opacity, g.strength, 0.0, 0.0],
                    p0: [g.size.max(0.05), 0.0, 0.0, 0.0],
                    colour: g.colour.to_linear(),
                    ..LayerGpu::ZERO
                },
                Drawn::Shading(_) | Drawn::Shader(_) => continue,
            };
            out.push(r);
        }
        out
    }

    /// How far past the centre a ball of `radius` with this stack reaches, in radii.
    pub fn reach(&self, radius: f32) -> f32 {
        let mut past_edge: f32 = 0.0;
        for d in &self.drawn {
            match &d.layer {
                Drawn::Glow(g) if g.strength > 0.0 && g.opacity > 0.0 => past_edge = past_edge.max(HALO_REACH * g.size),
                Drawn::Outline(o) => {
                    let out = match o.sits {
                        OutlineSits::Inside => 0.0,
                        OutlineSits::Centre => 0.5 * o.width,
                        OutlineSits::Outside => o.width,
                    };
                    past_edge = past_edge.max(out.max(0.0) / radius.max(1e-3));
                }
                _ => {}
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
        }
    }
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
    use super::*;
    use crate::registry;
    use bpe_core::{Scene, SlotId, ThingId, Value, layered_key, resolve};

    /// A new ball's resolved Look after `f` changes its scene.
    fn stack_after(f: impl FnOnce(&bpe_core::Registry, &mut Scene, ThingId)) -> Stack {
        let reg = registry();
        let mut s = Scene::empty(&reg);
        let b = crate::add_thing(&mut s, &reg, "balls", ThingId::ROOT).unwrap();
        f(&reg, &mut s, b);
        Stack::of(&resolve(&s, &reg).things[&b].slots[&SlotId::Look]).unwrap()
    }

    #[test]
    fn a_new_ball_draws_one_body_record_and_reaches_just_past_its_shape() {
        let stack = stack_after(|_, _, _| {});
        let mut atlas = GradientAtlas::default();
        let records = stack.records(&mut atlas, 0.0);
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
        let records = stack.records(&mut GradientAtlas::default(), 0.0);
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
    fn masks_stay_with_their_shown_layer() {
        let stack = stack_after(|reg, s, b| {
            let pat = s.add_layer(reg, b, SlotId::Look, PATTERN).unwrap();
            s.add_mask(reg, b, SlotId::Look, pat).unwrap();
            let off = s.add_mask(reg, b, SlotId::Look, pat).unwrap();
            s.set_layer_enabled(reg, b, SlotId::Look, off, false).unwrap();
            let hidden = s.add_layer(reg, b, SlotId::Look, OUTLINE).unwrap();
            s.add_mask(reg, b, SlotId::Look, hidden).unwrap();
            s.set_layer_enabled(reg, b, SlotId::Look, hidden, false).unwrap();
            s.add_layer(reg, b, SlotId::Look, SHADING).unwrap();
        });
        assert_eq!(stack.drawn.len(), 2, "the hidden outline is left out");
        assert_eq!(stack.drawn[0].masks.len(), 1, "the hidden mask is left out");
        assert!(matches!(stack.drawn[1].layer, Drawn::Shading(_)) && stack.drawn[1].masks.is_empty());
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
    fn choice_labels_read_as_words() {
        assert_eq!(choice_label(ColourFrom::SameAsBody), "Same as body");
        assert_eq!(choice_label(OutlineSits::Inside), "Inside");
    }
}
