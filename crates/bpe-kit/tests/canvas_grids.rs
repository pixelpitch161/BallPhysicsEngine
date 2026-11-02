//! The canvas grids of the shipped Marks scenes, pinned so new kinds never change old paint.

use bpe_kit::{CanvasState, kit, resolve_scene, scenes};
use bpe_runtime::{Playback, state};

/// "scene name / canvas name = grid hash" at 5 s, for every canvas of the first seven Marks scenes.
fn grid_hashes(count: usize) -> Vec<String> {
    let kit = kit();
    let mut out = Vec::new();
    for (name, scene) in scenes::marks(&kit.registry).into_iter().take(count) {
        let resolved = resolve_scene(&scene, &kit.registry);
        let mut pb = Playback::new(&scene, &resolved, kit.sims.clone());
        pb.seek(5.0);
        for (id, thing) in &scene.things {
            if thing.kind != "canvas" {
                continue;
            }
            let st = state::<CanvasState>(pb.world().states[id].as_ref());
            out.push(format!("{name} / {} = {:016x}", thing.name, st.grid.hash()));
        }
    }
    out
}

#[test]
fn golden_scenes_keep_their_grids() {
    let got = grid_hashes(7);
    // Recorded on the commit before covers, scrapes and claims existed.
    let want = [
        "Paint pour / Canvas = 3a2d373a229b9019",
        "Peg paint / Canvas = d72be2cf903abe73",
        "Wet pour / Canvas = 511cfe0d2b054298",
        "Watercolour garden / Canvas = cdde65b264246cb7",
        "Colour mixing / Canvas = a8cb7d0e454433ae",
    ];
    assert_eq!(got, want);
}
