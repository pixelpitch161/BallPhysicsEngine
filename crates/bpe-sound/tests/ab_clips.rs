//! The FM / Phase distortion A/B clips promised to the owner before FM and PD were locked: each
//! mode on the same patch, once with the Modulation layer first in Osc A's chain (true FM or PD
//! of the oscillator) and once after a low-pass filter (phase modulation of the filtered
//! sound). Run by hand:
//!
//! `AB_OUT=<folder> cargo test --release -p bpe-sound --test ab_clips -- --ignored`

use bpe_core::{Address, LayerId, Scene, Settings, SlotId, ThingId, Value, layered_key};
use bpe_kit::sound::{FILTER, MODULATION, VoiceSettings, Wave, starter_voice};
use bpe_kit::{Kit, kit, new_scene};
use bpe_sound::offline::render_to_wav;

fn shape_of(scene: &Scene) -> ThingId {
    scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap()
}

/// The test scene (a ball bouncing in a circle) whose hits play a saw on Osc A, moved by a sine
/// on Osc B an octave up (B itself not heard), each note held 450 ms.
fn patch(kit: &Kit, mode: &str, filter_first: bool) -> Scene {
    let mut scene = new_scene(&kit.registry);
    let shape = shape_of(&scene);
    let mut card = starter_voice(&kit.registry);
    let mut v = VoiceSettings::from_params(&card.params);
    v.a.wave = Wave::Saw;
    v.b.on = false;
    v.b.wave = Wave::Sine;
    v.b.octave = 1;
    let mut params = v.to_params();
    params.set(&layered_key(LayerId(0), "sustain"), Value::Number(70.0));
    params.set(&layered_key(LayerId(0), "gate_ms"), Value::Number(450.0));
    card.params = params;
    scene.things.get_mut(&shape).unwrap().slots.insert(SlotId::Sound, card);
    let add = |scene: &mut Scene, kind: &str, set: &[(&str, Value)]| {
        let id = scene.add_layer_in(&kit.registry, shape, SlotId::Sound, kind, Some(0)).unwrap();
        for (k, v) in set {
            let a = Address::on(shape, SlotId::Sound, &layered_key(id, k));
            scene.set_at(&kit.registry, &a, v.clone()).unwrap();
        }
    };
    let filter =
        [("slope", Value::Choice("db24".into())), ("cutoff", Value::Number(900.0)), ("reso", Value::Number(35.0))];
    let modulation =
        [("kind", Value::Choice(mode.into())), ("source", Value::Text("b".into())), ("amount", Value::Number(45.0))];
    if filter_first {
        add(&mut scene, FILTER, &filter);
        add(&mut scene, MODULATION, &modulation);
    } else {
        add(&mut scene, MODULATION, &modulation);
        add(&mut scene, FILTER, &filter);
    }
    scene
}

#[test]
#[ignore = "writes listening clips; run by hand"]
fn render_fm_pd_ab_clips() {
    let kit = kit();
    let out = std::env::var_os("AB_OUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fm-pd-ab"));
    std::fs::create_dir_all(&out).unwrap();
    for (mode, word) in [("fm", "fm"), ("phase_distortion", "pd")] {
        for (filter_first, place) in [(false, "first"), (true, "after-filter")] {
            let scene = patch(&kit, mode, filter_first);
            let path = out.join(format!("{word}-{place}.wav"));
            render_to_wav(&scene, &kit, &path, 4.0, |_| true).unwrap();
            let bytes = std::fs::metadata(&path).unwrap().len();
            assert!(bytes > 4 * 48_000 * 2 * 4, "{}: {bytes} bytes", path.display());
            println!("{}", path.display());
        }
    }
}
