//! Drawing an emitter: its balls, as a Balls thing's draw, and its marker, which flashes when it
//! fires and when a wave lands on it, with rings rippling out from each landing. Everything is
//! read from the world as it is at the moment drawn, so scrubbing and export match.

use bpe_core::{Blend, ResolvedThing, Settings};
use bpe_gfx::{Around, DrawCx, Frame, Particle, ThingDraw};
use glam::Vec2;

use super::sim::{EmitterState, emitter_state};
use super::{EmitterSettings, KIND, Marker};
use crate::things::balls::draw::BallsDraw;

pub struct EmitterDraw;

/// The sprite shapes the marker is made of.
const RINGS: u32 = 5;
const DOTS: u32 = 6;

/// How wide a marker ring's band is, as a share of its radius.
const MARKER_BAND: f32 = 0.28;
/// How wide a ripple's band is, in pixels.
const RIPPLE_WIDTH: f32 = 3.0;

/// How strong the marker's flash is at `time` seconds: each firing (`fire` strong) and landing
/// (`land` strong) adds a flash that fades over `fade` seconds.
pub fn flash_at(st: &EmitterState, s: &EmitterSettings, rate: u32, time: f64) -> f32 {
    let fade = s.flash_fade.max(1e-3) as f64;
    let since = |step: u64| time - step as f64 / rate as f64;
    let strength = |ago: f64, amount: f32| {
        if (0.0..=fade).contains(&ago) { amount.max(0.0) * (1.0 - (ago / fade) as f32).powi(2) } else { 0.0 }
    };
    let fired = st.fired.iter().rev().map(|f| since(f.step)).take_while(|&a| a <= fade);
    let landed = st.landings.iter().rev().map(|l| since(l.step)).take_while(|&a| a <= fade);
    let fire: f32 = fired.map(|a| strength(a, s.flash_fire)).fold(0.0, f32::max);
    let land: f32 = landed.map(|a| strength(a, s.flash_land)).fold(0.0, f32::max);
    fire.max(land)
}

/// The ripple rings out at `time`: each one's radius and how much of it shows.
pub fn ripples_at(st: &EmitterState, s: &EmitterSettings, rate: u32, time: f64) -> Vec<(f32, f32)> {
    if !s.ripple {
        return Vec::new();
    }
    let fade = s.ripple_fade.max(0.05) as f64;
    let rings = s.ripple_rings.clamp(1, 10);
    // Each ring sets out a little after the one before it.
    let gap = fade * 0.15;
    let last = fade + gap * (rings - 1) as f64;
    let mut out = Vec::new();
    for l in st.landings.iter().rev() {
        let ago = time - l.step as f64 / rate as f64;
        if ago < 0.0 {
            continue;
        }
        if ago > last {
            break;
        }
        for k in 0..rings {
            let t = (ago - gap * k as f64) / fade;
            if !(0.0..1.0).contains(&t) {
                continue;
            }
            let t = t as f32;
            // Out fast at first, slowing as it fades.
            let reach = 1.0 - (1.0 - t).powi(2);
            out.push((s.marker_size.max(1.0) + reach * s.ripple_far.max(0.0), (1.0 - t).powi(2)));
        }
    }
    out
}

/// The marker's sprites, the ripples' and the flash's, for an emitter with settings `s` at `at`.
pub fn marker_sprites(s: &EmitterSettings, at: Vec2, flash: f32, ripples: &[(f32, f32)]) -> Vec<Particle> {
    let mut out = Vec::new();
    let c = s.marker_colour.to_linear();
    let size = s.marker_size.max(1.0);
    // The glow and the flash: a soft light that adds to what is under it.
    let glow = if s.marker == Marker::None { 0.0 } else { s.marker_glow.clamp(0.0, 1.0) };
    for (radius, light) in [(size * 2.5, glow * 0.6), (size * (2.0 + 3.0 * flash), flash)] {
        if light > 0.002 {
            let mut p = Particle::new(DOTS, at, 0.0, radius, [c[0], c[1], c[2], -light]);
            p.style[2] = 1.0;
            out.push(p);
        }
    }
    for &(radius, shows) in ripples {
        let mut p = Particle::new(RINGS, at, 0.0, radius, [c[0], c[1], c[2], shows * c[3]]);
        p.style[3] = (RIPPLE_WIDTH / radius).clamp(0.0, 1.0);
        p.style[2] = 0.3;
        out.push(p);
    }
    // Brighter while it flashes.
    let lit = 1.0 + flash;
    let lit = [c[0] * lit, c[1] * lit, c[2] * lit, c[3]];
    if matches!(s.marker, Marker::RingWithDot | Marker::Ring) {
        let mut p = Particle::new(RINGS, at, 0.0, size, lit);
        p.style[3] = MARKER_BAND;
        p.style[2] = 0.15;
        out.push(p);
    }
    match s.marker {
        Marker::RingWithDot => out.push(Particle::new(DOTS, at, 0.0, size * 0.32, lit)),
        Marker::Dot => out.push(Particle::new(DOTS, at, 0.0, size * 0.6, lit)),
        Marker::Ring | Marker::None => {}
    }
    out
}

