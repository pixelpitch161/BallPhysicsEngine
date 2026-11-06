//! Rules part 1's and part 5's scenes, where rules move the physics (their own goldens,
//! `tests/golden-rules*.txt`). Not in [`super::all`]: every other golden stays as it was.

use bpe_core::{BlockId, Registry, RuleId, Scene, Settings, ThingId, Value, Who};
use glam::Vec2;

use super::{BALLS, Builder, SHAPE};
use crate::{ShapeSettings, add_thing};

pub fn rule_scenes(reg: &Registry) -> Vec<(String, Scene)> {
    vec![
        ("Breakable bricks".into(), breakable_bricks(reg)),
        ("Logic lanes".into(), logic_lanes(reg)),
        ("Glide and back".into(), glide_and_back(reg)),
        ("Per-ball bounce".into(), per_ball_bounce(reg)),
        ("Split chain".into(), split_chain(reg)),
        ("Zones and signals".into(), zones_and_signals(reg)),
        ("Slow motion".into(), slow_motion(reg)),
    ]
}

/// One block of a rule: its kind, the block it follows and its settings.
fn block(
    scene: &mut Scene,
    reg: &Registry,
    rule: RuleId,
    kind: &str,
    after: Option<BlockId>,
    set: &[(&str, Value)],
) -> BlockId {
    let id = scene.add_block(reg, rule, kind, after).expect("a kit block");
    for (k, v) in set {
        scene.set_block_param(reg, rule, id, k, v.clone()).unwrap_or_else(|e| panic!("{kind} {k}: {e}"));
    }
    id
}

/// A rule whose blocks follow one another.
fn lane(scene: &mut Scene, reg: &Registry, name: &str, blocks: &[(&str, Vec<(&str, Value)>)]) -> RuleId {
    let rule = scene.add_rule(name);
    let mut last = None;
    for (kind, set) in blocks {
        last = Some(block(scene, reg, rule, kind, last, set));
    }
    rule
}

fn n(x: f64) -> Value {
    Value::Number(x)
}

fn c(name: &str) -> Value {
    Value::Choice(name.into())
}

fn t(text: &str) -> Value {
    Value::Text(text.into())
}

fn who(w: Who) -> Value {
    Value::Who(w)
}

/// A fast ball in a ring with eight bricks (small boxes tagged "brick") that break when it hits
/// them.
pub fn breakable_bricks(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Breakable bricks");
    b.shape(ShapeSettings::circle(508.0, 3.0));
    b.balls("Ball", Vec2::new(0.0, 200.0), |s| {
        s.radius = 16.0;
        s.velocity = Vec2::new(520.0, -780.0);
    });
    b.bounce(|s| s.bounciness = 1.0);
    let mut scene = b.finish();
    for k in 0..8 {
        let id = add_thing(&mut scene, reg, "shape", ThingId::ROOT).expect("a shape");
        let thing = scene.things.get_mut(&id).expect("just added");
        let (col, row) = ((k % 4) as f32, (k / 4) as f32);
        thing.transform.position = Vec2::new(-240.0 + col * 160.0, -260.0 + row * 110.0);
        thing.params = ShapeSettings::rectangle(110.0, 40.0, 3.0).to_params();
        thing.tags.insert("brick".into());
    }
    lane(
        &mut scene,
        reg,
        "Break bricks",
        &[("when.hit", vec![("whom", who(Who::Tag("brick".into())))]), ("do.break", vec![("who", who(Who::Other))])],
    );
    // Every third hit on the ring speeds the ball up a little.
    lane(
        &mut scene,
        reg,
        "Speed up",
        &[
            ("when.hit", vec![("whom", who(Who::Thing { thing: SHAPE, ball: None }))]),
            ("if.every", vec![("n", Value::Int(3))]),
            ("do.motion", vec![("amount", n(60.0))]),
        ],
    );
    scene
}

