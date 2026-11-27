//! A Water scene's things (Water mode part 1): the Course, Pieces, Walls, Liquids, Pours,
//! Drains, Forces and Lamps, as data. None has settings of its own: everything is on its cards
//! (`crate::modules::water`), and where a placed thing is, is its own place (`Thing.transform`).
//! None steps or draws yet, so the ball engine ignores them.

use bpe_core::{Registry, Scene, SettingDef, SimKind, SlotId, ThingId, ThingKind, Value};

pub mod draft;
pub mod finals;
pub mod make;

pub use crate::modules::water::StartingLook;
use crate::modules::water::{BACK, LIGHT, WORLD};

/// The one Course: the solid the water runs through, carved away where it is open.
pub const COURSE: &str = "water_course";
/// A ready-made or drawn solid piece: a ramp, a funnel, a wheel.
pub const PIECE: &str = "water_piece";
/// A drawn line of solid.
pub const WALL: &str = "water_wall";
/// A liquid: its feel, its look, how it mixes.
pub const LIQUID: &str = "water_liquid";
/// Where a liquid comes in.
pub const POUR: &str = "water_pour";
/// Where liquid leaves.
pub const DRAIN: &str = "water_drain";
/// A magnet, a whirlpool, wind, a heater or a cooler.
pub const FORCE: &str = "water_force";
/// A light in the course.
pub const LAMP: &str = "water_lamp";

/// Every Water thing kind's id, in the order the Add menu's families list them.
pub const KINDS: [&str; 8] = [COURSE, PIECE, WALL, LIQUID, POUR, DRAIN, FORCE, LAMP];

/// The name a new Water scene has until it is given one.
pub const NEW_NAME: &str = "Water course";

/// Whether a thing of `kind` sits at a place in the course (a marker on the Water stage): every
/// Water thing but the Course, which is the whole stage, and a Liquid, which is wherever it is
/// poured.
pub fn is_placed(kind: &str) -> bool {
    KINDS.contains(&kind) && kind != COURSE && kind != LIQUID
}

/// One Water thing kind.
pub struct WaterKind {
    id: &'static str,
    name: &'static str,
    icon: &'static str,
    help: &'static str,
    family: &'static str,
    single: bool,
    slots: &'static [SlotId],
    default_slots: &'static [SlotId],
    /// What the editor calls its slots, where a slot's own name doesn't fit (a placed thing's one
    /// card is called after the thing).
    label: &'static [(SlotId, &'static str)],
}

impl ThingKind for WaterKind {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        self.name
    }
    fn icon(&self) -> &'static str {
        self.icon
    }
    fn help(&self) -> &'static str {
        self.help
    }
    fn settings(&self) -> &'static [SettingDef] {
        &[]
    }
    fn slots(&self) -> &'static [SlotId] {
        self.slots
    }
    fn default_slots(&self) -> &'static [SlotId] {
        self.default_slots
    }
    fn can_stick_to_screen(&self) -> bool {
        false
    }
    fn single(&self) -> bool {
        self.single
    }
    fn family(&self) -> &'static str {
        self.family
    }
    fn slot_label(&self, slot: SlotId) -> &'static str {
        if slot == SlotId::Main && self.slots == MAIN {
            return self.name;
        }
        self.label.iter().find(|(s, _)| *s == slot).map_or(slot.name(), |(_, label)| label)
    }
    fn in_sim(&self, sim: SimKind) -> bool {
        sim == SimKind::Water
    }
}

const SOLID: &[SlotId] = &[SlotId::Form, SlotId::Material, SlotId::Moves];
const MAIN: &[SlotId] = &[SlotId::Main];

