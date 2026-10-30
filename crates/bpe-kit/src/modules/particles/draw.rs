//! Turning a thing's recent hits into bursts.

use bpe_core::{Gradient, Settings, SlotId, ThingId, Transition};
use bpe_gfx::{Burst, DrawCx, Frame, GradientAtlas, HitMark, ModuleDraw, PaintGpu};

use super::{BurstSource, HitBurst};
use crate::paint::gpu::repeat_code;
use crate::paint::{Follow, Inputs, Paint, beats_at, colour_at};

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
        let time = cx.time as f32;
        let own = OwnColours::new(cx, &burst.colour);
        let mut bursts = Vec::new();
        for mark in frame.hits.iter().filter(|m| m.thing == cx.thing.id) {
            if mark.time > time || time - mark.time >= burst.life || mark.speed < burst.least_speed {
                continue;
            }
            bursts.push(burst_of(&burst, mark, &own, cx.thing.id));
        }
        if burst.source == BurstSource::Own && !bursts.is_empty() {
            let (paint, follow) = own.on_gpu(&burst.colour, &mut frame.gradients);
            for b in &mut bursts {
                (b.paint, b.follow) = (paint, follow);
            }
        }
        frame.bursts.extend(bursts);
    }
}

/// What a burst's own Colours read this frame: the frame's time and beats, and the camera for
/// the screen place of each hit.
pub struct OwnColours {
    camera: bpe_runtime::CameraView,
    /// Stuck to the screen: hit points are screen places already.
    screen: bool,
    time: f32,
    beats: f32,
}

impl OwnColours {
    pub fn new(cx: &DrawCx, p: &Paint) -> Self {
        let beats =
            if p.follow == Follow::Beat { beats_at(cx.log, cx.video_time, cx.world.steps_per_second) } else { 0.0 };
        Self { camera: cx.world.camera, screen: cx.resolved.screen, time: cx.time as f32, beats }
    }

    /// The one colour a burst's particles all take from `p`, for the follows worked out once
    /// per burst: where the hit was, how hard, and the time and beats now. Life and Order are
    /// worked out per particle on the GPU and start from the first stop here.
    pub fn colour(&self, p: &Paint, mark: &HitMark) -> [f32; 4] {
        let place = glam::Vec2::from(mark.point);
        let inputs = Inputs {
            count: 1,
            place,
            place_screen: if self.screen { place } else { self.camera.to_screen(place) },
            start: place,
            speed: mark.speed,
            beats: self.beats,
            time: self.time,
            ..Inputs::default()
        };
        colour_at(p, &inputs)
    }

    /// How the GPU paints the particles of `p`: Life runs each particle along the gradient by
    /// its age (follow 1), starting where the scroll has moved it; Order gives each one of the
    /// stops at random (follow 2), the stops baked evenly spaced as Steps with their count in
    /// `scale`. Anything else, and a single stop, is one colour per burst (follow 0, nothing
    /// baked).
    pub fn on_gpu(&self, p: &Paint, atlas: &mut GradientAtlas) -> (PaintGpu, u32) {
        let finite = |x: f32| if x.is_finite() { x } else { 0.0 };
        // One colour is the same on every particle, and exact on the CPU.
        if p.colours.stops.len() <= 1 {
            return (PaintGpu::default(), 0);
        }
        match p.follow {
            Follow::Life => {
                let gpu = PaintGpu {
                    row: atlas.row(&p.colours) as f32,
                    t0: finite(self.time * p.scroll),
                    steps: if p.colours.transition == Transition::Steps { 1.0 } else { 0.0 },
                    repeat: repeat_code(p.repeat),
                    ..PaintGpu::default()
                };
                (gpu, 1)
            }
            Follow::Order => {
                let colours: Vec<_> = p.colours.stops.iter().map(|s| s.color).collect();
                let stops = Gradient { transition: Transition::Steps, ..Gradient::even(&colours) };
                let gpu = PaintGpu {
                    row: atlas.row(&stops) as f32,
                    steps: 1.0,
                    scale: colours.len().max(1) as f32,
                    ..PaintGpu::default()
                };
                (gpu, 2)
            }
            _ => (PaintGpu::default(), 0),
        }
    }
}

