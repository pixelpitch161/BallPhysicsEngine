//! Test scenes for Text part 3 (letters as walls), pinned by `tests/golden_text.rs`: each of the
//! real-letter looks in the new scene's ring, and a spinning word balls bounce off.

use bpe_core::{Registry, Scene, ThingId, Value};
use glam::Vec2;

use crate::new_scene;
use crate::things::text::looks::{Look, add_text};

/// The new scene with its own ball switched off and `look` added.
fn with_look(reg: &Registry, look: Look) -> Scene {
    let mut scene = new_scene(reg);
    let ball = scene.order.iter().copied().find(|id| scene.things[id].kind == "balls").expect("a ball");
    scene.things.get_mut(&ball).expect("ball").enabled = false;
    add_text(&mut scene, reg, look, ThingId::ROOT).expect("a Text");
    scene.name = look.name().into();
    scene
}

/// The new scene's ball bouncing round a slowly spinning "SPIN" it bounces off.
fn spinning_word(reg: &Registry) -> Scene {
    let mut scene = new_scene(reg);
    scene.name = "Spinning word".into();
    let id = crate::add_thing(&mut scene, reg, crate::things::text::KIND, ThingId::ROOT).expect("a Text");
    let t = scene.things.get_mut(&id).expect("just added");
    t.screen = Some(false);
    t.transform.position = Vec2::new(0.0, 120.0);
    for (k, v) in [
        ("words", Value::Text("SPIN".into())),
        ("size", Value::Number(160.0)),
        ("spin", Value::Number(25.0)),
        ("balls", Value::Choice("bounce_off".into())),
    ] {
        t.params.set(k, v);
    }
    let ball = scene.order.iter().copied().find(|id| scene.things[id].kind == "balls").expect("a ball");
    scene.things.get_mut(&ball).expect("ball").params.set("velocity", Value::Vec2([420.0, -300.0]));
    scene
}

/// Every Text part 3 test scene, named.
pub fn text_scenes(reg: &Registry) -> Vec<(String, Scene)> {
    let looks = [
        Look::BallsInsideAWord,
        Look::BounceOffLetters,
        Look::PourInAndFill,
        Look::LettersBreakApart,
        Look::LettersThatPlayNotes,
    ];
    looks
        .into_iter()
        .map(|l| (l.name().to_owned(), with_look(reg, l)))
        .chain(std::iter::once(("Spinning word".to_owned(), spinning_word(reg))))
        .collect()
}