/// A ring of balls whose rules join lanes with Logic.
pub fn logic_lanes(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Logic lanes");
    b.shape(ShapeSettings::circle(508.0, 3.0));
    b.balls("Ring", Vec2::ZERO, |s| {
        s.radius = 12.0;
        s.ring(24, 200.0);
        s.velocity = Vec2::new(150.0, -100.0);
    });
    let mut scene = b.finish();
    // A beat then a hit within a quarter second: that ball speeds up.
    let rule = scene.add_rule("Beat then hit");
    let beat = scene.add_when(reg, rule, "when.beat").expect("a when");
    let hit = scene.add_when(reg, rule, "when.hit").expect("a when");
    let then = block(&mut scene, reg, rule, "logic", Some(beat), &[("mode", c("then")), ("within", n(0.25))]);
    scene.link(reg, rule, hit, then).expect("a link");
    block(&mut scene, reg, rule, "do.motion", Some(then), &[("amount", n(150.0))]);
    // Every fifth hit of each ball: it swells for a second.
    lane(
        &mut scene,
        reg,
        "Swell",
        &[
            ("when.hit", vec![]),
            ("if.every", vec![("n", Value::Int(5)), ("per", c("each_ball"))]),
            (
                "do.change",
                vec![
                    ("setting", t("radius")),
                    ("how", c("add")),
                    ("to_number", n(6.0)),
                    ("glide", n(0.2)),
                    ("lasts", c("for_a_time")),
                    ("seconds", n(1.0)),
                ],
            ),
        ],
    );
    // Every 1.5 s, if two of (the clock, a hit, a strong hit) come within 0.05 s: gravity flips
    // up for half a second.
    let rule = scene.add_rule("Flip");
    let clock = scene.add_when(reg, rule, "when.time").expect("a when");
    for (k, v) in [("repeat", Value::Bool(true)), ("every", n(1.5))] {
        scene.set_block_param(reg, rule, clock, k, v).expect("a setting");
    }
    let hit = scene.add_when(reg, rule, "when.hit").expect("a when");
    let strong = scene.add_when(reg, rule, "when.hit").expect("a when");
    scene.set_block_param(reg, rule, strong, "min_strength", n(400.0)).expect("a setting");
    let any = block(&mut scene, reg, rule, "logic", Some(clock), &[("mode", c("any_n")), ("within", n(0.05))]);
    scene.link(reg, rule, hit, any).expect("a link");
    scene.link(reg, rule, strong, any).expect("a link");
    block(
        &mut scene,
        reg,
        rule,
        "do.change",
        Some(any),
        &[
            ("who", who(Who::Thing { thing: ThingId::ROOT, ball: None })),
            ("setting", t("gravity")),
            ("to_pair", Value::Vec2([0.0, -400.0])),
            ("glide", n(0.25)),
            ("lasts", c("for_a_time")),
            ("seconds", n(0.5)),
        ],
    );
    scene
}

/// One ball whose bounciness swells every second and that is kicked off the wall now and then.
pub fn glide_and_back(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Glide and back");
    b.shape(ShapeSettings::circle(508.0, 3.0));
    b.balls("Ball", Vec2::new(0.0, -200.0), |s| {
        s.radius = 30.0;
        s.velocity = Vec2::new(400.0, 0.0);
    });
    let mut scene = b.finish();
    lane(
        &mut scene,
        reg,
        "Swell",
        &[
            ("when.time", vec![("repeat", Value::Bool(true)), ("every", n(1.0))]),
            (
                "do.change",
                vec![
                    ("who", who(Who::Thing { thing: BALLS, ball: None })),
                    ("card", c("bounce")),
                    ("setting", t("bounciness")),
                    ("how", c("multiply")),
                    ("times", n(1.2)),
                    ("glide", n(0.25)),
                    ("lasts", c("for_a_time")),
                    ("seconds", n(0.5)),
                ],
            ),
        ],
    );
    lane(
        &mut scene,
        reg,
        "Kick",
        &[
            ("when.hit", vec![]),
            ("if.cooldown", vec![("seconds", n(0.4))]),
            ("do.motion", vec![("mode", c("push")), ("way", c("away_from_the_hit")), ("amount", n(200.0))]),
        ],
    );
    scene
}

/// A ring of balls: the first half turn lively on their first hit, and the first six hard hits
/// break their balls.
pub fn per_ball_bounce(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Per-ball bounce");
    b.shape(ShapeSettings::circle(508.0, 3.0));
    b.balls("Ring", Vec2::ZERO, |s| {
        s.radius = 10.0;
        s.ring(24, 220.0);
        s.velocity = Vec2::new(0.0, -300.0);
    });
    b.bounce(|s| s.bounciness = 0.8);
    let mut scene = b.finish();
    lane(
        &mut scene,
        reg,
        "Lively",
        &[
            ("when.hit", vec![]),
            ("if.compare", vec![("value", t("ball")), ("test", c("under")), ("amount", n(12.5))]),
            ("do.change", vec![("card", c("bounce")), ("setting", t("bounciness")), ("to_number", n(1.02))]),
        ],
    );
    lane(
        &mut scene,
        reg,
        "Shatter",
        &[
            ("when.hit", vec![("min_strength", n(500.0))]),
            ("if.only", vec![("n", Value::Int(6))]),
            ("do.break", vec![]),
        ],
    );
    scene
}

