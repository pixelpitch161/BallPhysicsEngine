//! The test scenes: ones that exercise every setting and pin the physics in the golden tests.
//! They are not in the program (see the `test-scenes` feature); the editor always opens to a
//! blank project. They keep the values the settings had before the blank project's defaults
//! (see [`classic_look`], [`classic_motion`] and [`classic_scene_settings`]), so the goldens and
//! pictures stay put.

#[path = "scenes_emitter.rs"]
mod emitter;
#[path = "scenes_stage5.rs"]
mod stage5;
pub use emitter::emitter_scenes;
pub use stage5::{stage5, stage5_with_twins};

use bpe_core::{
    Address, Attach, Blend, Color, Ease, Gradient, Params, Registry, Scene, Settings, Slot, SlotId, ThingId, Value,
};
use glam::Vec2;
use std::collections::BTreeMap;

use crate::modules::look::layers::particles::{ParticleSource, ParticlesLayer};
use crate::modules::look::layers::trail::{TrailLayer, TrailSource};
use crate::paint::{Follow, InsideFrom, InsidePaint, Paint, Pattern, Repeat};
use crate::{BallsSettings, BasicBounce, BasicLook, BasicMotion, SceneSettings, ShapeSettings, Style, new_scene};

const SHAPE: ThingId = ThingId(1);
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

/// The stage 3 scenes: balls hitting balls in a tall box, and balls riding a wall.
pub fn stage3(reg: &Registry) -> Vec<(String, Scene)> {
    build_all(reg, &[ball_pit, riders])
}

/// Every shipped scene, stage by stage.
pub fn all(reg: &Registry) -> Vec<(String, Scene)> {
    let mut scenes = stage2(reg);
    scenes.extend(stage3(reg));
    scenes.extend(stage5(reg));
    scenes.push(("Colour paints".into(), colour_demo(reg)));
    scenes.push(("Blend modes".into(), blend_demo(reg)));
    scenes.push(("Graded sunset".into(), graded_sunset(reg)));
    scenes.push(("Ellipse focus".into(), ellipse_focus(reg)));
    scenes.extend(emitter_scenes(reg));
    scenes
}

/// A look with the glow every look had before the blank project's defaults.
pub fn classic_look() -> BasicLook {
    BasicLook { glow: 0.12, ..BasicLook::default() }
}

/// Motion with gravity on, the way it was before the blank project's defaults.
pub fn classic_motion() -> BasicMotion {
    BasicMotion { gravity_scale: 1.0, ..BasicMotion::default() }
}

/// A bounce with the top speed and biggest size every bounce had before they could be turned off.
pub fn classic_bounce() -> BasicBounce {
    BasicBounce { no_speed_limit: false, no_size_limit: false, ..BasicBounce::default() }
}

/// Gives the scene settings the dark blue background and the bloom they had before the blank
/// project's grey and no bloom, keeping anything else already set.
pub fn classic_scene_settings(scene: &mut Scene) {
    let root = scene.things.get_mut(&ThingId::ROOT).expect("root");
    let mut s = SceneSettings::from_params(&root.params);
    s.background = Paint {
        colours: Gradient::even(&[Color::rgb(0x02, 0x02, 0x2a), Color::rgb(0x01, 0x00, 0x06)]),
        ..Paint::default()
    };
    s.bloom = 0.15;
    root.params = s.to_params();
}

/// The pink look the test scenes' first ball wears.
pub fn classic_ball_look() -> BasicLook {
    BasicLook { fill: Paint::solid(Color::rgb(0xff, 0x5c, 0xa8)), ..classic_look() }
}

/// Turns every look and motion card still at the new defaults into the classic ones, so a
/// fresh starter in a test scene glows and falls the way it used to. The test scenes are written
/// with the old look on balls too; every such card then becomes the stack of layers that looks
/// most like it (see [`layers_from_basic`]).
pub fn classic_cards(scene: &mut Scene) {
    for thing in scene.things.values_mut() {
        let balls = thing.kind == "balls";
        for card in thing.slots.values_mut() {
            match card.kind.as_str() {
                "look.basic" if BasicLook::from_params(&card.params) == BasicLook::default() => {
                    card.params = classic_look().to_params();
                }
                "motion.basic" if BasicMotion::from_params(&card.params) == BasicMotion::default() => {
                    card.params = classic_motion().to_params();
                }
                // The test scenes keep the speed and size caps bounces had before.
                "bounce.basic" => {
                    for key in ["no_speed_limit", "no_size_limit"] {
                        if card.params.get(key).is_none() {
                            card.params.set(key, Value::Bool(false));
                        }
                    }
                }
                _ => {}
            }
            if balls && card.kind == "look.basic" {
                *card = layers_from_basic(&BasicLook::from_params(&card.params));
            }
        }
    }
    // Trail and Particles cards (as test scenes are written) become layers of the Look.
    crate::modules::look::layers::cards_to_layers::cards_to_layers(scene);
}

/// Puts an old Trail or Particles card of `kind` with `params` on `thing` (on Scene settings,
/// on every ball group), for [`classic_cards`] to turn into a layer; false for another kind.
pub(crate) fn put_old_card(scene: &mut Scene, thing: ThingId, kind: &str, params: &Params) -> bool {
    use crate::modules::effects::upgrade;
    use crate::modules::look::layers::cards_to_layers::{OLD_PARTICLES, OLD_TRAIL};
    if upgrade::is_old(kind) {
        // The scene's one effect becomes its Effects stack; one on a thing is left out.
        if thing == ThingId::ROOT
            && let Some(stack) = upgrade::stack_of(kind, params)
        {
            scene.things.get_mut(&thing).expect("the scene").slots.insert(SlotId::Shader, stack);
        }
        return true;
    }
    let slot = match kind {
        OLD_TRAIL => SlotId::Trail,
        OLD_PARTICLES => SlotId::Particles,
        _ => return false,
    };
    let takers: Vec<ThingId> = if thing == ThingId::ROOT {
        scene.order.iter().copied().filter(|id| scene.things[id].kind == "balls").collect()
    } else {
        vec![thing]
    };
    for id in takers {
        let card = Slot { kind: kind.into(), params: params.clone(), ..Slot::default() };
        scene.things.get_mut(&id).expect("listed").slots.insert(slot, card);
    }
    true
}

/// The stack of layers that draws most like the old look `look` on a ball: its Body (shape and
/// colours), a Pattern for what was inside, a Shine for the Shiny style, an Outline, a Glow, a
/// Blend layer for a blend, and a reaction for each of flash, hue shift per hit, stretch, rainbow
/// and turning that was on. Opacity has no layer yet and is dropped.
pub fn layers_from_basic(look: &BasicLook) -> Slot {
    use crate::modules::look::layers::{self as l, *};
    use bpe_core::{Layer, LayerId, layered_key};
    let mut card = Slot::new(l::CARD);
    let mut add = |kind: &str, name: &str, params: Params| {
        let id = LayerId(card.layers.len() as u32);
        card.layers.push(Layer { id, kind: kind.into(), name: name.into(), ..Layer::default() });
        for (k, v) in params.iter() {
            card.params.set(&layered_key(id, k), v.clone());
        }
    };
    let body =
        BodyLayer { ball_shape: look.ball_shape, roundness: look.roundness, fill: look.fill.clone(), opacity: 1.0 };
    add(l::BODY, "Body", body.to_params());
    if look.inside.pattern != Pattern::None {
        add(l::PATTERN, "Pattern", PatternLayer { inside: look.inside.clone(), ..PatternLayer::default() }.to_params());
    }
    if look.style == Style::Shiny {
        add(l::SHINE, "Shine", ShineLayer::default().to_params());
    }
    let outline_colour = look.outline_paint.colours.first();
    if look.outline > 0.0 {
        let outline = OutlineLayer {
            width: look.outline,
            colour_from: ColourFrom::Own,
            colour: bpe_core::Gradient::solid(outline_colour),
            ..OutlineLayer::default()
        };
        add(l::OUTLINE, "Outline", outline.to_params());
    }
    if look.glow > 0.0 {
        let glow = GlowLayer {
            strength: look.glow,
            size: look.glow_size,
            colour_from: if look.outline > 0.0 { ColourFrom::Own } else { ColourFrom::SameAsBody },
            colour: bpe_core::Gradient::solid(outline_colour),
            ..GlowLayer::default()
        };
        add(l::GLOW, "Glow", glow.to_params());
    }
    if look.blend != Blend::Normal || look.blend_amount < 1.0 {
        let blend = l::BlendLayer { blend: look.blend, blend_amount: look.blend_amount, blend_each: look.blend_each };
        add(l::BLEND, "Blend", blend.to_params());
    }
    if look.flash > 0.0 {
        let flash = FlashOnHit { flash: look.flash, flash_size: look.flash_size, flash_seconds: look.flash_seconds };
        add(l::FLASH, "Flash on hit", flash.to_params());
    }
    if look.hue_per_hit != 0.0 {
        add(l::HUE, "Colour on hit", ColourOnHit { hue_per_hit: look.hue_per_hit }.to_params());
    }
    if look.stretch > 0.0 {
        add(l::STRETCH, "Stretch when fast", StretchWhenFast { stretch: look.stretch }.to_params());
    }
    if look.rainbow != 0.0 {
        add(l::RAINBOW, "Colour over time", ColourOverTime { rainbow: look.rainbow }.to_params());
    }
    if look.turn != 0.0 || look.spin != 0.0 || look.face_motion {
        let turning = Turning { turn: look.turn, spin: look.spin, face_motion: look.face_motion };
        add(l::TURNING, "Turning", turning.to_params());
    }
    card
}

