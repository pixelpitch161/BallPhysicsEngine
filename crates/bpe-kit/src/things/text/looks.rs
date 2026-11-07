//! The starting looks a Text can be added with (Text part 1: the spec's #1, #12, #13, #14, #18,
//! #24; part 2: the live ones, Hit counter, Countdown timer, Lyrics on the beat, Labels on balls
//! and Race leaderboard). A look only fills settings when it is picked from the Add thing menu; nothing here makes a
//! scene of its own.

use bpe_core::{Color, EditError, Gradient, Registry, Scene, Settings, SlotId, ThingId, Value, layered_key};
use glam::Vec2;

use super::{KIND, ShowBy, TextMove, TextPath, TextSettings, TimedBy};
use crate::text::Align;
use crate::modules::look::shape as look;

/// A Text's starting look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    Empty,
    HookTitle,
    SpinningRing,
    NeonSign,
    GlassLetters,
    ShadowLettering,
    FlowingGradient,
    HitCounter,
    CountdownTimer,
    LyricsOnTheBeat,
    LabelsOnBalls,
    RaceLeaderboard,
}

/// The looks the Add thing menu lists under Looks, in order.
pub const LOOKS: [Look; 11] = [
    Look::HookTitle,
    Look::SpinningRing,
    Look::NeonSign,
    Look::GlassLetters,
    Look::ShadowLettering,
    Look::FlowingGradient,
    Look::HitCounter,
    Look::CountdownTimer,
    Look::LyricsOnTheBeat,
    Look::LabelsOnBalls,
    Look::RaceLeaderboard,
];

impl Look {
    pub fn name(self) -> &'static str {
        match self {
            Look::Empty => "Empty text",
            Look::HookTitle => "Hook title",
            Look::SpinningRing => "Spinning word ring",
            Look::NeonSign => "Neon sign",
            Look::GlassLetters => "Glass letters",
            Look::ShadowLettering => "Shadow lettering",
            Look::FlowingGradient => "Flowing gradient",
            Look::HitCounter => "Hit counter",
            Look::CountdownTimer => "Countdown timer",
            Look::LyricsOnTheBeat => "Lyrics on the beat",
            Look::LabelsOnBalls => "Labels on balls",
            Look::RaceLeaderboard => "Race leaderboard",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            Look::Empty => "Plain white words on the screen, ready to type into.",
            Look::HookTitle => "A bold white title with a black outline and a drop shadow, at the top of the screen.",
            Look::SpinningRing => "Words round a circle that slowly turns, in the world.",
            Look::NeonSign => "Glowing pink tube letters that light up what is around them.",
            Look::GlassLetters => "Clear glass letters that bend what is behind them.",
            Look::ShadowLettering => "Warm letters lit by a lamp, throwing long shadows.",
            Look::FlowingGradient => "Rainbow colours flowing through the letters.",
            Look::HitCounter => "A big number counting every hit, at the top of the screen.",
            Look::CountdownTimer => "A clock counting down from 30 seconds.",
            Look::LyricsOnTheBeat => "Lines that pop in one at a time on the beat, then fade.",
            Look::LabelsOnBalls => "Each ball's number riding on it.",
            Look::RaceLeaderboard => "The five balls furthest down, their order changing as they race.",
        }
    }

    /// Whether the look needs the scene lit.
    fn lit(self) -> bool {
        matches!(self, Look::NeonSign | Look::GlassLetters | Look::ShadowLettering)
    }
}

