//! The scenes the stages ship with: the new-scene default and the ones that exercise every
//! setting. They double as the determinism test scenes.

#[path = "scenes_stage5.rs"]
mod stage5;
pub use stage5::{stage5, stage5_with_twins};

use bpe_core::{
    Address, Attach, Blend, Choice, Color, Ease, Gradient, InstrumentRef, InstrumentSource, ModuleGraph, ModuleId,
    ModuleKind, MusicKey, Owner, Params, Progression, ProgressionRef, PulseSource, Registry, Scene, SettingValue,
    Settings, SlotId, SlotLink, Target, ThingId, Value,
};
use bpe_graph::{Catalog, Graph, Link, Param};
use bpe_mix::Instrument;
use bpe_vm::{Role, catalog};
use glam::Vec2;
use std::collections::BTreeMap;

use crate::graphs::module_graph;
use crate::modules::sound::{Order, PlaysWhen};
use crate::paint::{Follow, InsideFrom, InsidePaint, Paint, Pattern, PlaceShape, Repeat};
use crate::{
    Align, Appear, ArenaSettings, ArenaShape, Arrangement, BallsSettings, BasicBounce, BasicLook, BasicMotion,
    BeatSync, BurstSource, CanvasSettings, CanvasStyle, DrumSound, EmitterSettings, FieldKindChoice, FieldSettings,
    FieldShape, Flocking, Font, HitBurst, LiquidSettings, MarksBrushes, MelodySound, MovePath, NoteSound,
    ObstacleSettings, ObstacleShape, Piece, Pinned, RulesKind, SceneSettings, SoftSettings, SoftShape, SplatOnto,
    SplatShape, SplatSkin, StartsAs, StreakTrail, Style, SwarmLook, SwarmSettings, TextSettings, TrailSource, Turn,
    WidthBy, new_scene, recipes,
};

const ARENA: ThingId = ThingId(1);
const BALLS: ThingId = ThingId(2);

fn build_all(reg: &Registry, builders: &[fn(&Registry) -> Scene]) -> Vec<(String, Scene)> {
    builders
        .iter()
        .map(|build| {
            let scene = build(reg);
            (scene.name.clone(), scene)
        })
        .collect()
}

/// The five stage 2 scenes, in order, each with its name.
pub fn stage2(reg: &Registry) -> Vec<(String, Scene)> {
    build_all(reg, &[one_ball, reel_block, box_drop, spinning_hexagon, shrinking_ring])
}

/// The five stage 3 scenes: obstacles, folders, riders, rounded arenas and balls hitting balls.
pub fn stage3(reg: &Registry) -> Vec<(String, Scene)> {
    build_all(reg, &[peg_board, spinning_bars, ball_pit, riders, pitch_climb])
}

/// The five stage 4 scenes: rules on things, and modules whose graphs have been edited.
pub fn stage4(reg: &Registry) -> Vec<(String, Scene)> {
    build_all(reg, &[grow_on_hit, split_on_hit, colour_by_speed, bouncy_when_slow, wind_and_melody])
}

/// The five stage 6 scenes: an emitter, fields, a liquid on the processor, one on the graphics
/// card, and a swarm.
pub fn stage6(reg: &Registry) -> Vec<(String, Scene)> {
    build_all(reg, &[fountain, wind_tunnel, small_pool, deep_pool, fireflies])
}

/// The five stage 7 scenes: soft bodies, captions, keyframes, a drum machine, a melody and beat
/// sync.
pub fn stage7(reg: &Registry) -> Vec<(String, Scene)> {
    build_all(reg, &[jelly_drop, rope_bridge, cloth_hammock, captioned_reel, drum_beat])
}

/// The twelve Marks scenes: trails on a canvas, paint thrown onto a ring and a spinning bar, pegs
/// that take paint, the rich paint demos (wet paint that drips, a watercolour garden, neon ink on a
/// ring and colours that mix), and the surfaces: a scratch card, a colour war, a land grab, a
/// sand garden and fog glass.
pub fn marks(reg: &Registry) -> Vec<(String, Scene)> {
    build_all(
        reg,
        &[
            paint_pour,
            splat_ring,
            peg_paint,
            wet_pour,
            watercolour_garden,
            neon_splat_ring,
            colour_mixing,
            scratch_to_reveal,
            colour_war,
            territory,
            sand_garden,
            fog_glass,
        ],
    )
}

/// Every shipped scene, stage by stage.
pub fn all(reg: &Registry) -> Vec<(String, Scene)> {
    let mut scenes = stage2(reg);
    scenes.extend(stage3(reg));
    scenes.extend(stage4(reg));
    scenes.extend(stage5(reg));
    scenes.extend(stage6(reg));
    scenes.extend(stage7(reg));
    scenes.push(("Colour paints".into(), colour_demo(reg)));
    scenes.push(("Blend modes".into(), blend_demo(reg)));
    scenes.push(("Graded sunset".into(), graded_sunset(reg)));
    scenes.extend(marks(reg));
    scenes
}

/// The scenes the Start window offers, in order: a spread of what the editor can do.
pub const STARTERS: [&str; 9] = [
    "Reel block",
    "Lava ball",
    "Spinning hexagon",
    "Peg board",
    "Long drop",
    "Deep pool",
    "Fireflies",
    "Jelly drop",
    "Drum beat",
];

/// The [`STARTERS`], each with its name. Long drop is not in [`all`], so the golden tests keep
/// the scenes they had.
pub fn starters(reg: &Registry) -> Vec<(String, Scene)> {
    let mut all = all(reg);
    all.push(("Long drop".into(), long_drop(reg)));
    STARTERS.iter().filter_map(|name| all.iter().position(|(n, _)| n == name).map(|i| all.swap_remove(i))).collect()
}

/// The camera scene: a peg course four screens tall, with the camera following the ball down,
/// punching in on hits and staying inside the course.
pub fn long_drop(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Long drop");
    b.scene_settings(|s| {
        s.gravity = Vec2::new(0.0, 900.0);
        s.off_shot_fade = 1.5;
    });
    // The course runs from just above the first screen to the bottom of the fourth.
    let course = Vec2::new(0.0, 2880.0);
    b.arena(|a| {
        a.shape = ArenaShape::Box;
        a.width = 1000.0;
        a.height = 7680.0;
        a.corner = 60.0;
    });
    b.scene.things.get_mut(&ARENA).expect("arena").transform.position = course;
    let pegs = b.add(
        "obstacle",
        ThingId::ROOT,
        "Pegs",
        course + Vec2::new(0.0, 200.0),
        obstacle(|o| {
            o.shape = ObstacleShape::PegGrid;
            o.columns = 8;
            o.rows = 60;
            o.spacing = 115.0;
            o.radius = 10.0;
            o.stagger = true;
        }),
    );
    b.tag(pegs, "pegs");
    let peg_sound = NoteSound {
        instrument: b.built_in(Instrument::Marimba),
        plays_when: PlaysWhen::BeingHit,
        order: Order::Speed,
        ..NoteSound::default()
    };
    b.own_module(pegs, "sound.notes", "Peg sound", peg_sound.to_params());
    b.balls("Ball", Vec2::new(30.0, -820.0), |s| {
        s.radius = 22.0;
        s.velocity = Vec2::new(60.0, 0.0);
    });
    b.scene.set_slot_off(BALLS, SlotId::Sound);
    let bumpy = BasicBounce { bounciness: 0.55, ..BasicBounce::default() };
    b.own_module(BALLS, "bounce.basic", "Bumpy", bumpy.to_params());
    b.text("Title", Vec2::new(0.0, -780.0), |t| {
        t.text = "Long drop".into();
        t.font = Font::Anton;
        t.size = 110.0;
        t.outline = 6.0;
    });
    let camera = crate::CameraSettings {
        position: Vec2::new(0.0, -480.0),
        follow: true,
        follow_target: Target::Thing(BALLS),
        smoothness: 0.5,
        look_ahead: 0.2,
        slack: 0.05,
        punch: 0.12,
        punch_recovery: 0.25,
        hits_from: Target::Thing(BALLS),
        stay_inside: true,
        box_size: Vec2::new(1080.0, 7680.0 + 120.0),
        box_centre: course,
        ..crate::CameraSettings::default()
    };
    b.add("camera", ThingId::ROOT, "Camera", Vec2::ZERO, camera.to_params());
    b.finish()
}

/// The colour showcase: a block of balls painted by where they are at an angle with rings of the
/// same gradient inside each one, a ring whose colours run round its path, streaks and sparks
/// that change colour over their life, and a background glowing from the centre.
pub fn colour_demo(reg: &Registry) -> Scene {
    let hex = |h: u32| Color::rgb((h >> 16) as u8, (h >> 8) as u8, h as u8);
    let mut b = Builder::new(reg, "Colour paints");
    b.scene_settings(|s| {
        s.gravity = Vec2::new(0.0, 700.0);
        s.bloom = 0.35;
        s.background = Paint {
            colours: Gradient::even(&[hex(0x2b1458), hex(0x140a33), hex(0x05040f)]),
            shape: PlaceShape::Round,
            span: 1150.0,
            ..Paint::default()
        };
    });
    b.arena(|a| {
        a.radius = 470.0;
        a.thickness = 12.0;
    });
    let neon = [hex(0xff3ec8), hex(0xffb82e), hex(0x3ee6ff), hex(0x8a4dff), hex(0xff3ec8)];
    let ring = BasicLook {
        fill: Paint {
            colours: Gradient::even(&neon),
            follow: Follow::Path,
            repeat: Repeat::Repeat,
            scroll: 0.12,
            ..Paint::default()
        },
        glow: 0.7,
        ..BasicLook::default()
    };
    b.own_module(ARENA, "look.basic", "Ring look", ring.to_params());
    b.balls("Block", Vec2::new(-20.0, -170.0), |s| {
        s.arrangement = Arrangement::Block;
        s.columns = 8;
        s.rows = 8;
        s.spacing = 46.0;
        s.radius = 19.0;
        s.velocity = Vec2::new(-420.0, 220.0);
    });
    b.ball_look(|l| {
        let colours = Gradient::even(&[hex(0xffd23e), hex(0xff6a5c), hex(0xff3ec8), hex(0x8a4dff), hex(0x3e9bff)]);
        l.fill =
            Paint { colours: colours.clone(), angle: 45.0, span: 700.0, repeat: Repeat::Mirror, ..Paint::default() };
        l.inside = InsidePaint {
            pattern: Pattern::Rings,
            from: InsideFrom::OwnGradient,
            colours,
            repeat: Repeat::Mirror,
            angle: 0.0,
            size: 1.2,
            scroll: 0.25,
            ..InsidePaint::default()
        };
        l.glow = 0.3;
    });
    let streak = StreakTrail {
        seconds: 0.35,
        width: 0.55,
        brightness: 0.45,
        source: TrailSource::Own,
        colour: Paint {
            colours: Gradient::even(&[hex(0xffffff), hex(0x7cf4ff), hex(0x8a4dff), hex(0x2b1458)]),
            follow: Follow::Life,
            ..Paint::default()
        },
        ..StreakTrail::default()
    };
    b.own_module(BALLS, "trail.streak", "Life streak", streak.to_params());
    let sparks = HitBurst {
        count: 6,
        speed: 360.0,
        size: 3.0,
        life: 0.55,
        least_speed: 160.0,
        source: BurstSource::Own,
        colour: Paint {
            colours: Gradient::even(&[hex(0xffffff), hex(0xffd23e), hex(0xff6a2e), hex(0xc8104a)]),
            follow: Follow::Life,
            ..Paint::default()
        },
        ..HitBurst::default()
    };
    b.own_module(BALLS, "particles.burst", "Life sparks", sparks.to_params());
    let title = b.text("Title", Vec2::new(0.0, -740.0), |t| {
        t.text = "Colour paints".into();
        t.size = 130.0;
        t.outline = 6.0;
        t.outline_paint = Paint::solid(hex(0x05040f));
    });
    let title_look = BasicLook {
        fill: Paint { colours: Gradient::even(&neon[..4]), follow: Follow::Order, ..Paint::default() },
        glow: 0.0,
        ..BasicLook::default()
    };
    let words = b.own_module(title, "look.basic", "Title look", title_look.to_params());
    let caption = b.text("Caption", Vec2::new(0.0, 700.0), |t| {
        t.text = "place  path  life  centre".into();
        t.size = 56.0;
        t.outline = 0.0;
    });
    b.scene.link(reg, caption, SlotId::Look, words).expect("text has a Look slot");
    b.finish()
}