/// Gives `id` a card of `kind` in `slot`, as `add_slot` does, except that the old look is put on
/// balls too: test scenes are written with it and [`classic_cards`] turns it into layers.
pub(crate) fn put_card(scene: &mut Scene, reg: &Registry, id: ThingId, slot: SlotId, kind: &str) {
    let balls = scene.things.get(&id).is_some_and(|t| t.kind == "balls");
    if balls && kind == "look.basic" {
        scene.things.get_mut(&id).expect("checked above").slots.insert(slot, Slot::new(kind));
        scene.tracks.retain(|a, _| !(a.thing == id && a.slot == Some(slot)));
        return;
    }
    scene.add_slot(reg, id, slot, kind).expect("the kind has the slot");
}

/// The Body colours of a ball's Look, old or layered.
pub(crate) fn ball_fill(card: &Slot) -> Paint {
    if card.kind == "look.basic" {
        return BasicLook::from_params(&card.params).fill;
    }
    let body = card.layers.first().map(|l| l.id).unwrap_or_default();
    let prefix = format!("{body}/");
    let params = Params(
        card.params.iter().filter_map(|(k, v)| Some((k.strip_prefix(&prefix)?.to_owned(), v.clone()))).collect(),
    );
    crate::modules::look::layers::BodyLayer::from_params(&params).fill
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
            colours: {
                let mut g = Gradient::even(&[hex(0x2b1458), hex(0x140a33), hex(0x05040f)]);
                g.layout.form = Some(bpe_core::GradientForm::Round);
                g
            },
            span: 1150.0,
            ..Paint::default()
        };
    });
    b.shape(ShapeSettings::circle(470.0, 12.0));
    b.balls("Block", Vec2::new(-20.0, -170.0), |s| {
        s.radius = 19.0;
        s.block(8, 8, 46.0);
        s.velocity = Vec2::new(-420.0, 220.0);
    });
    b.ball_look(|l| {
        let colours = Gradient::even(&[hex(0xffd23e), hex(0xff6a5c), hex(0xff3ec8), hex(0x8a4dff), hex(0x3e9bff)]);
        l.fill =
            Paint { colours: colours.clone(), angle: 45.0, span: 700.0, repeat: Repeat::Mirror, ..Paint::default() };
        l.inside = InsidePaint {
            pattern: Pattern::Gradient,
            form: bpe_core::GradientForm::Rings,
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
    let streak = TrailLayer {
        seconds: 0.35,
        width: 0.55,
        brightness: 0.45,
        source: TrailSource::Own,
        colour: Paint {
            colours: Gradient::even(&[hex(0xffffff), hex(0x7cf4ff), hex(0x8a4dff), hex(0x2b1458)]),
            follow: Follow::Life,
            ..Paint::default()
        },
        ..TrailLayer::default()
    };
    b.own_module(BALLS, "trail.streak", streak.to_params());
    let sparks = ParticlesLayer {
        count: 6,
        speed: 360.0,
        size: 3.0,
        life: 0.55,
        least_speed: 160.0,
        source: ParticleSource::Own,
        colour: Paint {
            colours: Gradient::even(&[hex(0xffffff), hex(0xffd23e), hex(0xff6a2e), hex(0xc8104a)]),
            follow: Follow::Life,
            ..Paint::default()
        },
        ..ParticlesLayer::default()
    };
    b.own_module(BALLS, "particles.burst", sparks.to_params());
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
    b.shape(ShapeSettings::rectangle(1000.0, 1400.0, 8.0));
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
            {
                let mut s = BallsSettings { radius: 70.0, velocity: Vec2::new(vx, 0.0), ..BallsSettings::default() };
                s.block(5, 1, 95.0);
                s
            }
            .to_params(),
        );
        let look = BasicLook {
            fill: Paint { colours: Gradient::even(&colours.map(hex)), follow: Follow::Order, ..Paint::default() },
            blend,
            ..classic_look()
        };
        b.own_module(id, "look.basic", look.to_params());
    }
    // Streaks that add their light.
    let comets = b.add(
        "balls",
        ThingId::ROOT,
        "Comets",
        Vec2::new(0.0, 450.0),
        {
            let mut s = BallsSettings { radius: 14.0, velocity: Vec2::new(520.0, 380.0), ..BallsSettings::default() };
            s.ring(3, 60.0);
            s
        }
        .to_params(),
    );
    let comet_look = BasicLook { fill: Paint::solid(hex(0xffffff)), glow: 0.6, ..classic_look() };
    b.own_module(comets, "look.basic", comet_look.to_params());
    let streak = TrailLayer {
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
        ..TrailLayer::default()
    };
    b.own_module(comets, "trail.streak", streak.to_params());
    // Styled balls whose hue drifts around one colour.
    b.balls("Drifters", Vec2::new(-260.0, 600.0), |s| {
        s.radius = 22.0;
        s.block(8, 2, 52.0);
        s.velocity = Vec2::new(160.0, -90.0);
    });
    let drift = BasicLook {
        fill: Paint { colours: Gradient::even(&[hex(0xff7a3e)]), vary_hue: 25.0, wobble: 0.5, ..Paint::default() },
        style: Style::Shiny,
        ..classic_look()
    };
    b.own_module(BALLS, "look.basic", drift.to_params());
    b.finish()
}

/// The palette and grade showcase: balls painted from the Sunset palette in a ring whose wall
/// wears the Triad colours that match it, a cluster of glowing balls that Screen onto each other,
/// and a warm, contrasty grade with a vignette whose Sunset gradient map comes in over the video.
pub fn graded_sunset(reg: &Registry) -> Scene {
    let hex = |h: u32| Color::rgb((h >> 16) as u8, (h >> 8) as u8, h as u8);
    let mut sunset = crate::paint::palettes::built_in()
        .into_iter()
        .find(|p| p.name == "Sunset")
        .expect("Sunset is a built-in palette")
        .colours;
    // Mixed in light, as the palette was when the scene was made, so its pictures don't move.
    sunset.mix = bpe_core::Mix::Light;
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
    b.shape(ShapeSettings::circle(470.0, 14.0));
    b.balls("Sunset balls", Vec2::new(0.0, -150.0), |s| {
        s.radius = 22.0;
        s.block(7, 5, 52.0);
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
        {
            let mut s = BallsSettings { radius: 55.0, velocity: Vec2::new(-260.0, -380.0), ..BallsSettings::default() };
            s.ring(5, 50.0);
            s
        }
        .to_params(),
    );
    let screen = BasicLook {
        fill: Paint { colours: sunset.clone(), follow: Follow::Order, ..Paint::default() },
        glow: 0.0,
        blend: Blend::Screen,
        blend_each: true,
        ..classic_look()
    };
    b.own_module(glow, "look.basic", screen.to_params());
    b.track(
        own(ThingId::ROOT, "grade_map_amount"),
        &[(0.0, Value::Number(0.0), Ease::Linear), (duration as f64, Value::Number(0.5), Ease::Linear)],
    );
    b.finish()
}

/// Where one of a thing's cards comes from while a test scene is being built. Every thing ends
/// up owning its cards; this only lets a scene set "every thing's motion" in one place, the way
/// these scenes were first written.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Src {
    /// The builder's shared card for the slot, copied in when the scene is finished.
    Shared,
    /// A copy of the card of the nearest thing above that has one (Scene settings excepted).
    Follow,
}

/// A new scene with a name, and handles on its things and the cards its things share.
struct Builder<'a> {
    reg: &'a Registry,
    scene: Scene,
    /// One card per slot that every thing of a fitting kind gets a copy of instead of its own
    /// starter, so a scene's looks, motion and bounce can be set in one place.
    shared: BTreeMap<SlotId, Slot>,
    /// The cards that come from somewhere else when the scene is finished. A card not listed is
    /// the thing's own.
    src: BTreeMap<(ThingId, SlotId), Src>,
}

