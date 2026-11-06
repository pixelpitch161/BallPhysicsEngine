//! A rig's fixtures: the lights under it in the tree, and what every rig above a light does to
//! each of its lamps.

use bpe_core::{Resolved, Scene, Settings, ThingId};
use bpe_gfx::DrawCx;

use super::pattern::{self, Clock, Drive};
use super::{Across, RIG, RigSettings, StepsOn};
use crate::modules::light_scenes::ScenesNow;
use crate::things::light::{LIGHT, LightSettings, RIDES, draw::places};
use crate::things::scene::SceneSettings;

/// Whether `id` is a rig.
pub fn is_rig(tree: &Scene, id: ThingId) -> bool {
    tree.things.get(&id).is_some_and(|t| t.kind == RIG)
}

/// The rigs above `id`, nearest first.
pub fn rigs_of(tree: &Scene, id: ThingId) -> Vec<ThingId> {
    tree.ancestors(id).into_iter().filter(|&a| is_rig(tree, a)).collect()
}

/// What a light rides: its nearest ancestor that isn't a rig (rigs only group lights), when that
/// isn't the scene itself.
pub fn ridden(tree: &Scene, id: ThingId) -> Option<ThingId> {
    tree.ancestors(id).into_iter().find(|&a| !is_rig(tree, a)).filter(|&a| a != ThingId::ROOT)
}

/// Whether the light `id` rides a Balls, Emitter or Shape thing.
pub fn rides_anything(tree: &Scene, id: ThingId) -> bool {
    ridden(tree, id).and_then(|p| tree.things.get(&p)).is_some_and(|p| RIDES.contains(&p.kind.as_str()))
}

/// The lights a rig drives: every enabled Light thing under it, in tree order.
pub fn fixtures(tree: &Scene, resolved: &Resolved, rig: ThingId) -> Vec<ThingId> {
    tree.descendants(rig).into_iter().filter(|id| resolved.things.get(id).is_some_and(|r| r.kind == LIGHT)).collect()
}

/// How many lamps the light `id` shines from now (one per ball when it rides every ball).
fn lamp_count(cx: &DrawCx, id: ThingId) -> usize {
    let (Some(thing), Some(resolved)) = (cx.tree.things.get(&id), cx.scene.things.get(&id)) else { return 0 };
    let other = DrawCx { thing, resolved, ..*cx };
    places(&other, &LightSettings::from_params(&resolved.params)).len()
}

/// Where a rig's pattern is at the moment drawn.
fn clock(cx: &DrawCx, rig: ThingId, s: &RigSettings) -> Clock {
    let scene = cx.scene.things.get(&ThingId::ROOT).map(|r| SceneSettings::from_params(&r.params)).unwrap_or_default();
    if s.steps_on != StepsOn::Hits {
        return pattern::clock_in_time(s, &scene, cx.time);
    }
    let heard = cx.world.matching(&s.heard_from, rig);
    let hits = pattern::hits_heard(cx.log, cx.world.steps_per_second, &heard, cx.time, s.least_gap.max(0.0) as f64);
    pattern::clock_on_hits(s, &hits, cx.time)
}

/// What every rig above the light being drawn does to each of its `lamps` lamps, nearest rig
/// first and stacked. Every lamp is left as it is when the light is in no rig.
pub fn drives(cx: &DrawCx, scenes: &ScenesNow, lamps: usize) -> Vec<Drive> {
    let me = cx.resolved.id;
    let mut out = vec![Drive::NONE; lamps];
    for rig in rigs_of(cx.tree, me) {
        let Some(r) = cx.scene.things.get(&rig) else { continue };
        let s = RigSettings::from_params(&scenes.params(rig, &r.params));
        let all = fixtures(cx.tree, cx.scene, rig);
        let Some(at) = all.iter().position(|&f| f == me) else { continue };
        let clock = clock(cx, rig, &s);
        match s.across {
            Across::Lights => {
                let d = pattern::drive(&s, clock, at, all.len());
                for o in &mut out {
                    *o = o.then(d);
                }
            }
            Across::EveryLamp => {
                let before: usize = all[..at].iter().map(|&f| lamp_count(cx, f)).sum();
                let after: usize = all[at + 1..].iter().map(|&f| lamp_count(cx, f)).sum();
                let n = before + lamps + after;
                for (j, o) in out.iter_mut().enumerate() {
                    *o = o.then(pattern::drive(&s, clock, before + j, n));
                }
            }
        }
    }
    out
}
