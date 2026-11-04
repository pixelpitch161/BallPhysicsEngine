//! The stage 5 scenes: the stage 2 scenes dressed up with looks, trails, particles and shaders.
//! Each one simulates exactly like its stage 2 twin, which the golden tests check, since
//! nothing here touches physics.

use bpe_core::{Blend, Color, Gradient, Params, Registry, Scene, Settings, SlotId, ThingId, Value};

use crate::modules::look::layers::particles::{MadeOf, ParticlesLayer};
use crate::modules::look::layers::trail::{TrailLayer, TrailSource};
use crate::paint::{InsideFrom, InsidePaint, Paint, Pattern};
use crate::{BallShape, BasicLook, Style, scenes::stage2};

const BALLS: ThingId = ThingId(2);

/// How a stage 2 scene gets dressed up.
type Dress = fn(&Registry, &mut Scene);

/// The five stage 5 scenes, in order, each with its name and the stage 2 scene it was built on.
pub fn stage5_with_twins(reg: &Registry) -> Vec<(String, Scene, String)> {
    let twins = stage2(reg);
    let dress: [(&str, usize, Dress); 5] = [
        ("Lava ball", 0, lava_ball),
        ("Confetti drop", 1, confetti_drop),
        ("Spark block", 2, spark_block),
        ("Kaleido hexagon", 3, kaleido_hexagon),
        ("Neon ring", 4, neon_ring),
    ];
    dress
        .iter()
        .map(|(name, twin, build)| {
            let (twin_name, mut scene) = twins[*twin].clone();
            scene.name = (*name).into();
            build(reg, &mut scene);
            super::classic_cards(&mut scene);
            ((*name).to_owned(), scene, twin_name)
        })
        .collect()
}

/// The five stage 5 scenes, in order, each with its name.
pub fn stage5(reg: &Registry) -> Vec<(String, Scene)> {
    stage5_with_twins(reg).into_iter().map(|(name, scene, _)| (name, scene)).collect()
}

/// A new card on one thing. Asked of Scene settings for a slot it doesn't have (a burst or a
/// trail), every thing that has the slot gets a copy instead.
fn own_module(scene: &mut Scene, reg: &Registry, thing: ThingId, kind: &str, params: Params) {
    if super::put_old_card(scene, thing, kind, &params) {
        return;
    }
    let slot = reg.module(kind).expect("kit kinds are registered").slot();
    let root_has = reg.thing("scene").is_some_and(|k| k.slots().contains(&slot));
    let has_slot = |id: &ThingId| reg.thing(&scene.things[id].kind).is_some_and(|k| k.slots().contains(&slot));
    let takers: Vec<ThingId> = if thing != ThingId::ROOT || root_has {
        // A card a kind no longer has (a Shader card on balls, now a layer) is left out.
        [thing].into_iter().filter(has_slot).collect()
    } else {
        scene
            .order
            .iter()
            .copied()
            .filter(|id| *id != ThingId::ROOT)
            .filter(|id| reg.thing(&scene.things[id].kind).is_some_and(|k| k.slots().contains(&slot)))
            .collect()
    };
    for id in takers {
        super::put_card(scene, reg, id, slot, kind);
        scene.things.get_mut(&id).and_then(|t| t.slots.get_mut(&slot)).expect("just added").params = params.clone();
    }
}

/// The ball's look card, replaced by a styled one: `look` with the twin's colours, then
/// finished by `f`, which sees them.
fn style_balls(scene: &mut Scene, reg: &Registry, look: BasicLook, f: impl FnOnce(&mut BasicLook)) {
    // Keep the twin's colours.
    let mut look = BasicLook { fill: super::ball_fill(&scene.things[&BALLS].slots[&SlotId::Look]), ..look };
    f(&mut look);
    own_module(scene, reg, BALLS, "look.basic", look.to_params());
}

fn number(key: &str, x: f64) -> (String, Value) {
    (key.to_owned(), Value::Number(x))
}

fn params(pairs: impl IntoIterator<Item = (String, Value)>) -> Params {
    let mut p = Params::default();
    for (k, v) in pairs {
        p.set(&k, v);
    }
    p
}

/// One ball, molten: a lava material, a hot halo and an orange streak.
fn lava_ball(reg: &Registry, scene: &mut Scene) {
    style_balls(
        scene,
        reg,
        BasicLook { style: Style::Shiny, glow: 0.6, glow_size: 1.4, ..super::classic_look() },
        |_| {},
    );
    own_module(scene, reg, BALLS, "shader.lava", params([number("heat", 1.8), number("scale", 2.0)]));
    let trail = TrailLayer {
        seconds: 0.7,
        width: 0.8,
        brightness: 0.8,
        source: TrailSource::Own,
        colour: Paint { colours: Gradient::solid(Color::rgb(0xff, 0x70, 0x10)), ..TrailLayer::default().colour },
        ..TrailLayer::default()
    };
    own_module(scene, reg, BALLS, "trail.streak", trail.to_params());
}

/// The reel block raining confetti, each tiny ball with a short streak.
fn confetti_drop(reg: &Registry, scene: &mut Scene) {
    let burst = ParticlesLayer {
        made_of: MadeOf::Confetti,
        count: 5,
        speed: 260.0,
        size: 4.0,
        life: 1.2,
        gravity: 500.0,
        drag: 1.5,
        // Confetti covers what is behind it, as it always has.
        blend: Blend::Normal,
        ..ParticlesLayer::default()
    };
    own_module(scene, reg, ThingId::ROOT, "particles.burst", burst.to_params());
    let trail = TrailLayer { seconds: 0.2, width: 0.6, brightness: 0.35, ..TrailLayer::default() };
    own_module(scene, reg, ThingId::ROOT, "trail.streak", trail.to_params());
}