/// The blend showcase over a background that runs from dark to light: four rows of overlapping
/// balls (Multiply, Screen, Overlay, Difference), a bar that subtracts, streaks that add, and a
/// group of styled balls whose hues drift from ball to ball and over time.
pub fn blend_demo(reg: &Registry) -> Scene {
    let hex = |h: u32| Color::rgb((h >> 16) as u8, (h >> 8) as u8, h as u8);
    let mut b = Builder::new(reg, "Blend modes");
    b.scene_settings(|s| {
        s.gravity = Vec2::ZERO;
        s.background = Paint {
            colours: Gradient::even(&[hex(0x0b1030), hex(0x3a4a8c), hex(0xd9a38c), hex(0xf4ecd8)]),
            angle: 90.0,
            span: 1500.0,
            ..Paint::default()
        };
    });
    b.arena(|a| {
        a.shape = ArenaShape::Box;
        a.width = 1000.0;
        a.height = 1400.0;
        a.thickness = 8.0;
    });
    let rim = BasicLook { fill: Paint::solid(hex(0xffffff)), ..BasicLook::default() };
    b.own_module(ARENA, "look.basic", "Rim look", rim.to_params());
    // Screen suits the dark top, Multiply the light bottom.
    let rows = [
        ("Screen", Blend::Screen, [0x3ee6ff, 0x4d7bff, 0xff3ec8], -170.0),
        ("Difference", Blend::Difference, [0xffffff, 0xffb82e, 0x3ee6ff], -130.0),
        ("Overlay", Blend::Overlay, [0xffd23e, 0x3eff9a, 0x3e9bff], 190.0),
        ("Multiply", Blend::Multiply, [0xff9a3e, 0xff4f7b, 0x8a4dff], 150.0),
    ];
    for (i, (name, blend, colours, vx)) in rows.into_iter().enumerate() {
        let y = -470.0 + 190.0 * i as f32;
        let id = b.add(
            "balls",
            ThingId::ROOT,
            name,
            Vec2::new(-190.0, y),
            BallsSettings {
                arrangement: Arrangement::Block,
                columns: 5,
                rows: 1,
                spacing: 95.0,
                radius: 70.0,
                velocity: Vec2::new(vx, 0.0),
                ..BallsSettings::default()
            }
            .to_params(),
        );
        let look = BasicLook {
            fill: Paint { colours: Gradient::even(&colours.map(hex)), follow: Follow::Order, ..Paint::default() },
            blend,
            ..BasicLook::default()
        };
        b.own_module(id, "look.basic", &format!("{name} look"), look.to_params());
        b.text(&format!("{name} label"), Vec2::new(0.0, y), |t| {
            t.text = name.into();
            t.size = 44.0;
            t.outline = 4.0;
            t.outline_paint = Paint::solid(hex(0x000000));
        });
    }
    // A bar that subtracts its colour from what is under it.
    let bar = b.add(
        "obstacle",
        ThingId::ROOT,
        "Subtract bar",
        Vec2::new(0.0, 290.0),
        obstacle(|o| {
            o.shape = ObstacleShape::Bar;
            o.length = 800.0;
            o.thickness = 56.0;
        }),
    );
    let subtract = BasicLook { fill: Paint::solid(hex(0x66c8ff)), blend: Blend::Subtract, ..BasicLook::default() };
    b.own_module(bar, "look.basic", "Subtract look", subtract.to_params());
    // Streaks that add their light.
    let comets = b.add(
        "balls",
        ThingId::ROOT,
        "Comets",
        Vec2::new(0.0, 450.0),
        BallsSettings {
            arrangement: Arrangement::Ring,
            count: 3,
            ring_radius: 60.0,
            radius: 14.0,
            velocity: Vec2::new(520.0, 380.0),
            ..BallsSettings::default()
        }
        .to_params(),
    );
    let comet_look = BasicLook { fill: Paint::solid(hex(0xffffff)), glow: 0.6, ..BasicLook::default() };
    b.own_module(comets, "look.basic", "Comet look", comet_look.to_params());
    let streak = StreakTrail {
        seconds: 0.6,
        width: 0.8,
        brightness: 0.9,
        source: TrailSource::Own,
        colour: Paint {
            colours: Gradient::even(&[hex(0xffffff), hex(0x7cf4ff), hex(0xff3ec8)]),
            follow: Follow::Life,
            ..Paint::default()
        },
        blend: Blend::Add,
        ..StreakTrail::default()
    };
    b.own_module(comets, "trail.streak", "Add streak", streak.to_params());
    // Styled balls whose hue drifts around one colour.
    b.balls("Drifters", Vec2::new(-260.0, 600.0), |s| {
        s.arrangement = Arrangement::Block;
        s.columns = 8;
        s.rows = 2;
        s.spacing = 52.0;
        s.radius = 22.0;
        s.velocity = Vec2::new(160.0, -90.0);
    });
    let drift = BasicLook {
        fill: Paint { colours: Gradient::even(&[hex(0xff7a3e)]), vary_hue: 25.0, wobble: 0.5, ..Paint::default() },
        style: Style::Shiny,
        ..BasicLook::default()
    };
    b.own_module(BALLS, "look.basic", "Drift look", drift.to_params());
    b.text("Title", Vec2::new(0.0, -640.0 - 150.0), |t| {
        t.text = "Blend modes".into();
        t.size = 120.0;
        t.outline = 6.0;
        t.outline_paint = Paint::solid(hex(0x05040f));
    });
    b.finish()
}

/// The palette and grade showcase: balls painted from the Sunset palette in a ring whose wall
/// wears the Triad colours that match it, a cluster of glowing balls that Screen onto each other,
/// and a warm, contrasty grade with a vignette whose Sunset gradient map comes in over the video.
pub fn graded_sunset(reg: &Registry) -> Scene {
    use crate::paint::harmony::{Harmony, add_matching};
    let hex = |h: u32| Color::rgb((h >> 16) as u8, (h >> 8) as u8, h as u8);
    let sunset = crate::paint::palettes::built_in()
        .into_iter()
        .find(|p| p.name == "Sunset")
        .expect("Sunset is a built-in palette")
        .colours;
    let mut b = Builder::new(reg, "Graded sunset");
    let duration = 20.0;
    b.scene_settings(|s| {
        s.duration = duration;
        s.gravity = Vec2::new(0.0, 600.0);
        s.bloom = 0.3;
        s.background = Paint {
            colours: Gradient::even(&[hex(0x1a0b3a), hex(0x2b1055), hex(0x4a2a7a)]),
            angle: 90.0,
            span: 1900.0,
            ..Paint::default()
        };
        s.grade_warmth = 0.3;
        s.grade_contrast = 0.2;
        s.grade_vignette = 0.4;
        s.grade_map = sunset.clone();
        s.grade_map_amount = 0.0;
    });
    b.arena(|a| {
        a.radius = 470.0;
        a.thickness = 14.0;
    });
    // The wall: the Sunset coral and the two colours a third of the way round the wheel from it.
    let coral = Gradient::even(&[sunset.stops[3].color]);
    let wall = BasicLook {
        fill: Paint {
            colours: add_matching(&coral, Harmony::Triad),
            follow: Follow::Path,
            repeat: Repeat::Mirror,
            scroll: 0.08,
            ..Paint::default()
        },
        glow: 0.5,
        ..BasicLook::default()
    };
    b.own_module(ARENA, "look.basic", "Wall look", wall.to_params());
    b.balls("Sunset balls", Vec2::new(0.0, -150.0), |s| {
        s.arrangement = Arrangement::Block;
        s.columns = 7;
        s.rows = 5;
        s.spacing = 52.0;
        s.radius = 22.0;
        s.velocity = Vec2::new(300.0, -120.0);
    });
    b.ball_look(|l| {
        l.fill = Paint { colours: sunset.clone(), follow: Follow::Order, ..Paint::default() };
        l.glow = 0.2;
    });
    // Overlapping glow balls that Screen onto each other as well as onto the scene, so where
    // they cross they brighten.
    let glow = b.add(
        "balls",
        ThingId::ROOT,
        "Glow balls",
        Vec2::new(0.0, 230.0),
        BallsSettings {
            arrangement: Arrangement::Ring,
            count: 5,
            ring_radius: 50.0,
            radius: 55.0,
            velocity: Vec2::new(-260.0, -380.0),
            ..BallsSettings::default()
        }
        .to_params(),
    );
    let screen = BasicLook {
        fill: Paint { colours: sunset.clone(), follow: Follow::Order, ..Paint::default() },
        glow: 0.0,
        blend: Blend::Screen,
        blend_each: true,
        ..BasicLook::default()
    };
    b.own_module(glow, "look.basic", "Screen look", screen.to_params());
    b.text("Title", Vec2::new(0.0, -740.0), |t| {
        t.text = "Graded sunset".into();
        t.size = 120.0;
        t.outline = 6.0;
        t.outline_paint = Paint::solid(hex(0x1a0b3a));
    });
    b.text("Caption", Vec2::new(0.0, 700.0), |t| {
        t.text = "palette  triad  grade".into();
        t.size = 56.0;
        t.outline = 0.0;
    });
    b.track(
        own(ThingId::ROOT, "grade_map_amount"),
        &[(0.0, Value::Number(0.0), Ease::Linear), (duration as f64, Value::Number(0.5), Ease::Linear)],
    );
    b.finish()
}

/// A ready-made rule's builder, by its menu name.
fn recipe(name: &str) -> fn(&mut Graph) -> Vec<(u32, String)> {
    recipes().into_iter().find(|r| r.name == name).unwrap_or_else(|| panic!("no recipe called {name}")).add
}

/// The block of `kind` in a graph that has exactly one.
fn block(g: &Graph, kind: &str) -> u32 {
    g.nodes.iter().find(|n| n.kind == kind).unwrap_or_else(|| panic!("no {kind} block")).id
}

fn set(g: &mut Graph, node: u32, name: &str, p: Param) {
    g.node_mut(node).expect("a block of the graph").params.insert(name.into(), p);
}

fn wire(g: &mut Graph, role: Role, from: (u32, u8), to: (u32, u8)) {
    assert!(g.connect(Link { from, to }, catalog(role)), "{from:?} can't wire into {to:?}");
}

/// A new scene with a name, and handles on its things and the modules its things share.
/// Gives every thing its own modules: a module several things use becomes one copy each (with
/// the same settings and keyframes), and one only a single thing uses becomes that thing's.
/// Modules nothing uses are left in the library.
pub(crate) fn own_everything(scene: &mut Scene) {
    let links: Vec<(ThingId, SlotId, ModuleId)> = scene
        .order
        .iter()
        .flat_map(|id| {
            scene.things[id].slots.iter().filter_map(move |(slot, link)| match link {
                SlotLink::Linked { module, .. } => Some((*id, *slot, *module)),
                _ => None,
            })
        })
        .collect();
    let shared: std::collections::BTreeSet<ModuleId> =
        links.iter().map(|l| l.2).filter(|m| scene.users_of(*m).len() > 1).collect();
    let mut emptied = Vec::new();
    for (id, slot, m) in links {
        let Some(module) = scene.library.modules.get(&m) else { continue };
        if module.owner == Some(id) {
            continue;
        }
        if !shared.contains(&m) && module.owner.is_none() {
            scene.library.modules.get_mut(&m).expect("checked").owner = Some(id);
            continue;
        }
        scene.own_linked(id, slot).expect("the slot is linked");
        if scene.users_of(m).is_empty() {
            emptied.push(m);
        }
    }
    for m in emptied {
        let _ = scene.remove_module(m);
    }
}

struct Builder<'a> {
    reg: &'a Registry,
    scene: Scene,
    /// One module per slot that every thing of a fitting kind uses instead of its own starter,
    /// so a scene's looks, motion, bounce and sound can be set in one place.
    shared: BTreeMap<SlotId, ModuleId>,
}

