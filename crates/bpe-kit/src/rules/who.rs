//! Who a block points at, read against the world.

use bpe_core::{ThingId, Who};
use bpe_runtime::rules::WHOLE;
use bpe_runtime::{Firing, Party, StageCx, World};
use glam::Vec2;

/// A ball's number in its thing, counted from 1, when `party` is a live ball's row.
pub fn ball_number(world: &World, party: Party) -> Option<u32> {
    let b = world.bodies.get(&party.thing)?;
    let i = party.item as usize;
    (i < b.len()).then(|| b.number[i] + 1)
}

/// Whether `party` is a ball (a row of a thing with balls).
pub fn is_ball(world: &World, party: Party) -> bool {
    world.bodies.get(&party.thing).is_some_and(|b| (party.item as usize) < b.len())
}

/// In a When block (and If: Is): whether `party` counts as `who`. That, Other and Group count as
/// anything there. A ball has its thing's tags and those a rule gave it alone.
pub fn matches(cx: &StageCx, who: &Who, party: Party) -> bool {
    let world = cx.world;
    match who {
        Who::Tag(tag) if is_ball(world, party) && cx.ball_has_tag(party.thing, party.item, tag) => true,
        _ => matches_thing(world, who, party),
    }
}

/// [`matches`] by the thing's own tags only (a ball's own tags aren't known here).
pub fn matches_thing(world: &World, who: &Who, party: Party) -> bool {
    match who {
        Who::That | Who::Other | Who::Group | Who::Anything => true,
        Who::Thing { thing, ball } => {
            party.thing == *thing && ball.is_none_or(|n| is_ball(world, party) && ball_number(world, party) == Some(n))
        }
        Who::Tag(tag) => world.tags.get(&party.thing).is_some_and(|t| t.contains(tag)),
        Who::Kind(kind) => world.kind_of(party.thing) == Some(kind.as_str()),
    }
}

/// A thing, or one ball of it (its body row).
pub type Target = (ThingId, Option<u32>);

fn side(world: &World, party: Option<Party>) -> Option<Target> {
    let p = party?;
    Some((p.thing, is_ball(world, p).then_some(p.item)))
}

/// In an If or Do block: what `who` names for `firing`. That is the firing's subject (its ball
/// when it is one), Other the other side, Group the subject's whole thing, a thing (or one of its
/// balls, by number) by id, and Tag, Kind and Anything every built thing that matches, whole, in
/// id order (never the scene settings). Broken things are left out.
pub fn targets(cx: &StageCx, who: &Who, firing: &Firing) -> Vec<Target> {
    let world = cx.world;
    let out: Vec<Target> = match who {
        Who::That => side(world, firing.subject).into_iter().collect(),
        Who::Other => side(world, firing.other).into_iter().collect(),
        Who::Group => firing.subject.map(|p| (p.thing, None)).into_iter().collect(),
        Who::Thing { thing, ball: None } => vec![(*thing, None)],
        Who::Thing { thing, ball: Some(n) } => world
            .bodies
            .get(thing)
            .and_then(|b| b.row_of(n.saturating_sub(1)).filter(|&r| *n >= 1 && b.alive[r]))
            .map(|r| (*thing, Some(r as u32)))
            .into_iter()
            .collect(),
        Who::Tag(_) | Who::Kind(_) | Who::Anything => {
            let mut out: Vec<Target> = world
                .names
                .keys()
                .filter(|&&t| t != ThingId::ROOT && matches_thing(world, who, Party { thing: t, item: WHOLE }))
                .map(|&t| (t, None))
                .collect();
            // Balls a rule tagged alone, their thing not tagged.
            if let Who::Tag(tag) = who {
                for (&(thing, row), tags) in cx.ball_tags {
                    let live = world.bodies.get(&thing).is_some_and(|b| b.alive.get(row as usize) == Some(&true));
                    if live && tags.contains(tag) && !out.contains(&(thing, None)) {
                        out.push((thing, Some(row)));
                    }
                }
            }
            out
        }
    };
    out.into_iter().filter(|(t, _)| world.names.contains_key(t) && !cx.is_broken(*t)).collect()
}

/// Where a target is: a ball's place; for a thing with balls its first live ball's; else its
/// pose, else its anchor.
pub fn place_of(world: &World, (thing, row): Target) -> Vec2 {
    if let Some(b) = world.bodies.get(&thing) {
        if let Some(r) = row.map(|r| r as usize).filter(|&r| r < b.len()) {
            return b.position[r];
        }
        if let Some(r) = b.alive.iter().position(|&a| a) {
            return b.position[r];
        }
    }
    match world.poses.get(&thing) {
        Some(p) => p.position,
        None => world.anchor(thing).position,
    }
}
