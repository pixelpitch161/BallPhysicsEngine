//! Every built-in scene (`bpe_kit::library`) is tidy, opens, plays its whole length and draws a picture
//! (spec `docs/superpowers/specs/2026-11-10-built-in-scenes-design.md`, "Checks"). Its sound is checked in
//! `bpe-sound/tests/library_sound.rs`.

use bpe_core::{Registry, Scene, Settings, ThingId, load, resolve, save};
use bpe_gfx::{Frame, Renderer, build_frame, test_device};
use bpe_kit::library::{self, Entry, Level, PayoffEvent, SoundLevel};
use bpe_kit::{SceneSettings, kit};
use bpe_runtime::events::EventKind;
use bpe_runtime::{Log, Playback, World};
use std::collections::BTreeSet;

fn is_kebab(id: &str) -> bool {
    !id.is_empty()
        && id.split('-').all(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()))
}

#[test]
fn ids_names_and_info_are_tidy() {
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    for e in library::all() {
        let i = &e.info;
        assert!(is_kebab(i.id), "{}: kebab case", i.id);
        assert!(ids.insert(i.id), "{}: id used twice", i.id);
        assert!(names.insert(i.name), "{}: name used twice", i.name);
        assert!(std::ptr::eq(library::find(i.id).expect("found"), e));
        assert!(!i.about.trim().is_empty() && !i.change_first.trim().is_empty(), "{}", i.id);
        assert!(!i.uses.is_empty(), "{}: uses", i.id);
        assert!((1..=2).contains(&i.kinds.len()), "{}: one or two kinds", i.id);
        assert!(0.0 < i.poster_seconds && i.poster_seconds < i.seconds, "{}: poster inside the video", i.id);
        assert!(0.0 < i.preview_seconds && i.preview_seconds <= 15.0 && i.preview_seconds <= i.seconds, "{}", i.id);
        if i.level == Level::Starter {
            assert_eq!(i.sound, SoundLevel::Simple, "{}: a starter has one simple sound", i.id);
        }
        if i.level == Level::Spectacle {
            assert!((25.0..=40.0).contains(&i.seconds), "{}: a spectacle is 25 to 40 s", i.id);
            assert_eq!(i.sound, SoundLevel::Designed, "{}", i.id);
        }
    }
    assert!(library::find("no-such-scene").is_none());
}

#[test]
fn levels_so_far() {
    let count = |l: Level| library::all().iter().filter(|e| e.info.level == l).count();
    assert_eq!((count(Level::Starter), count(Level::InBetween), count(Level::Spectacle)), (8, 0, 0));
}

/// Every kind the scene names (things, cards, layers, rule blocks) is one the program has.
fn all_kinds_known(scene: &Scene, reg: &Registry, id: &str) {
    for t in scene.things.values() {
        assert!(reg.thing(&t.kind).is_some(), "{id}: thing kind {}", t.kind);
        for (slot, card) in &t.slots {
            assert!(reg.module(&card.kind).is_some(), "{id}: {} {slot:?} card {}", t.name, card.kind);
            for l in &card.layers {
                assert!(reg.layer(&l.kind).is_some(), "{id}: {} layer {}", t.name, l.kind);
            }
        }
    }
    for rule in &scene.rules.list {
        for b in &rule.blocks {
            assert!(reg.block(&b.kind).is_some(), "{id}: rule {} block {}", rule.name, b.kind);
        }
    }
}

#[test]
fn every_scene_builds_tidily() {
    let reg = kit().registry;
    for e in library::all() {
        let id = e.info.id;
        let scene = e.scene(&reg);
        assert_eq!(scene.name, e.info.name);
        all_kinds_known(&scene, &reg, id);
        let resolved = resolve(&scene, &reg);
        assert!(resolved.problems.is_empty(), "{id}: {:?}", resolved.problems);
        let s = SceneSettings::from_params(&scene.things[&ThingId::ROOT].params);
        assert!((s.duration as f64 - e.info.seconds).abs() < 1e-3, "{id}: length");
        let mut names = BTreeSet::new();
        for t in scene.things.values() {
            assert!(names.insert(t.name.as_str()), "{id}: two things called {}", t.name);
        }
        for rule in &scene.rules.list {
            let default = rule.name.strip_prefix("Rule ").is_some_and(|n| n.chars().all(|c| c.is_ascii_digit()));
            assert!(!default && !rule.name.trim().is_empty(), "{id}: rule named {:?}", rule.name);
        }
        let back = load(&save(&scene), &reg).unwrap_or_else(|err| panic!("{id}: {err}"));
        assert_eq!(back, scene, "{id}: saves and opens again the same");
    }
}