impl<'a> Builder<'a> {
    fn new(reg: &'a Registry, name: &str) -> Builder<'a> {
        let mut scene = new_scene(reg);
        scene.name = name.into();
        let mut shared = BTreeMap::new();
        for (slot, kind, module_name) in [
            (SlotId::Look, "look.basic", "Plain look"),
            (SlotId::Motion, "motion.basic", "Default motion"),
            (SlotId::Bounce, "bounce.basic", "Default bounce"),
            (SlotId::Sound, "sound.notes", "Default sound"),
        ] {
            let m = scene.add_module(reg, kind, module_name).expect("kit kinds are registered");
            shared.insert(slot, m);
        }
        let (pluck, chords) =
            (crate::starters::shared_pluck(&mut scene), crate::starters::shared_progression(&mut scene));
        let sound = &mut scene.library.modules.get_mut(&shared[&SlotId::Sound]).expect("just added").params;
        sound.set("instrument", pluck.to_value());
        sound.set("chords", chords.to_value());
        let mut b = Builder { reg, scene, shared };
        b.share(ARENA, &SlotId::ALL);
        b.share(BALLS, &[SlotId::Motion, SlotId::Bounce, SlotId::Sound]);
        b
    }

    /// Swaps `thing`'s own starters in `slots` for the shared modules of the same kind, and
    /// drops the starters.
    fn share(&mut self, thing: ThingId, slots: &[SlotId]) {
        // A liquid's and a swarm's starters are made for them, so they keep their own.
        if matches!(self.scene.things[&thing].kind.as_str(), "liquid" | "swarm") {
            return;
        }
        for slot in slots {
            let Some(&shared) = self.shared.get(slot) else { continue };
            let Some(SlotLink::Linked { module, .. }) = self.scene.things[&thing].slots.get(slot).cloned() else {
                continue;
            };
            let own = &self.scene.library.modules[&module];
            if own.owner != Some(thing) || own.kind != self.scene.library.modules[&shared].kind {
                continue;
            }
            self.scene.link(self.reg, thing, *slot, shared).expect("the thing has the slot");
            self.scene.remove_module(module).expect("nothing else uses a fresh starter");
        }
    }

    /// The finished scene, with every thing owning the modules it uses.
    fn finish(mut self) -> Scene {
        own_everything(&mut self.scene);
        self.scene
    }

    fn scene_settings(&mut self, f: impl FnOnce(&mut SceneSettings)) {
        let root = self.scene.things.get_mut(&ThingId::ROOT).expect("root");
        // Keep what the new scene set.
        let mut s = SceneSettings::from_params(&root.params);
        f(&mut s);
        root.params = s.to_params();
    }

    /// A library instrument playing the built-in `which` with its usual knobs, added the first
    /// time it is asked for.
    fn built_in(&mut self, which: Instrument) -> InstrumentRef {
        let found = self
            .scene
            .library
            .instruments
            .iter()
            .find(|(_, i)| matches!(&i.source, InstrumentSource::BuiltIn { which: w, .. } if w == which.name()));
        if let Some((&id, _)) = found {
            return InstrumentRef(Some(id));
        }
        InstrumentRef(Some(self.scene.add_instrument(crate::built_in_instrument(which))))
    }

    /// The library's copy of the starter patch `name`, added the first time it's asked for.
    fn starter(&mut self, name: &str) -> InstrumentRef {
        if let Some((&id, _)) = self.scene.library.instruments.iter().find(|(_, i)| i.name == name) {
            return InstrumentRef(Some(id));
        }
        let (name, graph, voices) =
            bpe_synth::starters().into_iter().find(|(n, ..)| *n == name).expect("a starter patch of that name");
        InstrumentRef(Some(self.scene.add_instrument(bpe_core::Instrument {
            name: name.into(),
            source: InstrumentSource::Patch { graph },
            voices,
            ..Default::default()
        })))
    }

    /// The shared sound's chords become the preset `name`, in the key on `root`.
    fn chords(&mut self, name: &str, root: u8) {
        let preset = bpe_mix::PRESETS.iter().find(|p| p.name == name).expect("a known preset");
        let a = of_module(self.shared[&SlotId::Sound], "chords");
        let id = self.scene.value_at(self.reg, &a).and_then(|v| ProgressionRef::from_value(&v)).and_then(|r| r.0);
        let p = Progression {
            name: preset.name.into(),
            key: MusicKey { root, mode: preset.mode },
            chords: preset.chords(root),
            ..Progression::default()
        };
        self.scene.set_progression(id.expect("a new scene has chords"), p).expect("the progression is there");
    }

    fn arena(&mut self, f: impl FnOnce(&mut ArenaSettings)) {
        let mut s = ArenaSettings::default();
        f(&mut s);
        self.scene.things.get_mut(&ARENA).expect("arena").params = s.to_params();
    }

    fn balls(&mut self, name: &str, position: Vec2, f: impl FnOnce(&mut BallsSettings)) {
        let mut s = BallsSettings::default();
        f(&mut s);
        let thing = self.scene.things.get_mut(&BALLS).expect("balls");
        thing.name = name.into();
        thing.transform.position = position;
        thing.params = s.to_params();
    }

    /// The module that fills `slot` on `thing`.
    fn module_of(&self, thing: ThingId, slot: SlotId) -> ModuleId {
        let resolved = bpe_core::resolve_slot(&self.scene, self.reg, thing, slot, &mut Vec::new());
        resolved.expect("the thing has a module in that slot").module
    }

    fn set_params(&mut self, m: ModuleId, params: bpe_core::Params) {
        self.scene.library.modules.get_mut(&m).expect("module").params = params;
    }

    fn root_look(&mut self, f: impl FnOnce(&mut BasicLook)) {
        let mut s = BasicLook::default();
        f(&mut s);
        let m = self.shared[&SlotId::Look];
        self.set_params(m, s.to_params());
    }

    /// The balls' look in `colours`, from the block's first row to its last as the balls start
    /// out: follows where each ball started, top to bottom, across the block's height. Balls
    /// that start in one row (or not in a block) all wear the first colour.
    fn ball_look_by_row(&mut self, colours: &[Color]) {
        let thing = &self.scene.things[&BALLS];
        let s = BallsSettings::from_params(&thing.params);
        let height = s.rows.saturating_sub(1) as f32 * s.spacing;
        let fill = if s.arrangement == Arrangement::Block && height > 0.0 {
            Paint {
                colours: Gradient::even(colours),
                follow: Follow::Start,
                angle: 90.0,
                centre: thing.transform.position,
                span: height,
                ..Paint::default()
            }
        } else {
            Paint::solid(colours.first().copied().unwrap_or(Color::WHITE))
        };
        self.ball_look(|l| l.fill = fill);
    }

    fn ball_look(&mut self, f: impl FnOnce(&mut BasicLook)) {
        let mut s = BasicLook { fill: Paint::solid(Color::rgb(0xff, 0x5c, 0xa8)), ..BasicLook::default() };
        f(&mut s);
        let m = self.module_of(BALLS, SlotId::Look);
        self.set_params(m, s.to_params());
    }

    fn motion(&mut self, f: impl FnOnce(&mut BasicMotion)) {
        let mut s = BasicMotion::default();
        f(&mut s);
        let m = self.shared[&SlotId::Motion];
        self.set_params(m, s.to_params());
    }

    fn bounce(&mut self, f: impl FnOnce(&mut BasicBounce)) {
        let mut s = BasicBounce::default();
        f(&mut s);
        let m = self.shared[&SlotId::Bounce];
        self.set_params(m, s.to_params());
    }

    fn sound(&mut self, f: impl FnOnce(&mut NoteSound)) {
        // Every built-in scene plays the library Pluck and the shared chords unless it says.
        let now = NoteSound::from_params(&self.scene.library.modules[&self.shared[&SlotId::Sound]].params);
        let mut s = NoteSound { instrument: now.instrument, chords: now.chords, ..NoteSound::default() };
        f(&mut s);
        let m = self.shared[&SlotId::Sound];
        self.set_params(m, s.to_params());
    }

    /// A new module linked onto one thing. On the scene settings it also takes the place of the
    /// shared module for its slot, so every thing that used that one uses this instead (the way
    /// they used to inherit it). On a folder, the things inside that used the shared module
    /// follow the folder instead.
    fn own_module(&mut self, thing: ThingId, kind: &str, name: &str, params: bpe_core::Params) -> ModuleId {
        let m = self.scene.add_module(self.reg, kind, name).expect("kit kinds are registered");
        self.set_params(m, params);
        let slot = self.scene.library.modules[&m].slot;
        let shared = self.shared.get(&slot).copied();
        let users: Vec<ThingId> = self
            .scene
            .things
            .values()
            .filter(|t| matches!(t.slots.get(&slot), Some(SlotLink::Linked { module, .. }) if Some(*module) == shared))
            .map(|t| t.id)
            .collect();
        if thing == ThingId::ROOT {
            if self.reg.thing("scene").is_some_and(|k| k.accepts(slot, kind)) {
                self.scene.link(self.reg, thing, slot, m).expect("the scene settings have the slot");
            }
            if let Some(old) = shared {
                for user in users {
                    self.scene.link(self.reg, user, slot, m).expect("the thing has the slot");
                }
                self.shared.insert(slot, m);
                let _ = self.scene.remove_module(old);
            }
            return m;
        }
        self.scene.link(self.reg, thing, slot, m).expect("the kind has the slot");
        for user in users {
            if self.scene.ancestors(user).contains(&thing) {
                self.scene.things.get_mut(&user).expect("thing").slots.insert(slot, SlotLink::Inherit);
            }
        }
        m
    }

    /// A new thing of `kind` under `parent`, named and placed, with its settings, and the starter
    /// modules the kit gives that kind (a liquid's surface look, a swarm's flocking).
    fn add(&mut self, kind: &str, parent: ThingId, name: &str, position: Vec2, params: Params) -> ThingId {
        let id = crate::add_thing(&mut self.scene, self.reg, kind, parent).expect("kit kinds are registered");
        self.scene.rename_thing(id, name);
        self.share(id, &SlotId::ALL);
        let thing = self.scene.things.get_mut(&id).expect("just added");
        thing.transform.position = position;
        thing.params = params;
        id
    }

    fn soft(&mut self, name: &str, position: Vec2, f: impl FnOnce(&mut SoftSettings)) -> ThingId {
        let mut s = SoftSettings::default();
        f(&mut s);
        self.add("soft", ThingId::ROOT, name, position, s.to_params())
    }

    fn text(&mut self, name: &str, position: Vec2, f: impl FnOnce(&mut TextSettings)) -> ThingId {
        let mut s = TextSettings::default();
        f(&mut s);
        self.add("text", ThingId::ROOT, name, position, s.to_params())
    }

    /// Keys on one setting: (time, value, ease into the next).
    fn track(&mut self, a: Address, keys: &[(f64, Value, Ease)]) {
        for (time, value, ease) in keys {
            let i = self.scene.set_key(self.reg, &a, *time, value.clone()).expect("the setting takes keys");
            self.scene.set_ease(&a, i, *ease);
        }
    }

    fn pulse(&mut self, a: Address, source: PulseSource, amount: Value, decay: f32) {
        self.scene.set_pulse(self.reg, &a, source, amount, decay).expect("the setting pulses");
    }
    /// Makes `id` ride on `parent`.
    fn ride(&mut self, id: ThingId, parent: ThingId, offset: Vec2, turn_with_parent: bool) {
        self.scene.set_parent(id, parent, Attach::Ride { offset, turn_with_parent, item: None }).expect("no loops");
    }

    fn tag(&mut self, id: ThingId, tag: &str) {
        self.scene.things.get_mut(&id).expect("thing").tags.insert(tag.into());
    }

    fn turn(&mut self, id: ThingId, degrees: f32) {
        self.scene.things.get_mut(&id).expect("thing").transform.rotation = degrees.to_radians();
    }

    /// A Rules module on `thing`, with `build` adding its rules and naming the settings to show
    /// at Simple depth.
    fn rules(&mut self, thing: ThingId, name: &str, build: impl FnOnce(&mut Graph) -> Vec<(u32, String)>) {
        let m = self.scene.add_module(self.reg, "rules.graph", name).expect("rules are registered");
        let mut graph = RulesKind.template().expect("rules have a template");
        let simple = build(&mut graph).into_iter().collect();
        self.scene.library.modules.get_mut(&m).expect("module").graph =
            Some(ModuleGraph { graph, template: "rules.graph".into(), custom: true, simple });
        self.scene.link(self.reg, thing, SlotId::Rules, m).expect("the thing has a Rules slot");
    }

    /// Edits the shared module for `slot` as nodes, starting from its template with the
    /// module's settings in.
    fn custom(&mut self, slot: SlotId, edit: impl FnOnce(&mut Graph)) {
        let reg = self.reg;
        let m = self.shared[&slot];
        let module = self.scene.library.modules.get_mut(&m).expect("module");
        let kind = reg.module(&module.kind).expect("a registered kind");
        let mut g = module_graph(kind, module).expect("the kind has a template");
        edit(&mut g.graph);
        g.custom = true;
        module.graph = Some(g);
    }
}

fn own(thing: ThingId, key: &str) -> Address {
    Address { owner: Owner::Thing(thing), slot: None, key: key.into() }
}

fn of_module(m: ModuleId, key: &str) -> Address {
    Address { owner: Owner::Module(m), slot: None, key: key.into() }
}

fn obstacle(f: impl FnOnce(&mut ObstacleSettings)) -> Params {
    let mut s = ObstacleSettings::default();
    f(&mut s);
    s.to_params()
}

/// The new-scene default: a circle arena and one ball.
fn one_ball(reg: &Registry) -> Scene {
    Builder::new(reg, "One ball").finish()
}

/// The reference reel: a block of 784 tiny balls in a big ring, coloured by row.
fn reel_block(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Reel block");
    b.scene_settings(|s| {
        s.gravity = Vec2::new(0.0, 160.0);
        s.duration = 23.0;
    });
    b.arena(|a| {
        a.radius = 508.0;
        a.thickness = 3.0;
    });
    b.balls("Block", Vec2::new(37.0, -127.0), |s| {
        s.arrangement = Arrangement::Block;
        s.columns = 28;
        s.rows = 28;
        s.spacing = 19.0;
        s.radius = 4.5;
        s.velocity = Vec2::new(-480.0, 265.0);
    });
    b.ball_look_by_row(&[
        Color::rgb(0x3e, 0x9b, 0xff),
        Color::rgb(0x8a, 0x4d, 0xff),
        Color::rgb(0xff, 0x3e, 0xc8),
        Color::rgb(0xff, 0x6a, 0x5c),
        Color::rgb(0xff, 0xb8, 0x2e),
    ]);
    let arena_look = BasicLook { fill: Paint::solid(Color::rgb(0xd8, 0xdc, 0xd0)), ..BasicLook::default() }.to_params();
    b.own_module(ARENA, "look.basic", "Arena look", arena_look);
    b.finish()
}

/// A box with a block of balls dropped in, losing a little speed on every hit.
fn box_drop(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Box drop");
    b.scene_settings(|s| s.gravity = Vec2::new(0.0, 900.0));
    b.arena(|a| {
        a.shape = ArenaShape::Box;
        a.width = 900.0;
        a.height = 1500.0;
    });
    b.balls("Block", Vec2::new(0.0, -500.0), |s| {
        s.arrangement = Arrangement::Block;
        s.columns = 10;
        s.rows = 10;
        s.spacing = 40.0;
        s.radius = 12.0;
        s.velocity = Vec2::new(200.0, 0.0);
    });
    b.bounce(|s| s.bounciness = 0.85);
    b.motion(|s| s.drag = 0.05);
    b.finish()
}

/// A turning hexagon with a ring of balls that gain a little speed on every hit.
fn spinning_hexagon(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Spinning hexagon");
    b.arena(|a| {
        a.shape = ArenaShape::Polygon;
        a.sides = 6;
        a.radius = 450.0;
        a.spin = 25.0;
    });
    b.balls("Ring", Vec2::ZERO, |s| {
        s.arrangement = Arrangement::Ring;
        s.count = 24;
        s.radius = 10.0;
        s.ring_radius = 200.0;
        s.velocity = Vec2::ZERO;
    });
    b.bounce(|s| {
        s.speed_gain = 1.02;
        s.top_speed = 2500.0;
    });
    b.root_look(|l| l.glow = 0.5);
    b.finish()
}

/// A ring that closes in while one ball grows with every hit against a lively wall.
fn shrinking_ring(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Shrinking ring");
    b.arena(|a| {
        a.radius = 500.0;
        a.shrink = 0.03;
        a.min_size = 200.0;
    });
    b.balls("Ball", Vec2::new(0.0, -200.0), |s| {
        s.radius = 30.0;
        s.velocity = Vec2::new(600.0, -300.0);
    });
    b.bounce(|s| {
        s.grow = 1.5;
        s.max_size = 90.0;
    });
    let marimba = b.built_in(Instrument::Marimba);
    b.sound(|s| s.instrument = marimba);
    let lively = BasicBounce { bounciness: 1.05, takes_over: true, ..BasicBounce::default() }.to_params();
    b.own_module(ARENA, "bounce.basic", "Lively wall", lively);
    b.finish()
}

// ---------- stage 7 ----------

/// A jelly dropped into a ring with a ball to knock it about.
fn jelly_drop(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Jelly drop");
    b.scene_settings(|s| s.gravity = Vec2::new(0.0, 900.0));
    b.arena(|a| {
        a.radius = 480.0;
        a.thickness = 4.0;
    });
    b.balls("Ball", Vec2::new(-200.0, -380.0), |s| {
        s.radius = 30.0;
        s.velocity = Vec2::new(300.0, 0.0);
    });
    let jelly = b.soft("Jelly", Vec2::new(0.0, -350.0), |s| {
        s.shape = SoftShape::Jelly;
        s.size = 110.0;
        s.softness = 0.6;
        s.velocity = Vec2::new(220.0, 0.0);
    });
    let look = BasicLook { fill: Paint::solid(Color::rgb(0xff, 0xb0, 0x20)), glow: 0.3, ..BasicLook::default() };
    b.own_module(jelly, "look.basic", "Jelly look", look.to_params());
    b.finish()
}

/// A rope slung across a box with balls dropped onto it.
fn rope_bridge(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Rope bridge");
    b.arena(|a| {
        a.shape = ArenaShape::Box;
        a.width = 900.0;
        a.height = 1500.0;
    });
    b.balls("Balls", Vec2::new(0.0, -700.0), |s| {
        s.arrangement = Arrangement::Block;
        s.columns = 4;
        s.rows = 1;
        s.spacing = 150.0;
        s.radius = 28.0;
        s.velocity = Vec2::ZERO;
    });
    b.ball_look_by_row(&[Color::rgb(0x3e, 0x9b, 0xff), Color::rgb(0xff, 0x3e, 0xc8)]);
    let rope = b.soft("Rope", Vec2::new(0.0, -100.0), |s| {
        s.shape = SoftShape::Rope;
        s.size = 760.0;
        s.softness = 0.3;
        s.detail = 40;
        s.thickness = 10.0;
        s.pinned = Pinned::BothEnds;
    });
    let look = BasicLook { fill: Paint::solid(Color::rgb(0xe8, 0xe8, 0xe8)), ..BasicLook::default() };
    b.own_module(rope, "look.basic", "Rope look", look.to_params());
    let marimba = b.built_in(Instrument::Marimba);
    b.sound(|s| s.instrument = marimba);
    b.finish()
}

/// A cloth slung from its two top corners, swinging as a ring of balls lands in it.
fn cloth_hammock(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Cloth hammock");
    b.scene_settings(|s| s.gravity = Vec2::new(0.0, 700.0));
    b.arena(|a| {
        a.shape = ArenaShape::Box;
        a.width = 900.0;
        a.height = 1800.0;
    });
    b.balls("Balls", Vec2::new(0.0, -800.0), |s| {
        s.arrangement = Arrangement::Ring;
        s.count = 5;
        s.ring_radius = 80.0;
        s.radius = 26.0;
        s.velocity = Vec2::ZERO;
    });
    let cloth = b.soft("Cloth", Vec2::new(0.0, -350.0), |s| {
        s.shape = SoftShape::Cloth;
        s.size = 700.0;
        s.height = 500.0;
        s.detail = 26;
        s.rows = 20;
        s.softness = 0.45;
        s.pinned = Pinned::TopCorners;
    });
    let look = BasicLook { fill: Paint::solid(Color::rgb(0x3e, 0x9b, 0xff)), ..BasicLook::default() };
    b.own_module(cloth, "look.basic", "Cloth look", look.to_params());
    b.finish()
}

/// The reel's ring with a block of balls, a hit counter that jumps on hits, a countdown, and
/// gravity, bloom and glow that grow over the video.
fn captioned_reel(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Captioned reel");
    b.scene_settings(|s| s.gravity = Vec2::new(0.0, 160.0));
    b.arena(|a| {
        a.radius = 508.0;
        a.thickness = 3.0;
    });
    b.balls("Block", Vec2::new(30.0, -120.0), |s| {
        s.arrangement = Arrangement::Block;
        s.columns = 14;
        s.rows = 14;
        s.spacing = 30.0;
        s.radius = 7.0;
        s.velocity = Vec2::new(-420.0, 240.0);
    });
    b.ball_look_by_row(&[Color::rgb(0x3e, 0x9b, 0xff), Color::rgb(0xff, 0x3e, 0xc8), Color::rgb(0xff, 0xb8, 0x2e)]);
    let hits = b.text("Hit counter", Vec2::new(0.0, -780.0), |t| {
        t.text = "Hits {hits}".into();
        t.font = Font::Anton;
        t.size = 120.0;
        t.appear = Appear::Pop;
        t.outline = 6.0;
    });
    b.text("Countdown", Vec2::new(0.0, 760.0), |t| {
        t.text = "{countdown}".into();
        t.font = Font::SpaceMono;
        t.size = 150.0;
        t.align = Align::Center;
    });
    b.pulse(own(hits, "size"), PulseSource::Hits, Value::Number(24.0), 0.25);
    b.track(
        own(ThingId::ROOT, "gravity"),
        &[(0.0, Value::Vec2([0.0, 160.0]), Ease::Smooth), (12.0, Value::Vec2([0.0, 900.0]), Ease::Smooth)],
    );
    b.track(
        own(ThingId::ROOT, "bloom"),
        &[(0.0, Value::Number(0.15), Ease::Linear), (20.0, Value::Number(0.6), Ease::Linear)],
    );
    let ball_look = b.module_of(BALLS, SlotId::Look);
    b.track(
        of_module(ball_look, "glow"),
        &[(0.0, Value::Number(0.1), Ease::EaseIn), (20.0, Value::Number(1.2), Ease::EaseIn)],
    );
    b.finish()
}

/// A spinning hexagon at 128 bpm: hits land on beats, the walls play drums and the beat keeps
/// time, while the ball plays a tune.
fn drum_beat(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Drum beat");
    b.scene_settings(|s| {
        s.bpm = 128.0;
        s.beat_sync = BeatSync::Warp;
        s.warp_strength = 0.5;
    });
    b.chords("Epic minor", 9);
    b.arena(|a| {
        a.shape = ArenaShape::Polygon;
        a.sides = 6;
        a.radius = 470.0;
        a.spin = 25.0;
    });
    b.balls("Ball", Vec2::new(0.0, -300.0), |s| {
        s.radius = 40.0;
        s.velocity = Vec2::new(380.0, -120.0);
    });
    let drums = DrumSound { piece: Piece::Auto, on_beat: Piece::Auto, beat_volume: -4.0, ..DrumSound::default() };
    b.own_module(ThingId::ROOT, "sound.drums", "Drum machine", drums.to_params());
    let melody = MelodySound {
        file: "builtin:twinkle".into(),
        instrument: b.starter("Warm pad"),
        volume: -2.0,
        ..MelodySound::default()
    };
    b.own_module(BALLS, "sound.melody", "Tune", melody.to_params());
    b.text("Clock", Vec2::new(0.0, 850.0), |t| {
        t.text = "{time}".into();
        t.font = Font::VarelaRound;
        t.size = 72.0;
        t.appear = Appear::Fade;
    });
    b.finish()
}

/// A Plinko board: a staggered grid of pegs that sound when hit, and a shower of balls that
/// bump into each other on the way down.
fn peg_board(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Peg board");
    b.scene_settings(|s| s.gravity = Vec2::new(0.0, 900.0));
    b.arena(|a| {
        a.shape = ArenaShape::Box;
        a.width = 900.0;
        a.height = 1500.0;
        a.corner = 60.0;
    });
    let pegs = b.add(
        "obstacle",
        ThingId::ROOT,
        "Pegs",
        Vec2::new(0.0, 100.0),
        obstacle(|o| {
            o.shape = ObstacleShape::PegGrid;
            o.columns = 7;
            o.rows = 9;
            o.spacing = 100.0;
            o.radius = 9.0;
        }),
    );
    b.tag(pegs, "pegs");
    let peg_sound = NoteSound {
        instrument: b.built_in(Instrument::Marimba),
        plays_when: PlaysWhen::BeingHit,
        order: Order::Speed,
        ..NoteSound::default()
    };
    b.own_module(pegs, "sound.notes", "Peg sound", peg_sound.to_params());
    b.balls("Drops", Vec2::new(0.0, -650.0), |s| {
        s.arrangement = Arrangement::Block;
        s.columns = 8;
        s.rows = 6;
        s.spacing = 30.0;
        s.radius = 8.0;
        s.velocity = Vec2::ZERO;
    });
    let bumpy = BasicBounce { hit_balls: true, bounciness: 0.6, ..BasicBounce::default() };
    b.own_module(BALLS, "bounce.basic", "Bumpy", bumpy.to_params());
    // Only the pegs make a sound.
    b.scene.set_slot_off(BALLS, SlotId::Sound);
    b.ball_look(|l| {
        l.fill = Paint {
            colours: Gradient::even(&[
                Color::rgb(0xff, 0xd1, 0x4a),
                Color::rgb(0xff, 0x5c, 0xa8),
                Color::rgb(0x6a, 0x8d, 0xff),
            ]),
            follow: Follow::Hits,
            span: 10.0,
            ..Paint::default()
        };
    });
    b.finish()
}

/// Three bars in a folder, turning together, with a ring of balls speeding up around them.
fn spinning_bars(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Spinning bars");
    b.arena(|a| a.radius = 500.0);
    let bars = b.add("folder", ThingId::ROOT, "Bars", Vec2::ZERO, Params::default());
    for (i, degrees) in [0.0f32, 60.0, 120.0].into_iter().enumerate() {
        let bar = b.add(
            "obstacle",
            bars,
            &format!("Bar {}", i + 1),
            Vec2::ZERO,
            obstacle(|o| {
                o.shape = ObstacleShape::Bar;
                o.length = 600.0;
                o.thickness = 10.0;
                o.spin = 40.0;
            }),
        );
        b.ride(bar, bars, Vec2::ZERO, true);
        b.turn(bar, degrees);
    }
    b.balls("Ring", Vec2::ZERO, |s| {
        s.arrangement = Arrangement::Ring;
        s.count = 18;
        s.radius = 12.0;
        s.ring_radius = 380.0;
        s.velocity = Vec2::ZERO;
    });
    b.bounce(|s| s.speed_gain = 1.01);
    let bar_look = BasicLook { fill: Paint::solid(Color::rgb(0x7f, 0xe8, 0xff)), glow: 0.3, ..BasicLook::default() };
    b.own_module(bars, "look.basic", "Bar look", bar_look.to_params());
    b.finish()
}

/// A capsule full of balls that pile up on each other, coloured by how often they have hit.
fn ball_pit(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Ball pit");
    b.scene_settings(|s| {
        s.gravity = Vec2::new(0.0, 1200.0);
        s.duration = 15.0;
    });
    b.arena(|a| {
        a.shape = ArenaShape::Capsule;
        a.width = 700.0;
        a.height = 1400.0;
    });
    b.balls("Pit", Vec2::new(0.0, -300.0), |s| {
        s.arrangement = Arrangement::Block;
        s.columns = 10;
        s.rows = 14;
        s.spacing = 48.0;
        s.radius = 20.0;
        s.velocity = Vec2::new(120.0, 0.0);
    });
    b.bounce(|s| {
        s.hit_balls = true;
        s.bounciness = 0.5;
        s.friction = 0.3;
    });
    b.ball_look(|l| {
        l.fill = Paint {
            colours: Gradient::even(&[
                Color::rgb(0x3e, 0x9b, 0xff),
                Color::rgb(0xff, 0x3e, 0xc8),
                Color::rgb(0xff, 0xb8, 0x2e),
            ]),
            follow: Follow::Hits,
            span: 12.0,
            ..Paint::default()
        };
    });
    b.sound(|s| s.min_gap = 0.03);
    b.finish()
}

/// Balls and a peg riding a turning hexagon; the balls change hue with every hit and pop after
/// forty.
fn riders(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Riders");
    b.arena(|a| {
        a.shape = ArenaShape::Polygon;
        a.sides = 6;
        a.radius = 450.0;
        a.spin = 30.0;
    });
    b.balls("Rim riders", Vec2::ZERO, |s| {
        s.arrangement = Arrangement::Ring;
        s.count = 6;
        s.radius = 14.0;
        s.ring_radius = 60.0;
        s.velocity = Vec2::ZERO;
    });
    b.ride(BALLS, ARENA, Vec2::new(0.0, -330.0), true);
    let peg = b.add("obstacle", ARENA, "Peg", Vec2::ZERO, obstacle(|o| o.radius = 24.0));
    b.ride(peg, ARENA, Vec2::new(0.0, 200.0), true);
    b.bounce(|s| s.pops_after = 40);
    b.ball_look(|l| l.hue_per_hit = 12.0);
    b.finish()
}

/// One ball blown sideways, playing a climbing note on every hit and barely bouncing off the
/// soft bar.
fn pitch_climb(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Pitch climb");
    b.balls("Ball", Vec2::new(0.0, -300.0), |s| {
        s.radius = 30.0;
        s.velocity = Vec2::new(350.0, 0.0);
    });
    let bar = b.add(
        "obstacle",
        ThingId::ROOT,
        "Soft bar",
        Vec2::new(0.0, 350.0),
        obstacle(|o| {
            o.shape = ObstacleShape::Bar;
            o.length = 500.0;
            o.thickness = 12.0;
            o.path = MovePath::Line;
            o.path_distance = Vec2::new(150.0, 0.0);
            o.path_seconds = 6.0;
        }),
    );
    b.tag(bar, "soft");
    b.bounce(|s| {
        s.special = true;
        s.special_target = Target::Tag("soft".into());
        s.special_bounciness = 0.3;
    });
    let bell = b.starter("Glass FM bell");
    b.sound(|s| {
        s.order = Order::Hits;
        s.instrument = bell;
    });
    b.chords("Dreamy loop", 9);
    b.motion(|s| s.wind = Vec2::new(60.0, 0.0));
    b.finish()
}

/// Three balls that grow a little on every hit, counting their hits.
fn grow_on_hit(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Grow on hit");
    b.balls("Trio", Vec2::ZERO, |s| {
        s.arrangement = Arrangement::Ring;
        s.count = 3;
        s.ring_radius = 160.0;
        s.radius = 18.0;
        s.velocity = Vec2::new(420.0, -260.0);
    });
    b.rules(BALLS, "Grow", |g| {
        let mut simple = recipe("Grow on hit")(g);
        let grow = block(g, "do.grow");
        set(g, grow, "by", Param::Number(1.0));
        simple.extend(recipe("Count hits")(g));
        simple
    });
    b.finish()
}

/// One ball that splits in two on every hit, until there are 64.
fn split_on_hit(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Split on hit");
    b.balls("Ball", Vec2::new(0.0, -200.0), |s| {
        s.radius = 40.0;
        s.velocity = Vec2::new(500.0, -200.0);
    });
    b.bounce(|s| s.bounciness = 1.02);
    b.rules(BALLS, "Split", |g| recipe("Split on hit")(g));
    b.finish()
}

/// A block of balls, each tinted by how hard it last hit: blue for soft, red for hard.
fn colour_by_speed(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Colour by speed");
    b.scene_settings(|s| s.gravity = Vec2::new(0.0, 700.0));
    b.balls("Block", Vec2::new(0.0, -300.0), |s| {
        s.arrangement = Arrangement::Block;
        s.columns = 4;
        s.rows = 4;
        s.spacing = 70.0;
        s.radius = 22.0;
        s.velocity = Vec2::new(260.0, 0.0);
    });
    b.bounce(|s| s.bounciness = 0.95);
    b.rules(BALLS, "Colour by speed", |g| {
        let c = catalog(Role::Rules);
        let hit = g.add(c.spec("when.hit").unwrap(), [40.0, 40.0]);
        let part = g.add(c.spec("math").unwrap(), [260.0, 200.0]);
        set(g, part, "op", Param::Choice("divide".into()));
        set(g, part, "b", Param::Number(1500.0));
        wire(g, Role::Rules, (hit, 3), (part, 0));
        let hue = g.add(c.spec("clamp").unwrap(), [480.0, 200.0]);
        set(g, hue, "min", Param::Number(0.0));
        set(g, hue, "max", Param::Number(1.0));
        wire(g, Role::Rules, (part, 0), (hue, 0));
        let colour = g.add(c.spec("color.hsv").unwrap(), [700.0, 200.0]);
        wire(g, Role::Rules, (hue, 0), (colour, 0));
        let tint = g.add(c.spec("do.color").unwrap(), [920.0, 40.0]);
        wire(g, Role::Rules, (hit, 0), (tint, 0));
        wire(g, Role::Rules, (colour, 0), (tint, 2));
        vec![(tint, "amount".into())]
    });
    b.finish()
}

/// A bounce graph that keeps more speed on slow hits than on fast ones, so the ball settles
/// into a steady bounce instead of dying or running away.
fn bouncy_when_slow(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Bouncy when slow");
    b.arena(|a| {
        a.shape = ArenaShape::Box;
        a.width = 900.0;
        a.height = 1400.0;
    });
    b.balls("Ball", Vec2::new(-200.0, -300.0), |s| {
        s.radius = 36.0;
        s.velocity = Vec2::new(350.0, 0.0);
    });
    b.custom(SlotId::Bounce, |g| {
        let c = catalog(Role::Bounce);
        let input = block(g, "bounce.in");
        let reflect = block(g, "reflect");
        g.links.retain(|l| l.to != (reflect, 2));
        let slow = g.add(c.spec("compare").unwrap(), [40.0, 640.0]);
        set(g, slow, "op", Param::Choice("less".into()));
        set(g, slow, "b", Param::Number(300.0));
        wire(g, Role::Bounce, (input, 4), (slow, 0));
        let keep = g.add(c.spec("choose").unwrap(), [300.0, 640.0]);
        set(g, keep, "then", Param::Number(1.15));
        set(g, keep, "else", Param::Number(0.9));
        wire(g, Role::Bounce, (slow, 0), (keep, 0));
        wire(g, Role::Bounce, (keep, 0), (reflect, 2));
    });
    b.finish()
}

/// A motion graph with a wind that swings left and right, and a sound graph that walks up a
/// scale with each ball's hits.
fn wind_and_melody(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Wind and melody");
    b.scene_settings(|s| s.gravity = Vec2::new(0.0, 350.0));
    b.balls("Ring", Vec2::ZERO, |s| {
        s.arrangement = Arrangement::Ring;
        s.count = 6;
        s.ring_radius = 220.0;
        s.radius = 16.0;
        s.velocity = Vec2::new(0.0, -200.0);
    });
    let marimba = b.built_in(Instrument::Marimba);
    b.sound(|s| s.instrument = marimba);
    b.custom(SlotId::Motion, |g| {
        let c = catalog(Role::Motion);
        let input = block(g, "motion.in");
        let output = block(g, "motion.out");
        let from = g.links.iter().find(|l| l.to == (output, 0)).expect("the template wires the velocity").from;
        g.links.retain(|l| l.to != (output, 0));
        let faster = g.add(c.spec("math").unwrap(), [40.0, 800.0]);
        set(g, faster, "op", Param::Choice("multiply".into()));
        set(g, faster, "b", Param::Number(2.0));
        wire(g, Role::Motion, (input, 5), (faster, 0));
        let swing = g.add(c.spec("fn").unwrap(), [300.0, 800.0]);
        set(g, swing, "f", Param::Choice("sin".into()));
        wire(g, Role::Motion, (faster, 0), (swing, 0));
        let strength = g.add(c.spec("math").unwrap(), [560.0, 800.0]);
        set(g, strength, "op", Param::Choice("multiply".into()));
        set(g, strength, "b", Param::Number(400.0));
        wire(g, Role::Motion, (swing, 0), (strength, 0));
        let wind = g.add(c.spec("vec.make").unwrap(), [820.0, 800.0]);
        wire(g, Role::Motion, (strength, 0), (wind, 0));
        let over_dt = g.add(c.spec("vec.scale").unwrap(), [1080.0, 800.0]);
        wire(g, Role::Motion, (wind, 0), (over_dt, 0));
        wire(g, Role::Motion, (input, 4), (over_dt, 1));
        let blown = g.add(c.spec("vec.math").unwrap(), [1340.0, 500.0]);
        set(g, blown, "op", Param::Choice("add".into()));
        wire(g, Role::Motion, from, (blown, 0));
        wire(g, Role::Motion, (over_dt, 0), (blown, 1));
        wire(g, Role::Motion, (blown, 0), (output, 0));
    });
    b.custom(SlotId::Sound, |g| {
        let c = catalog(Role::Sound);
        let input = block(g, "sound.in");
        let output = block(g, "sound.out");
        g.links.retain(|l| l.to != (output, 1));
        let step = g.add(c.spec("math").unwrap(), [820.0, 360.0]);
        set(g, step, "op", Param::Choice("modulo".into()));
        set(g, step, "b", Param::Number(12.0));
        wire(g, Role::Sound, (input, 6), (step, 0));
        let key = g.add(c.spec("math").unwrap(), [1080.0, 360.0]);
        set(g, key, "op", Param::Choice("add".into()));
        set(g, key, "a", Param::Number(48.0));
        wire(g, Role::Sound, (step, 0), (key, 1));
        wire(g, Role::Sound, (key, 0), (output, 1));
    });
    b.finish()
}

/// Drops shot up from a spout, falling back through each other inside a ring.
fn fountain(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Fountain");
    b.arena(|a| a.radius = 500.0);
    b.balls("Drops", Vec2::new(0.0, 380.0), |s| {
        s.radius = 9.0;
        s.velocity = Vec2::ZERO;
    });
    b.bounce(|s| s.bounciness = 0.9);
    b.ball_look_by_row(&[Color::rgb(0x3e, 0xd8, 0xff), Color::rgb(0x8a, 0x4d, 0xff), Color::rgb(0xff, 0x5c, 0xa8)]);
    let spout = EmitterSettings {
        target: Target::Thing(BALLS),
        rate: 25.0,
        speed: 900.0,
        spread: 30.0,
        limit: 400,
        ..EmitterSettings::default()
    };
    b.add("emitter", ThingId::ROOT, "Spout", Vec2::new(0.0, 380.0), spout.to_params());
    b.finish()
}

/// Light feathers blown across a tall box while heavy stones fall through a whirl that
/// catches everything.
fn wind_tunnel(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Wind tunnel");
    b.scene_settings(|s| {
        s.gravity = Vec2::new(0.0, 300.0);
        s.duration = 20.0;
    });
    b.arena(|a| {
        a.shape = ArenaShape::Box;
        a.width = 900.0;
        a.height = 1500.0;
    });
    b.balls("Feathers", Vec2::new(-220.0, -520.0), |s| {
        s.arrangement = Arrangement::Block;
        s.columns = 6;
        s.rows = 6;
        s.spacing = 22.0;
        s.radius = 6.0;
        s.velocity = Vec2::ZERO;
    });
    b.tag(BALLS, "light");
    let stones = BallsSettings {
        arrangement: Arrangement::Block,
        columns: 3,
        rows: 3,
        spacing: 60.0,
        radius: 16.0,
        velocity: Vec2::ZERO,
        ..BallsSettings::default()
    };
    let stones = b.add("balls", ThingId::ROOT, "Stones", Vec2::new(220.0, -520.0), stones.to_params());
    b.tag(stones, "heavy");
    let breeze = FieldSettings {
        kind: FieldKindChoice::Wind,
        direction: 0.0,
        strength: 900.0,
        shape: FieldShape::Everywhere,
        targets: Target::Tag("light".into()),
        ..FieldSettings::default()
    };
    b.add("field", ThingId::ROOT, "Breeze", Vec2::ZERO, breeze.to_params());
    let whirl = FieldSettings {
        kind: FieldKindChoice::Vortex,
        radius: 350.0,
        strength: 1200.0,
        targets: Target::Anything,
        ..FieldSettings::default()
    };
    b.add("field", ThingId::ROOT, "Whirl", Vec2::new(0.0, 200.0), whirl.to_params());
    b.finish()
}

/// A ball dropped into a small pool that runs on the processor.
fn small_pool(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Small pool");
    b.scene_settings(|s| s.gravity = Vec2::new(0.0, 900.0));
    b.arena(|a| a.radius = 480.0);
    b.balls("Ball", Vec2::new(0.0, -500.0), |s| {
        s.radius = 40.0;
        s.velocity = Vec2::ZERO;
    });
    b.bounce(|s| s.bounciness = 0.6);
    let pool = LiquidSettings { count: 1500, fill: 0.4, ..LiquidSettings::default() };
    b.add("liquid", ThingId::ROOT, "Pool", Vec2::ZERO, pool.to_params());
    b.finish()
}

/// A block of balls dropped into a deep pool of twelve thousand particles on the graphics card.
fn deep_pool(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Deep pool");
    b.scene_settings(|s| s.gravity = Vec2::new(0.0, 900.0));
    b.arena(|a| {
        a.shape = ArenaShape::Box;
        a.width = 900.0;
        a.height = 1500.0;
    });
    b.balls("Block", Vec2::new(0.0, -400.0), |s| {
        s.arrangement = Arrangement::Block;
        s.columns = 3;
        s.rows = 2;
        s.spacing = 90.0;
        s.radius = 30.0;
        s.velocity = Vec2::ZERO;
    });
    let deep = LiquidSettings { count: 12_000, ..LiquidSettings::default() };
    b.add("liquid", ThingId::ROOT, "Deep", Vec2::ZERO, deep.to_params());
    b.finish()
}

/// Twenty thousand points of light drawn to one ball bouncing round a ring.
fn fireflies(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Fireflies");
    b.arena(|a| a.radius = 500.0);
    b.balls("Ball", Vec2::ZERO, |s| {
        s.radius = 30.0;
        s.velocity = Vec2::new(400.0, -300.0);
    });
    let swarm = SwarmSettings { count: 20_000, ..SwarmSettings::default() };
    let swarm = b.add("swarm", ThingId::ROOT, "Fireflies", Vec2::ZERO, swarm.to_params());
    let look = SwarmLook {
        colours: Paint {
            colours: Gradient::even(&[
                Color::rgb(0xff, 0xe0, 0x66),
                Color::rgb(0x8a, 0xff, 0x80),
                Color::rgb(0x40, 0xc8, 0xff),
            ]),
            follow: Follow::Order,
            // Its flocking's top speed, should it follow speed.
            span: Flocking::default().top_speed,
            ..Paint::default()
        },
        ..SwarmLook::default()
    };
    let m = b.module_of(swarm, SlotId::Look);
    b.set_params(m, look.to_params());
    let m = b.module_of(swarm, SlotId::Motion);
    b.set_params(m, Flocking { attraction: 1.2, ..Flocking::default() }.to_params());
    b.finish()
}

/// Six rainbow balls in a box, each painting a trail as wide as itself over a canvas that fits
/// the box, for twenty seconds.
fn paint_pour(reg: &Registry) -> Scene {
    pour(reg, "Paint pour", |_, _| {})
}

/// [`paint_pour`] under `name`, with `tweak` to change the canvas and the balls' brush.
fn pour(reg: &Registry, name: &str, tweak: impl FnOnce(&mut CanvasSettings, &mut MarksBrushes)) -> Scene {
    let mut b = Builder::new(reg, name);
    b.scene_settings(|s| {
        s.gravity = Vec2::new(0.0, 500.0);
        s.duration = 20.0;
    });
    b.arena(|a| {
        a.shape = ArenaShape::Box;
        a.width = 900.0;
        a.height = 1500.0;
    });
    b.balls("Painters", Vec2::new(0.0, -300.0), |s| {
        s.arrangement = Arrangement::Ring;
        s.count = 6;
        s.radius = 26.0;
        s.ring_radius = 140.0;
        s.velocity = Vec2::new(260.0, -120.0);
    });
    b.bounce(|s| s.bounciness = 0.95);
    b.ball_look(|l| {
        l.fill = Paint {
            colours: Gradient::even(&[
                Color::rgb(0xff, 0x3e, 0x5c),
                Color::rgb(0xff, 0xb8, 0x2e),
                Color::rgb(0xe8, 0xf0, 0x3e),
                Color::rgb(0x3e, 0xe0, 0x7a),
                Color::rgb(0x3e, 0x9b, 0xff),
                Color::rgb(0xa0, 0x4d, 0xff),
            ]),
            follow: Follow::Order,
            ..Paint::default()
        };
    });
    // The canvas fits the box and sits behind the balls. It is added last, so the balls already
    // have their brushes: paint trail on, as wide as the ball.
    let mut canvas = CanvasSettings::default();
    let mut brush =
        MarksBrushes { paint_on: true, paint_width_by: WidthBy::Size, paint_taper: 0.3, ..MarksBrushes::default() };
    tweak(&mut canvas, &mut brush);
    b.add("canvas", ThingId::ROOT, "Canvas", Vec2::ZERO, canvas.to_params());
    let brushes = b.module_of(BALLS, SlotId::Marks);
    b.set_params(brushes, brush.to_params());
    b.finish()
}

/// A box arena 900×1500 under `gravity`, 20 s, balls in a ring at (0, −200) called `balls_name`,
/// and a canvas called `canvas_name` with `canvas`, the balls' brushes set to `brush`.
#[allow(clippy::too_many_arguments)]
fn surface_demo(
    reg: &Registry,
    name: &str,
    canvas_name: &str,
    gravity: Vec2,
    colours: &[Color],
    balls: impl FnOnce(&mut BallsSettings),
    canvas: CanvasSettings,
    brush: MarksBrushes,
) -> Scene {
    let mut b = Builder::new(reg, name);
    b.scene_settings(|s| {
        s.gravity = gravity;
        s.duration = 20.0;
    });
    b.arena(|a| {
        a.shape = ArenaShape::Box;
        a.width = 900.0;
        a.height = 1500.0;
    });
    b.balls("Balls", Vec2::new(0.0, -200.0), |s| {
        s.arrangement = Arrangement::Ring;
        balls(s);
    });
    b.bounce(|s| s.bounciness = 1.0);
    b.ball_look(|l| {
        l.fill = Paint { colours: Gradient::even(colours), follow: Follow::Order, ..Paint::default() };
    });
    // Added last, so the balls already have their brushes.
    b.add("canvas", ThingId::ROOT, canvas_name, Vec2::ZERO, canvas.to_params());
    let brushes = b.module_of(BALLS, SlotId::Marks);
    b.set_params(brushes, brush.to_params());
    b.finish()
}

/// The colours `values` (0xrrggbb).
fn hexes(values: &[u32]) -> Vec<Color> {
    values.iter().map(|v| Color::rgb((v >> 16) as u8, (v >> 8) as u8, *v as u8)).collect()
}

/// Adds a caption `text` at `position` to a finished scene.
fn add_caption(reg: &Registry, scene: &mut Scene, name: &str, position: Vec2, text: &str, size: f32) {
    let id = crate::add_thing(scene, reg, "text", ThingId::ROOT).expect("text is registered");
    let thing = scene.things.get_mut(&id).expect("the new text");
    thing.name = name.into();
    thing.transform.position = position;
    let settings = TextSettings { text: text.into(), size, ..TextSettings::from_params(&thing.params) };
    thing.params = settings.to_params();
}

/// A foil scratch card: five balls scratch the cover off a "YOU WIN", chipping flakes away. A
/// caption counts the revealed share, and the video ends two seconds after 60 % is scraped off.
fn scratch_to_reveal(reg: &Registry) -> Scene {
    let mut scene = scratch_card_scene(reg);
    add_caption(reg, &mut scene, "Revealed", Vec2::new(0.0, -800.0), "{Card.revealed}% revealed", 72.0);
    let m = scene.add_module(reg, "rules.graph", "Finish").expect("rules are registered");
    scene.library.modules.get_mut(&m).expect("module").owner = Some(BALLS);
    let mut graph = RulesKind.template().expect("rules have a template");
    let c = catalog(Role::Rules);
    let when = graph.add(c.spec("when.canvas").expect("When a canvas reaches"), [40.0, 40.0]);
    set(&mut graph, when, "number", Param::Choice("Revealed".into()));
    set(&mut graph, when, "level", Param::Number(60.0));
    let end = graph.add(c.spec("do.end").expect("End the video"), [260.0, 40.0]);
    set(&mut graph, end, "after", Param::Number(2.0));
    wire(&mut graph, Role::Rules, (when, 0), (end, 0));
    scene.library.modules.get_mut(&m).expect("module").graph =
        Some(ModuleGraph { graph, template: "rules.graph".into(), custom: true, simple: Default::default() });
    scene.link(reg, BALLS, SlotId::Rules, m).expect("the balls have a Rules slot");
    scene
}

fn scratch_card_scene(reg: &Registry) -> Scene {
    surface_demo(
        reg,
        "Scratch to reveal",
        "Card",
        Vec2::new(0.0, 600.0),
        &hexes(&[0xffffff, 0xffd23e, 0xff5c8a, 0x3ee0d0, 0xa04dff]),
        |s| {
            s.count = 5;
            s.radius = 30.0;
            s.ring_radius = 160.0;
            s.velocity = Vec2::new(300.0, -150.0);
        },
        crate::CanvasStart::ScratchCard.settings(),
        MarksBrushes {
            paint_on: false,
            scrape_on: true,
            scrape_width_by: WidthBy::Size,
            scrape_width: 1.0,
            chip_on: true,
            chip_shape: crate::ChipShape::Splat,
            chip_size: 30.0,
            chip_min_speed: 80.0,
            ..MarksBrushes::default()
        },
    )
}

/// Four colours fight over a dark floor, tile by tile, in no gravity. A meter and a caption show
/// who is winning.
fn colour_war(reg: &Registry) -> Scene {
    let mut scene = colour_war_scene(reg);
    let meter = crate::add_thing(&mut scene, reg, "meter", ThingId::ROOT).expect("meter is registered");
    let thing = scene.things.get_mut(&meter).expect("the new meter");
    thing.name = "Score".into();
    thing.transform.position = Vec2::new(0.0, -820.0);
    let settings =
        crate::MeterSettings { width: 900.0, thickness: 48.0, ..crate::MeterSettings::from_params(&thing.params) };
    thing.params = settings.to_params();
    add_caption(reg, &mut scene, "Leader", Vec2::new(0.0, -740.0), "{War.leader} leads", 56.0);
    scene
}

fn colour_war_scene(reg: &Registry) -> Scene {
    surface_demo(
        reg,
        "Colour war",
        "War",
        Vec2::ZERO,
        &hexes(&[0xff3e5c, 0x3e9bff, 0x3ee07a, 0xffd23e]),
        |s| {
            s.count = 4;
            s.radius = 34.0;
            s.ring_radius = 250.0;
            s.velocity = Vec2::new(420.0, -260.0);
        },
        crate::CanvasStart::ColourWar.settings(),
        MarksBrushes {
            paint_on: false,
            claim_on: true,
            claim_width_by: WidthBy::Size,
            claim_width: 1.2,
            claim_hits: true,
            claim_hit_size: 60.0,
            claim_team: crate::ClaimTeam::ByColour,
            ..MarksBrushes::default()
        },
    )
}

/// Three colours claim land that keeps spreading on its own, smooth rather than tiled.
fn territory(reg: &Registry) -> Scene {
    let canvas = CanvasSettings {
        kind: crate::CanvasKind::Paint,
        starts_as: StartsAs::Colour,
        start_colour: Color::rgb(0x10, 0x10, 0x18),
        tile_size: 0.0,
        grow: 0.6,
        take_over: true,
        border: 0.3,
        ..CanvasSettings::default()
    };
    surface_demo(
        reg,
        "Territory",
        "Land",
        Vec2::new(0.0, 400.0),
        &hexes(&[0xff5ca8, 0x3ee0d0, 0xb6ff3b]),
        |s| {
            s.count = 3;
            s.radius = 26.0;
            s.ring_radius = 200.0;
            s.velocity = Vec2::new(320.0, -200.0);
        },
        canvas,
        MarksBrushes {
            paint_on: false,
            claim_on: true,
            claim_width: 1.0,
            claim_hits: true,
            claim_hit_size: 80.0,
            claim_team: crate::ClaimTeam::ByColour,
            ..MarksBrushes::default()
        },
    )
}

/// Balls plough furrows through a bed of sand that settles back.
fn sand_garden(reg: &Registry) -> Scene {
    let canvas = CanvasSettings { ridges: 0.7, grows_back: 6.0, ..crate::CanvasStart::Sand.settings() };
    surface_demo(
        reg,
        "Sand garden",
        "Sand",
        Vec2::new(0.0, 300.0),
        &hexes(&[0x5b3a29, 0x2f4858, 0x33658a, 0x86bbd8, 0xf26419, 0xf6ae2d]),
        |s| {
            s.count = 6;
            s.radius = 22.0;
            s.ring_radius = 200.0;
            s.velocity = Vec2::new(260.0, -120.0);
        },
        canvas,
        MarksBrushes { paint_on: false, scrape_on: true, scrape_width: 1.0, ..MarksBrushes::default() },
    )
}

/// Balls wipe clear a misted window; drops run down the clear streaks.
fn fog_glass(reg: &Registry) -> Scene {
    let canvas =
        CanvasSettings { density: 0.75, grows_back: 5.0, drops: 0.6, ..crate::CanvasStart::FogGlass.settings() };
    surface_demo(
        reg,
        "Fog glass",
        "Fog",
        Vec2::new(0.0, 500.0),
        &hexes(&[0xff3e5c, 0xffb82e, 0x3ee07a, 0x3e9bff, 0xa04dff]),
        |s| {
            s.count = 5;
            s.radius = 30.0;
            s.ring_radius = 180.0;
            s.velocity = Vec2::new(300.0, -200.0);
        },
        canvas,
        MarksBrushes { paint_on: false, scrape_on: true, scrape_width: 1.4, ..MarksBrushes::default() },
    )
}

/// Balls in a ring arena throw paint at the ring, which smears it along its line, and at a
/// spinning bar, which wears the prints and carries them round. No canvas: the things hold it.
fn splat_ring(reg: &Registry) -> Scene {
    let colours = [Color::rgb(0xff, 0x5c, 0xa8), Color::rgb(0xff, 0xd2, 0x3e), Color::rgb(0x3e, 0xe0, 0xd0)];
    ring_scene(reg, "Splat ring", colours, |_, _| {})
}

/// [`splat_ring`] under `name`, the balls' colours `colours`, with `tweak` to change the ring's
/// and the bar's settings (their paint skins are among them).
fn ring_scene(reg: &Registry, name: &str, colours: [Color; 3], tweak: impl FnOnce(&mut Params, &mut Params)) -> Scene {
    let mut b = Builder::new(reg, name);
    b.scene_settings(|s| {
        s.gravity = Vec2::new(0.0, 300.0);
        s.duration = 20.0;
    });
    b.arena(|a| {
        a.radius = 480.0;
        a.thickness = 22.0;
    });
    let mut bar_params = obstacle(|o| {
        o.shape = ObstacleShape::Bar;
        o.length = 560.0;
        o.thickness = 40.0;
        o.spin = 25.0;
    });
    tweak(&mut b.scene.things.get_mut(&ARENA).expect("arena").params, &mut bar_params);
    let bar = b.add("obstacle", ThingId::ROOT, "Spinning bar", Vec2::ZERO, bar_params);
    b.balls("Splatters", Vec2::new(0.0, -200.0), |s| {
        s.arrangement = Arrangement::Ring;
        s.count = 8;
        s.radius = 18.0;
        s.ring_radius = 200.0;
        s.velocity = Vec2::new(220.0, 0.0);
    });
    b.bounce(|s| s.bounciness = 1.0);
    b.ball_look(|l| {
        l.fill = Paint { colours: Gradient::even(&colours), follow: Follow::Order, ..Paint::default() };
    });
    // The balls carry the colour; the ring and the bar each have a brush of their own that
    // decides how they take it.
    let ring_brush = MarksBrushes {
        paint_on: false,
        splat_on: true,
        splat_size: 70.0,
        splat_by_speed: 0.5,
        splat_skin: SplatSkin::Spread,
        ..MarksBrushes::default()
    };
    b.own_module(ARENA, "marks.brushes", "Ring paint", ring_brush.to_params());
    let bar_brush = MarksBrushes {
        paint_on: false,
        splat_on: true,
        splat_shape: SplatShape::Splat,
        splat_turn: Turn::FaceHit,
        splat_size: 45.0,
        splat_skin: SplatSkin::Print,
        ..MarksBrushes::default()
    };
    b.own_module(bar, "marks.brushes", "Bar paint", bar_brush.to_params());
    b.finish()
}

/// Peg board where the pegs take the paint: every ball splats the pegs it hits, and its trail
/// stays on a canvas behind.
fn peg_paint(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Peg paint");
    b.scene_settings(|s| {
        s.gravity = Vec2::new(0.0, 900.0);
        s.duration = 20.0;
    });
    b.arena(|a| {
        a.shape = ArenaShape::Box;
        a.width = 900.0;
        a.height = 1500.0;
        a.corner = 60.0;
    });
    let pegs = b.add(
        "obstacle",
        ThingId::ROOT,
        "Pegs",
        Vec2::new(0.0, 100.0),
        obstacle(|o| {
            o.shape = ObstacleShape::PegGrid;
            o.columns = 7;
            o.rows = 9;
            o.spacing = 100.0;
            o.radius = 24.0;
        }),
    );
    b.tag(pegs, "pegs");
    b.balls("Drops", Vec2::new(0.0, -650.0), |s| {
        s.arrangement = Arrangement::Block;
        s.columns = 5;
        s.rows = 2;
        s.spacing = 44.0;
        s.radius = 9.0;
        s.velocity = Vec2::ZERO;
    });
    let bumpy = BasicBounce { bounciness: 0.7, ..BasicBounce::default() };
    b.own_module(BALLS, "bounce.basic", "Bumpy", bumpy.to_params());
    b.ball_look(|l| {
        l.fill = Paint {
            colours: Gradient::even(&[
                Color::rgb(0xff, 0x5c, 0xa8),
                Color::rgb(0xff, 0xd2, 0x3e),
                Color::rgb(0x3e, 0xe0, 0xd0),
                Color::rgb(0x8a, 0x6d, 0xff),
            ]),
            follow: Follow::Order,
            ..Paint::default()
        };
    });
    b.add("canvas", ThingId::ROOT, "Canvas", Vec2::ZERO, CanvasSettings::default().to_params());
    let brushes = b.module_of(BALLS, SlotId::Marks);
    let brush = MarksBrushes {
        paint_on: true,
        paint_width_by: WidthBy::Size,
        splat_on: true,
        splat_shape: SplatShape::Splat,
        splat_size: 36.0,
        splat_onto: SplatOnto::ThingHit,
        splat_skin: SplatSkin::Print,
        ..MarksBrushes::default()
    };
    b.set_params(brushes, brush.to_params());
    b.finish()
}

/// A pour with wet paint: the trails and splats stay glossy for a while, bleed into each other
/// and drip down the canvas.
fn wet_pour(reg: &Registry) -> Scene {
    pour(reg, "Wet pour", |canvas, brush| {
        canvas.style = CanvasStyle::WetPaint;
        canvas.dry_time = 2.5;
        canvas.drips = 0.6;
        canvas.bleed = 0.15;
        canvas.gloss = 0.7;
        brush.splat_on = true;
        brush.splat_onto = SplatOnto::Canvases;
    })
}

/// Balls fall through a peg board over a cream canvas, leaving soft watercolour trails and
/// blooms that spread and darken at their edges as they dry.
fn watercolour_garden(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Watercolour garden");
    b.scene_settings(|s| {
        s.gravity = Vec2::new(0.0, 900.0);
        s.duration = 20.0;
    });
    b.arena(|a| {
        a.shape = ArenaShape::Box;
        a.width = 900.0;
        a.height = 1500.0;
    });
    b.add(
        "obstacle",
        ThingId::ROOT,
        "Pegs",
        Vec2::new(0.0, 100.0),
        obstacle(|o| {
            o.shape = ObstacleShape::PegGrid;
            o.columns = 6;
            o.rows = 7;
            o.spacing = 120.0;
            o.radius = 14.0;
            o.stagger = true;
        }),
    );
    b.balls("Blooms", Vec2::new(0.0, -650.0), |s| {
        s.arrangement = Arrangement::Block;
        s.columns = 4;
        s.rows = 3;
        s.spacing = 60.0;
        s.radius = 14.0;
        s.velocity = Vec2::ZERO;
    });
    b.ball_look(|l| {
        l.fill = Paint {
            colours: Gradient::even(&[
                Color::rgb(0xe0, 0x7a, 0x8f),
                Color::rgb(0xf2, 0xb8, 0x80),
                Color::rgb(0x8f, 0xb8, 0xde),
                Color::rgb(0x9a, 0xd1, 0xa8),
            ]),
            follow: Follow::Order,
            ..Paint::default()
        };
    });
    let canvas = CanvasSettings {
        starts_as: StartsAs::Colour,
        start_colour: Color::rgb(0xf4, 0xef, 0xe4),
        style: CanvasStyle::Watercolour,
        dry_time: 3.0,
        bleed: 0.5,
        grain: 0.6,
        edge_dark: 0.6,
        ..CanvasSettings::default()
    };
    b.add("canvas", ThingId::ROOT, "Canvas", Vec2::ZERO, canvas.to_params());
    let brushes = b.module_of(BALLS, SlotId::Marks);
    let brush = MarksBrushes {
        paint_on: true,
        paint_width_by: WidthBy::Size,
        paint_width: 1.6,
        splat_on: true,
        splat_onto: SplatOnto::Canvases,
        splat_size: 40.0,
        ..MarksBrushes::default()
    };
    b.set_params(brushes, brush.to_params());
    b.finish()
}

/// The splat ring in glowing neon: paint that drips down the ring and glows while it is wet.
fn neon_splat_ring(reg: &Registry) -> Scene {
    let colours = [Color::rgb(0xff, 0x2b, 0xd6), Color::rgb(0x29, 0xf0, 0xff), Color::rgb(0xb6, 0xff, 0x3b)];
    ring_scene(reg, "Neon splat ring", colours, |ring, bar| {
        ring.set("skin_style", Value::Choice("neon_ink".into()));
        ring.set("skin_dry_time", Value::Number(2.5));
        ring.set("skin_drips", Value::Number(0.5));
        bar.set("skin_style", Value::Choice("neon_ink".into()));
        bar.set("skin_dry_time", Value::Number(2.5));
    })
}

/// Three balls in red, yellow and blue paint their trails and pick up each other's wet paint
/// where they cross, so the colours mix.
fn colour_mixing(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Colour mixing");
    b.scene_settings(|s| {
        s.gravity = Vec2::new(0.0, 200.0);
        s.duration = 20.0;
    });
    b.arena(|a| {
        a.shape = ArenaShape::Box;
        a.width = 900.0;
        a.height = 1500.0;
    });
    b.balls("Painters", Vec2::ZERO, |s| {
        s.arrangement = Arrangement::Ring;
        s.count = 3;
        s.radius = 30.0;
        s.ring_radius = 200.0;
        s.velocity = Vec2::new(320.0, -160.0);
    });
    b.bounce(|s| s.bounciness = 1.0);
    b.ball_look(|l| {
        l.fill = Paint {
            colours: Gradient::even(&[
                Color::rgb(0xe8, 0x34, 0x3a),
                Color::rgb(0xf6, 0xc6, 0x23),
                Color::rgb(0x2f, 0x6f, 0xe8),
            ]),
            follow: Follow::Order,
            ..Paint::default()
        };
    });
    let canvas =
        CanvasSettings { style: CanvasStyle::WetPaint, dry_time: 4.0, bleed: 0.25, ..CanvasSettings::default() };
    b.add("canvas", ThingId::ROOT, "Canvas", Vec2::ZERO, canvas.to_params());
    let brushes = b.module_of(BALLS, SlotId::Marks);
    let brush = MarksBrushes {
        paint_on: true,
        paint_width_by: WidthBy::Size,
        paint_width: 1.0,
        paint_pickup: 0.6,
        paint_pickup_fade: 4.0,
        ..MarksBrushes::default()
    };
    b.set_params(brushes, brush.to_params());
    b.finish()
}

#[cfg(test)]
mod tests {
    use crate::{registry, scenes};
    use bpe_core::{SlotId, Value};