/// Every Water thing kind, in [`KINDS`]' order.
fn kinds() -> [WaterKind; 8] {
    let placed = |id, name, icon, help, family| WaterKind {
        id,
        name,
        icon,
        help,
        family,
        single: false,
        slots: MAIN,
        default_slots: MAIN,
        label: &[],
    };
    [
        WaterKind {
            id: COURSE,
            name: "Course",
            icon: "course",
            help: "The solid the water runs through. Carve it away to make the caves, channels and pools.",
            family: "Other",
            single: true,
            slots: &[SlotId::Form, SlotId::Material],
            default_slots: &[SlotId::Form, SlotId::Material],
            label: &[(SlotId::Form, "Carving"), (SlotId::Material, "Rock")],
        },
        WaterKind {
            id: PIECE,
            name: "Piece",
            icon: "piece",
            help: "A ready-made solid piece: a ramp, a funnel, a pipe, a wheel, a bucket. It can move.",
            family: "Course",
            single: false,
            slots: SOLID,
            default_slots: SOLID,
            label: &[(SlotId::Form, "Shape")],
        },
        WaterKind {
            id: WALL,
            name: "Wall",
            icon: "wall",
            help: "A line of solid you draw with the pen. It can move.",
            family: "Course",
            single: false,
            slots: SOLID,
            default_slots: SOLID,
            label: &[(SlotId::Form, "Line")],
        },
        WaterKind {
            id: LIQUID,
            name: "Liquid",
            icon: "liquid",
            help: "A liquid: how it flows, how it looks and how it mixes. Pours bring it into the course.",
            family: "Liquids",
            single: false,
            slots: &[SlotId::Feel, SlotId::Look, SlotId::Mixing, SlotId::Heat],
            default_slots: &[SlotId::Feel, SlotId::Look, SlotId::Mixing],
            label: &[],
        },
        placed(
            POUR,
            "Pour",
            "pour",
            "Where a liquid comes in: a tap, rain, a drip, a burst, or a fill at the start.",
            "Liquids",
        ),
        placed(DRAIN, "Drain", "drain", "Where liquid leaves the course.", "Liquids"),
        placed(
            FORCE,
            "Force",
            "force",
            "Pushes or heats the liquid near it: a magnet, a whirlpool, wind, a heater or a cooler.",
            "Forces",
        ),
        placed(LAMP, "Lamp", "lamp", "A light in the course: a spot, a strip or the sun.", "Lights"),
    ]
}

/// Adds every Water thing kind.
pub fn register(reg: &mut Registry) {
    for k in kinds() {
        reg.add_thing(k);
    }
}

/// The scene's Course.
pub fn course_of(scene: &Scene) -> Option<ThingId> {
    scene.order.iter().copied().find(|id| scene.things.get(id).is_some_and(|t| t.kind == COURSE))
}

/// The strokes carved out of the scene's Course (none when it has no Carving card).
pub fn carving_of(scene: &Scene) -> bpe_core::Strokes {
    let card = course_of(scene).and_then(|id| scene.things[&id].slots.get(&SlotId::Form));
    match card.and_then(|c| c.params.get("carved")) {
        Some(Value::Strokes(s)) => s.clone(),
        _ => bpe_core::Strokes::default(),
    }
}