/// Rules part 5: one ball that splits in three on each of its first hard hits (children too, up
/// to six splits), tags each ball that hits, counts the hits of tagged balls and, at 20, spawns a
/// ring of small balls from the middle.
pub fn split_chain(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Split chain");
    b.shape(ShapeSettings::circle(508.0, 3.0));
    b.balls("Ball", Vec2::new(0.0, -100.0), |s| {
        s.radius = 40.0;
        s.velocity = Vec2::new(350.0, -500.0);
    });
    b.bounce(|s| s.bounciness = 0.95);
    let mut scene = b.finish();
    lane(
        &mut scene,
        reg,
        "Split",
        &[
            ("when.hit", vec![("min_strength", n(250.0))]),
            ("if.only", vec![("n", Value::Int(6))]),
            ("do.split", vec![("into", Value::Int(3)), ("spread", n(50.0)), ("speed", n(1.05))]),
        ],
    );
    lane(&mut scene, reg, "Mark", &[("when.hit", vec![]), ("do.tag", vec![("tag", t("touched"))])]);
    lane(
        &mut scene,
        reg,
        "Count",
        &[("when.hit", vec![("who", who(Who::Tag("touched".into())))]), ("do.count", vec![])],
    );
    lane(
        &mut scene,
        reg,
        "Burst",
        &[
            ("when.count", vec![("of", c("a_counter")), ("amount", n(20.0))]),
            (
                "do.spawn",
                vec![
                    ("into", who(Who::Thing { thing: BALLS, ball: None })),
                    ("at", c("a_things_place")),
                    ("place", who(Who::Thing { thing: SHAPE, ball: None })),
                    ("count", Value::Int(8)),
                    ("way", c("all_round")),
                    ("speed", n(250.0)),
                    ("size", c("set_size")),
                    ("radius", n(8.0)),
                ],
            ),
        ],
    );
    scene
}

/// Rules part 5: a ring of balls falling through a zone balls pass through. Entering it sends a
/// signal that pushes the ball up; leaving it counts; a ball's speed going over 700 shrinks it a
/// while; every second hit of the ring by a ball removes it, and a removal makes the zone grow.
pub fn zones_and_signals(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Zones and signals");
    b.shape(ShapeSettings::circle(508.0, 3.0));
    b.balls("Ring", Vec2::new(0.0, -150.0), |s| {
        s.radius = 10.0;
        s.ring(12, 120.0);
        s.velocity = Vec2::new(120.0, 0.0);
    });
    let mut scene = b.finish();
    let zone = add_thing(&mut scene, reg, "shape", ThingId::ROOT).expect("a shape");
    let thing = scene.things.get_mut(&zone).expect("just added");
    thing.transform.position = Vec2::new(0.0, 250.0);
    thing.params = ShapeSettings { passes: true, ..ShapeSettings::circle(140.0, 3.0) }.to_params();
    let zone_who = who(Who::Thing { thing: zone, ball: None });
    lane(
        &mut scene,
        reg,
        "Enter",
        &[("when.enter", vec![("zone", zone_who.clone())]), ("do.signal", vec![("name", t("lift"))])],
    );
    lane(
        &mut scene,
        reg,
        "Lift",
        &[
            ("when.signal", vec![("name", t("lift"))]),
            ("do.motion", vec![("mode", c("push")), ("way", c("up")), ("amount", n(500.0))]),
        ],
    );
    lane(
        &mut scene,
        reg,
        "Leave",
        &[
            ("when.enter", vec![("zone", zone_who.clone()), ("way", c("leaves"))]),
            ("do.count", vec![("counter", t("left"))]),
        ],
    );
    lane(
        &mut scene,
        reg,
        "Fast",
        &[
            ("when.level", vec![("level", n(700.0))]),
            (
                "do.change",
                vec![
                    ("setting", t("radius")),
                    ("how", c("multiply")),
                    ("times", n(0.6)),
                    ("lasts", c("for_a_time")),
                    ("seconds", n(0.5)),
                ],
            ),
        ],
    );
    lane(
        &mut scene,
        reg,
        "Out",
        &[
            ("when.hit", vec![("whom", who(Who::Thing { thing: SHAPE, ball: None }))]),
            ("if.every", vec![("n", Value::Int(2)), ("per", c("each_ball"))]),
            ("if.counter", vec![("counter", t("left")), ("test", c("over")), ("amount", n(2.0))]),
            ("do.break", vec![]),
        ],
    );
    lane(
        &mut scene,
        reg,
        "Grow",
        &[
            ("when.removed", vec![("what", c("broken"))]),
            (
                "do.change",
                vec![
                    ("who", zone_who),
                    ("setting", t("size")),
                    ("how", c("add")),
                    ("to_number", n(10.0)),
                    ("lasts", c("for_good")),
                ],
            ),
        ],
    );
    scene
}

/// Rules part 4's scenes, where health moves the physics (their own goldens,
/// `tests/golden-rules-health*.txt`).
pub fn health_scenes(reg: &Registry) -> Vec<(String, Scene)> {
    vec![("Health bricks".into(), health_bricks(reg)), ("Health group".into(), health_group(reg))]
}