/// A Text with the `look` filled in, under `parent`; a lit look gives the scene its Lighting card
/// when it has none, and Shadow lettering adds its lamp beside it.
pub fn add_text(scene: &mut Scene, reg: &Registry, look: Look, parent: ThingId) -> Result<ThingId, EditError> {
    let mut s = TextSettings::default();
    let mut screen = true;
    let mut position = Vec2::ZERO;
    let solid = |c: Color| Value::Gradient(Gradient::solid(c));
    // Layers to add, each with its settings and its sub-layers'.
    type Sub = (&'static str, Vec<(&'static str, Value)>);
    type Layer = (&'static str, Vec<(&'static str, Value)>, Vec<Sub>);
    let mut layers: Vec<Layer> = Vec::new();
    let mut card: Vec<(&'static str, Value)> = Vec::new();
    let mut parent = parent;
    // A black outline round white letters, readable over anything.
    let outlined = |width: f64| -> Layer {
        (
            look::OUTLINE,
            vec![
                ("width", Value::Number(width)),
                ("sits", Value::Choice("outside".into())),
                ("colour_from", Value::Choice("own".into())),
                ("colour", solid(Color::rgb(0, 0, 0))),
            ],
            vec![],
        )
    };
    match look {
        Look::Empty => {}
        Look::HookTitle => {
            s.words = "WAIT FOR\nTHE LAST\nONE…".into();
            s.size = 120.0;
            s.line_spacing = 0.95;
            // Each word pops in after the one before.
            s.show = ShowBy::WordByWord;
            s.appear = TextMove::Pop;
            s.each = 0.12;
            position = Vec2::new(0.0, -620.0);
            let shadow = vec![
                ("angle", Value::Number(90.0)),
                ("distance", Value::Number(8.0)),
                ("softness", Value::Number(6.0)),
                ("colour", Value::Color(Color::rgb(0, 0, 0))),
                ("opacity", Value::Number(0.6)),
            ];
            layers.push((look::PAINT, vec![("colours", solid(Color::WHITE))], vec![(look::SUB_SHADOW, shadow)]));
            layers.push((
                look::OUTLINE,
                vec![
                    ("width", Value::Number(6.0)),
                    ("sits", Value::Choice("outside".into())),
                    ("colour_from", Value::Choice("own".into())),
                    ("colour", solid(Color::rgb(0, 0, 0))),
                ],
                vec![],
            ));
        }
        Look::SpinningRing => {
            s.words = "ROUND AND ROUND • ROUND AND ROUND • ".into();
            s.font = "Bebas Neue".into();
            s.size = 70.0;
            s.letter_spacing = 6.0;
            s.path = TextPath::RoundACircle;
            s.radius = 330.0;
            s.spin = 30.0;
            screen = false;
            layers.push((look::PAINT, vec![("colours", solid(Color::rgb(0xff, 0xd8, 0x4a)))], vec![]));
        }
        Look::NeonSign => {
            s.words = "NEON".into();
            s.font = "Monoton".into();
            s.size = 200.0;
            screen = false;
            let pink = Color::rgb(0xff, 0x3d, 0xf0);
            // The dark tube behind the light.
            layers.push((look::PAINT, vec![("colours", solid(Color::rgb(0x2a, 0x0a, 0x28)))], vec![]));
            layers.push((
                look::OUTLINE,
                vec![
                    ("width", Value::Number(6.0)),
                    ("sits", Value::Choice("centre".into())),
                    ("colour_from", Value::Choice("own".into())),
                    ("colour", solid(pink)),
                ],
                vec![(look::SUB_LIGHT, vec![("strength", Value::Number(2.0))])],
            ));
            layers.push((
                look::GLOW,
                vec![("colour_from", Value::Choice("own".into())), ("colour", solid(pink))],
                vec![],
            ));
        }
        Look::GlassLetters => {
            s.words = "GLASS".into();
            s.font = "Bowlby One".into();
            s.size = 220.0;
            screen = false;
            use crate::modules::look::surface::{SurfaceKind, preset_values};
            use bpe_core::Choice;
            card.push(("surface", Value::Choice(SurfaceKind::Glass.name().into())));
            card.extend(preset_values(SurfaceKind::Glass));
        }
        Look::ShadowLettering => {
            s.words = "SUN".into();
            s.size = 260.0;
            screen = false;
            let mut g = Gradient::even(&[Color::rgb(0xff, 0xe7, 0xb0), Color::rgb(0xc4, 0x7f, 0x34)]);
            g.mix = bpe_core::Mix::Even;
            layers.push((
                look::PAINT,
                vec![
                    ("colours", Value::Gradient(g)),
                    ("colours~form", Value::Choice("straight".into())),
                    ("colours~turn", Value::Number(90.0)),
                ],
                vec![],
            ));
        }
        Look::FlowingGradient => {
            s.words = "FLOW".into();
            s.size = 220.0;
            let rainbow = crate::paint::palettes::built_in()
                .into_iter()
                .find(|p| p.name == "Rainbow")
                .map(|p| p.colours)
                .unwrap_or_else(|| Gradient::solid(Color::WHITE));
            layers.push((
                look::PAINT,
                vec![
                    ("colours", Value::Gradient(rainbow)),
                    ("colours~form", Value::Choice("straight".into())),
                    ("colours~cycle", Value::Number(0.3)),
                    ("colours~wave", Value::Number(1.0)),
                ],
                vec![],
            ));
        }
        Look::HitCounter => {
            s.words = "{hits}".into();
            s.size = 260.0;
            position = Vec2::new(0.0, -600.0);
            layers.push((look::PAINT, vec![("colours", solid(Color::WHITE))], vec![]));
            layers.push(outlined(10.0));
        }
        Look::CountdownTimer => {
            s.words = "{countdown 30|m:ss}".into();
            s.font = "Space Mono".into();
            s.size = 150.0;
            position = Vec2::new(0.0, -620.0);
            layers.push((look::PAINT, vec![("colours", solid(Color::WHITE))], vec![]));
            layers.push(outlined(6.0));
        }
        Look::LyricsOnTheBeat => {
            s.words = "HERE IT COMES\nWATCH IT FALL\nONE MORE TIME\nDROP".into();
            s.size = 110.0;
            s.show = ShowBy::LineByLine;
            s.timed_by = TimedBy::Beats;
            s.every = 4.0;
            s.appear = TextMove::Pop;
            s.disappear = TextMove::Fade;
            s.stays = 1.8;
            position = Vec2::new(0.0, -450.0);
            layers.push((look::PAINT, vec![("colours", solid(Color::WHITE))], vec![]));
            layers.push(outlined(7.0));
        }
        Look::LabelsOnBalls => {
            s.words = "{ball number}".into();
            s.size = 36.0;
            screen = false;
            // Inside the picked group of balls, else the first there is.
            let rides = |id: &ThingId| {
                scene.things.get(id).is_some_and(|t| matches!(t.kind.as_str(), "balls" | "emitter"))
            };
            if !rides(&parent)
                && let Some(first) = scene.order.iter().find(|id| rides(id))
            {
                parent = *first;
            }
            layers.push((look::PAINT, vec![("colours", solid(Color::WHITE))], vec![]));
            layers.push(outlined(4.0));
        }
        Look::RaceLeaderboard => {
            s.words = "{standings down 5}".into();
            s.font = "Space Mono".into();
            s.size = 42.0;
            s.align = Align::Left;
            s.line_spacing = 1.3;
            position = Vec2::new(-470.0, -760.0);
            layers.push((look::PAINT, vec![("colours", solid(Color::WHITE))], vec![]));
            layers.push(outlined(4.0));
        }
    }
    let id = crate::add_thing(scene, reg, KIND, parent)?;
    // An Empty text keeps the name a new Text gets; a look is named after itself.
    let name = (look != Look::Empty).then(|| {
        if let Some(t) = scene.things.get_mut(&id) {
            t.name.clear();
        }
        scene.free_name(look.name())
    });
    if let Some(t) = scene.things.get_mut(&id) {
        t.params = s.to_params();
        if let Some(name) = name {
            t.name = name;
        }
        t.screen = Some(screen);
        t.transform.position = position;
    }
    for (kind, params, subs) in layers {
        let layer = scene.add_layer(reg, id, SlotId::Look, kind)?;
        put(scene, id, layer, &params);
        for (sub_kind, sub_params) in subs {
            let sub = scene.add_sub_layer(reg, id, SlotId::Look, layer, sub_kind)?;
            put(scene, id, sub, &sub_params);
        }
    }
    if let Some(c) = scene.card_mut(id, SlotId::Look) {
        for (k, v) in card {
            c.params.set(k, v);
        }
    }
    if look.lit() {
        let lit = scene.things.get(&ThingId::ROOT).is_some_and(|r| r.slots.contains_key(&SlotId::Lighting));
        if !lit {
            scene.add_slot(reg, ThingId::ROOT, SlotId::Lighting, crate::modules::lighting::LIGHTING)?;
        }
    }
    if look == Look::ShadowLettering {
        use crate::things::light::{LightType, add_light};
        let lamp = add_light(scene, reg, LightType::Point, parent)?;
        if let Some(l) = scene.things.get_mut(&lamp) {
            l.params.set("position", Value::Vec2([position.x - 300.0, position.y - 700.0]));
            l.params.set("colour", Value::Color(Color::rgb(0xff, 0xe8, 0xc8)));
        }
    }
    Ok(id)
}

