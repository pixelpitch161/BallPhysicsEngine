//! Turning a thing's recent hits into bursts.

use bpe_core::{Settings, SlotId, ThingId};
use bpe_gfx::{Burst, DrawCx, Frame, HitMark, ModuleDraw};

use super::{BurstSource, HitBurst};

pub struct BurstDraw;

/// The speed at which a hit is at full strength, for bursts that follow the impact.
pub const FULL_HIT_SPEED: f32 = 600.0;

impl ModuleDraw for BurstDraw {
    fn kind(&self) -> &'static str {
        "particles.burst"
    }

    fn draw(&self, cx: &DrawCx, slot: SlotId, frame: &mut Frame) {
        let Some(rslot) = cx.resolved.slots.get(&slot) else { return };
        // Only things that hit let off bursts: a root module is inherited by every ball group.
        if !cx.world.bodies.contains_key(&cx.thing.id) {
            return;
        }
        let burst = HitBurst::from_params(&rslot.params);
        if burst.count <= 0 || burst.life <= 0.0 {
            return;
        }
        let own = burst.color.to_linear();
        let time = cx.time as f32;
        let mut bursts = Vec::new();
        for mark in frame.hits.iter().filter(|m| m.thing == cx.thing.id) {
            if mark.time > time || time - mark.time >= burst.life || mark.speed < burst.least_speed {
                continue;
            }
            bursts.push(burst_of(&burst, mark, own, cx.thing.id));
        }
        frame.bursts.extend(bursts);
    }
}

/// The burst one hit lets off.
pub fn burst_of(burst: &HitBurst, mark: &HitMark, own: [f32; 4], thing: ThingId) -> Burst {
    let strength = if burst.follow_impact { 0.6 + 0.4 * (mark.speed / FULL_HIT_SPEED).min(1.5) } else { 1.0 };
    let color = match burst.source {
        BurstSource::Ball => mark.color,
        BurstSource::Own => own,
    };
    Burst {
        origin: mark.point,
        normal: mark.normal,
        time: mark.time,
        color: bpe_gfx::blend_alpha(color),
        count: burst.count.max(0) as u32,
        speed: burst.speed.max(0.0) * strength,
        spread: burst.spread,
        size: burst.size.max(0.0),
        life: burst.life,
        gravity: burst.gravity,
        drag: burst.drag,
        shape: burst.preset.code(),
        additive: burst.additive(),
        seed: seed_of(thing, mark),
    }
}

/// A number that tells one hit from every other, the same every frame, so its particles don't
/// change as the clip is scrubbed.
fn seed_of(thing: ThingId, mark: &HitMark) -> u32 {
    let mut h: u32 = 0x811c_9dc5 ^ thing.0.wrapping_mul(0x9e37_79b1);
    for bits in [mark.time.to_bits(), mark.point[0].to_bits(), mark.point[1].to_bits()] {
        h ^= bits;
        h = h.wrapping_mul(0x0100_0193);
    }
    h ^ (h >> 16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{kit, new_scene, registry};
    use bpe_core::{Value, resolve};
    use bpe_gfx::build_frame;

    #[test]
    fn a_root_burst_goes_off_at_each_recent_hit_of_the_balls() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        let m = scene.add_module(&reg, "particles.burst", "Sparks").unwrap();
        scene.library.modules.get_mut(&m).unwrap().params.set("life", Value::Number(2.0));
        scene.link(&reg, ThingId::ROOT, SlotId::Particles, m).unwrap();
        let resolved = resolve(&scene, &reg);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let kit = kit();
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(4.0);
        let rate = playback.world().steps_per_second;
        let recent = playback.log().events().iter().filter(|e| e.time(rate) > 2.0 && e.time(rate) <= 4.0).count();
        assert!(recent > 0);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 4.0, &kit.draws, &mut frame);
        assert_eq!(frame.bursts.len(), recent, "{:?}", frame.hits);
        let b = frame.bursts[0];
        assert_eq!(b.count, 12);
        assert!(b.additive);
        assert_eq!(b.shape, 0);
        let pink = bpe_core::Color::rgb(0xff, 0x5c, 0xa8).to_linear();
        assert_eq!(b.color, pink);
        // Hits are listed for longer than this burst lasts; the first burst is the oldest hit
        // still within its life.
        let mark = frame.hits.iter().find(|h| 4.0 - h.time < 2.0).unwrap();
        assert_eq!((b.normal, b.origin, b.time), (mark.normal, mark.point, mark.time));
        // The speed follows the impact.
        let expected = 380.0 * (0.6 + 0.4 * (mark.speed / FULL_HIT_SPEED).min(1.5));
        assert!((b.speed - expected).abs() < 1e-3);
        // Seeds differ between hits and stay put between frames.
        if frame.bursts.len() > 1 {
            assert_ne!(frame.bursts[0].seed, frame.bursts[1].seed);
        }
        let mut again = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 4.0, &kit.draws, &mut again);
        assert_eq!(again.bursts, frame.bursts);

        // Its own colour, a covering preset, and a least speed nothing reaches.
        let module = scene.library.modules.get_mut(&m).unwrap();
        module.params.set("source", Value::Choice("own".into()));
        module.params.set("preset", Value::Choice("smoke".into()));
        module.params.set("color", Value::Color(bpe_core::Color::rgb(0, 0, 255)));
        let resolved = resolve(&scene, &reg);
        build_frame(&scene, &resolved, playback.world(), playback.log(), 4.0, &kit.draws, &mut frame);
        assert!(!frame.bursts[0].additive);
        assert_eq!(frame.bursts[0].color, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(frame.bursts[0].shape, 1);
        scene.library.modules.get_mut(&m).unwrap().params.set("least_speed", Value::Number(1e6));
        let resolved = resolve(&scene, &reg);
        build_frame(&scene, &resolved, playback.world(), playback.log(), 4.0, &kit.draws, &mut frame);
        assert!(frame.bursts.is_empty());
    }

    #[test]
    fn the_arena_and_the_root_let_off_nothing_themselves() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        let m = scene.add_module(&reg, "particles.burst", "Sparks").unwrap();
        scene.link(&reg, ThingId::ROOT, SlotId::Particles, m).unwrap();
        let resolved = resolve(&scene, &reg);
        let kit = kit();
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(4.0);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 4.0, &kit.draws, &mut frame);
        let hits = frame.hits.iter().filter(|h| h.thing == ThingId(2) && 4.0 - h.time < 0.6).count();
        assert_eq!(frame.bursts.len(), hits);
    }
}