    #[test]
    fn every_thing_in_a_built_in_scene_has_its_own_modules() {
        let reg = registry();
        let mut all = scenes::all(&reg);
        all.push(("Long drop".into(), scenes::long_drop(&reg)));
        for (name, scene) in &all {
            for (id, thing) in &scene.things {
                for (slot, link) in &thing.slots {
                    let bpe_core::SlotLink::Linked { module, .. } = link else { continue };
                    let m = &scene.library.modules[module];
                    assert_eq!(scene.users_of(*module), [*id], "{name}: {} {slot:?} shares {}", thing.name, m.name);
                    assert_eq!(m.owner, Some(*id), "{name}: {} {slot:?} uses {}", thing.name, m.name);
                }
            }
        }
    }

    #[test]
    fn starters_are_nine_named_scenes_from_the_collection() {
        let reg = registry();
        let starters = scenes::starters(&reg);
        assert_eq!(starters.len(), 9);
        let mut all: std::collections::BTreeMap<String, bpe_core::Scene> = scenes::all(&reg).into_iter().collect();
        all.insert("Long drop".into(), scenes::long_drop(&reg));
        let mut names = std::collections::BTreeSet::new();
        for (name, scene) in &starters {
            assert_eq!(all.get(name), Some(scene), "{name} is one of the shipped scenes");
            assert_eq!(&scene.name, name);
            assert!(names.insert(name.clone()), "{name} twice");
        }
        assert_eq!(starters[0].0, "Reel block", "the reel the first video was made from leads");
    }