/// The colour (linear light) each ball of one round of an emitter with settings `s` wearing
/// the Look `look` starts in, in the order of its directions: the group gradient's when the look
/// has one, else its Body's, by the ball's place in the round.
pub fn round_colours(look: Option<&bpe_core::ResolvedSlot>, s: &EmitterSettings) -> Vec<[f32; 4]> {
    use crate::modules::look::layers::gpu::Stack;
    use crate::modules::look::layers::group::{BallInGroup, GroupShapeInfo, group_colour, mixed};
    use crate::paint::{Inputs, colour_at};
    let n = super::fire::first_count(s);
    let Some(stack) = look.and_then(Stack::of) else { return vec![[1.0; 4]; n as usize] };
    let layout = super::fire::layout(s);
    let angle = stack.group.as_ref().map_or(0.0, |g| g.angle);
    let info = GroupShapeInfo::of(&layout, angle, s.seed);
    let fill = crate::modules::look::draw::for_balls(stack.body.fill.clone());
    (0..n)
        .map(|k| {
            let way = super::fire::direction(s, k, n, 1, 0, 0);
            let place = layout.get(k as usize).copied().unwrap_or_default();
            let at = Inputs { index: k as usize, count: n as usize, place, start: place, ..Inputs::default() };
            let base = colour_at(&fill, &at);
            match &stack.group {
                Some(g) => {
                    let ball = BallInGroup {
                        number: k,
                        count: n,
                        offset: place,
                        round: 1,
                        rounds: s.round_span(),
                        launch: way,
                        ..BallInGroup::default()
                    };
                    mixed(base, group_colour(g, &info, &ball), g.mix, g.amount)
                }
                None => base,
            }
        })
        .collect()
}

/// Draws the marker of the emitter `cx` draws, under its balls.
fn draw_marker(cx: &DrawCx, frame: &mut Frame) {
    let id = cx.thing.id;
    // A broken emitter's marker is gone at once; only its balls play out their look.
    if cx.world.rules().is_some_and(|r| r.is_broken(id)) {
        return;
    }
    let s = EmitterSettings::from_params(&cx.resolved.params);
    let Some(st) = emitter_state(cx.world, id) else { return };
    let rate = cx.world.steps_per_second;
    let at = cx.world.poses.get(&id).map_or(Vec2::ZERO, |p| p.position);
    let flash = if s.marker == Marker::None { 0.0 } else { flash_at(st, &s, rate, cx.time) };
    let ripples = ripples_at(st, &s, rate, cx.time);
    frame.particles.extend(marker_sprites(&s, at, flash, &ripples));
}