/// Runs the scene its whole length.
fn play(e: &Entry, reg: &Registry) -> (Scene, World, Log) {
    let kit = kit();
    let scene = e.scene(reg);
    let resolved = resolve(&scene, reg);
    let mut world = World::build(&scene, &resolved, &kit.sims);
    let mut log = Log::default();
    world.advance_to(e.info.seconds, &kit.sims, &mut log);
    (scene, world, log)
}

/// Plays every scene once, its whole length: nothing goes wrong on the way, and its payoff lands on time.
#[test]
fn every_scene_plays_its_full_length_and_lands_its_payoff() {
    let reg = kit().registry;
    for e in library::all() {
        let id = e.info.id;
        let (scene, world, log) = play(e, &reg);
        if let Some(rules) = world.rules() {
            for rule in &scene.rules.list {
                assert_eq!(rules.problem(rule.id), None, "{id}: rule {}", rule.name);
                assert_eq!(rules.busy_at(rule.id), None, "{id}: rule {} too busy", rule.name);
            }
        }
        for (thing, bodies) in &world.bodies {
            for row in bodies.live_rows() {
                if bodies.alive[row] {
                    assert!(
                        bodies.position[row].is_finite() && bodies.velocity[row].is_finite(),
                        "{id}: thing {thing:?} ball {row} went to infinity"
                    );
                }
            }
        }
        // Every scene so far is about things hitting; a later one that isn't (words, liquid) can say so here.
        assert!(log.events().iter().any(|ev| ev.kind == EventKind::Hit), "{id}: nothing ever hits");
        if let Some(payoff) = e.info.payoff {
            payoff_lands(e, payoff, &scene, &world, &log);
        }
    }
}

fn payoff_lands(e: &Entry, payoff: library::Payoff, scene: &Scene, world: &World, log: &Log) {
    let id = e.info.id;
    let rate = world.steps_per_second as f64;
    let when = match payoff.what {
        PayoffEvent::RuleFires(name) => {
            let rule = scene.rules.list.iter().find(|r| r.name == name).unwrap_or_else(|| panic!("{id}: {name}"));
            log.events()
                .iter()
                .find(|ev| ev.kind == EventKind::Rule && ev.b.as_ref().is_some_and(|b| (b.item >> 16) == rule.id.0))
                .map(|ev| ev.step as f64 / rate)
        }
        PayoffEvent::Breaks(name) => {
            let thing = scene.things.values().find(|t| t.name == name).unwrap_or_else(|| panic!("{id}: {name}"));
            log.events()
                .iter()
                .find(|ev| ev.kind == EventKind::Break && ev.a.thing == thing.id)
                .map(|ev| ev.step as f64 / rate)
        }
        PayoffEvent::Counter(..) => panic!("{id}: counter payoffs are checked from part 3"),
    };
    let when = when.unwrap_or_else(|| panic!("{id}: the payoff never happens"));
    assert!(
        (when - payoff.at_seconds).abs() <= payoff.within_seconds,
        "{id}: the payoff is at {when:.2} s, not {:.2} ± {:.2}",
        payoff.at_seconds,
        payoff.within_seconds
    );
}

/// Whether more than 2% of the picture differs clearly from its first pixel.
fn has_a_picture(pixels: &[u8]) -> bool {
    let first = &pixels[..4];
    let differ =
        pixels.chunks_exact(4).filter(|p| p.iter().zip(first).take(3).any(|(a, b)| a.abs_diff(*b) > 8)).count();
    differ * 50 > pixels.len() / 4
}

fn save_png(path: &std::path::Path, w: u32, h: u32, pixels: &[u8]) {
    let file = std::fs::File::create(path).expect("a file");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.write_header().and_then(|mut w| w.write_image_data(pixels)).expect("written");
}

#[test]
fn every_scene_draws_a_picture() {
    let Ok((device, queue)) = test_device() else { return };
    let kit = kit();
    let mut renderer = Renderer::new(&device, 270, 480);
    for e in library::all() {
        let id = e.info.id;
        let scene = e.scene(&kit.registry);
        let resolved = resolve(&scene, &kit.registry);
        let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
        for t in [e.info.poster_seconds, e.info.seconds * 0.25, e.info.seconds * 0.75] {
            playback.seek(t);
            let mut frame = Frame::default();
            build_frame(&scene, &resolved, playback.world(), playback.log(), t, &kit.draws, &mut frame);
            renderer.render(&device, &queue, &frame);
            while renderer.programs_waiting() {
                renderer.render(&device, &queue, &frame);
            }
            let pixels = renderer.read_output(&device, &queue).expect("the picture");
            if !has_a_picture(&pixels) {
                let path = std::env::temp_dir().join(format!("library-{id}-{t:.1}.png"));
                save_png(&path, 270, 480, &pixels);
                panic!("{id}: the picture at {t:.1} s is one flat colour (saved to {})", path.display());
            }
        }
    }
}
