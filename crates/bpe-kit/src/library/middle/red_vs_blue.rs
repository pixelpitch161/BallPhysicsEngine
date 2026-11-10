//! Red vs Blue: two big balls fight in an arena with spinning bumpers. Every hit on a bumper or the
//! other ball takes health by how hard it was; the first to run out shatters and the other wins.

use bpe_core::{Registry, Scene, SlotId, Target, ThingId};
use glam::Vec2;

use crate::library::build::{Lib, Tone, choice, hex, n, text};
use crate::library::{Entry, Kind, Level, Payoff, PayoffEvent, SceneInfo, SoundLevel};
use crate::modules::health::{AtZero, HealthSettings, HitsTake};
use crate::modules::look::layers::trail::TRAIL;
use crate::paint::Paint;
use crate::rules::breaks::BreakLook;
use crate::sound::{Plays, Root, Scale};
use crate::things::shape::ShapeFill;
use crate::things::text::TextSettings;
use crate::things::text::looks::Look;
use crate::{BallsSettings, BasicBounce, BasicMotion, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "red-vs-blue",
        name: "Red vs Blue",
        level: Level::InBetween,
        kinds: &[Kind::BattlesAndRaces],
        about: "Two big balls fight in an arena of spinning bumpers. Every hit on a bumper or the other ball takes \
                health by how hard it was; the first to run out shatters and the other wins.",
        uses: &[
            "Balls",
            "Health: by strength",
            "Shape: spinning bumpers",
            "Rules: health, set text, camera, slow \
                 motion",
            "Text: live health",
            "Sound",
        ],
        change_first: "How much a hit takes (each ball's Health card, Per px/s of strength), the balls' speed and \
                       the bumpers.",
        sound: SoundLevel::Designed,
        seconds: 30.0,
        poster_seconds: 8.0,
        preview_seconds: 15.0,
        swatch: [0xff4a4a, 0x3d7bff],
        payoff: Some(Payoff { what: PayoffEvent::Breaks(LOSER), at_seconds: KO, within_seconds: 0.5 }),
    },
    build,
};

/// Who runs out of health first, and when.
const LOSER: &str = "Red";
const KO: f64 = 18.8;

/// The tag of what takes health when it hits a fighter: the bumpers and the other fighter (not the
/// arena's walls).
const HURTS: &str = "hurts";

/// Each fighter: name, where it starts, which way it goes, colour, its notes.
const FIGHTERS: [(&str, Vec2, Vec2, u32, [u8; 3]); 2] = [
    ("Red", Vec2::new(-220.0, -260.0), Vec2::new(620.0, 540.0), 0xff4a4a, [45, 48, 52]),
    ("Blue", Vec2::new(220.0, 520.0), Vec2::new(-560.0, -600.0), 0x3d7bff, [57, 60, 64]),
];

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 30.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x101018));
        s.bloom = 0.3;
    });
    let arena = lib.add("shape", ThingId::ROOT, "Arena");
    lib.at(arena, Vec2::new(0.0, 140.0));
    lib.params(arena, |s: &mut ShapeSettings| *s = ShapeSettings::rectangle(940.0, 1300.0, 14.0));
    lib.look(arena, "shape.chrome_edge");
    let bumpers = lib.add("folder", ThingId::ROOT, "Bumpers");
    for (k, (at, spin)) in
        [(Vec2::new(-230.0, 0.0), 90.0), (Vec2::new(230.0, 300.0), -90.0), (Vec2::new(0.0, 600.0), 60.0)]
            .into_iter()
            .enumerate()
    {
        let bumper = lib.add("shape", bumpers, &format!("Bumper {}", k + 1));
        lib.at(bumper, at);
        lib.params(bumper, |s: &mut ShapeSettings| {
            *s = ShapeSettings { fill: ShapeFill::Filled, spin, ..ShapeSettings::polygon(3, 70.0, 4.0) };
        });
        lib.look_in(bumper, "shape.solid", hex(0xe8e8f0));
        lib.scene.things.get_mut(&bumper).expect("a bumper").tags.insert(HURTS.into());
    }
    let title = lib.text(Look::HookTitle, "Title", "WHO WINS?");
    lib.at(title, Vec2::new(0.0, -800.0));
    let mut fighters = Vec::new();
    for (k, (name, at, velocity, colour, notes)) in FIGHTERS.iter().enumerate() {
        let ball = lib.add("balls", ThingId::ROOT, name);
        lib.at(ball, *at);
        lib.params(ball, |s: &mut BallsSettings| {
            s.radius = 70.0;
            s.velocity = *velocity;
        });
        lib.look_in(ball, "glow_orb", hex(*colour));
        lib.card(ball, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 0.0);
        lib.card(ball, SlotId::Bounce, |b: &mut BasicBounce| {
            b.bounciness = 1.0;
            b.hit_balls = true;
        });
        lib.layer(ball, SlotId::Look, TRAIL);
        lib.fill(ball, SlotId::Health);
        lib.card(ball, SlotId::Health, |h: &mut HealthSettings| {
            h.start = 100.0;
            h.takes = HitsTake::ByStrength;
            h.factor = 0.006;
            h.from = Target::Tag(HURTS.into());
            h.at_zero = AtZero::Break;
            h.look = BreakLook::Shatter;
        });
        lib.notes(ball, notes, Plays::Random, Root::A, Scale::Minor);
        lib.tone(ball, if k == 0 { Tone::Bass } else { Tone::Pluck });
        let health = lib.text(Look::HitCounter, &format!("{name} health"), &format!("{{health of {name}|0}}"));
        lib.params(health, |t: &mut TextSettings| t.size = 110.0);
        lib.at(health, Vec2::new(if k == 0 { -270.0 } else { 270.0 }, -620.0));
        lib.colour_in(health, hex(*colour));
        lib.scene.things.get_mut(&ball).expect("a fighter").tags.insert(HURTS.into());
        fighters.push((ball, health));
    }
    let t = lib.who(title);
    for (k, (ball, health)) in fighters.iter().enumerate() {
        let (loser, winner) = (FIGHTERS[k].0, FIGHTERS[1 - k].0);
        lib.rule_doing(
            &format!("{winner} wins when {loser} runs out"),
            &[("when.health", vec![("who", lib.who(*ball)), ("event", choice("reaches_zero"))])],
            &[
                ("do.text", vec![("who", t.clone()), ("words", text(&format!("{} WINS!", winner.to_uppercase())))]),
                // A broken ball has no health to show, so its count is set to 0 by hand.
                ("do.text", vec![("who", lib.who(*health)), ("words", text("0"))]),
                ("do.camera", vec![("mode", choice("zoom_punch")), ("punch", n(0.15))]),
                ("do.slow", vec![("speed", n(0.25)), ("lasts", choice("for_a_time")), ("seconds", n(1.5))]),
                // The winner can't be hurt any more.
                (
                    "do.change",
                    vec![
                        ("who", lib.who(fighters[1 - k].0)),
                        ("card", choice("health")),
                        ("setting", text("takes")),
                        ("to_choice", text("nothing")),
                    ],
                ),
            ],
        );
    }
    lib.reverb(2.0, 22.0);
    lib.finish()
}
