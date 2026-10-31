//! The stage 5 scenes: the stage 2 scenes dressed up with looks, trails, particles and shaders.
//! Each one simulates exactly like its stage 2 twin, which the golden tests check, since
//! nothing here touches physics.

use bpe_core::{Blend, Color, Gradient, Params, Registry, Scene, Settings, SlotId, ThingId, Value};

use crate::{
    BallFill, BallShape, BurstPreset, ClassicLook, HitBurst, StreakTrail, StyledLook, TrailSource, paint::Paint,
    scenes::stage2,
};

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
            ((*name).to_owned(), scene, twin_name)
        })
        .collect()
}

/// The five stage 5 scenes, in order, each with its name.
pub fn stage5(reg: &Registry) -> Vec<(String, Scene)> {
    stage5_with_twins(reg).into_iter().map(|(name, scene, _)| (name, scene)).collect()
}

/// A new module linked onto one thing. Asked of Scene settings for a slot it doesn't have
/// (a burst or a trail), the module is shared by every thing that has the slot instead, as if
/// they all used one scene-wide module.
fn own_module(scene: &mut Scene, reg: &Registry, thing: ThingId, kind: &str, name: &str, params: Params) {
    let m = scene.add_module(reg, kind, name).expect("kit kinds are registered");
    scene.library.modules.get_mut(&m).expect("just added").params = params;
    let slot = scene.library.modules[&m].slot;
    let root_has = reg.thing("scene").is_some_and(|k| k.slots().contains(&slot));
    if thing != ThingId::ROOT || root_has {
        scene.link(reg, thing, slot, m).expect("the kind has the slot");
        return;
    }
    let takers: Vec<ThingId> = scene
        .order
        .iter()
        .copied()
        .filter(|id| *id != ThingId::ROOT)
        .filter(|id| reg.thing(&scene.things[id].kind).is_some_and(|k| k.slots().contains(&slot)))
        .collect();
    for id in takers {
        scene.link(reg, id, slot, m).expect("the kind has the slot");
    }
}

/// The ball's look module, replaced by a styled one.
fn style_balls(scene: &mut Scene, reg: &Registry, look: StyledLook) {
    let m = bpe_core::resolve_slot(scene, reg, BALLS, SlotId::Look, &mut Vec::new()).expect("a look").module;
    let old = scene.library.modules[&m].clone();
    // Keep the twin's colours.
    let look = StyledLook { fill: ClassicLook::from_params(&old.params).fill, ..look };
    own_module(scene, reg, BALLS, "look.styled", &old.name, look.to_params());
    if scene.users_of(m).is_empty() {
        scene.library.modules.remove(&m);
    }
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
        StyledLook {
            glow: 0.6,
            glow_size: 1.4,
            pattern: Paint::solid(Color::rgb(0xff, 0xbf, 0x26)),
            ..StyledLook::default()
        },
    );
    own_module(scene, reg, BALLS, "shader.lava", "Lava", params([number("heat", 1.8), number("scale", 2.0)]));
    let trail = StreakTrail {
        seconds: 0.7,
        width: 0.8,
        brightness: 0.8,
        source: TrailSource::Own,
        colour: Paint { colours: Gradient::solid(Color::rgb(0xff, 0x70, 0x10)), ..StreakTrail::default().colour },
        ..StreakTrail::default()
    };
    own_module(scene, reg, BALLS, "trail.streak", "Lava streak", trail.to_params());
}

/// The reel block raining confetti, each tiny ball with a short streak.
fn confetti_drop(reg: &Registry, scene: &mut Scene) {
    let burst = HitBurst {
        preset: BurstPreset::Confetti,
        count: 5,
        speed: 260.0,
        size: 4.0,
        life: 1.2,
        gravity: 500.0,
        drag: 1.5,
        // Confetti covers what is behind it, as it always has.
        blend: Blend::Normal,
        ..HitBurst::default()
    };
    own_module(scene, reg, ThingId::ROOT, "particles.burst", "Confetti", burst.to_params());
    let trail = StreakTrail { seconds: 0.2, width: 0.6, brightness: 0.35, ..StreakTrail::default() };
    own_module(scene, reg, ThingId::ROOT, "trail.streak", "Short streaks", trail.to_params());
}

