//! Picture splats: which picture files a scene uses, and the slot each gets.
//!
//! The simulation never reads an image. It only gives each picture file a slot in
//! [`World::pictures`], so a Picture mark can name its picture by that number. The renderer
//! decodes the files. The file half of this module is for the editor: it copies the pictures a
//! scene uses into a folder beside the scene file, so they travel with it, and tells what is
//! missing.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use bpe_core::{Params, Scene, SlotId, SlotLink, Value};
use bpe_runtime::World;

use crate::modules::marks::{MarksBrushes, SplatShape};
use crate::things::canvas::{CanvasKind, CanvasState, Underneath};

/// Most pictures one scene can show.
pub const MAX_PICTURES: usize = 16;
/// A Picture mark's slot for a picture not in the table: drawn as the checker.
pub const NO_SLOT: u8 = 31;
/// A Picture mark's variant bit for Tinted.
pub const TINTED: u8 = 32;

/// Every distinct non-empty `splat_picture` of the world's brushes whose Splat is on with
/// Shape Picture, sorted (byte order of the strings as stored), the first MAX_PICTURES kept.
pub fn collect(world: &World) -> Vec<String> {
    let mut found = BTreeSet::new();
    for &thing in world.order() {
        let Some((_, state)) = world.module_state(thing, SlotId::Marks) else { continue };
        let Some(b) = state.as_any().downcast_ref::<MarksBrushes>() else { continue };
        if b.splat_on && b.splat_shape == SplatShape::Picture && !b.splat_picture.trim().is_empty() {
            found.insert(b.splat_picture.clone());
        }
    }
    // A cover's picture underneath.
    for &thing in world.order() {
        let Some(st) = world.states.get(&thing).and_then(|s| s.as_any().downcast_ref::<CanvasState>()) else {
            continue;
        };
        let s = &st.settings;
        if s.kind == CanvasKind::Cover && s.under == Underneath::Picture && !s.under_picture.trim().is_empty() {
            found.insert(s.under_picture.clone());
        }
    }
    found.into_iter().take(MAX_PICTURES).collect()
}

/// The slot of `path` in `world.pictures`, or NO_SLOT.
pub fn slot_of(world: &World, path: &str) -> u8 {
    world.pictures.iter().position(|p| p == path).map_or(NO_SLOT, |i| i as u8)
}

/// Fills `world.pictures` (idempotent: the same world always gives the same table).
pub fn fill(world: &mut World) {
    world.pictures = collect(world);
}

/// The settings picture paths are stored under.
const KEYS: [&str; 2] = ["splat_picture", "under_picture"];

/// Calls `f` for every non-empty picture setting among `params`.
fn visit_params(params: &mut Params, f: &mut dyn FnMut(&mut String)) {
    for key in KEYS {
        if let Some(Value::Text(text)) = params.0.get_mut(key)
            && !text.is_empty()
        {
            f(text);
        }
    }
}

/// Calls `f` for every picture setting in the scene: module settings, what modules kept when
/// their kind changed, and the changes a thing makes to a module it links.
fn visit(scene: &mut Scene, f: &mut dyn FnMut(&mut String)) {
    for module in scene.library.modules.values_mut() {
        visit_params(&mut module.params, f);
        for stash in module.stash.values_mut() {
            visit_params(&mut stash.params, f);
            for overrides in stash.overrides.values_mut() {
                visit_params(overrides, f);
            }
        }
    }
    for thing in scene.things.values_mut() {
        // A canvas's own settings live on the thing.
        visit_params(&mut thing.params, f);
        for link in thing.slots.values_mut() {
            if let SlotLink::Linked { overrides, .. } = link {
                visit_params(overrides, f);
            }
        }
    }
}

/// The folder a scene file's pictures are copied into: `<file stem> pictures`, beside the file.
pub fn pictures_folder(scene_file: &Path) -> PathBuf {
    let stem = scene_file.file_stem().map_or_else(|| "scene".to_owned(), |s| s.to_string_lossy().into_owned());
    scene_file.parent().unwrap_or(Path::new("")).join(format!("{stem} pictures"))
}

/// Every picture setting in the scene as stored, without repeats, sorted.
pub fn picture_paths(scene: &Scene) -> Vec<String> {
    let mut found = BTreeSet::new();
    // `visit` needs a scene it may change; this copy is dropped unchanged.
    visit(&mut scene.clone(), &mut |text| {
        found.insert(text.clone());
    });
    found.into_iter().collect()
}

