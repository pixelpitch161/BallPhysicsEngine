//! Colour wars: two teams of balls in a ring. When balls of the two teams meet, each has an even
//! chance of joining the other team, until one colour has every ball.

use bpe_core::{Gradient, Registry, Scene, SlotId, ThingId, Value, Who};
use glam::Vec2;

use crate::library::build::{Block, Lib, Tone, choice, hex, n, text};
use crate::library::{Entry, Kind, Level, Payoff, PayoffEvent, SceneInfo, SoundLevel};
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale};
use crate::things::emitter::{Directions, Marker, StartFrom};
use crate::things::text::TextSettings;
use crate::things::text::looks::Look;
use crate::{BasicBounce, BasicMotion};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "colour-wars",
        name: "Colour wars",
        level: Level::InBetween,
        kinds: &[Kind::BattlesAndRaces],
        about: "Two teams of balls in a ring. When a red and a blue ball meet, each has an even chance of joining \
                the other team, until one colour takes every ball.",
        uses: &[
            "Emitter: the teams",
            "Rules: fired, hit, chance, tag, change colour, count",
            "Text: live counts",
            "Sound",
        ],
        change_first: "How many balls a team has (each cannon's How many, and the win at both together), their speed \
                       and the chance a meeting turns a ball.",
        sound: SoundLevel::Designed,
        seconds: 30.0,
        poster_seconds: 9.0,
        preview_seconds: 15.0,
        swatch: [0xff4a4a, 0x3d7bff],
        payoff: Some(Payoff { what: PayoffEvent::Counter(WINNER, ALL as f64), at_seconds: WIN, within_seconds: 0.5 }),
    },
    build,
};

/// Balls a team starts with, and every ball there is.
const TEAM: u32 = 20;
const ALL: u32 = 2 * TEAM;
/// Which team takes every ball, and when.
const WINNER: &str = "Blue";
const WIN: f64 = 23.6;

/// Each team: name, tag, cannon's place, colour, notes.
const TEAMS: [(&str, &str, Vec2, u32, [u8; 3]); 2] = [
    ("Red", "red", Vec2::new(-230.0, 120.0), 0xff4a4a, [57, 60, 64]),
    ("Blue", "blue", Vec2::new(230.0, 120.0), 0x3d7bff, [64, 67, 71]),
];

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 30.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x101014));
        s.bloom = 0.25;
        // The coin tosses' seed: with this one the war is won a little before the end.
        s.seed = 8;
    });

    let arena = lib.ring("Arena", Vec2::new(0.0, 120.0), 480.0, 12.0, "shape.chalk");
    lib.notes(arena, &[45, 52, 57], Plays::Random, Root::A, Scale::Minor);
    lib.tone(arena, Tone::Mallet);
    let title = lib.text(Look::HookTitle, "Title", "RED OR BLUE?");
    lib.at(title, Vec2::new(0.0, -800.0));
    let mut cannons = Vec::new();
    for (k, (name, _, at, colour, notes)) in TEAMS.iter().enumerate() {
        let cannon = lib.emitter(&format!("{name} cannon"), *at, |e| {
            e.radius = 18.0;
            e.count = TEAM;
            e.directions = Directions::AllRound;
            e.start_from = StartFrom::Ring;
            // Far enough out that a team's balls don't touch when fired.
            e.start_radius = 125.0;
            e.speed = 520.0;
            e.marker = Marker::None;
            // A moment after the start, so the rules see them fired.
            e.start_delay = 0.1;
        });
        lib.look(cannon, "candy");
        set_colour(&mut lib, cannon, hex(*colour));
        lib.card(cannon, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 0.0);
        lib.card(cannon, SlotId::Bounce, |b: &mut BasicBounce| {
            b.bounciness = 1.0;
            b.hit_balls = true;
        });
        lib.notes(cannon, notes, Plays::Random, Root::A, Scale::Minor);
        lib.tone(cannon, Tone::Pluck);
        let count = lib.text(Look::HitCounter, &format!("{name} count"), &format!("{{counter {name}}}"));
        lib.params(count, |t: &mut TextSettings| t.size = 150.0);
        lib.at(count, Vec2::new(if k == 0 { -260.0 } else { 260.0 }, -560.0));
        lib.colour_in(count, hex(*colour));
        cannons.push(cannon);
    }
    for (k, cannon) in cannons.iter().enumerate() {
        let (name, tag, ..) = TEAMS[k];
        lib.rule_doing(
            &format!("{name} cannon's balls join {name}"),
            &[("when.fired", vec![("who", lib.who(*cannon)), ("each", choice("a_ball"))])],
            &[tag_it(Who::That, tag, true), count(name, 1.0)],
        );
    }
    for k in 0..2 {
        let (name, tag, _, colour, _) = TEAMS[k];
        let (other, other_tag, ..) = TEAMS[1 - k];
        // The ball that turns is the rule's own (That), so a ball hit by two at once turns only once.
        lib.rule_doing(
            &format!("A {} ball turns {}", other.to_lowercase(), name.to_lowercase()),
            &[
                (
                    "when.hit",
                    vec![("who", Value::Who(Who::Tag(other_tag.into()))), ("whom", Value::Who(Who::Tag(tag.into())))],
                ),
                ("if.chance", vec![("percent", n(50.0))]),
                ("if.cooldown", vec![("seconds", n(0.25)), ("per", choice("each_ball"))]),
            ],
            &[
                tag_it(Who::That, other_tag, false),
                tag_it(Who::That, tag, true),
                (
                    "do.change",
                    vec![
                        ("who", Value::Who(Who::That)),
                        ("card", choice("look")),
                        ("setting", text("0/colours")),
                        ("to_gradient", Value::Gradient(Gradient::solid(hex(colour)))),
                    ],
                ),
                count(name, 1.0),
                count(other, -1.0),
            ],
        );
    }
    let t = lib.who(title);
    for (name, ..) in TEAMS {
        lib.rule_doing(
            &format!("{name} wins with every ball"),
            &[("when.count", vec![("of", choice("a_counter")), ("counter", text(name)), ("amount", n(ALL as f64))])],
            &[
                ("do.text", vec![("who", t.clone()), ("words", text(&format!("{} WINS!", name.to_uppercase())))]),
                ("do.camera", vec![("mode", choice("zoom_punch")), ("punch", n(0.12))]),
            ],
        );
    }
    lib.reverb(2.2, 22.0);
    lib.finish()
}

/// Do: Tag, adding or taking away `tag` on `who`.
fn tag_it(who: Who, tag: &str, add: bool) -> Block<'static> {
    let how = if add { "add" } else { "remove" };
    ("do.tag", vec![("who", Value::Who(who)), ("how", choice(how)), ("tag", text(tag))])
}

/// Do: Count, adding `amount` to the counter `name`.
fn count(name: &str, amount: f64) -> Block<'static> {
    ("do.count", vec![("counter", text(name)), ("amount", n(amount))])
}

/// A ball look's Body colour.
fn set_colour(lib: &mut Lib, id: ThingId, colour: bpe_core::Color) {
    let card = lib.scene.card_mut(id, SlotId::Look).expect("a Look card");
    card.params.set("0/colours", Value::Gradient(Gradient::solid(colour)));
}