/// A new Water scene: Scene settings with their Water, Light and Back cards, a Camera and the
/// Course. Nothing else: no liquid and no pour. `look` is only recorded on the Water card (the
/// looks themselves arrive in a later part); an empty `name` gives [`NEW_NAME`].
pub fn new_water_scene(reg: &Registry, look: StartingLook, name: &str) -> Scene {
    let mut scene = Scene::empty(reg);
    scene.sim = SimKind::Water;
    scene.name = if name.trim().is_empty() { NEW_NAME.into() } else { name.trim().into() };
    crate::ensure_camera(&mut scene, reg);
    let root = ThingId::ROOT;
    for (slot, card) in [(SlotId::Water, WORLD), (SlotId::WaterLight, LIGHT), (SlotId::Back, BACK)] {
        scene.add_slot(reg, root, slot, card).expect("the kit has the Water scene's cards");
    }
    if look != StartingLook::Plain {
        use bpe_core::Choice;
        let card = scene.things.get_mut(&root).and_then(|t| t.slots.get_mut(&SlotId::Water)).expect("just added");
        card.params.set("look", Value::Choice(look.name().into()));
    }
    crate::add_thing(&mut scene, reg, COURSE, root).expect("the kit has the Course");
    scene
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::water as cards;
    use bpe_core::{EditError, Settings};

    #[test]
    fn a_new_water_scene_is_settings_a_camera_and_the_course() {
        let reg = crate::registry();
        let scene = new_water_scene(&reg, StartingLook::Neon, "Cave run");
        assert_eq!((scene.sim, scene.name.as_str()), (SimKind::Water, "Cave run"));
        let kinds: Vec<&str> = scene.order.iter().map(|id| scene.things[id].kind.as_str()).collect();
        assert_eq!(kinds, ["scene", "camera", COURSE]);
        let root: Vec<_> = scene.things[&ThingId::ROOT].slots.iter().map(|(s, c)| (*s, c.kind.as_str())).collect();
        assert_eq!(root, [(SlotId::Water, WORLD), (SlotId::WaterLight, LIGHT), (SlotId::Back, BACK)]);
        let resolved = bpe_core::resolve(&scene, &reg);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let w = cards::WaterWorld::from_params(&resolved.things[&ThingId::ROOT].slots[&SlotId::Water].params);
        assert_eq!(w.look, StartingLook::Neon);
        let text = bpe_core::save(&scene);
        assert!(text.contains("sim: Water"), "{text}");
        assert_eq!(bpe_core::load(&text, &reg).unwrap(), scene, "it saves and loads");
        assert_eq!(new_water_scene(&reg, StartingLook::Plain, " ").name, "Water course");
        assert_eq!(course_of(&scene).map(|id| scene.things[&id].name.as_str()), Some("Course"));
        assert!(carving_of(&scene).list.is_empty(), "a new course is all rock");
    }

    #[test]
    fn the_carving_is_read_from_the_course() {
        let reg = crate::registry();
        let mut scene = new_water_scene(&reg, StartingLook::Plain, "");
        let dot = bpe_core::Stroke { points: vec![[1.0, 2.0]], size: 50.0, strength: 1.0, ..Default::default() };
        let strokes = bpe_core::Strokes { list: vec![dot] };
        let course = course_of(&scene).unwrap();
        let card = scene.things.get_mut(&course).unwrap().slots.get_mut(&SlotId::Form).unwrap();
        card.params.set("carved", Value::Strokes(strokes.clone()));
        assert_eq!(carving_of(&scene), strokes);
        scene.things.get_mut(&course).unwrap().slots.remove(&SlotId::Form);
        assert!(carving_of(&scene).list.is_empty(), "a course from before the Carving card is all rock");
    }

    #[test]
    fn each_water_kind_starts_with_its_own_cards() {
        let reg = crate::registry();
        let want: [(&str, &[(SlotId, &str)]); 8] = [
            (COURSE, &[(SlotId::Form, cards::CARVE), (SlotId::Material, cards::MATERIAL)]),
            (
                PIECE,
                &[(SlotId::Form, cards::SHAPE), (SlotId::Material, cards::MATERIAL), (SlotId::Moves, cards::MOVES)],
            ),
            (WALL, &[(SlotId::Form, cards::LINE), (SlotId::Material, cards::MATERIAL), (SlotId::Moves, cards::MOVES)]),
            (LIQUID, &[(SlotId::Look, cards::LOOK), (SlotId::Feel, cards::FEEL), (SlotId::Mixing, cards::MIXING)]),
            (POUR, &[(SlotId::Main, cards::POUR)]),
            (DRAIN, &[(SlotId::Main, cards::DRAIN)]),
            (FORCE, &[(SlotId::Main, cards::FORCE)]),
            (LAMP, &[(SlotId::Main, cards::LAMP)]),
        ];
        let mut scene = new_water_scene(&reg, StartingLook::Plain, "");
        for (kind, cards) in want {
            let id = match kind {
                COURSE => course_of(&scene).unwrap(),
                _ => crate::add_thing(&mut scene, &reg, kind, ThingId::ROOT).unwrap(),
            };
            let thing = &scene.things[&id];
            let got: Vec<(SlotId, &str)> = thing.slots.iter().map(|(s, c)| (*s, c.kind.as_str())).collect();
            assert_eq!(got, cards, "{kind}");
            assert!(thing.slots.values().all(|c| c.params.0.is_empty() && c.layers.is_empty()), "{kind}");
            assert!(thing.params.0.is_empty(), "{kind} has no settings of its own");
            let k = reg.thing(kind).unwrap();
            assert_eq!(k.name(), thing.name, "{kind}");
            for slot in k.slots() {
                assert_eq!(reg.kinds_fitting(*slot, k).count(), 1, "{kind} {slot:?}: one card kind fits");
            }
        }
        // A Liquid's Heat card is added when wanted.
        let liquid = scene.order.iter().copied().find(|id| scene.things[id].kind == LIQUID).unwrap();
        crate::fill_slot(&mut scene, &reg, liquid, SlotId::Heat).unwrap();
        assert_eq!(scene.things[&liquid].slots[&SlotId::Heat].kind, cards::HEAT);
        let resolved = bpe_core::resolve(&scene, &reg);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        assert_eq!(resolved.things.len(), scene.things.len());
    }

    #[test]
    fn the_cards_are_named_for_what_they_are_on() {
        let reg = crate::registry();
        let label = |kind: &str, slot| reg.thing(kind).unwrap().slot_label(slot);
        assert_eq!((label(COURSE, SlotId::Form), label(COURSE, SlotId::Material)), ("Carving", "Rock"));
        assert_eq!(label(PIECE, SlotId::Material), "Material");
        assert_eq!((label(PIECE, SlotId::Form), label(WALL, SlotId::Form)), ("Shape", "Line"));
        assert_eq!((label(POUR, SlotId::Main), label(LAMP, SlotId::Main)), ("Pour", "Lamp"));
        assert_eq!(label("scene", SlotId::WaterLight), "Light");
        assert!(is_placed(POUR) && is_placed(PIECE) && !is_placed(COURSE) && !is_placed(LIQUID) && !is_placed("balls"));
    }

    #[test]
    fn water_kinds_go_only_in_water_scenes_and_ball_kinds_only_in_balls_scenes() {
        let reg = crate::registry();
        let mut balls = crate::new_studio_scene(&reg);
        let mut water = new_water_scene(&reg, StartingLook::Plain, "");
        for kind in reg.things().filter(|k| !["scene", "camera", COURSE].contains(&k.id())) {
            let in_water = KINDS.contains(&kind.id()) || kind.id() == "folder";
            let in_balls = !KINDS.contains(&kind.id());
            assert_eq!(kind.in_sim(SimKind::Water), in_water, "{}", kind.id());
            assert_eq!(kind.in_sim(SimKind::Balls), in_balls, "{}", kind.id());
            assert_eq!(
                crate::add_thing(&mut water, &reg, kind.id(), ThingId::ROOT).is_ok(),
                in_water,
                "{} in a Water scene",
                kind.id()
            );
            assert_eq!(
                crate::add_thing(&mut balls, &reg, kind.id(), ThingId::ROOT).is_ok(),
                in_balls,
                "{} in a Balls scene",
                kind.id()
            );
        }
        assert_eq!(crate::add_thing(&mut water, &reg, COURSE, ThingId::ROOT), Err(EditError::OnlyOne(COURSE.into())));
        assert_eq!(
            crate::add_thing(&mut balls, &reg, COURSE, ThingId::ROOT),
            Err(EditError::WrongScene(COURSE.into(), SimKind::Balls))
        );
        for both in ["scene", "camera", "folder"] {
            let k = reg.thing(both).unwrap();
            assert!(k.in_sim(SimKind::Balls) && k.in_sim(SimKind::Water), "{both}");
        }
    }

    #[test]
    fn a_water_scene_builds_an_empty_ball_world() {
        let reg = crate::registry();
        let mut scene = new_water_scene(&reg, StartingLook::Plain, "");
        for kind in KINDS.into_iter().filter(|k| *k != COURSE) {
            crate::add_thing(&mut scene, &reg, kind, ThingId::ROOT).unwrap();
        }
        let resolved = bpe_core::resolve(&scene, &reg);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let sims = crate::sims();
        let mut world = bpe_runtime::World::build(&scene, &resolved, &sims);
        assert!(world.bodies.is_empty());
        let mut log = bpe_runtime::Log::default();
        world.advance_to(0.5, &sims, &mut log);
        assert!(world.bodies.is_empty() && log.events().is_empty(), "nothing moves and nothing happens");
        let _ = bpe_runtime::fingerprint(&scene, &resolved, &sims);
    }

    #[test]
    fn water_things_vary_nothing() {
        let reg = crate::registry();
        for kind in KINDS {
            let k = reg.thing(kind).unwrap();
            assert!(k.settings().is_empty() && !k.can_stick_to_screen(), "{kind}");
            assert!(k.vary("anything").is_none(), "{kind}");
        }
    }
}