/// Copies every picture the scene names that is not already inside `pictures_folder(scene_file)`
/// into it, and rewrites each such setting to the relative path `<stem> pictures/<name>`, with
/// `/` separators. Relative paths are first read against `from` (the folder they were relative
/// to; `None` reads them against the scene file's folder). A different file with the same name
/// gets " 2", " 3" and so on before its extension; a byte-identical file is reused. Gives the
/// number of settings rewritten, or a plain message naming the first file that could not be
/// copied (its setting is left as it was). A picture that does not exist is left alone: the
/// status line names it.
pub fn adopt(scene: &mut Scene, from: Option<&Path>, scene_file: &Path) -> Result<usize, String> {
    let folder = pictures_folder(scene_file);
    let stem = scene_file.file_stem().map_or_else(|| "scene".to_owned(), |s| s.to_string_lossy().into_owned());
    let base = from.map(Path::to_path_buf).or_else(|| scene_file.parent().map(Path::to_path_buf));
    let mut done: BTreeMap<String, String> = BTreeMap::new();
    let mut error = None;
    let mut rewritten = 0;
    visit(scene, &mut |text| {
        let source = bpe_gfx::picture_path(text, base.as_deref());
        if source.parent().is_some_and(|p| p.components().eq(folder.components())) || !source.is_file() {
            return;
        }
        if let Some(new) = done.get(text.as_str()) {
            *text = new.clone();
            rewritten += 1;
            return;
        }
        match copy_into(&source, &folder) {
            Ok(name) => {
                let new = format!("{stem} pictures/{name}");
                done.insert(text.clone(), new.clone());
                *text = new;
                rewritten += 1;
            }
            Err(message) => {
                error.get_or_insert(message);
            }
        }
    });
    match error {
        Some(message) => Err(message),
        None => Ok(rewritten),
    }
}

/// Copies `source` into `folder` and gives the name it has there.
fn copy_into(source: &Path, folder: &Path) -> Result<String, String> {
    let name = source.file_name().map_or_else(|| "picture".to_owned(), |n| n.to_string_lossy().into_owned());
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) => (stem.to_owned(), format!(".{extension}")),
        None => (name.clone(), String::new()),
    };
    let cannot = |why: &dyn std::fmt::Display| format!("Could not copy the picture {name} next to the scene: {why}");
    let bytes = std::fs::read(source).map_err(|e| cannot(&e))?;
    let mut candidate = name.clone();
    let mut n = 1;
    loop {
        let target = folder.join(&candidate);
        match std::fs::read(&target) {
            Ok(existing) if existing == bytes => return Ok(candidate),
            Ok(_) => {
                n += 1;
                candidate = format!("{stem} {n}{extension}");
            }
            Err(_) => break,
        }
    }
    std::fs::create_dir_all(folder).map_err(|e| cannot(&e))?;
    std::fs::write(folder.join(&candidate), &bytes).map_err(|e| cannot(&e))?;
    Ok(candidate)
}

/// Rewrites every relative picture path to a full one read against `folder`.
pub fn absolutize(scene: &mut Scene, folder: &Path) {
    visit(scene, &mut |text| {
        *text = bpe_gfx::picture_path(text, Some(folder)).to_string_lossy().into_owned();
    });
}