/// The box drop with sparks flying off every hit and the picture flaring with them.
fn spark_block(reg: &Registry, scene: &mut Scene) {
    let burst = HitBurst { count: 10, speed: 420.0, size: 2.5, life: 0.5, least_speed: 120.0, ..HitBurst::default() };
    own_module(scene, reg, ThingId::ROOT, "particles.burst", "Sparks", burst.to_params());
    own_module(scene, reg, BALLS, "shader.rainbow_flow", "Rainbow blocks", params([number("bands", 1.5)]));
    own_module(scene, reg, ThingId::ROOT, "shader.hit_glow", "Flare", params([number("punch", 0.6)]));
}

/// The spinning hexagon seen through a kaleidoscope, with hexagonal balls.
fn kaleido_hexagon(reg: &Registry, scene: &mut Scene) {
    style_balls(
        scene,
        reg,
        StyledLook {
            ball_shape: BallShape::Hexagon,
            finish: BallFill::Slices,
            pattern: Paint::solid(Color::rgb(0x10, 0x10, 0x30)),
            outline: 2.0,
            outline_paint: Paint::solid(Color::rgb(0xff, 0xff, 0xff)),
            rainbow: 0.15,
            spin: 90.0,
            ..StyledLook::default()
        },
    );
    let trail = StreakTrail { seconds: 0.4, width: 0.5, brightness: 0.6, ..StreakTrail::default() };
    own_module(scene, reg, ThingId::ROOT, "trail.streak", "Streaks", trail.to_params());
    let kaleido = params([number("spin", 0.02), number("pulse", 0.3)]);
    own_module(scene, reg, ThingId::ROOT, "shader.kaleidoscope", "Kaleidoscope", kaleido);
}

/// The shrinking ring with a neon ball that stretches as it speeds up, rings on every hit and
/// ghosts of the picture flying into the screen.
fn neon_ring(reg: &Registry, scene: &mut Scene) {
    style_balls(
        scene,
        reg,
        StyledLook {
            glow: 1.2,
            glow_size: 1.6,
            outline: 4.0,
            outline_paint: Paint::solid(Color::rgb(0x40, 0xff, 0xff)),
            stretch: 0.6,
            ..StyledLook::default()
        },
    );
    let burst = HitBurst {
        preset: BurstPreset::Rings,
        count: 1,
        size: 90.0,
        life: 0.5,
        gravity: 0.0,
        drag: 0.0,
        follow_impact: false,
        ..HitBurst::default()
    };
    own_module(scene, reg, ThingId::ROOT, "particles.burst", "Rings", burst.to_params());
    let trail = StreakTrail { seconds: 0.5, width: 1.0, brightness: 0.5, ..StreakTrail::default() };
    own_module(scene, reg, BALLS, "trail.streak", "Neon streak", trail.to_params());
    let echo = params([number("fade", 0.85), number("zoom", 1.03), number("hue", 0.01)]);
    own_module(scene, reg, ThingId::ROOT, "shader.echo", "Echo", echo);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry;

    #[test]
    fn stage5_scenes_resolve_round_trip_and_match_their_files() {
        let reg = registry();
        let scenes = stage5(&reg);
        let names: Vec<&str> = scenes.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["Lava ball", "Confetti drop", "Spark block", "Kaleido hexagon", "Neon ring"]);
        for (name, scene) in &scenes {
            assert_eq!(&scene.name, name);
            assert!(bpe_core::resolve(scene, &reg).problems.is_empty(), "{name}");
            let text = bpe_core::save(scene);
            assert_eq!(&bpe_core::load(&text, &reg).unwrap(), scene, "{name}");
            let file = format!(
                "{}/../../scenes/next/{}.ron",
                env!("CARGO_MANIFEST_DIR"),
                name.to_lowercase().replace(' ', "-")
            );
            if std::env::var_os("BPE_BLESS").is_some() {
                std::fs::write(&file, &text).unwrap();
                continue;
            }
            let shipped =
                std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{file}: {e} (run with BPE_BLESS=1)"));
            assert_eq!(bpe_core::load(&shipped, &reg).unwrap(), *scene, "{name}: scenes/next is stale, BPE_BLESS=1");
        }
    }

    #[test]
    fn every_stage5_scene_uses_something_from_stage5() {
        let reg = registry();
        for (name, scene) in stage5(&reg) {
            let kinds: Vec<&str> = scene.library.modules.values().map(|m| m.kind.as_str()).collect();
            let new = kinds.iter().any(|k| {
                k.starts_with("shader.")
                    || k.starts_with("trail.")
                    || k.starts_with("particles.")
                    || *k == "look.styled"
            });
            assert!(new, "{name}: {kinds:?}");
        }
    }
}
