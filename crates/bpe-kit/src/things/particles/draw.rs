//! Drawing a Particles thing: nothing of its own, then each shown Look layer as an around layer
//! with a blend of its own (so other blends route through the frame's layers as trails' do),
//! every particle it draws as a sprite with what its sub-layers add. Particles carry no
//! lighting in part 1 (their `light`, `emit` and `extra` stay 0).

use bpe_core::{Blend, ResolvedThing, SlotId, ThingId};
use bpe_gfx::{Around, DrawCx, Frame, Particle, ThingDraw};
use bpe_runtime::EventKind;

use super::KIND;
use super::births::unit;
use super::look::{ColourFrom, Common, Drawn, LookLayer, Only, layers_of};
use super::sim::state_of;
use super::state::{ParticlesState, Speck};
use crate::modules::look::layers::around_subs::{Extras, HitInfo, Pieces, coloured, particle_with_extras};
use crate::modules::look::layers::particles::ShapeMadeOf;
use crate::modules::look::layers::trail::AroundPlace;
use crate::paint::{Inputs, colour_at};
use crate::things::balls::draw::hit_strength;

pub struct ParticlesDraw;

/// The thing's shown Look layers, bottom first.
fn layers(resolved: &ResolvedThing) -> Vec<LookLayer> {
    match resolved.slots.get(&SlotId::Look) {
        Some(look) if look.kind == super::look::CARD => layers_of(look),
        _ => Vec::new(),
    }
}

impl ThingDraw for ParticlesDraw {
    fn kind(&self) -> &'static str {
        KIND
    }

    /// The particles draw as around layers; the thing itself draws nothing.
    fn draw(&self, _cx: &DrawCx, _frame: &mut Frame) {}

    /// The thing itself never blends (its layers do, each on its own).
    fn blend(&self, _resolved: &ResolvedThing) -> Option<(Blend, f32, bool)> {
        Some((Blend::Normal, 1.0, false))
    }

    fn around(&self, resolved: &ResolvedThing) -> Vec<Around> {
        layers(resolved)
            .iter()
            .map(|l| Around {
                blend: l.common.blend,
                amount: l.common.blend_amount,
                behind: l.common.place == AroundPlace::Behind,
            })
            .collect()
    }

    fn draw_around(&self, cx: &DrawCx, index: usize, frame: &mut Frame) {
        let Some(layer) = layers(cx.resolved).into_iter().nth(index) else { return };
        let Some(st) = state_of(cx.world, cx.thing.id) else { return };
        let mut pieces = Pieces::default();
        draw_layer(cx, st, &layer, &mut pieces);
        pieces.into_frame(frame, layer.common.place == AroundPlace::InFront);
    }
}

/// Whether `layer` draws particle `s` at particle step `now`.
pub fn draws(c: &Common, s: &Speck, now: u32) -> bool {
    match c.only {
        Only::All => true,
        Only::YoungerThan => s.age(now) < c.age,
        Only::OlderThan => s.age(now) >= c.age,
        Only::AShare => unit(s.seed, 61) < c.share,
        Only::TheRest => unit(s.seed, 61) >= c.share,
    }
}

/// A particle's colour from `base` (linear light) at `alpha`, mixed in by the blend amount:
/// covering, or adding its light. A colour with no alpha, or a layer that adds, adds its light.
pub fn shade(c: &Common, base: [f32; 4], alpha: f32) -> [f32; 4] {
    let amount = match c.blend {
        Blend::Normal | Blend::Add => c.blend_amount.clamp(0.0, 1.0),
        _ => 1.0,
    };
    if c.blend != Blend::Add && base[3] > 0.0 {
        return [base[0], base[1], base[2], (base[3] * alpha * amount).min(1.0)];
    }
    [base[0], base[1], base[2], -(alpha * amount)]
}

/// The hits of the thing a Particles thing sits in (its parent), as its colour sub-layers read
/// them: the last before `cx.time` and how many so far.
fn parent_hits(cx: &DrawCx, parent: Option<ThingId>) -> HitInfo {
    let Some(parent) = parent else { return HitInfo::default() };
    let rate = cx.world.steps_per_second;
    let mut info = HitInfo::default();
    for e in cx.log.since(0) {
        let ours = e.a.thing == parent || e.b.is_some_and(|b| b.thing == parent);
        if e.kind != EventKind::Hit || !ours {
            continue;
        }
        let time = e.time(rate);
        if time > cx.time {
            break;
        }
        info.last = Some(((cx.time - time) as f32, hit_strength(e.speed)));
        info.hits += 1;
    }
    info
}

