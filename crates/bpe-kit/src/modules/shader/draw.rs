//! Turning a thing's Shader slot into an effect call for the renderer.

use std::borrow::Cow;

use bpe_core::{ModuleId, Params, ThingId, Value};
use bpe_gfx::effects::{Builtin, Header, MAX_PARAM_FLOATS, ParamKind};
use bpe_gfx::{DrawCx, EffectCall, Frame, ModuleDraw, thing_code};

use super::{BLEND, id_of};

/// Draws for one built-in effect's module kind.
pub struct EffectDraw(pub &'static Builtin);

impl ModuleDraw for EffectDraw {
    fn kind(&self) -> &'static str {
        id_of(self.0)
    }

    fn draw(&self, cx: &DrawCx, slot: bpe_core::SlotId, frame: &mut Frame) {
        let Some(rslot) = cx.resolved.slots.get(&slot) else { return };
        let root = cx.thing.id == ThingId::ROOT;
        let header = self.0.header();
        let blend = match rslot.params.get(BLEND) {
            Some(Value::Number(b)) => *b as f32,
            _ => 1.0,
        };
        frame.effects.push(EffectCall {
            key: call_key(cx.thing.id, rslot.module),
            wgsl: Cow::Borrowed(self.0.wgsl),
            params: pack_params(header, &rslot.params),
            blend,
            apply_to: if root { 0.0 } else { thing_code(cx.thing.id) },
            stage: header.stage,
        });
    }
}

/// Tells one thing's use of one module apart from every other, for the effect's history.
fn call_key(thing: ThingId, module: ModuleId) -> u64 {
    ((thing.0 as u64) << 32) | module.0 as u64
}

/// The settings packed the way the shader reads them: in header order, numbers and toggles one
/// float each, colours three (linear light). A missing setting takes the header's default.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{ARENA, BALLS, scene_with_balls};
    use bpe_core::{Color, SlotId, resolve};
    use bpe_gfx::effects::BUILTINS;
    use bpe_gfx::{Stage, build_frame};
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
        assert_eq!(pack_params(kaleidoscope, &p)[0], 8.0);
    }

    #[test]
    fn a_root_shader_is_a_picture_effect_and_a_thing_shader_is_a_material() {
        let (mut scene, _, kit) = scene_with_balls("single", |_| {});
        let reg = &kit.registry;
        let k = scene.add_module(reg, "shader.kaleidoscope", "Slices").unwrap();
        scene.link(reg, ThingId::ROOT, SlotId::Shader, k).unwrap();
        let lava = scene.add_module(reg, "shader.lava", "Lava").unwrap();
        scene.library.modules.get_mut(&lava).unwrap().params.set("blend", Value::Number(0.5));
        scene.link(reg, BALLS, SlotId::Shader, lava).unwrap();
        let resolved = resolve(&scene, reg);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        assert!(!resolved.things[&ARENA].slots.contains_key(&SlotId::Shader), "Scene settings' effect isn't inherited");
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.5, &kit.draws, &mut frame);
        assert_eq!(frame.effects.len(), 2, "one picture effect and one material");
        let picture = &frame.effects[0];
        assert_eq!((picture.apply_to, picture.stage, picture.blend), (0.0, Stage::Picture, 1.0));
        assert!(picture.wgsl.contains("name: Kaleidoscope"));
        let material = &frame.effects[1];
        assert_eq!((material.apply_to, material.stage, material.blend), (thing_code(BALLS), Stage::Things, 0.5));
        assert_ne!(picture.key, material.key);
    }
}