impl<'a> Builder<'a> {
    fn new(reg: &'a Registry, name: &str) -> Builder<'a> {
        let mut scene = new_scene(reg);
        scene.name = name.into();
        // The first ball is written with the old look, turned into layers when it is finished.
        put_card(&mut scene, reg, BALLS, SlotId::Look, "look.basic");
        scene.card_mut(BALLS, SlotId::Look).expect("just put").params = classic_ball_look().to_params();
        let shared = [(SlotId::Look, "look.basic"), (SlotId::Motion, "motion.basic"), (SlotId::Bounce, "bounce.basic")]
            .into_iter()
            .map(|(slot, kind)| (slot, Slot::new(kind)))
            .collect();
        let mut b = Builder { reg, scene, shared, src: BTreeMap::new() };
        b.share(SHAPE, &SlotId::ALL);
        b.share(BALLS, &[SlotId::Motion, SlotId::Bounce]);
        b
    }

    /// Makes `thing`'s own starters in `slots` copies of the shared cards of the same kind.
    fn share(&mut self, thing: ThingId, slots: &[SlotId]) {
        for slot in slots {
            let (Some(shared), Some(own)) = (self.shared.get(slot), self.scene.things[&thing].slots.get(slot)) else {
                continue;
            };
            if own.kind == shared.kind {
                self.src.insert((thing, *slot), Src::Shared);
            }
        }
    }

    /// The thing above `thing` whose card in `slot` it copies: the nearest one, below Scene
    /// settings, that has a card there.
    fn followed(&self, thing: ThingId, slot: SlotId) -> Option<ThingId> {
        self.scene.ancestors(thing).into_iter().filter(|a| *a != ThingId::ROOT).find(|a| {
            self.src.get(&(*a, slot)).is_some_and(|s| *s == Src::Shared)
                || (!self.src.contains_key(&(*a, slot)) && self.scene.things[a].slots.contains_key(&slot))
        })
    }

    /// The card that `thing` ends up with in `slot`, for changing: the shared one, the followed
    /// one, or its own.
    fn card_mut(&mut self, thing: ThingId, slot: SlotId) -> &mut Slot {
        match self.src.get(&(thing, slot)).copied() {
            Some(Src::Shared) => self.shared.get_mut(&slot).expect("shared slots are listed"),
            Some(Src::Follow) => {
                let above = self.followed(thing, slot).expect("a followed card");
                self.card_mut(above, slot)
            }
            None => self.scene.things.get_mut(&thing).and_then(|t| t.slots.get_mut(&slot)).expect("a card"),
        }
    }

    /// The finished scene: every thing owns a copy of the cards it took from elsewhere, and
    /// cards still at the new defaults get the classic values.
    fn finish(mut self) -> Scene {
        for id in self.scene.order.clone() {
            for slot in SlotId::ALL {
                let card = match self.src.get(&(id, slot)) {
                    None => continue,
                    Some(Src::Shared) => Some(self.shared[&slot].clone()),
                    Some(Src::Follow) => {
                        self.followed(id, slot).and_then(|a| self.scene.things[&a].slots.get(&slot).cloned())
                    }
                };
                let thing = self.scene.things.get_mut(&id).expect("listed in order");
                match card {
                    Some(card) => thing.slots.insert(slot, card),
                    None => thing.slots.remove(&slot),
                };
            }
            // Parents come first, so a follower copies a finished card.
            self.src.retain(|(t, _), _| *t != id);
        }
        classic_cards(&mut self.scene);
        self.scene
    }

    fn scene_settings(&mut self, f: impl FnOnce(&mut SceneSettings)) {
        let root = self.scene.things.get_mut(&ThingId::ROOT).expect("root");
        // Keep what the new scene set.
        let mut s = SceneSettings::from_params(&root.params);
        f(&mut s);
        root.params = s.to_params();
    }

    /// The scene's shape (thing 1, where the arena was) takes these settings.
    fn shape(&mut self, s: ShapeSettings) {
        self.scene.things.get_mut(&SHAPE).expect("the shape").params = s.to_params();
    }

    fn balls(&mut self, name: &str, position: Vec2, f: impl FnOnce(&mut BallsSettings)) {
        let mut s = BallsSettings::default();
        f(&mut s);
        let thing = self.scene.things.get_mut(&BALLS).expect("balls");
        thing.name = name.into();
        thing.transform.position = position;
        thing.params = s.to_params();
    }

    fn root_look(&mut self, f: impl FnOnce(&mut BasicLook)) {
        let mut s = classic_look();
        f(&mut s);
        self.shared.get_mut(&SlotId::Look).expect("shared").params = s.to_params();
    }

    /// The balls' look in `colours`, from the block's first row to its last as the balls start
    /// out: follows where each ball started, top to bottom, across the block's height. Balls
    /// that start in one row (or not in a block) all wear the first colour.
    fn ball_look_by_row(&mut self, colours: &[Color]) {
        let thing = &self.scene.things[&BALLS];
        let s = BallsSettings::from_params(&thing.params);
        let rows = (s.count.max(1)).div_ceil(s.columns.max(1));
        let height = rows.saturating_sub(1) as f32 * s.step();
        let fill = if s.group && s.shape == crate::GroupShape::Grid && height > 0.0 {
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
        let mut s = classic_ball_look();
        f(&mut s);
        self.card_mut(BALLS, SlotId::Look).params = s.to_params();
    }

    fn motion(&mut self, f: impl FnOnce(&mut BasicMotion)) {
        let mut s = classic_motion();
        f(&mut s);
        self.shared.get_mut(&SlotId::Motion).expect("shared").params = s.to_params();
    }

    fn bounce(&mut self, f: impl FnOnce(&mut BasicBounce)) {
        let mut s = classic_bounce();
        f(&mut s);
        self.shared.get_mut(&SlotId::Bounce).expect("shared").params = s.to_params();
    }

    /// A new card of `kind` on one thing. On the scene settings it also takes the place of the
    /// shared card for its slot, so every thing that shared that one gets this instead. On a
    /// folder (or anything with things under it), the things inside that shared the card copy
    /// this one instead.
    fn own_module(&mut self, thing: ThingId, kind: &str, params: Params) {
        if put_old_card(&mut self.scene, thing, kind, &params) {
            return;
        }
        let slot = self.reg.module(kind).expect("kit kinds are registered").slot();
        let users: Vec<ThingId> =
            self.src.iter().filter(|(k, s)| k.1 == slot && **s == Src::Shared).map(|(k, _)| k.0).collect();
        if thing == ThingId::ROOT {
            if self.reg.thing("scene").is_some_and(|k| k.slots().contains(&slot)) {
                self.scene.add_slot(self.reg, thing, slot, kind).expect("the scene settings have the slot");
                self.card_mut(thing, slot).params = params.clone();
            }
            if self.shared.contains_key(&slot) {
                self.shared.insert(slot, Slot { kind: kind.into(), params, ..Slot::default() });
            }
            return;
        }
        // A card a kind no longer has (a Shader card on balls, now a layer) is left out.
        let kind_of = self.scene.things.get(&thing).and_then(|t| self.reg.thing(&t.kind));
        if !kind_of.is_some_and(|k| k.slots().contains(&slot)) {
            return;
        }
        put_card(&mut self.scene, self.reg, thing, slot, kind);
        self.src.remove(&(thing, slot));
        self.card_mut(thing, slot).params = params;
        for user in users {
            if self.scene.ancestors(user).contains(&thing) {
                self.src.insert((user, slot), Src::Follow);
            }
        }
    }

    /// A new thing of `kind` under `parent`, named and placed, with its settings, and the starter
    /// modules the kit gives that kind.
    fn add(&mut self, kind: &str, parent: ThingId, name: &str, position: Vec2, params: Params) -> ThingId {
        let id = crate::add_thing(&mut self.scene, self.reg, kind, parent).expect("kit kinds are registered");
        if kind == "balls" {
            // Written with the old look, which `finish` turns into layers.
            put_card(&mut self.scene, self.reg, id, SlotId::Look, "look.basic");
        }
        self.scene.rename_thing(id, name);
        self.share(id, &SlotId::ALL);
        let thing = self.scene.things.get_mut(&id).expect("just added");
        thing.transform.position = position;
        thing.params = params;
        id
    }

    /// Keys on one setting: (time, value, ease into the next).
    fn track(&mut self, a: Address, keys: &[(f64, Value, Ease)]) {
        for (time, value, ease) in keys {
            let i = self.scene.set_key(self.reg, &a, *time, value.clone()).expect("the setting takes keys");
            self.scene.set_ease(&a, i, *ease);
        }
    }

    /// Makes `id` ride on `parent`.
    fn ride(&mut self, id: ThingId, parent: ThingId, offset: Vec2, turn_with_parent: bool) {
        self.scene.set_parent(id, parent, Attach::Ride { offset, turn_with_parent, item: None }).expect("no loops");
    }
}

fn own(thing: ThingId, key: &str) -> Address {
    Address::own(thing, key)
}

/// The new-scene default: a circle shape and one ball.
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
    b.shape(ShapeSettings::circle(508.0, 3.0));
    b.balls("Block", Vec2::new(37.0, -127.0), |s| {
        s.radius = 4.5;
        s.block(28, 28, 19.0);
        s.velocity = Vec2::new(-480.0, 265.0);
    });
    b.ball_look_by_row(&[
        Color::rgb(0x3e, 0x9b, 0xff),
        Color::rgb(0x8a, 0x4d, 0xff),
        Color::rgb(0xff, 0x3e, 0xc8),
        Color::rgb(0xff, 0x6a, 0x5c),
        Color::rgb(0xff, 0xb8, 0x2e),
    ]);
    b.finish()
}

/// A box with a block of balls dropped in, losing a little speed on every hit.
fn box_drop(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Box drop");
    b.scene_settings(|s| s.gravity = Vec2::new(0.0, 900.0));
    b.shape(ShapeSettings::rectangle(900.0, 1500.0, 3.0));
    b.balls("Block", Vec2::new(0.0, -500.0), |s| {
        s.radius = 12.0;
        s.block(10, 10, 40.0);
        s.velocity = Vec2::new(200.0, 0.0);
    });
    b.bounce(|s| s.bounciness = 0.85);
    b.motion(|s| s.drag = 0.05);
    b.finish()
}

/// A turning hexagon with a ring of balls that gain a little speed on every hit.
fn spinning_hexagon(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Spinning hexagon");
    b.shape(ShapeSettings { spin: 25.0, ..ShapeSettings::polygon(6, 450.0, 3.0) });
    b.balls("Ring", Vec2::ZERO, |s| {
        s.radius = 10.0;
        s.ring(24, 200.0);
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
    b.shape(ShapeSettings { shrink: 0.03, min_size: 200.0, ..ShapeSettings::circle(500.0, 3.0) });
    b.balls("Ball", Vec2::new(0.0, -200.0), |s| {
        s.radius = 30.0;
        s.velocity = Vec2::new(600.0, -300.0);
    });
    b.bounce(|s| {
        s.grow = 1.5;
        s.max_size = 90.0;
    });
    let lively = BasicBounce { bounciness: 1.05, takes_over: true, ..classic_bounce() }.to_params();
    b.own_module(SHAPE, "bounce.basic", lively);
    b.finish()
}

/// A tall box full of balls that pile up on each other, coloured by how often they have hit.
fn ball_pit(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Ball pit");
    b.scene_settings(|s| {
        s.gravity = Vec2::new(0.0, 1200.0);
        s.duration = 15.0;
    });
    b.shape(ShapeSettings::rectangle(700.0, 1400.0, 3.0));
    b.balls("Pit", Vec2::new(0.0, -300.0), |s| {
        s.radius = 20.0;
        s.block(10, 14, 48.0);
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
    b.finish()
}

/// An upright ellipse with a ring of 48 balls fired outward from its top focus, bouncing by their
/// middles: every wave meets at the other focus.
fn ellipse_focus(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Ellipse focus");
    b.scene_settings(|s| {
        s.gravity = Vec2::ZERO;
        s.duration = 20.0;
    });
    b.shape(ShapeSettings::ellipse(900.0, 1300.0, 3.0));
    b.scene.things.get_mut(&SHAPE).expect("the shape").transform.position = Vec2::ZERO;
    let focus = (650f32 * 650.0 - 450.0 * 450.0).sqrt();
    b.balls("Wave", Vec2::new(0.0, -focus), |s| {
        s.radius = 4.0;
        s.ring(48, 1.0);
        s.velocity = Vec2::new(500.0, 0.0);
        s.launch = crate::Launch::Outward;
    });
    b.bounce(|s| s.touch = crate::WallTouch::Middle);
    b.finish()
}

/// Balls riding a turning hexagon; the balls change hue with every hit and pop after
/// forty.
fn riders(reg: &Registry) -> Scene {
    let mut b = Builder::new(reg, "Riders");
    b.shape(ShapeSettings { spin: 30.0, ..ShapeSettings::polygon(6, 450.0, 3.0) });
    b.balls("Rim riders", Vec2::ZERO, |s| {
        s.radius = 14.0;
        s.ring(6, 60.0);
        s.velocity = Vec2::ZERO;
    });
    // The top rider starts at 380 px out, inside the 390 px flat top: a ball that starts on or
    // beyond a hollow wall's line stays outside it.
    b.ride(BALLS, SHAPE, Vec2::new(0.0, -320.0), true);
    b.bounce(|s| s.pops_after = 40);
    b.ball_look(|l| l.hue_per_hit = 12.0);
    b.finish()
}

/// A lit room for the lighting tests (lighting part 1a): a mid-grey background in a white ring,
/// one pink ball at the middle (drawn at 0 s, before it falls) and one Point light above it. Not in [`all`],
/// which pins the unlit pictures.
pub fn lit_room(reg: &Registry) -> Scene {
    use crate::things::light::{LightType, add_light};
    let mut scene = new_scene(reg);
    scene.name = "Lit room".into();
    let root = scene.things.get_mut(&ThingId::ROOT).expect("root");
    let mut s = SceneSettings::from_params(&root.params);
    s.background = Paint::solid(Color::rgb(0x80, 0x80, 0x80));
    s.bloom = 0.0;
    root.params = s.to_params();
    let ball = scene.things.values().find(|t| t.kind == "balls").map(|t| t.id).expect("a ball");
    scene.things.get_mut(&ball).expect("ball").transform.position = Vec2::ZERO;
    let light = add_light(&mut scene, reg, LightType::Point, ThingId::ROOT).expect("a light");
    scene.things.get_mut(&light).expect("light").params.set("position", Value::Vec2([0.0, -400.0]));
    scene
}

/// The lighting demo (lighting part 1a's video): a dark room in a white ring, twelve balls flung
/// out from the middle, each carrying a small light of its own, under a warm spot light, with a
/// cool light riding the middle of the group. Not in [`all`].
pub fn lit_demo(reg: &Registry) -> Scene {
    use crate::things::light::{LightSettings, LightType, add_light};
    let mut scene = lit_room(reg);
    scene.name = "Lit demo".into();
    let root = scene.things.get_mut(&ThingId::ROOT).expect("root");
    let mut s = SceneSettings::from_params(&root.params);
    s.background = Paint::solid(Color::rgb(0x2a, 0x2e, 0x3c));
    s.bloom = 0.2;
    root.params = s.to_params();
    let lighting = root.slots.get_mut(&SlotId::Lighting).expect("lit");
    lighting.params.set("ambient", Value::Number(0.12));
    lighting.params.set("contact", Value::Number(0.4));
    // The room's light becomes a warm spot from above.
    let first = scene.things.values().find(|t| t.kind == "light").map(|t| t.id).expect("a light");
    let mut spot = LightSettings::of_kind(LightType::Spot);
    spot.position = Vec2::new(0.0, -460.0);
    spot.cone = 90.0;
    spot.cone_edge = 25.0;
    spot.reach = 2200.0;
    spot.strength = 2.2;
    spot.height = 400.0;
    spot.size = 40.0;
    spot.colour = Color::rgb(0xff, 0xc8, 0x8a);
    scene.things.get_mut(&first).expect("light").params = spot.to_params();
    scene.rename_thing(first, "Spot light");
    let shape = scene.things.values().find(|t| t.kind == "shape").map(|t| t.id).expect("a shape");
    scene.things.get_mut(&shape).expect("shape").params.set("thickness", Value::Number(10.0));
    let ball = scene.things.values().find(|t| t.kind == "balls").map(|t| t.id).expect("a ball");
    let mut balls = BallsSettings::from_params(&scene.things[&ball].params);
    balls.ring(12, 140.0);
    balls.launch = crate::things::balls::Launch::Outward;
    balls.velocity = Vec2::new(0.0, -700.0);
    scene.things.get_mut(&ball).expect("ball").params = balls.to_params();
    // A small coloured light on each ball, and a cool one riding the middle of the group.
    let each = add_light(&mut scene, reg, LightType::Point, ball).expect("a light");
    let mut on_each = LightSettings::of_kind(LightType::Point);
    on_each.position = Vec2::ZERO;
    on_each.colour = Color::rgb(0xff, 0x5c, 0xa8);
    on_each.strength = 1.2;
    on_each.reach = 320.0;
    on_each.height = 40.0;
    on_each.size = 10.0;
    on_each.flare = 3.0;
    scene.things.get_mut(&each).expect("light").params = on_each.to_params();
    let middle = add_light(&mut scene, reg, LightType::Point, ball).expect("a light");
    let mut cool = LightSettings::of_kind(LightType::Point);
    cool.position = Vec2::ZERO;
    cool.rides_on = crate::things::light::RidesOn::TheMiddle;
    cool.colour = Color::rgb(0x6c, 0xb4, 0xff);
    cool.strength = 1.4;
    cool.reach = 1100.0;
    cool.height = 120.0;
    scene.things.get_mut(&middle).expect("light").params = cool.to_params();
    scene
}

/// The stage demo (lighting part 2a's video): five balls of five surfaces (Chrome, Glass,
/// Plastic, Toon, Metal) bouncing in a thick ring on a 3D stage, a glossy floor under the ring, a
/// warm key light, a cool fill and a magenta light between the balls and the wall. Not in [`all`].
pub fn stage_demo(reg: &Registry) -> Scene {
    use crate::modules::look::surface::{SurfaceKind, preset_values};
    use crate::things::light::{LightSettings, LightType, add_light};
    use bpe_core::Choice;
    let hex = |h: u32| Color::rgb((h >> 16) as u8, (h >> 8) as u8, h as u8);
    let mut b = Builder::new(reg, "Stage demo");
    b.scene_settings(|s| {
        s.duration = 10.0;
        s.gravity = Vec2::new(0.0, 900.0);
        s.bloom = 0.25;
        s.background = Paint {
            colours: Gradient::even(&[hex(0x30333d), hex(0x4a4c56)]),
            angle: 90.0,
            span: 1920.0,
            ..Paint::default()
        };
    });
    b.shape(ShapeSettings::circle(420.0, 28.0));
    let plain =
        |colour: Color| BasicLook { fill: Paint::solid(colour), glow: 0.0, style: Style::Flat, ..classic_look() };
    b.balls("Chrome", Vec2::new(-160.0, -180.0), |s| {
        s.radius = 52.0;
        s.velocity = Vec2::new(420.0, -260.0);
    });
    b.ball_look(|l| *l = plain(hex(0xdfe3ea)));
    let others = [
        ("Glass", hex(0x8fe3ff), Vec2::new(150.0, -120.0), Vec2::new(-360.0, -320.0), 50.0),
        ("Plastic", hex(0xe8344a), Vec2::new(0.0, 120.0), Vec2::new(300.0, -520.0), 46.0),
        ("Toon", hex(0xffc83c), Vec2::new(-200.0, 140.0), Vec2::new(-240.0, -420.0), 44.0),
        ("Metal", hex(0xe2b25a), Vec2::new(200.0, 160.0), Vec2::new(200.0, -600.0), 48.0),
    ];
    let mut ids = vec![("Chrome", BALLS)];
    for (name, colour, at, velocity, radius) in others {
        let id = b.add(
            "balls",
            ThingId::ROOT,
            name,
            at,
            BallsSettings { radius, velocity, ..BallsSettings::default() }.to_params(),
        );
        b.own_module(id, "look.basic", plain(colour).to_params());
        ids.push((name, id));
    }
    let mut scene = b.finish();
    for (name, id) in ids {
        let kind = match name {
            "Chrome" => SurfaceKind::Chrome,
            "Glass" => SurfaceKind::Glass,
            "Plastic" => SurfaceKind::Plastic,
            "Toon" => SurfaceKind::Toon,
            _ => SurfaceKind::Metal,
        };
        let card = scene.card_mut(id, SlotId::Look).expect("a look");
        card.params.set("surface", Value::Choice(kind.name().into()));
        for (k, v) in preset_values(kind) {
            card.params.set(k, v);
        }
    }
    let shape = scene.card_mut(SHAPE, SlotId::Look).expect("the shape's look");
    for (k, v) in preset_values(SurfaceKind::Plastic) {
        shape.params.set(k, v);
    }
    shape.params.set("surface", Value::Choice(SurfaceKind::Plastic.name().into()));
    let key = add_light(&mut scene, reg, LightType::Point, ThingId::ROOT).expect("a light");
    let mut warm = LightSettings::of_kind(LightType::Point);
    warm.position = Vec2::new(-340.0, -640.0);
    warm.height = 520.0;
    warm.strength = 2.6;
    warm.reach = 2800.0;
    warm.size = 70.0;
    warm.colour = hex(0xffe0bc);
    scene.things.get_mut(&key).expect("light").params = warm.to_params();
    scene.rename_thing(key, "Key light");
    let fill = add_light(&mut scene, reg, LightType::Point, ThingId::ROOT).expect("a light");
    let mut cool = LightSettings::of_kind(LightType::Point);
    cool.position = Vec2::new(440.0, -80.0);
    cool.height = 280.0;
    cool.strength = 1.0;
    cool.reach = 2000.0;
    cool.size = 40.0;
    cool.colour = hex(0x8fb8ff);
    scene.things.get_mut(&fill).expect("light").params = cool.to_params();
    scene.rename_thing(fill, "Fill light");
    let rim = add_light(&mut scene, reg, LightType::Point, ThingId::ROOT).expect("a light");
    let mut back = LightSettings::of_kind(LightType::Point);
    back.position = Vec2::new(0.0, -260.0);
    back.height = -150.0;
    back.strength = 1.6;
    back.reach = 1100.0;
    back.size = 30.0;
    back.colour = hex(0xff4fc8);
    scene.things.get_mut(&rim).expect("light").params = back.to_params();
    scene.rename_thing(rim, "Back light");
    let lighting = scene.card_mut(ThingId::ROOT, SlotId::Lighting).expect("lit");
    for (k, v) in [
        ("depth", Value::Choice("stage".into())),
        ("ambient", Value::Number(0.1)),
        ("sky_light", Value::Number(0.35)),
        ("contact", Value::Number(0.7)),
        ("looking_down", Value::Number(16.0)),
        ("quality", Value::Choice("high".into())),
    ] {
        lighting.params.set(k, v);
    }
    scene
}

/// The surfaces demo (lighting part 2b's video): nine balls of the new surfaces (Glass, Jelly,
/// Pearl, Flake, Brushed, Frosted, Velvet, Lava, Chrome) bouncing in a ring whose outline is a
/// mirror wall (the outside shows the inside, squeezed), on a 3D stage with a glossy floor, live
/// reflections and depth of field, under a warm key light, a cool fill and a magenta back light.
/// Not in [`all`].
pub fn surfaces_demo(reg: &Registry) -> Scene {
    use crate::modules::look::surface::{SurfaceKind, preset_values};
    use bpe_core::Choice;
    let hex = |h: u32| Color::rgb((h >> 16) as u8, (h >> 8) as u8, h as u8);
    let mut scene = stage_demo(reg);
    scene.name = "Surfaces demo".into();
    let old: Vec<ThingId> =
        scene.things.values().filter(|t| t.kind == "balls" && t.id != BALLS).map(|t| t.id).collect();
    for id in old {
        scene.remove_thing(id).expect("a ball");
    }
    let balls = [
        ("Glass", SurfaceKind::Glass, hex(0xbfefff), Vec2::new(-160.0, -180.0), Vec2::new(420.0, -260.0), 60.0),
        ("Jelly", SurfaceKind::Jelly, hex(0x3cd070), Vec2::new(150.0, -140.0), Vec2::new(-360.0, -320.0), 50.0),
        ("Pearl", SurfaceKind::Pearl, hex(0xf2ece4), Vec2::new(0.0, 120.0), Vec2::new(300.0, -520.0), 48.0),
        ("Flake", SurfaceKind::Flake, hex(0x2050d8), Vec2::new(-200.0, 140.0), Vec2::new(-240.0, -420.0), 46.0),
        ("Brushed", SurfaceKind::Brushed, hex(0xd8d4cc), Vec2::new(200.0, 160.0), Vec2::new(200.0, -600.0), 50.0),
        ("Frosted", SurfaceKind::Frosted, hex(0xe8f4ff), Vec2::new(-60.0, -60.0), Vec2::new(-150.0, 480.0), 52.0),
        ("Velvet", SurfaceKind::Velvet, hex(0x8a1840), Vec2::new(90.0, 20.0), Vec2::new(380.0, 260.0), 44.0),
        ("Lava", SurfaceKind::Lava, hex(0xff5a1c), Vec2::new(-100.0, 260.0), Vec2::new(-420.0, 120.0), 42.0),
        ("Chrome", SurfaceKind::Chrome, hex(0xdfe3ea), Vec2::new(60.0, -280.0), Vec2::new(260.0, 380.0), 50.0),
    ];
    let template = scene.things[&BALLS].clone();
    for (i, (name, kind, colour, at, velocity, radius)) in balls.into_iter().enumerate() {
        let id = if i == 0 {
            BALLS
        } else {
            let id = crate::add_thing(&mut scene, reg, "balls", ThingId::ROOT).expect("a ball");
            let t = scene.things.get_mut(&id).expect("ball");
            t.params = template.params.clone();
            t.slots.insert(SlotId::Look, template.slots[&SlotId::Look].clone());
            id
        };
        scene.rename_thing(id, name);
        let t = scene.things.get_mut(&id).expect("ball");
        let mut s = BallsSettings::from_params(&t.params);
        s.radius = radius;
        s.velocity = velocity;
        t.params = s.to_params();
        t.transform.position = at;
        let card = scene.card_mut(id, SlotId::Look).expect("a look");
        let body = card.layers.iter().find(|l| l.kind == "layer.body").map(|l| l.id).expect("a body");
        card.params.set(&bpe_core::layered_key(body, "colours"), Value::Gradient(Gradient::even(&[colour, colour])));
        card.params.set("surface", Value::Choice(kind.name().into()));
        for (k, v) in preset_values(kind) {
            card.params.set(k, v);
        }
    }
    let shape = scene.card_mut(SHAPE, SlotId::Look).expect("the shape's look");
    for (k, v) in [
        ("mirror", Value::Choice("outside".into())),
        ("mirror_strength", Value::Number(0.7)),
        ("squeeze", Value::Number(1.0)),
    ] {
        shape.params.set(k, v);
    }
    let lighting = scene.card_mut(ThingId::ROOT, SlotId::Lighting).expect("lit");
    for (k, v) in [
        ("floor", Value::Choice("at_the_bottom".into())),
        ("floor_surface", Value::Choice("glossy".into())),
        ("depth_of_field", Value::Bool(true)),
        ("focus_at", Value::Number(40.0)),
        ("dof_blur", Value::Number(0.05)),
    ] {
        lighting.params.set(k, v);
    }
    scene
}

/// The bake demo (lighting part 3's video): the surfaces demo set to bake on export, more bounce
/// light and the glass balls' caustics turned up. Not in [`all`].
pub fn bake_demo(reg: &Registry) -> Scene {
    let mut scene = surfaces_demo(reg);
    scene.name = "Bake demo".into();
    let lighting = scene.card_mut(ThingId::ROOT, SlotId::Lighting).expect("lit");
    for (k, v) in [
        ("on_export", Value::Choice("bake".into())),
        ("bounce", Value::Number(0.8)),
        ("glass_caustics", Value::Number(1.5)),
        ("light_bounces", Value::Number(2.0)),
    ] {
        lighting.params.set(k, v);
    }
    scene
}

/// The light show (lighting part 6's tests and video): a dark room in a white ring, twelve balls
/// flung out from the middle, a rig of four spots across the top playing a chase from its colour
/// list, a second rig inside the balls whose lamps (one on each ball) chase ball to ball, and
/// three lighting scenes shown in turn every bar: the chase, a sweep, then alternating colours on
/// a strobe. Not in [`all`].
pub fn light_show(reg: &Registry) -> Scene {
    use crate::modules::light_scenes::{ScenesPlay, add_scene, put_on_lights};
    use crate::things::light::{LightSettings, LightType, add_light};
    use crate::things::rig::{Across, Pattern as Show, RigColours, RigSettings, add_rig};
    use bpe_core::Choice as _;
    let mut scene = lit_room(reg);
    scene.name = "Light show".into();
    let root = scene.things.get_mut(&ThingId::ROOT).expect("root");
    let mut s = SceneSettings::from_params(&root.params);
    s.background = Paint::solid(Color::rgb(0x1c, 0x1e, 0x28));
    s.bloom = 0.25;
    root.params = s.to_params();
    let lighting = root.slots.get_mut(&SlotId::Lighting).expect("lit");
    lighting.params.set("ambient", Value::Number(0.08));
    // The room's lamp stays as a faint fill outside the rigs.
    let fill = scene.things.values().find(|t| t.kind == "light").map(|t| t.id).expect("a light");
    scene.things.get_mut(&fill).expect("light").params.set("strength", Value::Number(0.4));
    let shape = scene.things.values().find(|t| t.kind == "shape").map(|t| t.id).expect("a shape");
    scene.things.get_mut(&shape).expect("shape").params.set("thickness", Value::Number(10.0));
    let ball = scene.things.values().find(|t| t.kind == "balls").map(|t| t.id).expect("a ball");
    let mut balls = BallsSettings::from_params(&scene.things[&ball].params);
    balls.ring(12, 140.0);
    balls.launch = crate::things::balls::Launch::Outward;
    balls.velocity = Vec2::new(0.0, -700.0);
    scene.things.get_mut(&ball).expect("ball").params = balls.to_params();

    // The top rig: four spots chasing in its colours.
    let top = add_rig(&mut scene, reg, ThingId::ROOT).expect("a rig");
    for &id in &scene.children(top) {
        let mut spot = LightSettings::from_params(&scene.things[&id].params);
        spot.strength = 3.0;
        spot.cone = 34.0;
        spot.height = 260.0;
        spot.size = 30.0;
        spot.reach = 2600.0;
        scene.things.get_mut(&id).expect("spot").params = spot.to_params();
    }
    let set_rig = |scene: &mut Scene, rig: ThingId, f: &dyn Fn(&mut RigSettings)| {
        let mut r = RigSettings::from_params(&scene.things[&rig].params);
        f(&mut r);
        scene.things.get_mut(&rig).expect("rig").params = r.to_params();
    };
    set_rig(&mut scene, top, &|r| {
        r.colours_from = RigColours::TheList;
        r.tail = 1.5;
        r.off_level = 0.05;
    });

    // The ball rig: a lamp on every ball, chasing ball to ball on half beats.
    let ball_rig = crate::add_thing(&mut scene, reg, crate::things::rig::RIG, ball).expect("a rig");
    scene.rename_thing(ball_rig, "Ball rig");
    let lamp = add_light(&mut scene, reg, LightType::Point, ball_rig).expect("a light");
    let mut each = LightSettings::from_params(&scene.things[&lamp].params);
    each.colour = Color::rgb(0xff, 0x5c, 0xa8);
    each.strength = 1.6;
    each.reach = 300.0;
    each.height = 40.0;
    each.size = 10.0;
    scene.things.get_mut(&lamp).expect("light").params = each.to_params();
    set_rig(&mut scene, ball_rig, &|r| {
        r.across = Across::EveryLamp;
        r.every_beats = 0.5;
        r.lit_at_once = 3.0;
        r.tail = 2.0;
        r.off_level = 0.15;
    });

    // Three scenes: the chase, a sweep, then alternating colours with the ball rig strobing.
    add_scene(&mut scene, reg, 0.0).expect("scene 1");
    set_rig(&mut scene, top, &|r| {
        r.pattern = Show::Sweep;
        r.sweep_angle = 70.0;
        r.offset = 0.5;
        r.off_level = 1.0;
    });
    add_scene(&mut scene, reg, 0.0).expect("scene 2");
    set_rig(&mut scene, top, &|r| {
        r.pattern = Show::AlternateColours;
        r.every_beats = 0.5;
    });
    set_rig(&mut scene, ball_rig, &|r| {
        r.pattern = Show::Strobe;
        r.flash_length = 0.3;
        r.colours_from = RigColours::TheList;
    });
    add_scene(&mut scene, reg, 0.0).expect("scene 3");
    put_on_lights(&mut scene, reg, 0, 0.0).expect("back to scene 1");
    let card = scene.things.get_mut(&ThingId::ROOT).expect("root").slots.get_mut(&SlotId::Lighting).expect("lit");
    card.params.set("scenes_play", Value::Choice(ScenesPlay::InTurn.name().into()));
    card.params.set("scene_every", Value::Number(1.0));
    card.params.set("scene_fade", Value::Number(0.3));
    scene
}

/// Lighting part 1b's room: [`lit_room`] darkened (no ambient, its lamp at strength 0) with a
/// bigger ball in the middle whose Body gives off warm light. Tests switch things on from here.
pub fn glow_room(reg: &Registry) -> Scene {
    use crate::modules::look::layers::gives_light::{EmitFrom, GivesLight, SUB_LIGHT};
    let mut scene = lit_room(reg);
    scene.name = "Glow room".into();
    let root = scene.things.get_mut(&ThingId::ROOT).expect("root");
    let mut s = SceneSettings::from_params(&root.params);
    s.background = Paint::solid(Color::rgb(0x60, 0x60, 0x60));
    root.params = s.to_params();
    let lighting = root.slots.get_mut(&SlotId::Lighting).expect("lit");
    lighting.params.set("ambient", Value::Number(0.0));
    let lamp = scene.things.values().find(|t| t.kind == "light").map(|t| t.id).expect("a light");
    scene.things.get_mut(&lamp).expect("light").params.set("strength", Value::Number(0.0));
    let ball = scene.things.values().find(|t| t.kind == "balls").map(|t| t.id).expect("a ball");
    scene.things.get_mut(&ball).expect("ball").params.set("radius", Value::Number(60.0));
    let body = scene.things[&ball].slots[&SlotId::Look].layers[0].id;
    let sub = scene.add_sub_layer(reg, ball, SlotId::Look, body, SUB_LIGHT).expect("the Body gives off light");
    let glow = GivesLight {
        colour_from: EmitFrom::Own,
        colour: Color::rgb(0xff, 0xa0, 0x40),
        strength: 3.0,
        ..Default::default()
    };
    let card = scene.card_mut(ball, SlotId::Look).expect("a look");
    for (k, v) in glow.to_params().iter() {
        card.params.set(&bpe_core::layered_key(sub, k), v.clone());
    }
    scene
}

/// Lighting part 1b's demo (its review video): a dark, hazy room in a glowing ring, twelve
/// balls flung out from the middle that give off light (flaring on hits) and leave glowing
/// streaks, under a warm spot whose beam shows in the haze, with bounce light and Light blooms.
/// Not in [`all`].
pub fn glow_demo(reg: &Registry) -> Scene {
    use crate::modules::look::layers::gives_light::{EmitFrom, GivesLight, SHAPE_SUB_LIGHT, SUB_LIGHT};
    use crate::modules::look::layers::trail::TRAIL;
    use crate::things::light::{LightSettings, LightType};
    let hex = |h: u32| Color::rgb((h >> 16) as u8, (h >> 8) as u8, h as u8);
    let mut scene = lit_demo(reg);
    scene.name = "Glow demo".into();
    let root = scene.things.get_mut(&ThingId::ROOT).expect("root");
    let mut s = SceneSettings::from_params(&root.params);
    s.background = Paint::solid(Color::rgb(0x0c, 0x0d, 0x14));
    s.bloom = 0.1;
    root.params = s.to_params();
    let lighting = root.slots.get_mut(&SlotId::Lighting).expect("lit");
    for (key, v) in [("ambient", 0.03), ("bounce", 0.6), ("light_blooms", 0.6), ("haze", 0.18), ("haze_dims", 0.1)] {
        lighting.params.set(key, Value::Number(v));
    }
    // Only the spot stays: the balls light the room themselves.
    let lights: Vec<ThingId> = scene.things.values().filter(|t| t.kind == "light").map(|t| t.id).collect();
    for id in lights {
        let spot = LightSettings::from_params(&scene.things[&id].params);
        if spot.kind != LightType::Spot {
            scene.things.get_mut(&id).expect("light").params.set("strength", Value::Number(0.0));
        }
    }
    let set_all = |scene: &mut Scene, thing: ThingId, layer: bpe_core::LayerId, params: &Params| {
        let card = scene.card_mut(thing, SlotId::Look).expect("a look");
        for (k, v) in params.iter() {
            card.params.set(&bpe_core::layered_key(layer, k), v.clone());
        }
    };
    // The ring glows a cool blue.
    let shape = scene.things.values().find(|t| t.kind == "shape").map(|t| t.id).expect("a shape");
    let body = scene.things[&shape].slots[&SlotId::Look].layers[0].id;
    let sub = scene.add_sub_layer(reg, shape, SlotId::Look, body, SHAPE_SUB_LIGHT).expect("a shape's Body glows");
    let ring = GivesLight {
        colour_from: EmitFrom::Own,
        colour: Color::rgb(0x50, 0x9c, 0xff),
        strength: 0.5,
        flare: 3.0,
        flare_length: 0.3,
        ..Default::default()
    };
    set_all(&mut scene, shape, sub, &ring.to_params());
    // The balls glow their own colours and flare on hits, and their streaks glow too.
    let ball = scene.things.values().find(|t| t.kind == "balls").map(|t| t.id).expect("a ball");
    let body = scene.things[&ball].slots[&SlotId::Look].layers[0].id;
    let sub = scene.add_sub_layer(reg, ball, SlotId::Look, body, SUB_LIGHT).expect("a ball's Body glows");
    let glow = GivesLight { strength: 1.6, flare: 2.5, flare_length: 0.35, ..Default::default() };
    set_all(&mut scene, ball, sub, &glow.to_params());
    let trail = scene.add_layer(reg, ball, SlotId::Look, TRAIL).expect("a trail");
    let streak = TrailLayer {
        seconds: 0.5,
        width: 0.7,
        brightness: 0.8,
        source: TrailSource::Own,
        colour: Paint {
            colours: Gradient::even(&[hex(0xffffff), hex(0xff5ca8), hex(0x8a4dff)]),
            follow: Follow::Life,
            ..Paint::default()
        },
        ..TrailLayer::default()
    };
    set_all(&mut scene, ball, trail, &streak.to_params());
    let sub = scene.add_sub_layer(reg, ball, SlotId::Look, trail, SUB_LIGHT).expect("a trail glows");
    let lit_trail = GivesLight { strength: 1.0, ..Default::default() };
    set_all(&mut scene, ball, sub, &lit_trail.to_params());
    scene
}

/// Lighting part 4's demo (its review video): a dark club in a white ring. A drifting, patchy
/// haze the balls stir as they fly through; a red laser bouncing round the ring and off the
/// balls; a warm spot through a window gobo, flickering like a candle; eight balls that heat up
/// on every hit and glow under the UV lamp riding their middle; and a floor that glows green in
/// the dark where light has been. Not in [`all`].
pub fn club_demo(reg: &Registry) -> Scene {
    use crate::modules::look::layers::heat::{SUB_HEAT, SubHeat};
    use crate::things::light::{FlickerStyle, Gobo, LightSettings, LightType, RidesOn, add_light};
    let mut scene = lit_room(reg);
    scene.name = "Club demo".into();
    let root = scene.things.get_mut(&ThingId::ROOT).expect("root");
    let mut s = SceneSettings::from_params(&root.params);
    s.background = Paint::solid(Color::rgb(0x30, 0x30, 0x36));
    s.bloom = 0.25;
    root.params = s.to_params();
    let lighting = root.slots.get_mut(&SlotId::Lighting).expect("lit");
    for (key, v) in [
        ("ambient", Value::Number(0.04)),
        ("haze", Value::Number(0.3)),
        ("haze_dims", Value::Number(0.15)),
        ("haze_moves", Value::Choice("swirling".into())),
        ("haze_size", Value::Number(260.0)),
        ("patchiness", Value::Number(0.8)),
        ("haze_stir", Value::Number(2.0)),
        ("glow_dark", Value::Number(0.3)),
        ("glow_fade", Value::Number(2.5)),
        ("charge", Value::Number(3.0)),
        ("light_blooms", Value::Number(0.6)),
    ] {
        lighting.params.set(key, v);
    }
    // The room's lamp becomes a warm spot from the top through a window, flickering.
    let first = scene.things.values().find(|t| t.kind == "light").map(|t| t.id).expect("a light");
    let mut spot = LightSettings::of_kind(LightType::Spot);
    spot.position = Vec2::new(-180.0, -470.0);
    spot.direction = 70.0;
    spot.cone = 60.0;
    spot.cone_edge = 12.0;
    spot.reach = 1600.0;
    spot.strength = 2.4;
    spot.height = 300.0;
    spot.size = 20.0;
    spot.beam = 1.2;
    spot.colour = Color::rgb(0xff, 0xc8, 0x8a);
    spot.gobo = Gobo::Window;
    spot.gobo_size = 140.0;
    spot.gobo_sharpness = 0.8;
    spot.flicker = 0.35;
    spot.flicker_style = FlickerStyle::Candle;
    spot.flicker_speed = 6.0;
    scene.things.get_mut(&first).expect("light").params = spot.to_params();
    scene.rename_thing(first, "Window spot");
    let shape = scene.things.values().find(|t| t.kind == "shape").map(|t| t.id).expect("a shape");
    scene.things.get_mut(&shape).expect("shape").params.set("thickness", Value::Number(10.0));
    // Eight balls flung out from the middle.
    let ball = scene.things.values().find(|t| t.kind == "balls").map(|t| t.id).expect("a ball");
    let mut balls = BallsSettings::from_params(&scene.things[&ball].params);
    balls.ring(8, 120.0);
    balls.launch = crate::things::balls::Launch::Outward;
    balls.velocity = Vec2::new(0.0, -900.0);
    scene.things.get_mut(&ball).expect("ball").params = balls.to_params();
    // Their Body heats on every hit and glows under UV.
    let card = &scene.things[&ball].slots[&SlotId::Look];
    let body = card.layers.iter().find(|l| l.kind.as_str() == crate::modules::look::layers::BODY).expect("a Body").id;
    let card = scene.card_mut(ball, SlotId::Look).expect("a look");
    card.params.set(&bpe_core::layered_key(body, "under_uv"), Value::Choice("glows".into()));
    card.params.set(&bpe_core::layered_key(body, "uv_glow"), Value::Number(2.0));
    let heat = scene.add_sub_layer(reg, ball, SlotId::Look, body, SUB_HEAT).expect("the Body heats");
    let hot = SubHeat { per_hit: 0.9, cool_down: 1.2, glow: 2.0, ..SubHeat::default() };
    let card = scene.card_mut(ball, SlotId::Look).expect("a look");
    for (k, v) in hot.to_params().iter() {
        card.params.set(&bpe_core::layered_key(heat, k), v.clone());
    }
    // A UV lamp riding their middle.
    let uv = add_light(&mut scene, reg, LightType::Point, ball).expect("a light");
    let mut black = LightSettings::of_kind(LightType::Point);
    black.position = Vec2::ZERO;
    black.rides_on = RidesOn::TheMiddle;
    black.colour = Color::rgb(0x8a, 0x5c, 0xff);
    black.strength = 1.6;
    black.reach = 700.0;
    black.height = 100.0;
    black.ultraviolet = true;
    black.uv_visible = 0.25;
    scene.things.get_mut(&uv).expect("light").params = black.to_params();
    scene.rename_thing(uv, "UV lamp");
    // A laser from the left wall, bouncing round the ring and off the balls.
    let laser = add_light(&mut scene, reg, LightType::Laser, ThingId::ROOT).expect("a light");
    let mut beam = LightSettings::of_kind(LightType::Laser);
    beam.position = Vec2::new(-480.0, 150.0);
    beam.direction = -12.0;
    beam.spin = 9.0;
    beam.points = crate::things::light::Points::Spins;
    beam.bounces = 12;
    scene.things.get_mut(&laser).expect("light").params = beam.to_params();
    scene.rename_thing(laser, "Laser");
    scene
}

/// Lighting part 4's underwater demo: a ring under water, rippling caustics and sun rays from
/// the top, the water's colour taking over deeper down, and balls falling through it. Not in
/// [`all`].
pub fn water_demo(reg: &Registry) -> Scene {
    use crate::things::light::{LightSettings, LightType};
    let mut scene = lit_room(reg);
    scene.name = "Water demo".into();
    let root = scene.things.get_mut(&ThingId::ROOT).expect("root");
    let mut s = SceneSettings::from_params(&root.params);
    s.background = Paint::solid(Color::rgb(0x48, 0x78, 0x90));
    s.bloom = 0.15;
    root.params = s.to_params();
    let lighting = root.slots.get_mut(&SlotId::Lighting).expect("lit");
    for (key, v) in [
        ("ambient", Value::Number(0.18)),
        ("underwater", Value::Number(1.0)),
        ("water_top", Value::Number(-960.0)),
        ("depth_fade", Value::Number(900.0)),
        ("caustics", Value::Number(1.0)),
        ("caustic_size", Value::Number(140.0)),
        ("sun_rays", Value::Number(0.8)),
        ("haze", Value::Number(0.35)),
        ("haze_colour", Value::Color(Color::rgb(0x9a, 0xd8, 0xff))),
    ] {
        lighting.params.set(key, v);
    }
    let first = scene.things.values().find(|t| t.kind == "light").map(|t| t.id).expect("a light");
    let mut sun = LightSettings::of_kind(LightType::Sun);
    sun.direction = 100.0;
    sun.elevation = 55.0;
    sun.strength = 0.8;
    sun.colour = Color::rgb(0xe8, 0xf6, 0xff);
    // Long sun shadows would streak the whole tank: the caustics and rays carry it.
    sun.casts = false;
    scene.things.get_mut(&first).expect("light").params = sun.to_params();
    scene.rename_thing(first, "Sun");
    let ball = scene.things.values().find(|t| t.kind == "balls").map(|t| t.id).expect("a ball");
    let mut balls = BallsSettings::from_params(&scene.things[&ball].params);
    balls.ring(6, 160.0);
    balls.launch = crate::things::balls::Launch::Outward;
    balls.velocity = Vec2::new(0.0, -500.0);
    scene.things.get_mut(&ball).expect("ball").params = balls.to_params();
    scene
}

/// The looks of the effects demo (lighting part 8's video), in the order it shows them.
pub const EFFECTS_DEMOS: [&str; 12] = [
    "tunnel",
    "hue_storm",
    "ball_worlds",
    "gravity_lens",
    "thermal",
    "solarize",
    "arcs",
    "aura",
    "negative",
    "polar_tunnel",
    "liquid_light",
    "melting",
];

/// The effects demo (lighting part 8's video): one of [`EFFECTS_DEMOS`] over the lit demo's twelve
/// balls flung out in a ring. Not in [`all`].
pub fn effects_demo(reg: &Registry, look: &str) -> Scene {
    use crate::modules::effects::EFFECTS;
    use crate::modules::look::layers::{arcs::ARCS, aura::AURA};
    use crate::things::light::{LightSettings, LightType, add_light};
    let mut scene = lit_demo(reg);
    scene.name = format!("Effects demo: {look}");
    let ball = scene.things.values().find(|t| t.kind == "balls").map(|t| t.id).expect("a ball");
    let effect = |scene: &mut Scene, kind: &str, set: &[(&str, Value)]| {
        if scene.card(ThingId::ROOT, SlotId::Shader).is_none() {
            scene.add_slot(reg, ThingId::ROOT, SlotId::Shader, EFFECTS).expect("the scene takes Effects");
        }
        let layer = scene.add_layer(reg, ThingId::ROOT, SlotId::Shader, kind).expect("an effect");
        let card = scene.things.get_mut(&ThingId::ROOT).expect("root").slots.get_mut(&SlotId::Shader).expect("fx");
        for (key, value) in set {
            card.params.set(&bpe_core::layered_key(layer, key), value.clone());
        }
    };
    match look {
        "tunnel" => {
            effect(&mut scene, "picture.echo", &[("fade", Value::Number(0.93)), ("zoom", Value::Number(1.02))]);
            effect(&mut scene, "picture.kaleidoscope", &[]);
        }
        "hue_storm" => effect(&mut scene, "picture.hue_storm", &[]),
        "ball_worlds" => effect(&mut scene, "picture.ball_worlds", &[]),
        "gravity_lens" => effect(&mut scene, "picture.gravity_lens", &[]),
        "thermal" => effect(&mut scene, "picture.thermal", &[]),
        "solarize" => effect(&mut scene, "picture.solarize", &[]),
        "polar_tunnel" => {
            effect(&mut scene, "picture.polar_tunnel", &[]);
            effect(&mut scene, "picture.rainbow_echoes", &[]);
        }
        "liquid_light" => effect(&mut scene, "picture.liquid_light", &[]),
        "melting" => effect(&mut scene, "picture.melting", &[]),
        "arcs" => {
            let layer = scene.add_layer(reg, ball, SlotId::Look, ARCS).expect("arcs");
            let look = scene.things.get_mut(&ball).expect("ball").slots.get_mut(&SlotId::Look).expect("look");
            look.params.set(&bpe_core::layered_key(layer, "reach"), Value::Number(420.0));
            look.params.set(&bpe_core::layered_key(layer, "lights"), Value::Number(1.0));
        }
        "aura" => {
            let layer = scene.add_layer(reg, ball, SlotId::Look, AURA).expect("aura");
            let look = scene.things.get_mut(&ball).expect("ball").slots.get_mut(&SlotId::Look).expect("look");
            look.params.set(&bpe_core::layered_key(layer, "reach"), Value::Number(9.0));
            look.params.set(&bpe_core::layered_key(layer, "lines"), Value::Number(4.0));
        }
        "negative" => {
            let dark = add_light(&mut scene, reg, LightType::Point, ThingId::ROOT).expect("a light");
            let mut s = LightSettings::of_kind(LightType::Point);
            s.position = glam::Vec2::new(0.0, 150.0);
            s.negative = true;
            s.strength = 2.5;
            s.reach = 700.0;
            s.height = 100.0;
            scene.things.get_mut(&dark).expect("light").params = s.to_params();
            scene.rename_thing(dark, "Darkness");
        }
        other => panic!("no demo look {other}"),
    }
    scene
}

/// Lighting part 7's scenes, where light moves the physics (their own goldens,
/// `tests/golden-moths*.txt`). Not in [`all`]: every other golden stays as it was.
pub fn moth_scenes(reg: &Registry) -> Vec<(String, Scene)> {
    vec![
        ("Moths".into(), moths(reg)),
        ("Light pressure".into(), light_pressure(reg)),
        ("Shadow switch".into(), shadow_switch(reg)),
    ]
}

/// [`lit_room`] darkened, its balls a ring of `count` without gravity, their Motion set by `f`.
fn moth_room(reg: &Registry, count: u32, f: impl FnOnce(&mut BasicMotion)) -> (Scene, ThingId, ThingId) {
    let mut scene = lit_room(reg);
    let root = scene.things.get_mut(&ThingId::ROOT).expect("root");
    root.slots.get_mut(&SlotId::Lighting).expect("lit").params.set("ambient", Value::Number(0.05));
    let ball = scene.things.values().find(|t| t.kind == "balls").map(|t| t.id).expect("a ball");
    let mut balls = BallsSettings::from_params(&scene.things[&ball].params);
    balls.radius = 14.0;
    balls.ring(count, 220.0);
    balls.velocity = Vec2::new(120.0, -60.0);
    scene.things.get_mut(&ball).expect("ball").params = balls.to_params();
    let card = scene.card_mut(ball, SlotId::Motion).expect("a motion card");
    let mut m = BasicMotion::from_params(&card.params);
    m.gravity_scale = 0.0;
    f(&mut m);
    card.params = m.to_params();
    let light = scene.things.values().find(|t| t.kind == "light").map(|t| t.id).expect("a light");
    (scene, ball, light)
}

/// Moths: 30 balls drawn to a lamp that circles the room.
pub fn moths(reg: &Registry) -> Scene {
    let (mut scene, _, light) = moth_room(reg, 30, |m| {
        m.moths = crate::modules::motion::Moths::DrawnToLight;
        m.moth_pull = 1500.0;
        m.moth_near = 60.0;
        m.drag = 0.4;
    });
    let at = bpe_core::Address::own(light, "position");
    for (k, t) in [0.0, 2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 14.0, 16.0, 18.0, 20.0].iter().enumerate() {
        let a = k as f32 * std::f32::consts::FRAC_PI_2;
        let p = Value::Vec2([300.0 * a.cos(), 300.0 * a.sin()]);
        scene.set_key(reg, &at, *t, p).expect("a key");
    }
    scene
}

/// Light pressure: a low sun pushes 30 balls across the room, those in the others' shadows less.
pub fn light_pressure(reg: &Registry) -> Scene {
    use crate::things::light::{LightSettings, LightType};
    let (mut scene, _, light) = moth_room(reg, 30, |m| {
        m.moths = crate::modules::motion::Moths::PushedByLight;
        m.moth_pull = 900.0;
        m.moth_shadows = true;
    });
    let mut sun = LightSettings::of_kind(LightType::Sun);
    sun.direction = 20.0;
    sun.strength = 1.2;
    scene.things.get_mut(&light).expect("light").params = sun.to_params();
    scene
}

/// Shadow switch: a spot from above; each ball's Gravity × goes from 0 to 3 by how much it is in
/// shadow, and every ball's wind follows the light on the group.
pub fn shadow_switch(reg: &Registry) -> Scene {
    use crate::things::light::{LightSettings, LightType};
    let (mut scene, ball, light) = moth_room(reg, 24, |_| {});
    let mut spot = LightSettings::of_kind(LightType::Spot);
    spot.position = Vec2::new(0.0, -480.0);
    spot.cone = 70.0;
    spot.reach = 1400.0;
    scene.things.get_mut(&light).expect("light").params = spot.to_params();
    scene.things.get_mut(&ThingId::ROOT).expect("root").params.set("gravity", Value::Vec2([0.0, 300.0]));
    let card = scene.card_mut(ball, SlotId::Motion).expect("a motion card");
    card.params.set("gravity_scale~to", Value::Number(3.0));
    card.params.set("gravity_scale~by", Value::Choice("shadow".into()));
    let wind = bpe_core::Address::on(ball, SlotId::Motion, "wind");
    let source = bpe_core::PulseSource::Light { thing: ball, source: bpe_core::LightSource::On };
    scene.set_pulse(reg, &wind, source, Value::Vec2([250.0, 0.0]), 0.25).expect("wind follows light");
    scene
}

#[cfg(test)]
mod tests {
    use crate::{registry, scenes};

    #[test]
    fn stage_scenes_resolve_and_round_trip() {
        let reg = registry();
        let scenes = scenes::all(&reg);
        assert_eq!(scenes.len(), 18);
        let in_order: Vec<String> = [
            scenes::stage2(&reg),
            scenes::stage3(&reg),
            scenes::stage5(&reg),
            vec![
                ("Colour paints".into(), scenes::colour_demo(&reg)),
                ("Blend modes".into(), scenes::blend_demo(&reg)),
                ("Graded sunset".into(), scenes::graded_sunset(&reg)),
                ("Ellipse focus".into(), scenes::ellipse_focus(&reg)),
            ],
            scenes::emitter_scenes(&reg),
        ]
        .concat()
        .into_iter()
        .map(|(n, _)| n)
        .collect();
        let names: Vec<&str> = scenes.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, in_order);
        assert_eq!(names[..5], ["One ball", "Reel block", "Box drop", "Spinning hexagon", "Shrinking ring"]);
        assert_eq!(names[5..7], ["Ball pit", "Riders"]);
        assert_eq!(names[7..12], ["Lava ball", "Confetti drop", "Spark block", "Kaleido hexagon", "Neon ring"]);
        assert_eq!(names[12..16], ["Colour paints", "Blend modes", "Graded sunset", "Ellipse focus"]);
        assert_eq!(names[16..], ["Emitter rounds", "Emitter chain"]);
        for (name, scene) in &scenes {
            assert_eq!(&scene.name, name);
            assert!(crate::resolve_scene(scene, &reg).problems.is_empty(), "{name}");
            let text = bpe_core::save(scene);
            assert_eq!(&bpe_core::load(&text, &reg).unwrap(), scene, "{name}");
        }
    }

    #[test]
    fn no_test_scene_uses_additive_colours() {
        use bpe_core::Value;
        fn additive(v: &Value) -> bool {
            match v {
                Value::Color(c) => c.is_additive(),
                Value::Gradient(g) => g.stops.iter().any(|s| s.color.is_additive()),
                _ => false,
            }
        }
        let reg = registry();
        let all = scenes::all(&reg);
        for (name, scene) in &all {
            let mut values: Vec<(String, &Value)> = Vec::new();
            for (id, t) in &scene.things {
                values.extend(t.params.0.iter().map(|(k, v)| (format!("thing {id:?} {k}"), v)));
                for (slot, card) in &t.slots {
                    values.extend(card.params.0.iter().map(|(k, v)| (format!("card {id:?} {slot:?} {k}"), v)));
                }
            }
            for (a, track) in &scene.tracks {
                values.extend(track.keys.iter().map(|k| (format!("track {a:?}"), &k.value)));
            }
            for (at, v) in values {
                assert!(!additive(v), "{name}: {at} is an additive colour, use Blend Add instead");
            }
        }
    }
}