impl ThingDraw for EmitterDraw {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn blend(&self, resolved: &ResolvedThing) -> Option<(Blend, f32, bool)> {
        BallsDraw.blend(resolved)
    }

    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        draw_marker(cx, frame);
        BallsDraw.draw(cx, frame);
    }

    fn draw_over(&self, cx: &DrawCx, frame: &mut Frame) {
        BallsDraw.draw_over(cx, frame);
    }

    fn around(&self, resolved: &ResolvedThing) -> Vec<Around> {
        BallsDraw.around(resolved)
    }

    fn draw_around(&self, cx: &DrawCx, index: usize, frame: &mut Frame) {
        BallsDraw.draw_around(cx, index, frame);
    }

    fn light(&self, cx: &DrawCx, scene: &mut bpe_gfx::LightScene) {
        BallsDraw.light(cx, scene);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::run;
    use crate::things::emitter::test_scenes::{resolved, scene_with, shooter_and_catcher};
    use crate::things::emitter::{Catches, Earlier, FireWhen};
    use bpe_gfx::build_frame;

    fn frame_at(scene: &bpe_core::Scene, kit: &crate::Kit, time: f64) -> Frame {
        let r = resolved(scene, kit);
        let (world, log) = run(scene, &r, kit, time);
        let mut frame = Frame::default();
        build_frame(scene, &r, &world, &log, time, &kit.draws, &mut frame);
        frame
    }

    #[test]
    fn a_new_emitter_draws_its_balls_and_a_ring_with_a_dot_that_flashes_as_it_fires() {
        let at = Vec2::new(30.0, -40.0);
        let (scene, _, kit) = scene_with(at, |_| {});
        let frame = frame_at(&scene, &kit, 0.0);
        assert_eq!(frame.layered.len(), 10);
        let marker: Vec<&Particle> = frame.particles.iter().filter(|p| p.at() == at).collect();
        assert!(marker.iter().any(|p| p.shape[0] == RINGS && p.place[3] == 12.0), "the ring");
        assert!(marker.iter().any(|p| p.shape[0] == DOTS && p.color[3] > 0.0), "the dot");
        let glows = marker.iter().filter(|p| p.color[3] < 0.0).count();
        assert_eq!(glows, 2, "its glow and its firing flash");
        // Half a second on the flash is over.
        let later = frame_at(&scene, &kit, 0.5);
        let glows = later.particles.iter().filter(|p| p.place[0] == at.x && p.color[3] < 0.0).count();
        assert_eq!(glows, 1, "only its glow");
    }

    #[test]
    fn no_marker_draws_nothing_but_the_balls() {
        let (scene, _, kit) = scene_with(Vec2::ZERO, |s| {
            s.marker = Marker::None;
            s.ripple = false;
        });
        let frame = frame_at(&scene, &kit, 0.2);
        assert!(frame.particles.is_empty());
        assert_eq!(frame.layered.len(), 10);
    }

    #[test]
    fn a_wave_landing_flashes_the_catcher_and_ripples_out_from_it() {
        let (scene, _, catcher, kit) = shooter_and_catcher(3, |s| s.catches = Catches::PassThrough);
        let r = resolved(&scene, &kit);
        let (world, _) = run(&scene, &r, &kit, 0.8);
        let st = emitter_state(&world, catcher).unwrap();
        let landed = st.landings[0].step as f64 / 240.0;
        let s = EmitterSettings::from_params(&r.things[&catcher].params);
        assert!(flash_at(st, &s, 240, landed) > 0.85, "full at the landing");
        assert_eq!(flash_at(st, &s, 240, landed + 0.5), 0.0);
        let rings = ripples_at(st, &s, 240, landed + 0.5);
        assert_eq!(rings.len(), 3);
        assert!(rings.windows(2).all(|w| w[0].0 > w[1].0), "the first out is furthest");
        assert!(ripples_at(st, &s, 240, landed + 2.0).is_empty(), "all faded");
        let frame = frame_at(&scene, &kit, landed + 0.5);
        let at = Vec2::new(200.0, 0.0);
        let ripples = frame.particles.iter().filter(|p| p.at() == at && p.shape[0] == RINGS && p.place[3] > 20.0);
        assert_eq!(ripples.count(), 3);
    }

    #[test]
    fn a_fading_round_draws_fainter_until_it_goes() {
        let (scene, id, kit) = scene_with(Vec2::ZERO, |s| {
            s.fire_when = FireWhen::Every;
            s.every = 1.0;
            s.rounds = 2;
            s.count = 2;
            s.earlier = Earlier::FadeOut;
            s.fade_time = 1.0;
        });
        let frame = frame_at(&scene, &kit, 1.5);
        assert_eq!(frame.layered.len(), 4);
        let mut opacity: Vec<f32> = frame.layered.iter().map(|b| b.misc[0]).collect();
        opacity.sort_by(f32::total_cmp);
        let full = opacity[3];
        assert!((opacity[0] / full - 0.5).abs() < 0.02 && (opacity[1] / full - 0.5).abs() < 0.02, "{opacity:?}");
        assert!((opacity[2] - full).abs() < 1e-6);
        let r = resolved(&scene, &kit);
        let (world, _) = run(&scene, &r, &kit, 2.1);
        assert_eq!(world.bodies[&id].alive_count(), 2);
    }
}
