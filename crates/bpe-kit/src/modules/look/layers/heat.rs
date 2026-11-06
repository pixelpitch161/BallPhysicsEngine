//! **Heat glow** (lighting part 4): a sub-layer that makes its layer heat up on every hit and glow
//! red, then orange, yellow and white the hotter it gets, cooling down again between hits. With a
//! Lighting card its glow can also light the room round it. It goes on any drawn layer and on the
//! Body (a ball's or a shape's).
//!
//! Each ball (or the shape) has one heat, worked out from the log at the moment drawn and carried
//! in its instance's `hit[3]`: every hit adds Heat per hit times how hard it was, and the heat
//! cools by Cool down (to about a third in that time). Nothing is kept between frames.

use bpe_core::Settings;
use bpe_gfx::DrawCx;
use bpe_runtime::EventKind;

/// The ball sub-layer kind's id.
pub const SUB_HEAT: &str = "sub.heat";
/// The shape sub-layer kind's id.
pub const SHAPE_SUB_HEAT: &str = "shape.sub.heat";
/// What the sub-layer kind says about itself.
pub const HELP: &str = "Its layer heats up on every hit and glows red to white hot, cooling down between hits.";

/// How many cool-down times back hits still count (e^-8 of their heat is left).
const COUNTED: f64 = 8.0;

/// The Heat glow sub-layer's settings.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct SubHeat {
    /// How much heat a full-strength hit adds.
    #[setting(label = "Heat per hit", span(0, 2), simple, keyframe)]
    pub per_hit: f32,
    /// How long it takes to cool to about a third.
    #[setting(label = "Cool down", range(0.001, 1e9), span(0.1, 10), unit = "s", simple, keyframe)]
    pub cool_down: f32,
    /// How bright it glows when hot.
    #[setting(label = "Glow", span(0, 4), simple, keyframe, pulse)]
    pub glow: f32,
    /// How far toward white hot it goes: low stays a dull red, high reaches white.
    #[setting(label = "Hottest", span(0, 2), keyframe)]
    pub hottest: f32,
    /// How much its glow lights the room round it (with a Lighting card).
    #[setting(label = "Lights the room", span(0, 4), keyframe)]
    pub lights_room: f32,
}

impl Default for SubHeat {
    fn default() -> Self {
        Self { per_hit: 0.6, cool_down: 1.5, glow: 1.5, hottest: 1.0, lights_room: 1.0 }
    }
}

impl SubHeat {
    /// Its record (code 28, Flash's, with `amount.w` 1): Glow, Hottest, Lights the room.
    pub fn record(&self) -> bpe_gfx::LayerGpu {
        let finite = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
        bpe_gfx::LayerGpu {
            head: [bpe_gfx::LayerGpu::SUB_FLASH, 0, 0, 0],
            amount: [finite(self.glow), finite(self.hottest), finite(self.lights_room), 1.0],
            ..bpe_gfx::LayerGpu::ZERO
        }
    }
}

/// How hot a thing is at `now` after hits at (time, strength), oldest first, by `heat`.
pub fn heat_at(hits: impl IntoIterator<Item = (f64, f32)>, now: f64, heat: &SubHeat) -> f32 {
    let cool = heat.cool_down.max(1e-3) as f64;
    let mut sum = 0.0f64;
    for (t, strength) in hits {
        let age = now - t;
        if (0.0..=COUNTED * cool).contains(&age) {
            sum += strength as f64 * (-age / cool).exp();
        }
    }
    (sum as f32 * heat.per_hit).max(0.0)
}

/// Each ball's heat (by row) of the thing `cx` is drawing, from its hits.
pub fn ball_heats(cx: &DrawCx, count: usize, heat: &SubHeat) -> Vec<f32> {
    let id = cx.thing.id;
    let rate = cx.world.steps_per_second;
    let cool = heat.cool_down.max(1e-3) as f64;
    let since = ((cx.time - COUNTED * cool) * rate as f64).floor().max(0.0) as u64;
    let mut hits: Vec<Vec<(f64, f32)>> = vec![Vec::new(); count];
    for e in cx.log.since(since) {
        if e.kind != EventKind::Hit {
            continue;
        }
        let time = e.time(rate);
        if time > cx.time {
            break;
        }
        for side in std::iter::once(e.a).chain(e.b) {
            if side.thing == id
                && let Some(h) = hits.get_mut(side.item as usize)
            {
                h.push((time, crate::things::balls::draw::hit_strength(e.speed)));
            }
        }
    }
    hits.into_iter().map(|h| heat_at(h, cx.time, heat)).collect()
}

/// The heat of the shape `cx` is drawing: every ball that hits it heats it.
pub fn shape_heat(cx: &DrawCx, heat: &SubHeat) -> f32 {
    let id = cx.thing.id;
    let rate = cx.world.steps_per_second;
    let cool = heat.cool_down.max(1e-3) as f64;
    let since = ((cx.time - COUNTED * cool) * rate as f64).floor().max(0.0) as u64;
    let hits = cx
        .log
        .since(since)
        .iter()
        .filter(|e| e.kind == EventKind::Hit && e.b.is_some_and(|b| b.thing == id))
        .map(|e| (e.time(rate), crate::things::balls::draw::hit_strength(e.speed)))
        .take_while(|(t, _)| *t <= cx.time);
    heat_at(hits, cx.time, heat)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_add_heat_that_cools_down() {
        let heat = SubHeat { per_hit: 1.0, cool_down: 2.0, ..SubHeat::default() };
        assert_eq!(heat_at([], 1.0, &heat), 0.0);
        let one = heat_at([(1.0, 1.0)], 1.0, &heat);
        assert!((one - 1.0).abs() < 1e-6);
        let later = heat_at([(1.0, 1.0)], 3.0, &heat);
        assert!((later - (-1.0f32).exp()).abs() < 1e-5, "a third after one cool-down: {later}");
        let two = heat_at([(1.0, 1.0), (1.0, 0.5)], 1.0, &heat);
        assert!((two - 1.5).abs() < 1e-6, "hits add up");
        assert_eq!(heat_at([(5.0, 1.0)], 1.0, &heat), 0.0, "a hit still to come adds nothing");
    }
}