    #[test]
    fn stage_scenes_resolve_and_round_trip() {
        let reg = registry();
        let scenes = scenes::all(&reg);
        assert_eq!(scenes.len(), 45);
        let in_order: Vec<String> = [
            scenes::stage2(&reg),
            scenes::stage3(&reg),
            scenes::stage4(&reg),
            scenes::stage5(&reg),
            scenes::stage6(&reg),
            scenes::stage7(&reg),
            vec![
                ("Colour paints".into(), scenes::colour_demo(&reg)),
                ("Blend modes".into(), scenes::blend_demo(&reg)),
                ("Graded sunset".into(), scenes::graded_sunset(&reg)),
            ],
            scenes::marks(&reg),
        ]
        .concat()
        .into_iter()
        .map(|(n, _)| n)
        .collect();
        let names: Vec<&str> = scenes.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, in_order);
        assert_eq!(names[..5], ["One ball", "Reel block", "Box drop", "Spinning hexagon", "Shrinking ring"]);
        assert_eq!(names[5..10], ["Peg board", "Spinning bars", "Ball pit", "Riders", "Pitch climb"]);
        assert_eq!(
            names[10..15],
            ["Grow on hit", "Split on hit", "Colour by speed", "Bouncy when slow", "Wind and melody"]
        );
        assert_eq!(names[15..20], ["Lava ball", "Confetti drop", "Spark block", "Kaleido hexagon", "Neon ring"]);
        assert_eq!(names[20..25], ["Fountain", "Wind tunnel", "Small pool", "Deep pool", "Fireflies"]);
        assert_eq!(names[25..30], ["Jelly drop", "Rope bridge", "Cloth hammock", "Captioned reel", "Drum beat"]);
        assert_eq!(
            names[30..],
            [
                "Colour paints",
                "Blend modes",
                "Graded sunset",
                "Paint pour",
                "Splat ring",
                "Peg paint",
                "Wet pour",
                "Watercolour garden",
                "Neon splat ring",
                "Colour mixing",
                "Scratch to reveal",
                "Colour war",
                "Territory",
                "Sand garden",
                "Fog glass"
            ]
        );
        let mut scenes = scenes;
        scenes.push(("Long drop".into(), scenes::long_drop(&reg)));
        for (name, scene) in &scenes {
            assert_eq!(&scene.name, name);
            assert!(crate::resolve_scene(scene, &reg).problems.is_empty(), "{name}");
            let text = bpe_core::save(scene);
            assert_eq!(&bpe_core::load(&text, &reg).unwrap(), scene, "{name}");
            if std::env::var_os("BPE_BLESS").is_some() {
                let dir = format!("{}/../../scenes/next", env!("CARGO_MANIFEST_DIR"));
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(format!("{dir}/{}.ron", name.to_lowercase().replace(' ', "-")), text).unwrap();
            }
        }
    }