/// The sprite for particle `s` drawn by `drawn` at `size` (half size), coloured `colour`;
/// `flutters` when a force flips the particles over.
pub fn sprite(drawn: &Drawn, s: &Speck, size: f32, colour: [f32; 4], flutters: bool) -> Particle {
    let speed = s.velocity.length();
    match *drawn {
        Drawn::Sprites { made_of, softness } => {
            let mut p = Particle::new(made_of.as_ball().code(), s.place, s.turn, size, colour);
            p.style[0] = match made_of {
                ShapeMadeOf::Sparks => size + speed * 0.03,
                ShapeMadeOf::Streaks => size * 2.0 + speed * 0.08,
                ShapeMadeOf::Confetti => size * 1.6,
                ShapeMadeOf::Shards => size * 1.4,
                _ => size,
            };
            p.style[2] = softness.clamp(0.0, 1.0);
            // Flutter: a flat thing flipping over shows edge on, and darker on its back.
            if flutters {
                let face = s.flip.cos();
                p.place[3] = size * face.abs().max(0.12);
                p.style[1] = 0.7 + 0.3 * face;
            }
            p
        }
        Drawn::Streaks { stretch, least, width } => {
            let half_width = (size * width).max(0.0);
            let length = (speed * stretch).max(least).max(0.0);
            let angle = if speed > 1e-3 { s.velocity.y.atan2(s.velocity.x) } else { s.turn };
            let mut p = Particle::new(ShapeMadeOf::Streaks.as_ball().code(), s.place, angle, half_width, colour);
            p.style[0] = (length * 0.5).max(half_width);
            p
        }
    }
}

/// Draws `layer`'s particles of `st` into `pieces`.
pub fn draw_layer(cx: &DrawCx, st: &ParticlesState, layer: &LookLayer, pieces: &mut Pieces) {
    let c = &layer.common;
    if c.opacity <= 0.0 {
        return;
    }
    let screen = cx.resolved.screen;
    let hit = if layer.subs.iter().any(|s| s.reads_hits()) { parent_hits(cx, st.parent) } else { HitInfo::default() };
    // The colours of the parent's balls, when a layer takes them.
    let parent = st.parent.and_then(|p| Some((cx.tree.things.get(&p)?, cx.scene.things.get(&p)?)));
    let ball_cx = match (c.colour_from, parent) {
        (ColourFrom::ParentBall, Some((thing, resolved))) => Some(DrawCx { thing, resolved, ..*cx }),
        _ => None,
    };
    let ball_colour = ball_cx.as_ref().and_then(crate::things::balls::draw::ball_colors);
    let now = st.step;
    let flutters = st.forces.flips();
    let twinkle = c.twinkle.clamp(0.0, 1.0);
    for s in &st.specks {
        if !draws(c, s, now) {
            continue;
        }
        let t = s.life_at(now);
        let Some(grow) = c.size_curve.at(t) else { continue };
        let age = s.age(now);
        let flicker = 1.0
            - twinkle * (0.5 + 0.5 * (c.twinkle_speed * std::f32::consts::TAU * age + unit(s.seed, 67) * 6.283).sin());
        let alpha = (c.opacity * c.fade.at(t) * flicker).max(0.0);
        if alpha <= 0.0 {
            continue;
        }
        let own = || {
            let inputs = Inputs {
                // A stop picked once for each particle, for Follow: Order.
                index: (unit(s.seed, 41) * 1024.0) as usize,
                count: 1024,
                place: s.place,
                place_screen: if screen { s.place } else { cx.world.camera.to_screen(s.place) },
                start: s.home,
                speed: s.velocity.length(),
                life: t,
                time: cx.time as f32,
                seed: s.seed,
                ..Inputs::default()
            };
            colour_at(&c.colour, &inputs)
        };
        let base = match (&ball_colour, s.ball) {
            (Some(of), b) if b > 0 => of(b as usize - 1),
            _ => own(),
        };
        let size = s.size * c.size.max(0.0) * grow;
        if size <= 0.0 {
            continue;
        }
        let subs = &layer.subs;
        let colour = shade(c, coloured(subs, subs.len(), base, hit), alpha);
        let extras = Extras { subs, base, s: t, fade: alpha, unit: 1.0, hit, outline: 1.0, added: 1.0 };
        particle_with_extras(sprite(&layer.drawn, s, size, colour, flutters), &extras, pieces);
    }
}