/// Gives `thing` a Health card with `set` on it.
fn with_health(scene: &mut Scene, reg: &Registry, thing: ThingId, set: &[(&str, Value)]) {
    crate::fill_slot(scene, reg, thing, bpe_core::SlotId::Health).expect("a Health card fits");
    let card = scene.card_mut(thing, bpe_core::SlotId::Health).expect("just added");
    for (k, v) in set {
        card.params.set(k, v.clone());
    }
}

/// Breakable bricks with health: each brick takes three hits, and every hit that takes a
/// brick's health speeds the ball up a little.
pub fn health_bricks(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Health bricks");
    b.shape(ShapeSettings::circle(508.0, 3.0));
    b.balls("Ball", Vec2::new(0.0, 200.0), |s| {
        s.radius = 16.0;
        s.velocity = Vec2::new(520.0, -780.0);
    });
    b.bounce(|s| s.bounciness = 1.0);
    let mut scene = b.finish();
    for k in 0..8 {
        let id = add_thing(&mut scene, reg, "shape", ThingId::ROOT).expect("a shape");
        let thing = scene.things.get_mut(&id).expect("just added");
        let (col, row) = ((k % 4) as f32, (k / 4) as f32);
        thing.transform.position = Vec2::new(-240.0 + col * 160.0, -260.0 + row * 110.0);
        thing.params = ShapeSettings::rectangle(110.0, 40.0, 3.0).to_params();
        thing.tags.insert("brick".into());
        with_health(&mut scene, reg, id, &[("start", n(1.0 + (k % 3) as f64))]);
    }
    lane(
        &mut scene,
        reg,
        "Faster on bricks",
        &[
            ("when.health", vec![("who", who(Who::Tag("brick".into())))]),
            ("do.motion", vec![("who", who(Who::Other)), ("amount", n(40.0))]),
        ],
    );
    scene
}

/// A ring of balls whose health starts from 1 to 4 across them, taken by how hard they hit; each
/// shrinks as its health goes, and a ball under half its health gets a kick.
pub fn health_group(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Health group");
    b.shape(ShapeSettings::circle(508.0, 3.0));
    b.balls("Ring", Vec2::ZERO, |s| {
        s.radius = 14.0;
        s.ring(24, 200.0);
        s.velocity = Vec2::new(150.0, -100.0);
    });
    let mut scene = b.finish();
    let ring = scene.things.values().find(|t| t.kind == "balls").expect("the ring").id;
    let p = &mut scene.things.get_mut(&ring).expect("the ring").params;
    p.set("radius~to", n(6.0));
    p.set("radius~by", c("health"));
    with_health(
        &mut scene,
        reg,
        ring,
        &[
            ("start", n(1.0)),
            ("start~to", n(4.0)),
            ("start~by", c("number")),
            ("takes", c("by_strength")),
            ("factor", n(0.002)),
        ],
    );
    lane(
        &mut scene,
        reg,
        "Kick the weak",
        &[
            ("when.health", vec![]),
            ("if.health", vec![("measure", c("percent")), ("amount", n(50.0))]),
            ("do.motion", vec![("mode", c("push")), ("amount", n(120.0))]),
        ],
    );
    scene
}

/// Rules part 6: six balls in a spinning hexagon. Every third hit on it slows the world to 0.3×
/// for a second (gliding in and out), flashes the ball and plays a note on the hexagon; the
/// spin slows with the balls.
pub fn slow_motion(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Slow motion");
    let mut hexagon = ShapeSettings::circle(480.0, 4.0);
    hexagon.start = crate::things::shape::ShapeStart::Polygon;
    hexagon.sides = 6;
    hexagon.spin = 40.0;
    b.shape(hexagon);
    b.balls("Six", Vec2::ZERO, |s| {
        s.radius = 18.0;
        s.ring(6, 150.0);
        s.velocity = Vec2::new(380.0, -520.0);
    });
    b.bounce(|s| s.bounciness = 1.0);
    let mut scene = b.finish();
    let rule = scene.add_rule("Slow on every third");
    let hit = scene.add_when(reg, rule, "when.hit").expect("a when");
    let every = block(&mut scene, reg, rule, "if.every", Some(hit), &[("n", Value::Int(3))]);
    let slow = [("speed", n(0.3)), ("glide", n(0.2)), ("lasts", c("for_a_time")), ("seconds", n(1.0))];
    block(&mut scene, reg, rule, "do.slow", Some(every), &slow);
    block(&mut scene, reg, rule, "do.flash", Some(every), &[]);
    let shape = who(Who::Thing { thing: SHAPE, ball: None });
    block(&mut scene, reg, rule, "do.note", Some(every), &[("who", shape), ("note", Value::Int(67))]);
    scene
}
