//! The buses end to end: which bus a shape plays into, sends, mute and solo, with the test tone
//! on the classic test scene's shape.

use bpe_core::{Scene, SlotId, ThingId, Value};
use bpe_kit::{Kit, ensure_mixer, kit, new_scene, resolve_scene};
use bpe_sound::{BLOCK, Engine};

/// The classic test scene, its shape blipping into bus `bus` (a decaying blip, so there is a
/// lot of sound to compare).
fn toned(kit: &Kit, bus: u8) -> Scene {
    let mut scene = new_scene(&kit.registry);
    ensure_mixer(&mut scene, &kit.registry);
    let shape = scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap();
    let tags = &mut scene.things.get_mut(&shape).unwrap().tags;
    tags.insert("test-tone-decay=0.5".into());
    tags.insert(format!("test-bus={bus}"));
    scene
}

fn set(scene: &mut Scene, key: &str, v: Value) {
    scene.card_mut(ThingId::ROOT, SlotId::Mixer).unwrap().params.set(key, v);
}

fn render(scene: &Scene, kit: &Kit, seconds: f64) -> Vec<f32> {
    let resolved = resolve_scene(scene, &kit.registry);
    let mut e = Engine::new(scene, &resolved, kit);
    let blocks = (seconds * 48_000.0 / BLOCK as f64).ceil() as usize;
    let mut out = Vec::with_capacity(blocks * BLOCK * 2);
    let mut block = [0.0f32; BLOCK * 2];
    for _ in 0..blocks {
        e.render(&mut block);
        out.extend_from_slice(&block);
    }
    out
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
}

/// The largest difference between `a` and `b` scaled by `gain`.
fn off_by(a: &[f32], b: &[f32], gain: f32) -> f32 {
    a.iter().zip(b).fold(0.0f32, |m, (x, y)| m.max((x - y * gain).abs()))
}

const SECONDS: f64 = 4.0;

#[test]
fn a_shape_on_bus2_is_silent_while_bus2_is_off() {
    let kit = kit();
    let mut scene = toned(&kit, 2);
    assert_eq!(peak(&render(&scene, &kit, SECONDS)), 0.0, "Bus 2 starts off");
    set(&mut scene, "bus2_on", Value::Bool(true));
    let on = render(&scene, &kit, SECONDS);
    assert!(peak(&on) > 0.1, "on, it plays");
    // The same sound as on Bus 1.
    let one = render(&toned(&kit, 1), &kit, SECONDS);
    assert!(off_by(&on, &one, 1.0) < 1e-6);
}

#[test]
fn a_send_to_an_off_bus_is_silent() {
    let kit = kit();
    let mut scene = toned(&kit, 1);
    let one = render(&scene, &kit, SECONDS);
    set(&mut scene, "bus1_send2", Value::Number(0.0));
    let sent = render(&scene, &kit, SECONDS);
    assert!(off_by(&sent, &one, 1.0) < 1e-6, "Bus 2 is off: the send adds nothing");
    set(&mut scene, "bus2_on", Value::Bool(true));
    let sent = render(&scene, &kit, SECONDS);
    assert!(off_by(&sent, &one, 2.0) < 1e-5, "on, the send at 0 dB doubles the sound");
}

#[test]
fn post_fader_send_follows_the_fader() {
    let kit = kit();
    let base = toned(&kit, 1);
    let one = render(&base, &kit, SECONDS);
    let mut scene = base.clone();
    set(&mut scene, "bus1_fader", Value::Number(-6.0));
    set(&mut scene, "bus1_send2", Value::Number(-6.0));
    set(&mut scene, "bus2_on", Value::Bool(true));
    let out = render(&scene, &kit, SECONDS);
    let g = bpe_dsp::db_to_gain(-6.0);
    // Bus 1 at -6 dB, plus Bus 2 hearing Bus 1 after its fader, sent at -6 dB.
    assert!(off_by(&out, &one, g + g * g) < 1e-5);
}

#[test]
fn mute_silences() {
    let kit = kit();
    let mut scene = toned(&kit, 1);
    set(&mut scene, "bus1_mute", Value::Bool(true));
    assert_eq!(peak(&render(&scene, &kit, SECONDS)), 0.0);
}

#[test]
fn solo_plays_only_soloed_buses() {
    let kit = kit();
    let mut scene = toned(&kit, 1);
    let one = render(&scene, &kit, SECONDS);
    set(&mut scene, "bus2_on", Value::Bool(true));
    set(&mut scene, "bus2_solo", Value::Bool(true));
    assert_eq!(peak(&render(&scene, &kit, SECONDS)), 0.0, "Bus 2 soloed: Bus 1 is left out");
    set(&mut scene, "bus1_solo", Value::Bool(true));
    assert!(off_by(&render(&scene, &kit, SECONDS), &one, 1.0) < 1e-6, "both soloed: Bus 1 plays");
    // A solo on a bus that is off counts for nothing.
    set(&mut scene, "bus1_solo", Value::Bool(false));
    set(&mut scene, "bus2_on", Value::Bool(false));
    assert!(off_by(&render(&scene, &kit, SECONDS), &one, 1.0) < 1e-6);
}

#[test]
fn a_bus_limiter_keeps_the_sound_on_time() {
    let kit = kit();
    let base = toned(&kit, 1);
    let one = render(&base, &kit, SECONDS);
    let mut scene = base.clone();
    let id = scene.add_layer_in(&kit.registry, ThingId::ROOT, SlotId::Mixer, "fx.limiter", Some(0)).unwrap();
    let key = bpe_core::layered_key(id, "ceiling");
    set(&mut scene, &key, Value::Number(0.0));
    let limited = render(&scene, &kit, SECONDS);
    // The limiter's lookahead is taken back: the blips land on the same samples.
    let first = |x: &[f32]| x.iter().position(|v| *v != 0.0);
    assert_eq!(first(&limited), first(&one));
    assert!(off_by(&limited, &one, 1.0) < 1e-4, "a -12 dB blip is under a 0 dB ceiling");
}