/// The box drop with sparks flying off every hit and the picture flaring with them.
fn spark_block(reg: &Registry, scene: &mut Scene) {
    let burst = ParticlesLayer {
        count: 10,
        speed: 420.0,
        size: 2.5,
        life: 0.5,
        least_speed: 120.0,
        ..ParticlesLayer::default()
    };
    own_module(scene, reg, ThingId::ROOT, "particles.burst", burst.to_params());
    own_module(scene, reg, BALLS, "shader.rainbow_flow", params([number("bands", 1.5)]));
    own_module(scene, reg, ThingId::ROOT, "shader.hit_glow", params([number("punch", 0.6)]));
}

/// The spinning hexagon seen through a kaleidoscope, with hexagonal balls.
fn kaleido_hexagon(reg: &Registry, scene: &mut Scene) {
    style_balls(
        scene,
        reg,
        BasicLook {
            ball_shape: BallShape::Hexagon,
            style: Style::Flat,
            outline: 2.0,
            outline_paint: Paint::solid(Color::rgb(0xff, 0xff, 0xff)),
            rainbow: 0.15,
            spin: 90.0,
            ..super::classic_look()
        },
        // Slices of the ball's own colour and navy. An inside mask's Size counts half as many
        // repeats as the old Pattern size did, so the old 4 is 2; the look's Spin still turns
        // the shape and the slices together.
        |l| {
            let colours = Gradient::even(&[l.fill.colours.first(), Color::rgb(0x10, 0x10, 0x30)]);
            l.inside = InsidePaint {
                pattern: Pattern::Slices,
                from: InsideFrom::OwnGradient,
                colours,
                // The old pattern started at the shape's own turn; Inside's default Angle is 90.
                angle: 0.0,
                size: 2.0,
                ..InsidePaint::default()
            };
        },
    );
    let trail = TrailLayer { seconds: 0.4, width: 0.5, brightness: 0.6, ..TrailLayer::default() };
    own_module(scene, reg, ThingId::ROOT, "trail.streak", trail.to_params());
    let kaleido = params([number("spin", 0.02), number("pulse", 0.3)]);
    own_module(scene, reg, ThingId::ROOT, "shader.kaleidoscope", kaleido);
}

/// The shrinking ring with a neon ball that stretches as it speeds up, rings on every hit and
/// ghosts of the picture flying into the screen.
fn neon_ring(reg: &Registry, scene: &mut Scene) {
    style_balls(
        scene,
        reg,
        BasicLook {
            style: Style::Shiny,
            glow: 1.2,
            glow_size: 1.6,
            outline: 4.0,
            outline_paint: Paint::solid(Color::rgb(0x40, 0xff, 0xff)),
            stretch: 0.6,
            ..super::classic_look()
        },
        |_| {},
    );
    let burst = ParticlesLayer {
        made_of: MadeOf::Rings,
        count: 1,
        size: 90.0,
        life: 0.5,
        gravity: 0.0,
        drag: 0.0,
        follow_impact: false,
        ..ParticlesLayer::default()
    };
    own_module(scene, reg, ThingId::ROOT, "particles.burst", burst.to_params());
    let trail = TrailLayer { seconds: 0.5, width: 1.0, brightness: 0.5, ..TrailLayer::default() };
    own_module(scene, reg, BALLS, "trail.streak", trail.to_params());
    let echo = params([number("fade", 0.85), number("zoom", 1.03), number("hue", 0.01)]);
    own_module(scene, reg, ThingId::ROOT, "shader.echo", echo);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry;

    #[test]
    fn stage5_scenes_resolve_and_round_trip() {
        let reg = registry();
        let scenes = stage5(&reg);
        let names: Vec<&str> = scenes.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["Lava ball", "Confetti drop", "Spark block", "Kaleido hexagon", "Neon ring"]);
        for (name, scene) in &scenes {
            assert_eq!(&scene.name, name);
            assert!(bpe_core::resolve(scene, &reg).problems.is_empty(), "{name}");
            let text = bpe_core::save(scene);
            assert_eq!(&bpe_core::load(&text, &reg).unwrap(), scene, "{name}");
        }
    }

    #[test]
    fn every_stage5_scene_uses_something_from_stage5() {
        let reg = registry();
        for (name, scene) in stage5(&reg) {
            let cards: Vec<&bpe_core::Slot> = scene.things.values().flat_map(|t| t.slots.values()).collect();
            let kinds: Vec<&str> = cards.iter().map(|c| c.kind.as_str()).collect();
            let styled =
                cards.iter().any(|c| c.kind == "look.basic" && !BasicLook::from_params(&c.params).is_classic_ball());
            let around = cards.iter().flat_map(|c| &c.layers).any(|l| {
                use crate::modules::look::layers::{PARTICLES, TRAIL};
                l.kind == TRAIL || l.kind == PARTICLES
            });
            let new = styled || around || kinds.iter().any(|k| k.starts_with("shader."));
            assert!(new, "{name}: {kinds:?}");
        }
    }
}