/// The burst one hit lets off, in one colour; [`OwnColours::on_gpu`] gives the paint of an own
/// colour that changes from particle to particle.
pub fn burst_of(burst: &HitBurst, mark: &HitMark, own: &OwnColours, thing: ThingId) -> Burst {
    let strength = if burst.follow_impact { 0.6 + 0.4 * (mark.speed / FULL_HIT_SPEED).min(1.5) } else { 1.0 };
    let color = match burst.source {
        BurstSource::Ball => mark.color,
        BurstSource::Own => own.colour(&burst.colour, mark),
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
        paint: PaintGpu::default(),
        follow: 0,
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
        module.params.set("colours", Value::Gradient(bpe_core::Gradient::solid(bpe_core::Color::rgb(0, 0, 255))));
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
    fn own_colours_go_to_the_gpu_for_life_and_order_and_are_worked_out_once_otherwise() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        let m = scene.add_module(&reg, "particles.burst", "Sparks").unwrap();
        scene.link(&reg, ThingId::ROOT, SlotId::Particles, m).unwrap();
        let kit = kit();
        let mut playback = bpe_runtime::Playback::new(&scene, &resolve(&scene, &reg), kit.sims.clone());
        playback.seek(4.0);
        let (red, green, blue) =
            (bpe_core::Color::rgb(255, 0, 0), bpe_core::Color::rgb(0, 255, 0), bpe_core::Color::rgb(0, 0, 255));
        let mut draw = |follow: &str, colours: bpe_core::Gradient| {
            let params = &mut scene.library.modules.get_mut(&m).unwrap().params;
            params.set("source", Value::Choice("own".into()));
            params.set("follow", Value::Choice(follow.into()));
            params.set("colours", Value::Gradient(colours));
            let resolved = resolve(&scene, &reg);
            assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
            let mut frame = Frame::default();
            build_frame(&scene, &resolved, playback.world(), playback.log(), 4.0, &kit.draws, &mut frame);
            assert!(!frame.bursts.is_empty());
            frame
        };
        let three =
            bpe_core::Gradient { stops: bpe_core::Gradient::even(&[red, green, blue]).stops, ..Default::default() };

        // Life: each particle runs along the gradient on the GPU.
        let frame = draw("life", three.clone());
        let b = frame.bursts[0];
        assert_eq!(b.follow, 1);
        assert_eq!(frame.gradients.rows()[b.paint.row as usize][0], red.to_linear());
        assert_eq!(*frame.gradients.rows()[b.paint.row as usize].last().unwrap(), blue.to_linear());
        assert!(frame.bursts.iter().all(|o| o.paint == b.paint && o.follow == 1), "one row for every burst");

        // Order: one of the three stops each, baked as hard bands.
        let frame = draw("order", three.clone());
        let b = frame.bursts[0];
        assert_eq!((b.follow, b.paint.scale, b.paint.steps), (2, 3.0, 1.0));
        let row = &frame.gradients.rows()[b.paint.row as usize];
        assert_eq!([row[0], row[128], row[255]], [red.to_linear(), green.to_linear(), blue.to_linear()]);

        // Speed: one colour per burst, from how hard the hit was.
        let frame = draw(
            "speed",
            bpe_core::Gradient { stops: bpe_core::Gradient::even(&[red, blue]).stops, ..Default::default() },
        );
        assert!(frame.gradients.len() == 1, "nothing to bake");
        for (b, mark) in frame.bursts.iter().zip(frame.hits.iter().filter(|h| 4.0 - h.time < 0.6)) {
            assert_eq!(b.follow, 0);
            let t = (mark.speed / 1920.0).clamp(0.0, 1.0);
            let want = [1.0 - t, 0.0, t];
            assert!((0..3).all(|k| (b.color[k] - want[k]).abs() < 1e-4), "{:?} at {}", b.color, mark.speed);
        }

        // One colour over its life is that colour on the CPU, exactly.
        let frame = draw("life", bpe_core::Gradient::solid(green));
        assert_eq!((frame.bursts[0].follow, frame.bursts[0].color), (0, green.to_linear()));
        assert!(frame.gradients.len() == 1);
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