/// The mix and each bus's stem, rendered with stems recorded.
fn render_stems(scene: &Scene, kit: &Kit, seconds: f64) -> (Vec<f32>, [Vec<f32>; 3]) {
    let resolved = resolve_scene(scene, &kit.registry);
    let mut e = Engine::new(scene, &resolved, kit);
    e.record_stems(true);
    let blocks = (seconds * 48_000.0 / BLOCK as f64).ceil() as usize;
    let (mut mix, mut stems) = (Vec::new(), [Vec::new(), Vec::new(), Vec::new()]);
    let mut block = [0.0f32; BLOCK * 2];
    for _ in 0..blocks {
        e.render(&mut block);
        mix.extend_from_slice(&block);
        for (s, b) in stems.iter_mut().zip(e.stems()) {
            s.extend_from_slice(b);
        }
    }
    (mix, stems)
}

/// Bus 1 with the blip, sending to Bus 2 (on, with a limiter so the paths differ in time).
fn two_buses(kit: &Kit) -> Scene {
    let mut scene = toned(kit, 1);
    set(&mut scene, "bus1_send2", Value::Number(-3.0));
    set(&mut scene, "bus2_on", Value::Bool(true));
    set(&mut scene, "bus2_pan", Value::Number(40.0));
    let id = scene.add_layer_in(&kit.registry, ThingId::ROOT, SlotId::Mixer, "fx.limiter", Some(1)).unwrap();
    set(&mut scene, &bpe_core::layered_key(id, "ceiling"), Value::Number(0.0));
    scene
}

#[test]
fn stems_sum_to_the_mix_before_the_limiter() {
    let kit = kit();
    let scene = two_buses(&kit);
    let (mix, stems) = render_stems(&scene, &kit, SECONDS);
    assert!(peak(&stems[0]) > 0.1 && peak(&stems[1]) > 0.05, "both buses play");
    assert_eq!(peak(&stems[2]), 0.0, "Bus 3 is off");
    // The blip is far under the ceiling, so the master's limiter leaves it as it is.
    let off =
        mix.iter().enumerate().fold(0.0f32, |m, (i, v)| m.max((v - stems[0][i] - stems[1][i] - stems[2][i]).abs()));
    assert!(off < 1e-6, "{off}");
    // Written to float files, they still add up.
    let dir = std::env::temp_dir().join(format!("bpe-stems-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let video = dir.join("clip.mp4");
    let (buses, master) = bpe_sound::stem_paths(&scene, &video);
    assert!(buses[1].ends_with("clip - Bus 2.wav") && master.ends_with("clip - Master.wav"));
    let extras = bpe_sound::Extras { stems: Some((buses.clone(), master.clone())), midi: None };
    bpe_sound::render_sound(&scene, &kit, &dir.join("clip.wav"), SECONDS, &extras, |_| true).unwrap();
    let read = |p: &std::path::Path| -> Vec<f32> {
        let mut r = hound::WavReader::open(p).unwrap();
        let spec = r.spec();
        assert_eq!(
            (spec.bits_per_sample, spec.sample_format, spec.sample_rate, spec.channels),
            (32, hound::SampleFormat::Float, 48_000, 2)
        );
        r.samples::<f32>().map(|s| s.unwrap()).collect()
    };
    let files: Vec<Vec<f32>> = buses.iter().map(|p| read(p)).collect();
    let m = read(&master);
    let step = 1e-6;
    let off =
        m.iter().enumerate().fold(0.0f32, |acc, (i, v)| acc.max((v - files.iter().map(|f| f[i]).sum::<f32>()).abs()));
    assert!(off < step, "{off}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn muted_bus_stem_is_silent() {
    let kit = kit();
    let mut scene = two_buses(&kit);
    set(&mut scene, "bus2_mute", Value::Bool(true));
    let (mix, stems) = render_stems(&scene, &kit, SECONDS);
    assert_eq!(peak(&stems[1]), 0.0);
    assert!(mix.iter().zip(&stems[0]).all(|(a, b)| (a - b).abs() < 1e-6), "the mix is Bus 1 alone");
    // Soloing Bus 2 silences Bus 1's stem, and Bus 2 then hears nothing from Bus 1.
    let mut scene = two_buses(&kit);
    set(&mut scene, "bus2_solo", Value::Bool(true));
    let (_, stems) = render_stems(&scene, &kit, SECONDS);
    assert_eq!(peak(&stems[0]), 0.0);
    assert_eq!(peak(&stems[1]), 0.0);
}

#[test]
fn stems_of_buses_with_one_name_get_their_numbers() {
    let kit = kit();
    let mut scene = toned(&kit, 1);
    set(&mut scene, "bus2_name", Value::Text("Drums".into()));
    set(&mut scene, "bus3_name", Value::Text("drums".into()));
    set(&mut scene, "bus1_name", Value::Text("Master".into()));
    let (buses, master) = bpe_sound::stem_paths(&scene, std::path::Path::new("clip.mp4"));
    let names: Vec<String> = buses.iter().chain([&master]).map(|p| p.to_string_lossy().into_owned()).collect();
    assert_eq!(
        names,
        ["clip - Master (Bus 1).wav", "clip - Drums (Bus 2).wav", "clip - drums (Bus 3).wav", "clip - Master.wav"]
    );
}
