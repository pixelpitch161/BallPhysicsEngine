//! Golden hour: a slow, filmic scene. A low sun from the right sinks and reddens as golden balls drop one by
//! one and roll down a zigzag of gentle slopes, each landing playing the next note of the opening of Satie's
//! Gymnopédie No. 1 over a soft pad. The whole picture is graded warm, with a gentle S curve, orange highlights,
//! teal shadows, a vignette and grain, and the tune always finishes before the sun is gone.

use bpe_core::{Color, Gradient, Registry, Scene, SlotId, ThingId, Value};
use glam::Vec2;

use crate::library::build::{Lib, Tone, choice, hex, n, text};
use crate::library::{Entry, Kind, Level, Payoff, PayoffEvent, SceneInfo, SoundLevel};
use crate::modules::grade::basic::BasicPanel;
use crate::modules::grade::colour_grading::ColourGradingPanel;
use crate::modules::grade::effects::EffectsPanel;
use crate::modules::grade::tone_curve::ToneCurvePanel;
use crate::modules::grade::{BASIC, COLOUR_GRADING, EFFECTS, TONE_CURVE, ensure_grade, whole_of};
use crate::modules::lighting::LightingSettings;
use crate::paint::{Paint, Space};
use crate::things::emitter::{Directions, EmitterSettings, FireWhen, Marker};
use crate::things::light::LightType;
use crate::things::shape::ShapeFill;
use crate::things::text::TextSettings;
use crate::things::text::looks::Look;
use crate::{BallsSettings, BasicBounce, BasicMotion, SceneSettings, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "golden-hour",
        name: "Golden hour",
        level: Level::Spectacle,
        kinds: &[Kind::LightAndGlass, Kind::Music],
        about: "A low sun sinks and reddens as golden balls drop one by one and roll down gentle slopes, each \
                landing playing the next note of Satie's Gymnopédie No. 1 over a soft pad. Graded like film: warm, \
                with orange highlights, teal shadows, a vignette and grain.",
        uses: &[
            "Light: a low sun, keyframed",
            "Colour grading: Basic, Tone curve, Colour grading, Effects",
            "Emitter: one ball at a time",
            "Rules: hit, cooldown, counter, a melody",
            "Sound: piano and pad",
            "Text: a countdown",
        ],
        change_first: "The grade (Scene settings' Grade card), the sun (its Elevation and Colour keys), and the tune \
                       (the Piano's Sound card).",
        sound: SoundLevel::Designed,
        seconds: 36.0,
        poster_seconds: 30.0,
        preview_seconds: 14.0,
        swatch: [0xffa040, 0x1c0f1e],
        payoff: Some(Payoff {
            what: PayoffEvent::Counter(NOTES, TUNE.len() as f64),
            at_seconds: TUNE_DONE,
            within_seconds: 0.1,
        }),
    },
    build,
};

