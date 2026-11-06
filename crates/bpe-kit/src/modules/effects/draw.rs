//! Turning the scene's Effects card into effect calls for the renderer: one per shown layer,
//! bottom first.

use std::borrow::Cow;

use bpe_core::{Blend, LayerId, Params, SettingValue, SlotId, ThingId, Value};
use bpe_gfx::effects::{Header, MAX_PARAM_FLOATS, ParamKind, memory_seconds};
use bpe_gfx::{DrawCx, EffectCall, Frame, ModuleDraw, Stage, thing_code};

use super::{EFFECTS, MIX, OWN, builtin_of, takes_of};

/// Draws the Effects card.
pub struct EffectsDraw;

/// One effect layer read from its settings: its code, header and the call's numbers.
pub struct EffectLayer {
    pub wgsl: Cow<'static, str>,
    pub header: Header,
    pub params: [f32; MAX_PARAM_FLOATS],
    pub mix: f32,
    pub blend: Blend,
}

impl EffectLayer {
    /// Reads a layer of `kind` (plain keys, as `ResolvedSlot::layer_params` gives them); `None`
    /// when it isn't an effect or its own code doesn't read.
    pub fn read(kind: &str, p: &Params) -> Option<EffectLayer> {
        let (wgsl, header) = if kind == OWN {
            let code = match p.get("code") {
                Some(Value::Text(t)) => t.clone(),
                _ => super::own_start().to_owned(),
            };
            let header = bpe_gfx::effects::parse(&code).ok()?;
            (Cow::Owned(code), header)
        } else {
            let b = builtin_of(kind)?;
            (Cow::Borrowed(b.wgsl), b.header().clone())
        };
        let mix = match p.get(MIX) {
            Some(Value::Number(x)) if x.is_finite() => *x as f32,
            _ => 1.0,
        };
        let blend = p.get("blend").and_then(Blend::from_value).unwrap_or_default();
        let params = pack_params(&header, p);
        Some(EffectLayer { wgsl, header, params, mix, blend })
    }
}

impl ModuleDraw for EffectsDraw {
    fn kind(&self) -> &'static str {
        EFFECTS
    }

    fn draw(&self, cx: &DrawCx, slot: SlotId, frame: &mut Frame) {
        let Some(rslot) = cx.resolved.slots.get(&slot) else { return };
        let root = cx.thing.id == ThingId::ROOT;
        for layer in &rslot.layers {
            let p = rslot.layer_params(layer.id);
            let Some(e) = EffectLayer::read(&layer.kind, &p) else { continue };
            let glow_above = match p.get("glow_above") {
                Some(Value::Number(x)) if x.is_finite() => *x as f32,
                _ => 0.9,
            };
            let memory = memory_seconds(&e.header, &e.wgsl, &e.params);
            let stage = e.header.stage;
            frame.effects.push(EffectCall {
                params: e.params,
                blend: e.mix,
                mode: e.blend,
                takes: if stage == Stage::Picture { takes_of(&p).takes() } else { bpe_gfx::Takes::Whole },
                glow_above,
                memory,
                apply_to: if root { 0.0 } else { thing_code(cx.thing.id) },
                ..EffectCall::new(call_key(cx.thing.id, slot, layer.id), e.wgsl, stage)
            });
        }
    }
}

/// Tells one effect layer apart from every other, for its history.
fn call_key(thing: ThingId, slot: SlotId, layer: LayerId) -> u64 {
    ((thing.0 as u64) << 40) | ((slot as u64) << 32) | layer.0 as u64
}

/// The settings packed the way the shader reads them: in header order, numbers, choices (their
/// place in the list) and toggles one float each, colours three (linear light). A missing
/// setting takes the header's default.
pub fn pack_params(header: &Header, params: &Params) -> [f32; MAX_PARAM_FLOATS] {
    let mut out = [0.0; MAX_PARAM_FLOATS];
    let mut at = 0;
    for p in &header.params {
        let value = params.get(&p.key);
        match p.kind {
            ParamKind::Number { default, .. } => {
                out[at] = match value {
                    Some(Value::Number(v)) if v.is_finite() => *v as f32,
                    Some(Value::Int(v)) => *v as f32,
                    _ => default,
                }
            }
            ParamKind::Choice { default, names } => {
                out[at] = match value {
                    Some(Value::Choice(c)) => names.iter().position(|n| n == c).unwrap_or(default),
                    _ => default,
                } as f32
            }
            ParamKind::Toggle { default } => {
                out[at] = match value {
                    Some(Value::Bool(b)) => *b as u8 as f32,
                    _ => default as u8 as f32,
                }
            }
            ParamKind::Color { default } => {
                let linear = match value {
                    Some(Value::Color(c)) => c.to_linear(),
                    _ => [default[0], default[1], default[2], 1.0],
                };
                out[at..at + 3].copy_from_slice(&linear[..3]);
            }
        }
        at += p.floats();
    }
    out
}

