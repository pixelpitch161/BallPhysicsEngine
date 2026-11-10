//! A whole grade kept to come back to (colour grading part 6): **snapshots**, named versions of
//! the grade kept on the Grade card (`snapshots`), and **grade files** (`.grade`), one grade to use
//! in another project. Both hold a [`SavedGrade`]: the card's settings but its snapshots, its
//! masks, parts and panels, and the keyframes and pulses of its settings. Putting one on (a
//! restore or a load) replaces all of those and keeps the scene's own snapshots.

use super::{GRADE, GradeCard, WHOLE, ensure_grade, starting_card};
use bpe_core::{
    Address, EditError, GRADE_SNAPSHOTS_KEY, GradeSnapshot, Registry, SavedGrade, Scene, Settings, Slot, SlotId,
    ThingId, Value, clean_card, grade_file_text, layer_def, read_grade_file, split_layered,
};

/// Whether `a` is one of the Grade card's settings.
fn on_grade(a: &Address) -> bool {
    a.thing == ThingId::ROOT && a.slot == Some(SlotId::Grade)
}

fn snapshots_address() -> Address {
    Address::on(ThingId::ROOT, SlotId::Grade, GRADE_SNAPSHOTS_KEY)
}

/// The grade as it is: the scene's Grade card (or the card it would start with) with the keys and
/// pulses of its settings, its snapshots left out.
pub fn capture(scene: &Scene, reg: &Registry) -> SavedGrade {
    let card = scene
        .card(ThingId::ROOT, SlotId::Grade)
        .filter(|c| c.kind == GRADE)
        .cloned()
        .unwrap_or_else(|| starting_card(reg));
    let mut params = card.params;
    params.remove(GRADE_SNAPSHOTS_KEY);
    SavedGrade {
        params,
        layers: card.layers,
        used_ids: card.used_ids,
        tracks: scene.tracks.iter().filter(|(a, _)| on_grade(a)).map(|(a, t)| (a.key.clone(), t.clone())).collect(),
        pulses: scene.pulses.iter().filter(|(a, _)| on_grade(a)).map(|(a, p)| (a.key.clone(), p.clone())).collect(),
    }
}

/// Puts `grade` on the scene in place of its grade (giving it its Grade card first): the card's
/// settings, masks, parts and panels, and every key and pulse of its settings. The scene's
/// snapshots stay. True when anything changed.
pub fn put_grade(scene: &mut Scene, reg: &Registry, grade: &SavedGrade) -> bool {
    let before = (capture(scene, reg), scene.card(ThingId::ROOT, SlotId::Grade).is_some_and(|c| c.kind == GRADE));
    ensure_grade(scene, reg);
    let card = scene.card_mut(ThingId::ROOT, SlotId::Grade).expect("made above");
    let kept = card.params.get(GRADE_SNAPSHOTS_KEY).cloned();
    // Ids the card has given out stay given out, so a layer added later never takes the id of
    // one the editor still remembers (a folded panel, a selected mask).
    let given = card.next_layer_id().0;
    card.params = grade.params.clone();
    card.params.remove(GRADE_SNAPSHOTS_KEY);
    if let Some(kept) = kept {
        card.params.set(GRADE_SNAPSHOTS_KEY, kept);
    }
    card.layers = grade.layers.clone();
    card.used_ids = grade.used_ids.max(given);
    scene.tracks.retain(|a, _| !on_grade(a));
    scene.pulses.retain(|a, _| !on_grade(a));
    for (key, track) in &grade.tracks {
        scene.tracks.insert(Address::on(ThingId::ROOT, SlotId::Grade, key), track.clone());
    }
    for (key, pulse) in &grade.pulses {
        scene.pulses.insert(Address::on(ThingId::ROOT, SlotId::Grade, key), pulse.clone());
    }
    let after = capture(scene, reg);
    !before.1
        || (before.0.params, before.0.layers, before.0.tracks, before.0.pulses)
            != (after.params, after.layers, after.tracks, after.pulses)
}

/// The scene's snapshots, oldest first (none without a Grade card).
pub fn snapshots_of(scene: &Scene) -> Vec<GradeSnapshot> {
    match scene.card(ThingId::ROOT, SlotId::Grade).and_then(|c| c.params.get(GRADE_SNAPSHOTS_KEY)) {
        Some(Value::Snapshots(list)) => list.clone(),
        _ => Vec::new(),
    }
}

fn set_snapshots(scene: &mut Scene, reg: &Registry, list: Vec<GradeSnapshot>) -> Result<(), EditError> {
    ensure_grade(scene, reg);
    scene.set_at(reg, &snapshots_address(), Value::Snapshots(list))
}

/// "Snapshot 1", "Snapshot 2" …: the first not taken.
fn free_name(list: &[GradeSnapshot]) -> String {
    (1..).map(|n| format!("Snapshot {n}")).find(|name| !list.iter().any(|s| &s.name == name)).expect("a free name")
}