/// What the status line says about the scene's pictures, or `None` when all is well: the
/// missing or unreadable ones by file name, and a note past [`MAX_PICTURES`].
pub fn picture_note(scene: &Scene, folder: Option<&Path>) -> Option<String> {
    let paths = picture_paths(scene);
    if paths.is_empty() {
        return None;
    }
    let missing: Vec<String> = paths
        .iter()
        .filter(|p| !bpe_gfx::picture_readable(&bpe_gfx::picture_path(p, folder)))
        .map(|p| p.rsplit(['/', '\\']).next().unwrap_or(p).to_owned())
        .collect();
    let mut notes = Vec::new();
    match missing.len() {
        0 => {}
        1 => notes.push(format!("Picture not found: {} (a checker shows instead)", missing[0])),
        _ => notes.push(format!("Pictures not found: {} (a checker shows instead)", missing.join(", "))),
    }
    if paths.len() > MAX_PICTURES {
        notes.push(format!(
            "This scene uses {} pictures; only {MAX_PICTURES} can show, the others show a checker.",
            paths.len()
        ));
    }
    (!notes.is_empty()).then(|| notes.join(" \u{b7} "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{ARENA, BALLS, module_of};
    use crate::{add_thing, fill_slot, kit, new_scene, scenes};
    use bpe_core::{Params, Scene, ThingId, Value, resolve};
    use bpe_runtime::{Log, MarkShape, MarkTarget};
    use glam::Vec2;

    const CANVAS: ThingId = ThingId(3);

    fn brush(scene: &mut Scene, thing: ThingId, f: impl FnOnce(&mut Params)) {
        let kit = kit();
        fill_slot(scene, &kit.registry, thing, SlotId::Marks).unwrap();
        let m = module_of(scene, &kit.registry, thing, SlotId::Marks);
        let p = &mut scene.library.modules.get_mut(&m).unwrap().params;
        p.set("paint_on", Value::Bool(false));
        p.set("splat_on", Value::Bool(true));
        p.set("splat_shape", Value::Choice("picture".into()));
        f(p);
    }

    fn world_of(scene: &Scene, seconds: f64) -> (World, Log) {
        let kit = kit();
        let resolved = resolve(scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        crate::testing::run(scene, &resolved, &kit, seconds)
    }

    /// The new scene with a canvas (thing 3) and `f` to set things up.
    fn with_canvas(f: impl FnOnce(&mut Scene)) -> Scene {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        assert_eq!(add_thing(&mut scene, &kit.registry, "canvas", ThingId::ROOT).unwrap(), CANVAS);
        f(&mut scene);
        scene
    }

    #[test]
    fn pictures_are_collected_sorted_and_capped() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let mut ids = Vec::new();
        for _ in 0..17 {
            ids.push(add_thing(&mut scene, &kit.registry, "obstacle", ThingId::ROOT).unwrap());
        }
        // Out of order on purpose.
        for (k, &id) in ids.iter().enumerate().rev() {
            brush(&mut scene, id, |p| p.set("splat_picture", Value::Text(format!("p{k:02}.png"))));
        }
        brush(&mut scene, BALLS, |p| p.set("splat_picture", Value::Text("p17.png".into())));
        let (world, _) = world_of(&scene, 0.0);
        let want: Vec<String> = (0..16).map(|k| format!("p{k:02}.png")).collect();
        assert_eq!(collect(&world), want);
        assert_eq!(world.pictures, want);
    }

    #[test]
    fn a_scene_without_pictures_keeps_an_empty_table() {
        let kit = kit();
        let (_, scene) = scenes::marks(&kit.registry).into_iter().next().unwrap();
        let (world, _) = world_of(&scene, 0.0);
        assert!(world.pictures.is_empty());
    }

    #[test]
    fn picture_marks_carry_their_slot_and_tint() {
        let scene = with_canvas(|scene| {
            brush(scene, BALLS, |p| {
                p.set("splat_picture", Value::Text("b.png".into()));
                p.set("splat_picture_colours", Value::Choice("tinted".into()));
                p.set("splat_onto", Value::Choice("canvases".into()));
            });
            // Its own picture sorts first, so the ball's is slot 1. It lands on its own skin.
            brush(scene, ARENA, |p| p.set("splat_picture", Value::Text("a.png".into())));
        });
        let (world, log) = world_of(&scene, 3.0);
        assert_eq!(world.pictures, ["a.png", "b.png"]);
        let pictures: Vec<_> = log
            .marks()
            .iter()
            .filter(|m| m.shape == MarkShape::Picture && m.target == MarkTarget::Canvas(CANVAS))
            .collect();
        assert!(!pictures.is_empty());
        for m in pictures {
            assert_eq!(m.variant, 1 | TINTED);
        }
    }

    #[test]
    fn an_empty_picture_splats_as_a_splat() {
        let scene = with_canvas(|scene| {
            brush(scene, BALLS, |p| p.set("splat_onto", Value::Choice("canvases".into())));
        });
        let (world, log) = world_of(&scene, 3.0);
        assert!(world.pictures.is_empty());
        let marks: Vec<_> = log.marks().iter().filter(|m| m.target == MarkTarget::Canvas(CANVAS)).collect();
        assert!(!marks.is_empty());
        assert!(marks.iter().all(|m| m.shape == MarkShape::Splat));
    }

    #[test]
    fn a_picture_facing_a_floor_hit_stands_upright() {
        let scene = with_canvas(|scene| {
            // A box arena and a ball dropped straight down onto its flat floor.
            scene.things.get_mut(&ARENA).unwrap().params.set("shape", Value::Choice("box".into()));
            let ball = scene.things.get_mut(&BALLS).unwrap();
            ball.transform.position = Vec2::new(0.0, 0.0);
            ball.params.set("velocity", Value::Vec2([0.0, 0.0]));
            brush(scene, BALLS, |p| {
                p.set("splat_picture", Value::Text("x.png".into()));
                p.set("splat_turn", Value::Choice("face_hit".into()));
                p.set("splat_onto", Value::Choice("canvases".into()));
            });
        });
        let (_, log) = world_of(&scene, 4.0);
        let first = log.marks().iter().find(|m| m.shape == MarkShape::Picture).expect("a picture mark");
        let wrapped = (first.angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        assert!(wrapped.abs() < 0.05, "angle {wrapped}");
    }

    #[test]
    fn a_picture_past_the_table_gets_no_slot() {
        let scene = with_canvas(|scene| {
            brush(scene, BALLS, |p| p.set("splat_picture", Value::Text("x.png".into())));
        });
        let (world, _) = world_of(&scene, 0.0);
        assert_eq!(slot_of(&world, "x.png"), 0);
        assert_eq!(slot_of(&world, "zzz.png"), NO_SLOT);
    }

    #[test]
    fn picture_splats_paint_a_smaller_disc_on_the_grid() {
        use crate::things::canvas::sim::CanvasState;
        let scene = with_canvas(|scene| {
            brush(scene, BALLS, |p| {
                p.set("splat_picture", Value::Text("x.png".into()));
                p.set("splat_onto", Value::Choice("canvases".into()));
            });
        });
        let (world, _) = world_of(&scene, 3.0);
        let st = bpe_runtime::state::<CanvasState>(world.states[&CANVAS].as_ref());
        assert!(st.grid.covered() > 0.0);
    }

    // ---- the file half ----

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bpe-kit-adopt-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A tiny valid PNG of one `shade`, written to `path` (folders made).
    fn write_png(path: &Path, shade: u8) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = std::fs::File::create(path).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), 4, 4);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&[shade, 0, 0, 255].repeat(16)).unwrap();
    }

    /// A scene whose ball's brush names `picture`.
    fn named(picture: &str) -> Scene {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        brush(&mut scene, BALLS, |p| p.set("splat_picture", Value::Text(picture.into())));
        scene
    }

    fn the_picture(scene: &Scene) -> String {
        picture_paths(scene).into_iter().next().expect("a picture")
    }

    #[test]
    fn adopt_copies_into_the_scene_pictures_folder() {
        let tmp = temp("copies");
        let source = tmp.join("elsewhere/star.png");
        write_png(&source, 10);
        let mut scene = named(source.to_str().unwrap());
        let file = tmp.join("work/My scene.ron");
        assert_eq!(adopt(&mut scene, None, &file), Ok(1));
        let copy = tmp.join("work/My scene pictures/star.png");
        assert_eq!(std::fs::read(&copy).unwrap(), std::fs::read(&source).unwrap());
        assert_eq!(the_picture(&scene), "My scene pictures/star.png");
    }

    #[test]
    fn adopt_leaves_adopted_pictures_alone() {
        let tmp = temp("alone");
        let source = tmp.join("elsewhere/star.png");
        write_png(&source, 10);
        let mut scene = named(source.to_str().unwrap());
        let file = tmp.join("work/My scene.ron");
        adopt(&mut scene, None, &file).unwrap();
        let before = scene.clone();
        assert_eq!(adopt(&mut scene, None, &file), Ok(0));
        assert_eq!(scene, before);
    }

    #[test]
    fn adopt_reuses_an_identical_file() {
        let tmp = temp("same");
        let source = tmp.join("elsewhere/star.png");
        write_png(&source, 10);
        write_png(&tmp.join("work/My scene pictures/star.png"), 10);
        let mut scene = named(source.to_str().unwrap());
        assert_eq!(adopt(&mut scene, None, &tmp.join("work/My scene.ron")), Ok(1));
        assert!(!tmp.join("work/My scene pictures/star 2.png").exists());
        assert_eq!(the_picture(&scene), "My scene pictures/star.png");
    }

    #[test]
    fn adopt_renames_a_clash() {
        let tmp = temp("clash");
        let source = tmp.join("elsewhere/star.png");
        write_png(&source, 10);
        write_png(&tmp.join("work/My scene pictures/star.png"), 200);
        let mut scene = named(source.to_str().unwrap());
        assert_eq!(adopt(&mut scene, None, &tmp.join("work/My scene.ron")), Ok(1));
        assert!(tmp.join("work/My scene pictures/star 2.png").exists());
        assert_eq!(the_picture(&scene), "My scene pictures/star 2.png");
    }

    #[test]
    fn save_as_elsewhere_copies_again() {
        let tmp = temp("saveas");
        let source = tmp.join("elsewhere/star.png");
        write_png(&source, 10);
        let mut scene = named(source.to_str().unwrap());
        adopt(&mut scene, None, &tmp.join("work/My scene.ron")).unwrap();
        assert_eq!(adopt(&mut scene, Some(&tmp.join("work")), &tmp.join("other/Copy.ron")), Ok(1));
        assert!(tmp.join("other/Copy pictures/star.png").exists());
        assert_eq!(the_picture(&scene), "Copy pictures/star.png");
    }

    #[test]
    fn adopt_handles_spaces_and_non_ascii_names() {
        let tmp = temp("unicode");
        let source = tmp.join("ünï/my picture é.png");
        write_png(&source, 10);
        let mut scene = named(source.to_str().unwrap());
        assert_eq!(adopt(&mut scene, None, &tmp.join("work/My scene.ron")), Ok(1));
        assert!(tmp.join("work/My scene pictures/my picture é.png").exists());
        assert_eq!(the_picture(&scene), "My scene pictures/my picture é.png");
    }

    #[test]
    fn adopt_finds_pictures_in_overrides_and_stashes() {
        let tmp = temp("overrides");
        let source = tmp.join("elsewhere/star.png");
        write_png(&source, 10);
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        fill_slot(&mut scene, &kit.registry, BALLS, SlotId::Marks).unwrap();
        let link = scene.things.get_mut(&BALLS).unwrap().slots.get_mut(&SlotId::Marks).unwrap();
        let SlotLink::Linked { overrides, .. } = link else { panic!("a linked slot") };
        overrides.set(KEYS[0], Value::Text(source.to_str().unwrap().into()));
        assert_eq!(adopt(&mut scene, None, &tmp.join("work/My scene.ron")), Ok(1));
        assert_eq!(the_picture(&scene), "My scene pictures/star.png");
    }

    #[test]
    fn absolutize_makes_paths_full() {
        let tmp = temp("absolute");
        let mut scene = named("My scene pictures/star.png");
        absolutize(&mut scene, &tmp.join("work"));
        assert_eq!(the_picture(&scene), tmp.join("work").join("My scene pictures/star.png").to_string_lossy());
    }

    #[test]
    fn picture_note_names_missing_pictures() {
        let tmp = temp("note");
        write_png(&tmp.join("here.png"), 1);
        assert_eq!(picture_note(&named("here.png"), Some(&tmp)), None);
        assert_eq!(
            picture_note(&named("gone.png"), Some(&tmp)).as_deref(),
            Some("Picture not found: gone.png (a checker shows instead)")
        );
        assert_eq!(picture_note(&new_scene(&kit().registry), Some(&tmp)), None);
    }

    #[test]
    fn picture_note_counts_past_sixteen() {
        let tmp = temp("many");
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        for k in 0..18 {
            write_png(&tmp.join(format!("p{k:02}.png")), k as u8);
            let id = add_thing(&mut scene, &kit.registry, "obstacle", ThingId::ROOT).unwrap();
            brush(&mut scene, id, |p| p.set("splat_picture", Value::Text(format!("p{k:02}.png"))));
        }
        let note = picture_note(&scene, Some(&tmp)).expect("a note");
        assert!(note.contains("only 16 can show"), "{note}");
    }

    fn cover_with_picture(picture: &str) -> Scene {
        with_canvas(|scene| {
            let p = &mut scene.things.get_mut(&CANVAS).unwrap().params;
            p.set("kind", Value::Choice("cover".into()));
            p.set("under", Value::Choice("picture".into()));
            p.set("under_picture", Value::Text(picture.into()));
        })
    }

    #[test]
    fn under_pictures_join_the_table() {
        let scene = cover_with_picture("under.png");
        let (world, _) = world_of(&scene, 0.0);
        assert_eq!(world.pictures, vec!["under.png".to_string()]);
        assert_eq!(slot_of(&world, "under.png"), 0);
        // Only a cover whose underneath is a picture counts.
        let scene = with_canvas(|scene| {
            let p = &mut scene.things.get_mut(&CANVAS).unwrap().params;
            p.set("under_picture", Value::Text("under.png".into()));
        });
        assert!(world_of(&scene, 0.0).0.pictures.is_empty());
    }

    #[test]
    fn adopting_copies_under_pictures() {
        let tmp = temp("under");
        let source = tmp.join("elsewhere/star.png");
        write_png(&source, 10);
        let mut scene = cover_with_picture(source.to_str().unwrap());
        assert_eq!(adopt(&mut scene, None, &tmp.join("work/My scene.ron")), Ok(1));
        assert!(tmp.join("work/My scene pictures/star.png").exists());
        assert_eq!(the_picture(&scene), "My scene pictures/star.png");
    }
}