/// Whether the scene has an Effects card with a shown layer (the balls then list themselves for
/// the effects, and keep their hits).
pub fn scene_has_effects(scene: &bpe_core::Resolved) -> bool {
    scene
        .things
        .get(&ThingId::ROOT)
        .and_then(|r| r.slots.get(&SlotId::Shader))
        .is_some_and(|s| s.kind == EFFECTS && !s.layers.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{BALLS, SHAPE, scene_with_balls};
    use bpe_core::{Color, SlotId, layered_key, resolve};
    use bpe_gfx::effects::BUILTINS;
    use bpe_gfx::{Takes, build_frame};
    use bpe_runtime::{Log, World};

    #[test]
    fn params_pack_in_header_order_with_colours_taking_three() {
        let lava = BUILTINS.iter().find(|b| b.id == "lava").unwrap().header();
        let mut p = Params::default();
        p.set("heat", Value::Number(2.0));
        p.set("dark", Value::Color(Color::rgb(255, 255, 255)));
        let packed = pack_params(lava, &p);
        assert_eq!(&packed[..3], &[0.35, 3.0, 2.0]);
        assert_eq!(&packed[3..6], &[1.0, 1.0, 1.0]);
        assert!((packed[6] - 0.791).abs() < 0.01, "middle's red stays its default, linear: {}", packed[6]);
        let mirror = BUILTINS.iter().find(|b| b.id == "mirror").unwrap().header();
        let mut p = Params::default();
        p.set("top_bottom", Value::Bool(true));
        assert_eq!(&pack_params(mirror, &p)[..3], &[1.0, 1.0, 0.0]);
        let kaleidoscope = BUILTINS.iter().find(|b| b.id == "kaleidoscope").unwrap().header();
        let mut p = Params::default();
        p.set("slices", Value::Int(8));
        p.set("fold", Value::Choice("grid".into()));
        let packed = pack_params(kaleidoscope, &p);
        assert_eq!(packed[0], 8.0);
        let fold_at = kaleidoscope.params.iter().position(|d| d.key == "fold").unwrap();
        assert_eq!(packed[fold_at], 2.0, "a choice packs as its place in the list");
    }

    #[test]
    fn the_effects_card_makes_one_call_per_shown_layer_in_order() {
        let (mut scene, _, kit) = scene_with_balls("single", |_| {});
        let reg = &kit.registry;
        scene.add_slot(reg, ThingId::ROOT, SlotId::Shader, EFFECTS).unwrap();
        // Balls and shapes have Shader layers in their Look instead of an Effects card.
        for thing in [BALLS, SHAPE] {
            let refused = scene.add_slot(reg, thing, SlotId::Shader, EFFECTS);
            assert!(refused.is_err(), "no Effects card on a thing");
        }
        let k = scene.add_layer(reg, ThingId::ROOT, SlotId::Shader, "picture.kaleidoscope").unwrap();
        let echo = scene.add_layer(reg, ThingId::ROOT, SlotId::Shader, "picture.echo").unwrap();
        let hidden = scene.add_layer(reg, ThingId::ROOT, SlotId::Shader, "picture.film").unwrap();
        let card = scene.things.get_mut(&ThingId::ROOT).unwrap().slots.get_mut(&SlotId::Shader).unwrap();
        card.params.set(&layered_key(k, MIX), Value::Number(2.0));
        card.params.set(&layered_key(k, "takes"), Value::Choice("glow_only".into()));
        card.params.set(&layered_key(echo, "blend"), Value::Choice("add".into()));
        card.layers.iter_mut().find(|l| l.id == hidden).unwrap().enabled = false;

        let resolved = resolve(&scene, reg);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        assert!(!resolved.things[&SHAPE].slots.contains_key(&SlotId::Shader), "not inherited");
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.5, &kit.draws, &mut frame);
        assert_eq!(frame.effects.len(), 2, "the hidden layer draws nothing");
        let (first, second) = (&frame.effects[0], &frame.effects[1]);
        assert!(first.wgsl.contains("name: Kaleidoscope") && second.wgsl.contains("name: Feedback tunnel"));
        assert_eq!((first.blend, first.takes, first.apply_to, first.stage), (2.0, Takes::Glow, 0.0, Stage::Picture));
        assert_eq!((second.mode, second.takes), (Blend::Add, Takes::Whole));
        assert_eq!(first.memory, 0.0);
        // The echo's default fade, 0.9 a frame, is gone in under a second.
        assert!((second.memory - 1.316).abs() < 0.01, "{}", second.memory);
        assert_ne!(first.key, second.key);
        assert!(!frame.ball_marks.is_empty(), "the balls list themselves for the effects");
    }
}