/// Writes `params` as layer `layer`'s settings on the Text's Look card.
fn put(scene: &mut Scene, id: ThingId, layer: bpe_core::LayerId, params: &[(&str, Value)]) {
    if let Some(card) = scene.card_mut(id, SlotId::Look) {
        for (k, v) in params {
            card.params.set(&layered_key(layer, k), v.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::resolve;
    use bpe_gfx::{Frame, build_frame};
    use bpe_runtime::{Log, World};

    fn added(look: Look) -> (Scene, ThingId) {
        let reg = crate::registry();
        let mut scene = Scene::empty(&reg);
        let id = add_text(&mut scene, &reg, look, ThingId::ROOT).unwrap();
        (scene, id)
    }

    #[test]
    fn every_look_adds_a_text_that_resolves_cleanly() {
        let kit = crate::kit();
        let reg = &kit.registry;
        for look in std::iter::once(Look::Empty).chain(LOOKS) {
            // Beside a group of balls (the leaderboard and the labels read them), a moment in.
            let mut scene = Scene::empty(reg);
            let balls = crate::add_thing(&mut scene, reg, "balls", ThingId::ROOT).unwrap();
            let mut b = crate::BallsSettings::from_params(&scene.things[&balls].params);
            b.ring(4, 200.0);
            scene.things.get_mut(&balls).unwrap().params = b.to_params();
            add_text(&mut scene, reg, look, ThingId::ROOT).unwrap();
            let resolved = resolve(&scene, reg);
            assert!(resolved.problems.is_empty(), "{look:?}: {:?}", resolved.problems);
            let mut world = World::build(&scene, &resolved, &kit.sims);
            let mut log = Log::default();
            world.advance_to(1.0, &kit.sims, &mut log);
            let mut frame = Frame::default();
            build_frame(&scene, &resolved, &world, &log, 1.0, &kit.draws, &mut frame);
            let paths = frame.paths.len()
                + frame.overlay.as_ref().map_or(0, |o| o.paths.len())
                + frame.layers.iter().map(|l| l.frame.paths.len()).sum::<usize>();
            assert!(paths > 0, "{look:?} draws");
        }
    }

    #[test]
    fn the_lit_looks_light_the_scene() {
        for look in LOOKS {
            let (scene, _) = added(look);
            let lit = scene.things[&ThingId::ROOT].slots.contains_key(&SlotId::Lighting);
            assert_eq!(lit, look.lit(), "{look:?}");
            let lights = scene.things.values().filter(|t| t.kind == crate::things::light::LIGHT).count();
            assert_eq!(lights, (look == Look::ShadowLettering) as usize, "{look:?}");
        }
    }

    #[test]
    fn each_look_sets_its_words_and_font() {
        let table = [
            (Look::HookTitle, "WAIT FOR\nTHE LAST\nONE…", "Anton", true),
            (Look::SpinningRing, "ROUND AND ROUND • ROUND AND ROUND • ", "Bebas Neue", false),
            (Look::NeonSign, "NEON", "Monoton", false),
            (Look::GlassLetters, "GLASS", "Bowlby One", false),
            (Look::ShadowLettering, "SUN", "Anton", false),
            (Look::FlowingGradient, "FLOW", "Anton", true),
            (Look::HitCounter, "{hits}", "Anton", true),
            (Look::CountdownTimer, "{countdown 30|m:ss}", "Space Mono", true),
            (Look::LyricsOnTheBeat, "HERE IT COMES\nWATCH IT FALL\nONE MORE TIME\nDROP", "Anton", true),
            (Look::LabelsOnBalls, "{ball number}", "Anton", false),
            (Look::RaceLeaderboard, "{standings down 5}", "Space Mono", true),
        ];
        for (look, words, font, screen) in table {
            let (scene, id) = added(look);
            let s = TextSettings::from_params(&scene.things[&id].params);
            assert_eq!((s.words.as_str(), s.font.as_str()), (words, font), "{look:?}");
            assert_eq!(scene.things[&id].screen, Some(screen), "{look:?}");
            assert_eq!(scene.things[&id].name, look.name(), "{look:?}");
        }
        let (scene, id) = added(Look::SpinningRing);
        let s = TextSettings::from_params(&scene.things[&id].params);
        assert_eq!((s.path, s.radius, s.spin), (TextPath::RoundACircle, 330.0, 30.0));
        let (scene, id) = added(Look::GlassLetters);
        assert_eq!(scene.card(id, SlotId::Look).unwrap().params.get("surface"), Some(&Value::Choice("glass".into())));
    }

    #[test]
    fn the_live_looks_show_and_ride() {
        let (scene, id) = added(Look::HookTitle);
        let s = TextSettings::from_params(&scene.things[&id].params);
        assert_eq!((s.show, s.appear, s.each), (ShowBy::WordByWord, TextMove::Pop, 0.12));
        let (scene, id) = added(Look::LyricsOnTheBeat);
        let s = TextSettings::from_params(&scene.things[&id].params);
        assert_eq!((s.show, s.timed_by, s.every, s.stays), (ShowBy::LineByLine, TimedBy::Beats, 4.0, 1.8));
        // Labels go inside the first group of balls when none is picked.
        let reg = crate::registry();
        let mut scene = Scene::empty(&reg);
        let balls = crate::add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
        let id = add_text(&mut scene, &reg, Look::LabelsOnBalls, ThingId::ROOT).unwrap();
        assert_eq!(scene.things[&id].parent, Some(balls));
        // With no balls, under what was picked.
        let (scene, id) = added(Look::LabelsOnBalls);
        assert_eq!(scene.things[&id].parent, Some(ThingId::ROOT));
    }
}
