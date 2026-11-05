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

/// Draws the marker of the emitter `cx` draws, under its balls.
fn draw_marker(cx: &DrawCx, frame: &mut Frame) {
    let id = cx.thing.id;
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
}