    #[test]
    fn marks_scenes_resolve_cleanly() {
        let reg = registry();
        let marks = scenes::marks(&reg);
        let names: Vec<&str> = marks.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "Paint pour",
                "Splat ring",
                "Peg paint",
                "Wet pour",
                "Watercolour garden",
                "Neon splat ring",
                "Colour mixing",
                "Scratch to reveal",
                "Colour war",
                "Territory",
                "Sand garden",
                "Fog glass"
            ]
        );
        for (name, scene) in &marks {
            assert_eq!(&scene.name, name);
            assert!(crate::resolve_scene(scene, &reg).problems.is_empty(), "{name}");
            let canvases = scene.things.values().filter(|t| t.kind == "canvas").count();
            let brushed = scene.things.values().filter(|t| t.slots.contains_key(&SlotId::Marks)).count();
            assert!(brushed > 0, "{name}: something paints");
            // The splat ring has no canvas: the ring and the bar hold the paint themselves.
            assert_eq!(canvases, usize::from(name != "Splat ring" && name != "Neon splat ring"), "{name}: canvases");
        }
        let ring = &marks[1].1;
        let brush = |name: &str| {
            let thing = ring
                .things
                .values()
                .find(|t| t.id == bpe_core::ThingId(1) && name == "Arena" || t.name == name)
                .expect(name);
            let bpe_core::SlotLink::Linked { module, .. } = &thing.slots[&SlotId::Marks] else { panic!("{name}") };
            ring.library.modules[module].params.clone()
        };
        assert_eq!(brush("Arena").get("splat_skin"), Some(&Value::Choice("spread".into())));
        // Print is the default, so the bar's brush doesn't store it.
        assert_ne!(brush("Spinning bar").get("splat_skin"), Some(&Value::Choice("spread".into())));
        let peg = &marks[2].1;
        assert!(peg.things.values().any(|t| t.kind == "obstacle"), "pegs");
    }

    #[test]
    fn no_built_in_scene_uses_additive_colours() {
        use bpe_core::Value;
        fn additive(v: &Value) -> bool {
            match v {
                Value::Color(c) => c.is_additive(),
                Value::Gradient(g) => g.stops.iter().any(|s| s.color.is_additive()),
                _ => false,
            }
        }
        let reg = registry();
        let mut all = scenes::all(&reg);
        all.push(("Long drop".into(), scenes::long_drop(&reg)));
        for (name, scene) in &all {
            let mut values: Vec<(String, &Value)> = Vec::new();
            for (id, m) in &scene.library.modules {
                values.extend(m.params.0.iter().map(|(k, v)| (format!("module {id:?} {k}"), v)));
            }
            for (id, t) in &scene.things {
                values.extend(t.params.0.iter().map(|(k, v)| (format!("thing {id:?} {k}"), v)));
            }
            for (a, track) in &scene.tracks {
                values.extend(track.keys.iter().map(|k| (format!("track {a:?}"), &k.value)));
            }
            for (at, v) in values {
                assert!(!additive(v), "{name}: {at} is an additive colour, use Blend Add instead");
            }
        }
    }

    #[test]
    fn the_shipped_scene_files_match_the_code() {
        let reg = registry();
        let mut shipped = scenes::all(&reg);
        shipped.push(("Long drop".into(), scenes::long_drop(&reg)));
        for (name, scene) in shipped {
            let file = format!(
                "{}/../../scenes/next/{}.ron",
                env!("CARGO_MANIFEST_DIR"),
                name.to_lowercase().replace(' ', "-")
            );
            let text = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{file}: {e} (run with BPE_BLESS=1)"));
            assert_eq!(
                bpe_core::load(&text, &reg).unwrap(),
                scene,
                "{name}: scenes/next is stale, run with BPE_BLESS=1"
            );
        }
    }

    #[test]
    fn long_drop_keeps_the_ball_in_shot() {
        let kit = crate::kit();
        let scene = scenes::long_drop(&kit.registry);
        let resolved = crate::resolve_scene(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let mut world = bpe_runtime::World::build(&scene, &resolved, &kit.sims);
        let mut log = bpe_runtime::Log::default();
        let mut lowest = f32::MIN;
        for i in 1..=200 {
            world.advance_to(i as f64 * 0.1, &kit.sims, &mut log);
            let bodies = &world.bodies[&bpe_core::ThingId(2)];
            let p = bodies.position[0];
            lowest = lowest.max(p.y);
            assert!(
                world.camera.sees(world.video, p, -bodies.radius[0]),
                "at {:.1} s the ball at {p:?} left {:?}",
                i as f64 * 0.1,
                world.camera
            );
        }
        assert!(lowest > 2000.0, "the ball got down the course: {lowest}");
    }

    // ---- the rich paint demos ----

    fn marks_scene(name: &str) -> (bpe_core::Scene, bpe_core::Resolved, crate::Kit) {
        let kit = crate::kit();
        let (_, scene) = scenes::marks(&kit.registry).into_iter().find(|(n, _)| n == name).expect(name);
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        (scene, resolved, kit)
    }

    fn canvas_of(scene: &bpe_core::Scene) -> bpe_core::ThingId {
        *scene.things.iter().find(|(_, t)| t.kind == "canvas").expect("a canvas").0
    }

    #[test]
    fn wet_pour_drips_and_bleeds() {
        use bpe_runtime::{MarkShape, MarkTarget};
        let (scene, resolved, kit) = marks_scene("Wet pour");
        let canvas = canvas_of(&scene);
        let (_, log) = crate::testing::run(&scene, &resolved, &kit, 10.0);
        let has = |shape| log.marks().iter().any(|m| m.target == MarkTarget::Canvas(canvas) && m.shape == shape);
        assert!(has(MarkShape::Drip), "drips");
        assert!(has(MarkShape::Bleed), "bleeds");
    }

    #[test]
    fn watercolour_garden_bleeds() {
        use crate::things::canvas::sim::CanvasState;
        use bpe_runtime::MarkShape;
        let (scene, resolved, kit) = marks_scene("Watercolour garden");
        let canvas = canvas_of(&scene);
        let (world, log) = crate::testing::run(&scene, &resolved, &kit, 10.0);
        assert!(log.marks().iter().any(|m| m.shape == MarkShape::Bleed));
        assert!(crate::testing::canvas_state(&world, canvas).ticks_run > 0);
        let _: Option<&CanvasState> = None;
    }

    #[test]
    fn neon_splat_ring_drips_downhill() {
        use bpe_runtime::{MarkShape, MarkTarget};
        let (scene, resolved, kit) = marks_scene("Neon splat ring");
        let (_, log) = crate::testing::run(&scene, &resolved, &kit, 15.0);
        let drips: Vec<_> = log
            .marks()
            .iter()
            .filter(|m| m.target == MarkTarget::Skin(super::ARENA) && m.shape == MarkShape::Drip)
            .collect();
        assert!(!drips.is_empty(), "the ring drips");
        // Paint runs down the ring; a drop, where one comes to rest, sits low on it. (Most drips
        // run out of paint on the way: only the ones that start low reach the bottom.)
        for d in &drips {
            assert!(d.to.y >= d.from.y - 2.0, "{d:?} runs uphill");
            if d.from == d.to {
                assert!(d.from.y > 0.8 * 480.0, "{d:?}");
            }
        }
    }

    /// The linear distance between two colours' strongest channel difference.
    fn far(a: [f32; 4], b: [f32; 3]) -> f32 {
        (0..3).map(|c| (a[c] - b[c]).abs()).fold(0.0, f32::max)
    }

    #[test]
    fn colour_mixing_picks_up_colour() {
        use bpe_runtime::MarkShape;
        let (scene, resolved, kit) = marks_scene("Colour mixing");
        let mut world = bpe_runtime::World::build(&scene, &resolved, &kit.sims);
        let mut log = bpe_runtime::Log::default();
        let mut best = 0.0f32;
        for k in 1..=40 {
            world.advance_to(k as f64 * 0.5, &kit.sims, &mut log);
            let now = world.step;
            let sps = world.steps_per_second;
            for list in world.picked.values() {
                for p in list {
                    best = best.max(p.shown(now, sps));
                }
            }
        }
        assert!(best > 0.2, "a ball carried paint: {best}");
        let bases = [
            bpe_core::Color::rgb(0xe8, 0x34, 0x3a).to_linear(),
            bpe_core::Color::rgb(0xf6, 0xc6, 0x23).to_linear(),
            bpe_core::Color::rgb(0x2f, 0x6f, 0xe8).to_linear(),
        ];
        let mixed = log
            .marks()
            .iter()
            .any(|m| m.shape == MarkShape::Stroke && bases.iter().all(|b| far(m.colour, [b[0], b[1], b[2]]) > 0.15));
        assert!(mixed, "some stroke is a mix");
    }

    #[test]
    fn colour_mixing_log_is_the_same_without_pickup() {
        let (scene, resolved, kit) = marks_scene("Colour mixing");
        let (_, with) = crate::testing::run(&scene, &resolved, &kit, 20.0);
        let mut scene = scene;
        let brush = crate::testing::module_of(&scene, &kit.registry, super::BALLS, bpe_core::SlotId::Marks);
        scene.library.modules.get_mut(&brush).unwrap().params.set("paint_pickup", bpe_core::Value::Number(0.0));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let (_, without) = crate::testing::run(&scene, &resolved, &kit, 20.0);
        assert_eq!((with.hash(), with.len()), (without.hash(), without.len()));
    }

    #[test]
    fn evolution_marks_stay_bounded() {
        use bpe_core::Value;
        let (mut scene, _, kit) = crate::testing::painted(true, |_| {});
        let canvas_params = &mut scene.things.get_mut(&crate::testing::CANVAS).unwrap().params;
        canvas_params.set("fit", Value::Choice("custom".into()));
        canvas_params.set("size", Value::Vec2([1080.0, 1920.0]));
        canvas_params.set("bleed", Value::Number(0.5));
        canvas_params.set("drips", Value::Number(0.5));
        canvas_params.set("dry_time", Value::Number(2.0));
        let balls = scene.things.get_mut(&super::BALLS).unwrap();
        balls.params.set("arrangement", Value::Choice("block".into()));
        balls.params.set("columns", Value::Int(5));
        balls.params.set("rows", Value::Int(4));
        crate::testing::brushes(&mut scene, &kit, super::BALLS, |p| {
            p.set("paint_on", Value::Bool(true));
            p.set("paint_pickup", Value::Number(0.5));
        });
        let canvas = canvas_of(&scene);
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut world = bpe_runtime::World::build(&scene, &resolved, &kit.sims);
        let mut log = bpe_runtime::Log::default();
        for second in 1..=20 {
            world.advance_to(second as f64, &kit.sims, &mut log);
            let st = crate::testing::canvas_state(&world, canvas);
            assert!(st.runners.len() <= 96, "{} runners at {second} s", st.runners.len());
        }
        let st = crate::testing::canvas_state(&world, canvas);
        let (tx, ty) = st.grid.tile_dims();
        let wet_tiles_max = (tx * ty) as usize;
        let count = |shape| log.marks().iter().filter(|m| m.shape == shape).count();
        assert!(count(bpe_runtime::MarkShape::Drip) > 0 && count(bpe_runtime::MarkShape::Bleed) > 0, "it evolves");
        let per_second = log.marks().len() / 20;
        assert!(per_second <= 3000 + 30 * 20 + 30 * wet_tiles_max, "{per_second} marks a second");
    }
}