/// Saves the grade as it is as a new snapshot after the others. Returns its place.
pub fn add_snapshot(scene: &mut Scene, reg: &Registry) -> Result<usize, EditError> {
    let mut list = snapshots_of(scene);
    let grade = capture(scene, reg);
    list.push(GradeSnapshot { name: free_name(&list), grade });
    let at = list.len() - 1;
    set_snapshots(scene, reg, list)?;
    Ok(at)
}

/// Puts the grade as it is into snapshot `index`, keeping its name. True when it changed.
pub fn update_snapshot(scene: &mut Scene, reg: &Registry, index: usize) -> bool {
    let mut list = snapshots_of(scene);
    let grade = capture(scene, reg);
    let Some(s) = list.get_mut(index) else { return false };
    if s.grade == grade {
        return false;
    }
    s.grade = grade;
    set_snapshots(scene, reg, list).is_ok()
}

/// Names snapshot `index` anew; a blank name keeps the old one. True when it changed.
pub fn rename_snapshot(scene: &mut Scene, reg: &Registry, index: usize, name: &str) -> bool {
    let mut list = snapshots_of(scene);
    let name = name.trim();
    let Some(s) = list.get_mut(index) else { return false };
    if name.is_empty() || s.name == name {
        return false;
    }
    s.name = name.to_owned();
    set_snapshots(scene, reg, list).is_ok()
}

/// Snapshot `index` gone. True when it was there.
pub fn remove_snapshot(scene: &mut Scene, reg: &Registry, index: usize) -> bool {
    let mut list = snapshots_of(scene);
    if index >= list.len() {
        return false;
    }
    list.remove(index);
    set_snapshots(scene, reg, list).is_ok()
}

/// Puts snapshot `index`'s grade on the scene, cleaned as a loaded grade is (so a snapshot kept
/// from an earlier kit brings back only what the kit still has). True when anything changed.
pub fn restore_snapshot(scene: &mut Scene, reg: &Registry, index: usize) -> bool {
    let Some(s) = snapshots_of(scene).into_iter().nth(index) else { return false };
    match clean_grade(s.grade, reg) {
        Ok(grade) => put_grade(scene, reg, &grade),
        Err(_) => false,
    }
}

/// The scene's grade as the text of a `.grade` file (its snapshots left out).
pub fn grade_text(scene: &Scene, reg: &Registry) -> String {
    grade_file_text(&capture(scene, reg))
}

/// The grade a `.grade` file's text holds, cleaned (see `clean_grade`).
pub fn read_grade(text: &str, reg: &Registry) -> Result<SavedGrade, String> {
    clean_grade(read_grade_file(text)?, reg)
}

/// `grade` cleaned against the registry as a scene file is: settings and layers the kit doesn't
/// know or can't hold go; so do keys of settings the cleaned card doesn't have or that can't be
/// keyed (pulses likewise), and key values are pulled into their setting's range. A grade without
/// its Whole picture first is refused.
pub fn clean_grade(grade: SavedGrade, reg: &Registry) -> Result<SavedGrade, String> {
    let mut card = Slot::new(GRADE);
    card.params = grade.params;
    card.params.remove(GRADE_SNAPSHOTS_KEY);
    card.layers = grade.layers;
    card.used_ids = grade.used_ids;
    clean_card(&mut card, reg);
    let wholes = card.layers.iter().filter(|l| l.kind == WHOLE).count();
    if wholes != 1 || card.layers.first().is_none_or(|l| l.kind != WHOLE || l.parent.is_some()) {
        return Err("That file isn't a saved grade.".into());
    }
    let def_of = |key: &str| -> Option<&'static bpe_core::SettingDef> {
        match split_layered(key) {
            Some((id, plain)) => {
                let kind = card.layer(id).and_then(|l| reg.layer(&l.kind))?;
                let get = |k: &str| card.params.get(&bpe_core::layered_key(id, k)).cloned();
                layer_def(kind, &get, plain)
            }
            None => GradeCard::defs().iter().find(|d| d.key == key && d.key != GRADE_SNAPSHOTS_KEY),
        }
    };
    let tracks = grade
        .tracks
        .into_iter()
        .filter_map(|(key, mut track)| {
            let def = def_of(&key).filter(|d| d.flags.keyframe)?;
            if !track.keys.iter().all(|k| def.ty.accepts(&k.value)) {
                return None;
            }
            for k in &mut track.keys {
                k.value = def.clamp(k.value.clone());
            }
            track.range = def.range;
            Some((key, track))
        })
        .collect();
    let pulses = grade
        .pulses
        .into_iter()
        .filter_map(|(key, mut pulse)| {
            pulse.range = def_of(&key).filter(|d| d.flags.pulse)?.range;
            Some((key, pulse))
        })
        .collect();
    Ok(SavedGrade { params: card.params, layers: card.layers, used_ids: card.used_ids, tracks, pulses })
}

/// Puts the grade a `.grade` file's text holds on the scene, or says why it can't.
pub fn load_grade_text(scene: &mut Scene, reg: &Registry, text: &str) -> Result<(), String> {
    let grade = read_grade(text, reg)?;
    put_grade(scene, reg, &grade);
    Ok(())
}