const NOTES: &str = "Notes";
/// When the tune's last note plays (for the payoff check).
const TUNE_DONE: f64 = 26.9;
const SECONDS: f64 = 36.0;
const TEMPO: f32 = 72.0;
/// The opening phrase of Gymnopédie No. 1 (public domain), twice.
const TUNE: [u8; 20] = [66, 69, 67, 66, 61, 59, 61, 62, 57, 54, 66, 69, 67, 66, 61, 59, 61, 62, 57, 54];
/// The accompaniment's two chords, a bar each: G major seventh and D major seventh.
const CHORDS: [[u8; 4]; 2] = [[43, 59, 62, 66], [38, 57, 61, 66]];
/// The box the slopes sit in.
const BOX_AT: Vec2 = Vec2::new(-100.0, 130.0);
const BOX: Vec2 = Vec2::new(720.0, 1500.0);
/// How far each slope tilts, in radians.
const TILT: f32 = 0.2;

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, SECONDS);
    lib.scene_settings(|s: &mut SceneSettings| {
        // A dusk sky: deep violet above, warm at the horizon.
        s.background = Paint {
            colours: Gradient::even(&[Color::rgb(28, 16, 40), Color::rgb(92, 40, 52), Color::rgb(190, 96, 58)]),
            space: Space::Screen,
            ..Paint::default()
        };
        s.bloom = 0.35;
        s.tempo = TEMPO;
    });
    let frame = lib.add("shape", ThingId::ROOT, "Frame");
    lib.at(frame, BOX_AT);
    lib.params(frame, |s: &mut ShapeSettings| *s = ShapeSettings::rectangle(BOX.x, BOX.y, 12.0));
    lib.look_in(frame, "shape.gold_edge", hex(0xd8a050));

    // The setting sun: a glowing ball beside the box, sinking slowly over the whole scene.
    let sun_disc = lib.add("balls", ThingId::ROOT, "Setting sun");
    lib.at(sun_disc, Vec2::new(445.0, -430.0));
    lib.params(sun_disc, |s: &mut BallsSettings| {
        s.radius = 80.0;
        s.velocity = Vec2::new(0.0, 1000.0 / SECONDS as f32);
    });
    lib.card(sun_disc, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 0.0);
    lib.look_in(sun_disc, "glow_orb", hex(0xffb060));
    lib.unlit(sun_disc);
    lib.scene.things.get_mut(&sun_disc).expect("made").z_order = -1;

    // A zigzag of slopes, each fixed to one wall with a gap at its low end for the ball to drop through.
    let slopes = lib.add("folder", ThingId::ROOT, "Slopes");
    let inside = BOX.x / 2.0;
    let length = BOX.x - 130.0;
    for k in 0..4 {
        let low_left = k % 2 == 0;
        let slope = lib.add("shape", slopes, &format!("Slope {}", k + 1));
        let x = if low_left { inside - length / 2.0 } else { length / 2.0 - inside };
        lib.at(slope, Vec2::new(BOX_AT.x + x, -420.0 + 300.0 * k as f32));
        lib.scene.things.get_mut(&slope).expect("made").transform.rotation = if low_left { -TILT } else { TILT };
        lib.params(slope, |s: &mut ShapeSettings| {
            *s = ShapeSettings { fill: ShapeFill::Filled, ..ShapeSettings::rectangle(length, 22.0, 6.0) }
        });
        lib.look_in(slope, "shape.solid", hex(0x2a1a22));
    }

    let drops = lib.emitter("Dropper", Vec2::new(BOX_AT.x + inside - 80.0, -560.0), |s: &mut EmitterSettings| {
        s.radius = 26.0;
        s.directions = Directions::Fan;
        s.count = 1;
        s.speed = 0.0;
        s.fire_when = FireWhen::Every;
        s.every = 1.6;
        s.endless = true;
        s.most_balls = 18;
        s.marker = Marker::None;
    });
    lib.look(drops, "gold");
    lib.card(drops, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 1.0);
    lib.card(drops, SlotId::Bounce, |b: &mut BasicBounce| {
        b.bounciness = 0.35;
        b.friction = 0.0;
        b.hit_balls = true;
    });

    // The sun: low on the right, sinking and reddening, the shadows growing longer.
    let sun = lib.light("Sun", LightType::Sun, Vec2::ZERO, |l| {
        l.direction = 172.0;
        l.elevation = 22.0;
        l.sun_size = 6.0;
        l.strength = 2.0;
        l.colour = Color::rgb(255, 214, 150);
    });
    lib.keys(sun, None, "elevation", &[(0.0, n(22.0)), (SECONDS, n(4.0))]);
    lib.keys(
        sun,
        None,
        "colour",
        &[(0.0, Value::Color(Color::rgb(255, 214, 150))), (SECONDS, Value::Color(Color::rgb(255, 110, 60)))],
    );
    lib.keys(sun, None, "strength", &[(0.0, n(2.0)), (SECONDS, n(1.3))]);
    lib.card(ThingId::ROOT, SlotId::Lighting, |l: &mut LightingSettings| {
        l.ambient = 0.2;
        l.ambient_colour = Color::rgb(120, 90, 150);
        l.background_lit = 0.0;
    });

    // The film look.
    ensure_grade(&mut lib.scene, reg);
    let card = lib.scene.card(ThingId::ROOT, SlotId::Grade).expect("a Grade card");
    let whole = whole_of(card).expect("the whole picture");
    let panel = |kind: &str| card.layers.iter().find(|l| l.parent == Some(whole) && l.kind == kind).map(|l| l.id);
    let (basic, curve, grading, effects) = (
        panel(BASIC).expect("Basic"),
        panel(TONE_CURVE).expect("Tone curve"),
        panel(COLOUR_GRADING).expect("Colour grading"),
        panel(EFFECTS).expect("Effects"),
    );
    lib.layer_params(ThingId::ROOT, SlotId::Grade, basic, |b: &mut BasicPanel| {
        b.temp = 28.0;
        b.tint = 6.0;
        b.contrast = 10.0;
        b.highlights = -20.0;
        b.vibrance = 18.0;
    });
    lib.layer_params(ThingId::ROOT, SlotId::Grade, curve, |c: &mut ToneCurvePanel| {
        c.lights = 12.0;
        c.darks = -10.0;
        c.shadows = -6.0;
    });
    lib.layer_params(ThingId::ROOT, SlotId::Grade, grading, |g: &mut ColourGradingPanel| {
        g.shadows_hue = 190.0;
        g.shadows_saturation = 30.0;
        g.highlights_hue = 35.0;
        g.highlights_saturation = 40.0;
    });
    lib.layer_params(ThingId::ROOT, SlotId::Grade, effects, |e: &mut EffectsPanel| {
        e.vignette_amount = -35.0;
        e.grain_amount = 22.0;
        e.grain_size = 30.0;
    });

    let title = lib.text(Look::HookTitle, "Title", "GOLDEN HOUR");
    lib.params(title, |t: &mut TextSettings| t.size = 100.0);
    lib.at(title, Vec2::new(0.0, -830.0));
    lib.unlit(title);
    let clock = lib.text(Look::CountdownTimer, "Sunset clock", "SUNSET IN {countdown 36|m:ss}");
    lib.params(clock, |t: &mut TextSettings| t.size = 56.0);
    lib.at(clock, Vec2::new(0.0, -700.0));
    lib.unlit(clock);

    // The music: a piano and a pad, look-less, off the picture.
    let band = lib.add("folder", ThingId::ROOT, "Band");
    let speaker = |lib: &mut Lib, name: &str, tone: Tone| {
        let id = lib.add("shape", band, name);
        lib.at(id, Vec2::new(0.0, 1400.0));
        lib.params(id, |s: &mut ShapeSettings| {
            *s = ShapeSettings { passes: true, ..ShapeSettings::rectangle(40.0, 40.0, 4.0) }
        });
        lib.scene.things.get_mut(&id).expect("made").slots.remove(&SlotId::Look);
        lib.tone(id, tone);
        id
    };
    let piano = speaker(&mut lib, "Piano", Tone::Piano);
    lib.melody(piano, &TUNE.iter().map(std::slice::from_ref).collect::<Vec<_>>());
    let pad = speaker(&mut lib, "Pad", Tone::Pad);
    lib.melody(pad, &CHORDS.iter().map(|c| c.as_slice()).collect::<Vec<_>>());

    let who = |id: ThingId| Value::Who(bpe_core::Who::Thing { thing: id, ball: None });
    let next = |id: ThingId, velocity: f64| {
        ("do.note", vec![("who", who(id)), ("pitch_from", choice("the_shapes_next_note")), ("velocity", n(velocity))])
    };
    let tune_left = (
        "if.counter",
        vec![("counter", text(NOTES)), ("test", choice("under")), ("amount", n(TUNE.len() as f64 - 0.5))],
    );
    lib.rule_doing(
        "A landing plays the next note",
        &[
            (
                "when.hit",
                vec![
                    ("who", lib.who(drops)),
                    ("whom", Value::Who(bpe_core::Who::Kind("shape".into()))),
                    ("min_strength", n(60.0)),
                ],
            ),
            ("if.cooldown", vec![("seconds", n(1.5 * 60.0 / TEMPO as f64))]),
            tune_left,
        ],
        &[next(piano, 0.7), ("do.count", vec![("counter", text(NOTES)), ("amount", n(1.0))])],
    );
    lib.rule_doing("The pad, a chord a bar", &[("when.beat", vec![("every", choice("bar"))])], &[next(pad, 0.45)]);
    lib.rule_doing(
        "The tune is done",
        &[(
            "when.count",
            vec![
                ("of", choice("a_counter")),
                ("counter", text(NOTES)),
                ("reach", choice("rises_to")),
                ("amount", n(TUNE.len() as f64)),
            ],
        )],
        &[("do.text", vec![("who", lib.who(title)), ("words", text("GOODNIGHT, SUN"))])],
    );
    lib.reverb(3.2, 30.0);
    lib.finish()
}
