//! The dice: roll a scene, a thing, one card or one module from a seed, leaving locked, keyed
//! and never-random settings alone.

use crate::rng::Rng;
use bpe_core::{
    Address, Choice, Color, InstrumentId, InstrumentRef, InstrumentSource, ModuleId, Owner, ProgressionId,
    ProgressionRef, Registry, Scene, SettingDef, SettingValue, SlotId, SlotLink, ThingId, Value, ValueType,
};
use bpe_mix::{Instrument, PRESETS, Preset};

/// What one throw of the dice covers.
#[derive(Clone, Debug, PartialEq)]
pub enum Scope {
    /// Every thing's own settings, every library module, and every change a thing made on top.
    Scene,
    /// These things' own settings and the cards they link.
    Things(Vec<ThingId>),
    /// One card of one thing: its own settings (`slot: None`) or one slot.
    Card { thing: ThingId, slot: Option<SlotId> },
    /// One library module.
    Module(ModuleId),
}

/// One throw: the seed picks the values, the amount says how far from the current ones to go
/// (1 is all new, 0.3 is "more like this", 0 changes nothing).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Roll {
    pub seed: u32,
    pub amount: f32,
}

/// What a throw changed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rolled {
    pub changed: Vec<Address>,
}

/// Rolls every setting in `scope` that the dice may pick. Locked settings, keyed settings and
/// settings the dice never touch stay as they are. The same seed on the same scene gives the
/// same result, and each setting has its own dice, so locking one shifts none of the others.
pub fn roll(scene: &mut Scene, reg: &Registry, scope: &Scope, roll: Roll) -> Rolled {
    let mut rolled = Rolled::default();
    let addresses = addresses(scene, reg, scope);
    roll_progressions(scene, reg, &addresses, roll, &mut rolled);
    roll_instruments(scene, reg, &addresses, roll, &mut rolled);
    for a in addresses {
        if held(scene, &a) || scene.tracks.contains_key(&a) {
            continue;
        }
        let (Some(def), Some(current)) = (scene.def_at(reg, &a), scene.value_at(reg, &a)) else { continue };
        let mut dice = Rng::new(roll.seed, salt(&a));
        if let Some(v) = roll_value(def, &current, &mut dice, roll.amount)
            && v != current
            && scene.set_at(reg, &a, v).is_ok()
        {
            rolled.changed.push(a);
        }
    }
    fit_to_camera(scene, reg, &rolled);
    rolled
}

/// Rolls the chord progressions the scope's Chords settings point at: with a chance of
/// `amount`, a preset in the progression's mode replaces its chords, and close to "All new" its
/// key moves too. A setting on "Scene's" leaves Scene defaults' progression to its own setting. Each progression rolls once however many settings point at it, with the dice
/// of the first one. A locked Chords setting holds its progression.
fn roll_progressions(scene: &mut Scene, reg: &Registry, addresses: &[Address], roll: Roll, rolled: &mut Rolled) {
    let amount = roll.amount.clamp(0.0, 1.0);
    if amount <= 0.0 {
        return;
    }
    let mut done: Vec<ProgressionId> = Vec::new();
    for a in addresses {
        if held(scene, a) || scene.def_at(reg, a).is_none_or(|d| d.ty != ValueType::Progression) {
            continue;
        }
        let Some(r) = scene.value_at(reg, a).and_then(|v| ProgressionRef::from_value(&v)) else { continue };
        // "Scene's" belongs to Scene defaults' own Chords setting, which rolls it (or holds it).
        let Some(id) = r.0 else { continue };
        if done.contains(&id) {
            continue;
        }
        done.push(id);
        let Some(mut p) = scene.library.progressions.get(&id).cloned() else { continue };
        let mut dice = Rng::new(roll.seed, salt(a));
        if dice.unit() >= amount {
            continue;
        }
        if dice.unit() < structural_chance(amount) {
            p.transpose_to((dice.next_u32() % 12) as u8);
        }
        let fits: Vec<&Preset> = PRESETS.iter().filter(|preset| preset.mode == p.key.mode).collect();
        let preset = if fits.is_empty() {
            &PRESETS[dice.next_u32() as usize % PRESETS.len()]
        } else {
            fits[dice.next_u32() as usize % fits.len()]
        };
        p.key.mode = preset.mode;
        p.chords = preset.chords(p.key.root);
        if scene.set_progression(id, p).is_ok() {
            rolled.changed.push(a.clone());
        }
    }
}

/// Rolls the Instrument settings in the scope: with a chance of `amount`, each one picks a
/// built-in sound or one of the scene's library instruments. A built-in it picks joins the
/// library the first time. A setting on "Scene's" and a locked one stay as they are.
fn roll_instruments(scene: &mut Scene, reg: &Registry, addresses: &[Address], roll: Roll, rolled: &mut Rolled) {
    let amount = roll.amount.clamp(0.0, 1.0);
    if amount <= 0.0 {
        return;
    }
    for a in addresses {
        if held(scene, a) || scene.def_at(reg, a).is_none_or(|d| d.ty != ValueType::Instrument || !d.flags.random) {
            continue;
        }
        let Some(InstrumentRef(Some(current))) = scene.value_at(reg, a).and_then(|v| InstrumentRef::from_value(&v))
        else {
            continue;
        };
        let mut dice = Rng::new(roll.seed, salt(a));
        if dice.unit() >= amount {
            continue;
        }
        // The built-ins (not the drum kit) and the scene's patches.
        let built_ins: Vec<Instrument> = Instrument::ALL.iter().copied().filter(|i| *i != Instrument::Drums).collect();
        let patches: Vec<InstrumentId> = scene
            .library
            .instruments
            .iter()
            .filter(|(_, i)| matches!(i.source, InstrumentSource::Patch { .. }))
            .map(|(id, _)| *id)
            .collect();
        let pick = dice.next_u32() as usize % (built_ins.len() + patches.len());
        let id = match built_ins.get(pick) {
            Some(&which) => {
                let found = scene.library.instruments.iter().find(
                    |(_, i)| matches!(&i.source, InstrumentSource::BuiltIn { which: w, .. } if w == which.name()),
                );
                match found {
                    Some((&id, _)) => id,
                    None => scene.add_instrument(crate::built_in_instrument(which)),
                }
            }
            None => patches[pick - built_ins.len()],
        };
        if id != current && scene.set_at(reg, a, InstrumentRef(Some(id)).to_value()).is_ok() {
            rolled.changed.push(a.clone());
        }
    }
}

/// Arena sizes the dice picked, made to fit the camera's shot: the dice ranges suit a shot at
/// Zoom 1, so a zoomed-out camera gets bigger arenas and a zoomed-in one smaller.
fn fit_to_camera(scene: &mut Scene, reg: &Registry, rolled: &Rolled) {
    let Some(camera) = crate::camera_of(scene) else { return };
    let zoom = match scene.value_at(reg, &Address { owner: Owner::Thing(camera), slot: None, key: "zoom".into() }) {
        Some(Value::Number(z)) if z > 0.0 => z,
        _ => return,
    };
    if zoom == 1.0 {
        return;
    }
    for a in &rolled.changed {
        let Owner::Thing(id) = a.owner else { continue };
        let is_arena = scene.things.get(&id).is_some_and(|t| t.kind == "arena");
        if !is_arena || a.slot.is_some() || !matches!(a.key.as_str(), "radius" | "width" | "height") {
            continue;
        }
        if let Some(Value::Number(v)) = scene.value_at(reg, a) {
            let _ = scene.set_at(reg, a, Value::Number(v / zoom));
        }
    }
}

/// Whether a lock keeps the dice off `a`. A thing's slot setting it has not changed shows its
/// module's value, so a lock on that module setting holds it too.
fn held(scene: &Scene, a: &Address) -> bool {
    if scene.is_locked(a) {
        return true;
    }
    let (Owner::Thing(id), Some(slot)) = (a.owner, a.slot) else { return false };
    match scene.things.get(&id).and_then(|t| t.slots.get(&slot)) {
        Some(SlotLink::Linked { module, overrides }) if overrides.get(&a.key).is_none() => {
            scene.is_locked(&Address { owner: Owner::Module(*module), slot: None, key: a.key.clone() })
        }
        _ => false,
    }
}

/// The settings a throw over `scope` looks at, in tree order.
fn addresses(scene: &Scene, reg: &Registry, scope: &Scope) -> Vec<Address> {
    let mut out = Vec::new();
    match scope {
        Scope::Scene => {
            for &id in &scene.order {
                own_settings(scene, reg, id, &mut out);
                let Some(thing) = scene.things.get(&id) else { continue };
                for (&slot, link) in &thing.slots {
                    if let SlotLink::Linked { overrides, .. } = link {
                        for (key, _) in overrides.iter() {
                            let a = Address { owner: Owner::Thing(id), slot: Some(slot), key: key.clone() };
                            if scene.def_at(reg, &a).is_some_and(|d| d.flags.random) {
                                out.push(a);
                            }
                        }
                    }
                }
            }
            for &m in scene.library.modules.keys() {
                module_settings(scene, reg, m, &mut out);
            }
        }
        Scope::Things(ids) => {
            for &id in ids {
                own_settings(scene, reg, id, &mut out);
                let slots: Vec<SlotId> =
                    scene.things.get(&id).map(|t| t.slots.keys().copied().collect()).unwrap_or_default();
                for slot in slots {
                    card_settings(scene, reg, id, slot, &mut out);
                }
            }
        }
        Scope::Card { thing, slot: None } => own_settings(scene, reg, *thing, &mut out),
        Scope::Card { thing, slot: Some(slot) } => card_settings(scene, reg, *thing, *slot, &mut out),
        Scope::Module(m) => module_settings(scene, reg, *m, &mut out),
    }
    out
}

fn own_settings(scene: &Scene, reg: &Registry, id: ThingId, out: &mut Vec<Address>) {
    let Some(kind) = scene.things.get(&id).and_then(|t| reg.thing(&t.kind)) else { return };
    out.extend(kind.settings().iter().filter(|d| d.flags.random).map(|d| Address {
        owner: Owner::Thing(id),
        slot: None,
        key: d.key.into(),
    }));
}

fn module_settings(scene: &Scene, reg: &Registry, m: ModuleId, out: &mut Vec<Address>) {
    let Some(kind) = scene.library.modules.get(&m).and_then(|m| reg.module(&m.kind)) else { return };
    out.extend(kind.settings().iter().filter(|d| d.flags.random).map(|d| Address {
        owner: Owner::Module(m),
        slot: None,
        key: d.key.into(),
    }));
}

/// One slot card the way the inspector writes it: a module used by this thing alone, with no
/// changes on top, is edited itself; a shared module gets this thing's own changes instead; an
/// inherited or empty slot is left alone.
fn card_settings(scene: &Scene, reg: &Registry, id: ThingId, slot: SlotId, out: &mut Vec<Address>) {
    let Some(SlotLink::Linked { module, overrides }) = scene.things.get(&id).and_then(|t| t.slots.get(&slot)) else {
        return;
    };
    if overrides.is_empty() && scene.users_of(*module).len() == 1 {
        return module_settings(scene, reg, *module, out);
    }
    let Some(kind) = scene.library.modules.get(module).and_then(|m| reg.module(&m.kind)) else { return };
    out.extend(kind.settings().iter().filter(|d| d.flags.random).map(|d| Address {
        owner: Owner::Thing(id),
        slot: Some(slot),
        key: d.key.into(),
    }));
}

/// How far the dice go until someone moves "How much": far enough to surprise, near enough
/// that the scene stays recognisable.
pub const START_AMOUNT: f32 = 0.5;

/// A new value for `def`, or `None` when the dice leave this setting alone. `amount` 1 picks
/// something new; a smaller amount moves part of the way there from `current`.
pub fn roll_value(def: &SettingDef, current: &Value, dice: &mut Rng, amount: f32) -> Option<Value> {
    if !def.flags.random {
        return None;
    }
    let amount = amount.clamp(0.0, 1.0);
    if amount <= 0.0 {
        return None;
    }
    match current {
        Value::Number(x) => {
            let dice_range = def.dice_range()?;
            Some(Value::Number(roll_number(*x, dice_range, def.range, dice, amount)))
        }
        Value::Int(n) => {
            let dice_range = def.dice_range()?;
            Some(Value::Int(roll_number(*n as f64, dice_range, def.range, dice, amount).round() as i64))
        }
        Value::Bool(b) => (dice.unit() < amount * 0.5).then_some(Value::Bool(!b)),
        Value::Choice(_) => {
            let bpe_core::ValueType::Choice(names) = def.ty else { return None };
            let chance = if def.flags.structural { structural_chance(amount) } else { amount };
            if names.is_empty() || dice.unit() >= chance {
                return None;
            }
            Some(Value::Choice(names[dice.next_u32() as usize % names.len()].into()))
        }
        Value::Color(c) => {
            let rolled = if amount >= 1.0 { like(palette(dice, 1)[0], *c) } else { c.shift_hue(turn(dice, amount)) };
            Some(Value::Color(covering(rolled)))
        }
        Value::Gradient(g) => {
            // As many stops as before, each colour rolled like a colour; the stops between the
            // ends head for an even spread, give or take a tenth (at most 0.4 of the even gap, so
            // neighbours never swap however many stops there are), and the ends for 0 and 1.
            let mut rolled = g.clone();
            if amount >= 1.0 {
                for (stop, new) in rolled.stops.iter_mut().zip(palette(dice, g.stops.len())) {
                    stop.color = covering(like(new, stop.color));
                }
            } else {
                let t = turn(dice, amount);
                for stop in &mut rolled.stops {
                    stop.color = covering(stop.color.shift_hue(t));
                }
            }
            let last = rolled.stops.len().saturating_sub(1);
            // The ends head for 0 and 1 as far as `amount` says, landing there on a full roll.
            let to_end = |at: f32, end: f32| if amount >= 1.0 { end } else { at + (end - at) * amount };
            for (i, stop) in rolled.stops.iter_mut().enumerate() {
                stop.at = if i == 0 {
                    to_end(stop.at, 0.0)
                } else if i == last {
                    to_end(stop.at, 1.0)
                } else {
                    let even = i as f32 / last as f32;
                    let jitter = (0.4 / last as f32).min(0.1);
                    let goal = (even + (dice.unit() * 2.0 - 1.0) * jitter).clamp(0.0, 1.0);
                    stop.at + (goal - stop.at) * amount
                };
            }
            Some(Value::Gradient(rolled.sorted()))
        }
        Value::Vec2([x, y]) => Some(Value::Vec2(roll_vec2([*x, *y], def.dice_range(), def.range, dice, amount))),
        Value::Text(_) | Value::Target(_) => None,
    }
}

/// How likely a structural choice is to change: never up to three quarters, then rising to
/// certain at "All new", so a roll keeps a peg board a peg board unless asked for everything new.
fn structural_chance(amount: f32) -> f32 {
    ((amount - 0.75) / 0.25).clamp(0.0, 1.0)
}

/// The colour, made to cover if it was an additive one: light now comes from a module's Blend
/// setting, so the dice never roll the colour picker's old "Additive".
fn covering(c: Color) -> Color {
    if c.is_additive() { Color { a: 255, ..c } } else { c }
}

/// `new`'s hue and saturation with `old`'s brightness and transparency, so a dark background
/// stays dark and a see-through colour stays see-through.
fn like(new: Color, old: Color) -> Color {
    let (h, s, _) = new.to_hsv();
    Color { a: old.a, ..Color::from_hsv(h, s, old.to_hsv().2) }
}

/// Up to half a turn around the colour wheel either way, scaled by the amount.
fn turn(dice: &mut Rng, amount: f32) -> f32 {
    (dice.unit() * 2.0 - 1.0) * 0.5 * amount
}

/// A wide range of positive sizes is rolled on a log scale, so small and large come up as often.
fn log_scale(lo: f64, hi: f64) -> bool {
    lo > 0.0 && hi / lo >= 8.0
}

/// A number picked in the dice range `(lo, hi)` at amount 1; a smaller amount moves part of the
/// way there from `current`, wherever it is, staying inside the setting's own `range`. So a
/// little never snaps a value into the dice range first (a resting ball stays nearly at rest).
fn roll_number(current: f64, (lo, hi): (f64, f64), range: Option<(f64, f64)>, dice: &mut Rng, amount: f32) -> f64 {
    let log = log_scale(lo, hi);
    let pick = if hi <= lo {
        lo
    } else if log {
        (lo.ln() + (hi.ln() - lo.ln()) * dice.unit() as f64).exp().clamp(lo, hi)
    } else {
        (lo + (hi - lo) * dice.unit() as f64).clamp(lo, hi)
    };
    // The ends are exact: the pick itself at 1, else part of the way between.
    if amount >= 1.0 {
        return pick;
    }
    let a = amount as f64;
    let moved = if log && current > 0.0 {
        (current.ln() + (pick.ln() - current.ln()) * a).exp()
    } else {
        current + (pick - current) * a
    };
    match range {
        Some((min, max)) => moved.clamp(min, max),
        None => moved,
    }
}

fn roll_vec2(
    current: [f32; 2],
    dice_range: Option<(f64, f64)>,
    range: Option<(f64, f64)>,
    dice: &mut Rng,
    amount: f32,
) -> [f32; 2] {
    let speed = (current[0] * current[0] + current[1] * current[1]).sqrt();
    let angle = current[1].atan2(current[0]);
    let (new_speed, new_angle) = match dice_range {
        Some(dice_range) => {
            let new_speed = roll_number(speed as f64, dice_range, range, dice, amount) as f32;
            let picked = dice.unit() * std::f32::consts::TAU;
            let delta =
                (picked - angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            let new_angle = if speed == 0.0 { picked } else { angle + delta * amount };
            (new_speed, new_angle)
        }
        None => {
            let scale = 2f32.powf((dice.unit() * 2.0 - 1.0) * amount);
            let spin = (dice.unit() * 2.0 - 1.0) * std::f32::consts::PI * amount;
            (speed * scale, angle + spin)
        }
    };
    [new_speed * new_angle.cos(), new_speed * new_angle.sin()]
}

/// `n` colours that go together: hues stepped around the wheel (close, spread, opposite or
/// evenly), all bright and fairly saturated.
pub fn palette(dice: &mut Rng, n: usize) -> Vec<Color> {
    let base = dice.unit();
    let sign = if dice.unit() < 0.5 { -1.0 } else { 1.0 };
    let step = match dice.next_u32() % 4 {
        0 => dice.range(0.04, 0.1),
        1 => dice.range(0.1, 0.2),
        2 => 0.5 / (n.max(2) - 1) as f32,
        _ => dice.range(0.6, 0.9) / n.max(1) as f32,
    } * sign;
    let saturation = dice.range(0.55, 0.9);
    (0..n)
        .map(|i| {
            let s = (saturation + dice.range(-0.08, 0.08)).clamp(0.55, 0.9);
            let v = dice.range(0.85, 1.0);
            Color::from_hsv(base + step * i as f32, s, v)
        })
        .collect()
}

/// Each setting's own dice stream, from its address, so one lock never shifts another setting.
pub fn salt(a: &Address) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            hash ^= *b as u32;
            hash = hash.wrapping_mul(0x0100_0193);
        }
    };
    match a.owner {
        Owner::Thing(t) => eat(format!("thing:{}", t.0).as_bytes()),
        Owner::Module(m) => eat(format!("module:{}", m.0).as_bytes()),
    }
    eat(b"/");
    if let Some(slot) = a.slot {
        eat(format!("{slot:?}").as_bytes());
    }
    eat(b"/");
    eat(a.key.as_bytes());
    hash
}

/// A seed nobody has used yet, from the clock. Never 0, so "no seed" stays free to mean that.
pub fn fresh_seed() -> u32 {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(1);
    let folded = (nanos as u64 ^ (nanos >> 64) as u64) as u32 ^ (nanos >> 32) as u32;
    folded.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{add_thing, new_scene, registry};
    use bpe_core::{Gradient, Owner, SlotId, ThingId, Value, ValueType};

    fn def(reg: &bpe_core::Registry, kind: &str, key: &str) -> SettingDef {
        let defs = reg.thing(kind).map(|k| k.settings()).or_else(|| reg.module(kind).map(|k| k.settings()));
        defs.unwrap_or_else(|| panic!("no kind {kind}")).iter().find(|d| d.key == key).unwrap().clone()
    }

    fn rolls(def: &SettingDef, current: &Value, amount: f32, n: u32) -> Vec<Value> {
        (0..n).filter_map(|i| roll_value(def, current, &mut Rng::new(i, 1), amount)).collect()
    }

    fn starter_chords(scene: &Scene) -> bpe_core::Progression {
        scene.library.progressions.values().next().unwrap().clone()
    }

    #[test]
    fn rolling_the_scene_picks_a_preset_in_key() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        let r = roll(&mut scene, &reg, &Scope::Scene, Roll { seed: 7, amount: 1.0 });
        let p = starter_chords(&scene);
        assert!(PRESETS.iter().any(|preset| preset.chords(p.key.root) == p.chords), "{:?}", p.chords);
        assert!(r.changed.iter().any(|a| a.key == "chords"));
    }

    #[test]
    fn rolling_picks_an_instrument_and_respects_locks() {
        let reg = registry();
        let sound = |scene: &Scene| *scene.library.modules.iter().find(|(_, m)| m.kind == "sound.notes").unwrap().0;
        let mut scene = new_scene(&reg);
        let a = Address { owner: Owner::Module(sound(&scene)), slot: None, key: "instrument".into() };
        let before = scene.value_at(&reg, &a);
        let changed = (0..20).any(|seed| {
            let mut s = scene.clone();
            roll(&mut s, &reg, &Scope::Scene, Roll { seed, amount: 1.0 });
            let after = s.value_at(&reg, &a);
            let id = after.as_ref().and_then(InstrumentRef::from_value).and_then(|r| r.0);
            assert!(id.is_some_and(|id| s.library.instruments.contains_key(&id)), "points into the library");
            after != before
        });
        assert!(changed, "some roll picks another instrument");
        scene.set_locked(a.clone(), true);
        for seed in 0..20 {
            roll(&mut scene, &reg, &Scope::Scene, Roll { seed, amount: 1.0 });
        }
        assert_eq!(scene.value_at(&reg, &a), before);
    }

    #[test]
    fn a_locked_chords_setting_keeps_its_progression() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        let before = starter_chords(&scene);
        scene.set_locked(Address { owner: Owner::Thing(ThingId::ROOT), slot: None, key: "chords".into() }, true);
        for seed in 0..20 {
            roll(&mut scene, &reg, &Scope::Scene, Roll { seed, amount: 1.0 });
        }
        assert_eq!(starter_chords(&scene), before);
    }

    #[test]
    fn the_same_seed_rolls_the_same_chords() {
        let reg = registry();
        let (mut a, mut b) = (new_scene(&reg), new_scene(&reg));
        roll(&mut a, &reg, &Scope::Scene, Roll { seed: 3, amount: 0.9 });
        roll(&mut b, &reg, &Scope::Scene, Roll { seed: 3, amount: 0.9 });
        assert_eq!(starter_chords(&a), starter_chords(&b));
    }

    #[test]
    fn numbers_roll_inside_the_dice_range_and_log_scale_when_wide() {
        let reg = registry();
        let radius = def(&reg, "balls", "radius");
        let mut xs: Vec<f64> = rolls(&radius, &Value::Number(24.0), 1.0, 1000)
            .into_iter()
            .map(|v| match v {
                Value::Number(x) => x,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(xs.len(), 1000);
        assert!(xs.iter().all(|x| (6.0..=60.0).contains(x)), "inside 6..60");
        xs.sort_by(f64::total_cmp);
        assert!(xs[500] < 25.0, "a log scale favours smaller sizes, median {}", xs[500]);
        let spacing = def(&reg, "balls", "spacing");
        let mut ys: Vec<f64> = rolls(&spacing, &Value::Number(30.0), 1.0, 1000)
            .into_iter()
            .map(|v| match v {
                Value::Number(x) => x,
                other => panic!("{other:?}"),
            })
            .collect();
        ys.sort_by(f64::total_cmp);
        assert!((28.0..40.0).contains(&ys[500]), "a narrow range is linear, median {}", ys[500]);
    }

    #[test]
    fn ints_bools_choices_and_colours_roll() {
        let reg = registry();
        let count = def(&reg, "balls", "count");
        for v in rolls(&count, &Value::Int(12), 1.0, 200) {
            match v {
                Value::Int(n) => assert!((3..=200).contains(&n), "{n}"),
                other => panic!("{other:?}"),
            }
        }
        let follow = def(&reg, "particles.burst", "follow_impact");
        let bools = rolls(&follow, &Value::Bool(false), 1.0, 50);
        assert!(bools.contains(&Value::Bool(true)), "some flip");
        assert!(bools.len() < 50, "and some stay (no value returned)");
        let shape = def(&reg, "arena", "shape");
        let ValueType::Choice(names) = shape.ty else { panic!("shape is a choice") };
        for v in rolls(&shape, &Value::Choice(names[0].into()), 1.0, 50) {
            match v {
                Value::Choice(name) => assert!(names.contains(&name.as_str()), "{name}"),
                other => panic!("{other:?}"),
            }
        }
        let gradient = def(&reg, "look.classic", "colours");
        let red = Color::rgb(255, 0, 0);
        let one = rolls(&gradient, &Value::Gradient(Gradient::solid(red)), 1.0, 20);
        assert_eq!(one.len(), 20);
        assert!(one.iter().any(|v| *v != Value::Gradient(Gradient::solid(red))), "a new colour comes up");
        for v in one {
            let Value::Gradient(g) = v else { panic!("{v:?}") };
            assert_eq!(g.stops.len(), 1, "one colour stays one colour");
            assert_eq!(g.stops[0].at, 0.0);
        }
        let three = Gradient::even(&[red, Color::rgb(0, 255, 0), Color::rgb(0, 0, 255)]);
        let mut middles = std::collections::BTreeSet::new();
        for v in rolls(&gradient, &Value::Gradient(three), 1.0, 20) {
            let Value::Gradient(g) = v else { panic!("{v:?}") };
            assert_eq!(g.stops.len(), 3);
            assert_eq!((g.stops[0].at, g.stops[2].at), (0.0, 1.0), "the ends stay at the ends");
            assert!(g.stops.windows(2).all(|w| w[0].at <= w[1].at), "in order");
            middles.insert(g.stops[1].at.to_bits());
        }
        assert!(middles.len() > 1, "the stop between moves");
    }

    #[test]
    fn a_little_gradient_dice_moves_the_end_stops_part_way_to_the_ends() {
        let reg = registry();
        let gradient = def(&reg, "look.classic", "colours");
        let inset = Gradient {
            stops: [0.2, 0.5, 0.8].iter().map(|&at| bpe_core::Stop { at, color: Color::rgb(200, 40, 40) }).collect(),
            transition: bpe_core::Transition::Smooth,
        };
        for v in rolls(&gradient, &Value::Gradient(inset.clone()), 0.3, 20) {
            let Value::Gradient(g) = v else { panic!("{v:?}") };
            let (first, last) = (g.stops[0].at, g.stops[2].at);
            assert!((first - 0.14).abs() < 1e-5, "the first stop moves 0.3 of the way to 0: {first}");
            assert!((last - 0.86).abs() < 1e-5, "the last stop moves 0.3 of the way to 1: {last}");
        }
        for v in rolls(&gradient, &Value::Gradient(inset), 1.0, 5) {
            let Value::Gradient(g) = v else { panic!("{v:?}") };
            assert_eq!((g.stops[0].at, g.stops[2].at), (0.0, 1.0), "all the way: the ends");
        }
    }

    #[test]
    fn gradient_dice_keep_stop_count_and_sorted_stops() {
        let reg = registry();
        let gradient = def(&reg, "look.classic", "colours");
        let four = Gradient {
            stops: [0.0, 0.05, 0.1, 1.0]
                .iter()
                .map(|&at| bpe_core::Stop { at, color: Color::rgb(200, 40, 40) })
                .collect(),
            transition: bpe_core::Transition::Eased,
        };
        let mut colours = std::collections::BTreeSet::new();
        for v in rolls(&gradient, &Value::Gradient(four), 1.0, 50) {
            let Value::Gradient(g) = v else { panic!("{v:?}") };
            assert_eq!(g.stops.len(), 4, "the stop count stays");
            assert_eq!(g.transition, bpe_core::Transition::Eased, "the blend stays");
            assert!(g.stops.windows(2).all(|w| w[0].at <= w[1].at), "in order: {g:?}");
            assert_eq!((g.stops[0].at, g.stops[3].at), (0.0, 1.0), "the ends stay at the ends");
            for (i, s) in g.stops.iter().enumerate() {
                let even = i as f32 / 3.0;
                assert!((s.at - even).abs() <= 0.1 + 1e-6, "stop {i} at {} is near {even}", s.at);
            }
            colours.insert(g.stops[1].color.to_hex());
        }
        assert!(colours.len() > 10, "new colours come up");
        // With many stops the nudge shrinks to 0.4 of the gap, so no two stops ever cross.
        let twelve = Gradient::even(&(0..12).map(|i| Color::rgb(i * 20, 100, 200)).collect::<Vec<_>>());
        for v in rolls(&gradient, &Value::Gradient(twelve), 1.0, 50) {
            let Value::Gradient(g) = v else { panic!("{v:?}") };
            assert_eq!(g.stops.len(), 12);
            for (i, s) in g.stops.iter().enumerate() {
                let even = i as f32 / 11.0;
                assert!((s.at - even).abs() <= 0.4 / 11.0 + 1e-6, "stop {i} at {} is near {even}", s.at);
            }
            assert!(g.stops.windows(2).all(|w| w[1].at - w[0].at >= 0.2 / 11.0 - 1e-6), "gaps stay open: {g:?}");
        }
        // What the colours follow, their angle, span and scroll roll inside their dice ranges.
        for key in ["angle", "span", "scroll"] {
            let d = def(&reg, "look.classic", key);
            let (lo, hi) = d.dice_range().unwrap_or_else(|| panic!("{key} has a dice range"));
            let now = d.default.clone();
            let rolled = rolls(&d, &now, 1.0, 50);
            assert_eq!(rolled.len(), 50, "{key} rolls");
            for v in rolled {
                let Value::Number(x) = v else { panic!("{key}: {v:?}") };
                assert!((lo..=hi).contains(&x), "{key} {x} inside {lo}..{hi}");
            }
        }
        let follow = def(&reg, "look.classic", "follow");
        let ValueType::Choice(names) = follow.ty else { panic!("follow is a choice") };
        let picked: std::collections::BTreeSet<String> = rolls(&follow, &follow.default, 1.0, 50)
            .into_iter()
            .map(|v| match v {
                Value::Choice(n) => n,
                other => panic!("{other:?}"),
            })
            .collect();
        assert!(picked.len() > 1 && picked.iter().all(|n| names.contains(&n.as_str())), "{picked:?}");
    }

    #[test]
    fn shapes_and_arrangements_stay_put_until_close_to_all_new() {
        let reg = registry();
        for (kind, key) in
            [("balls", "arrangement"), ("obstacle", "shape"), ("arena", "shape"), ("soft", "shape"), ("field", "kind")]
        {
            let d = def(&reg, kind, key);
            assert!(d.flags.structural, "{kind}.{key} decides what the thing is");
            let ValueType::Choice(names) = d.ty else { panic!("{kind}.{key} is a choice") };
            let first = Value::Choice(names[0].into());
            assert!(rolls(&d, &first, 0.75, 200).is_empty(), "{kind}.{key} stays put at three quarters");
            assert!(rolls(&d, &first, 1.0, 50).iter().any(|v| *v != first), "{kind}.{key} changes at All new");
        }
        let order = def(&reg, "sound.notes", "order");
        assert!(!order.flags.structural);
        let ValueType::Choice(names) = order.ty else { panic!("order is a choice") };
        let first = Value::Choice(names[0].into());
        assert!(rolls(&order, &first, 0.5, 50).iter().any(|v| *v != first), "other choices change half way");
    }

    #[test]
    fn a_peg_board_rolled_half_way_keeps_room_for_its_balls() {
        let reg = registry();
        let (_, board) = crate::scenes::all(&reg).into_iter().find(|(n, _)| n == "Peg board").expect("the peg board");
        let number = |scene: &Scene, kind: &str, key: &str| -> f64 {
            let thing = scene.things.values().find(|t| t.kind == kind).expect(kind);
            match thing.params.get(key) {
                Some(Value::Number(x)) => *x,
                other => panic!("{kind}.{key} is {other:?}"),
            }
        };
        for seed in 0..60 {
            let mut scene = board.clone();
            roll(&mut scene, &reg, &Scope::Scene, Roll { seed, amount: START_AMOUNT });
            let gap = number(&scene, "obstacle", "spacing") - 2.0 * number(&scene, "obstacle", "radius");
            let ball = 2.0 * number(&scene, "balls", "radius");
            assert!(gap > ball, "seed {seed}: pegs {gap:.0} px apart jam balls {ball:.0} px wide");
        }
    }

    #[test]
    fn rolls_never_make_additive_colours() {
        let reg = registry();
        let glow = Color::additive(255, 136, 0);
        let top = def(&reg, "scene", "background_colours");
        let gradient = def(&reg, "look.classic", "colours");
        for amount in [0.3, 1.0] {
            for v in rolls(&top, &Value::Color(glow), amount, 20) {
                let Value::Color(c) = v else { panic!("{v:?}") };
                assert!(!c.is_additive(), "amount {amount}: {c:?}");
            }
            let list = Gradient::even(&[glow, Color::additive(0, 255, 0)]);
            for v in rolls(&gradient, &Value::Gradient(list), amount, 20) {
                let Value::Gradient(g) = v else { panic!("{v:?}") };
                assert!(g.stops.iter().all(|s| !s.color.is_additive()), "amount {amount}: {g:?}");
            }
        }
    }

    #[test]
    fn a_full_colour_roll_keeps_brightness_and_transparency() {
        let reg = registry();
        // A single colour rolls the same way whatever setting holds it.
        let top = def(&reg, "scene", "background_colours");
        let navy = Color { r: 10, g: 12, b: 48, a: 200 };
        let (_, _, dark) = navy.to_hsv();
        let new = rolls(&top, &Value::Color(navy), 1.0, 30);
        assert!(new.iter().any(|v| *v != Value::Color(navy)), "a new colour comes up");
        for v in new {
            let Value::Color(c) = v else { panic!("{v:?}") };
            let (_, _, value) = c.to_hsv();
            assert!((value - dark).abs() < 0.02, "a dark background stays dark: {c:?}");
            assert_eq!(c.a, 200, "and keeps its transparency");
        }
        let gradient = def(&reg, "look.classic", "colours");
        let list = [Color { r: 20, g: 20, b: 60, a: 128 }, Color::rgb(255, 200, 0)];
        for v in rolls(&gradient, &Value::Gradient(Gradient::even(&list)), 1.0, 30) {
            let Value::Gradient(new) = v else { panic!("{v:?}") };
            assert_eq!(new.stops.len(), 2);
            for (c, was) in new.stops.iter().map(|s| &s.color).zip(&list) {
                assert!((c.to_hsv().2 - was.to_hsv().2).abs() < 0.02, "{c:?} keeps the brightness of {was:?}");
                assert_eq!(c.a, was.a, "{c:?} keeps the transparency of {was:?}");
            }
        }
    }

    #[test]
    fn a_vec2_with_a_dice_range_rolls_its_speed_and_direction() {
        let reg = registry();
        let velocity = def(&reg, "balls", "velocity");
        let mut quadrants = [false; 4];
        for v in rolls(&velocity, &Value::Vec2([300.0, -200.0]), 1.0, 200) {
            let Value::Vec2([x, y]) = v else { panic!("{v:?}") };
            let speed = (x * x + y * y).sqrt();
            assert!((99.9..=900.1).contains(&speed), "{speed}");
            quadrants[(x >= 0.0) as usize + 2 * (y >= 0.0) as usize] = true;
        }
        assert_eq!(quadrants, [true; 4], "every direction comes up");
        let wind = def(&reg, "motion.basic", "wind");
        for v in rolls(&wind, &Value::Vec2([100.0, 0.0]), 0.5, 50) {
            let Value::Vec2([x, y]) = v else { panic!("{v:?}") };
            let speed = (x * x + y * y).sqrt();
            assert!((70.0..=142.0).contains(&speed), "without a range the speed changes by at most 2^amount: {speed}");
        }
    }

    #[test]
    fn partial_amount_moves_part_way() {
        let reg = registry();
        let radius = def(&reg, "balls", "radius");
        let full: Vec<f64> = rolls(&radius, &Value::Number(24.0), 1.0, 300)
            .into_iter()
            .map(|v| if let Value::Number(x) = v { x } else { unreachable!() })
            .collect();
        let part: Vec<f64> = rolls(&radius, &Value::Number(24.0), 0.3, 300)
            .into_iter()
            .map(|v| if let Value::Number(x) = v { x } else { unreachable!() })
            .collect();
        assert!(part.iter().all(|x| (6.0..=60.0).contains(x)));
        let away = |xs: &[f64]| xs.iter().map(|x| (x - 24.0).abs()).sum::<f64>() / xs.len() as f64;
        assert!(away(&part) < away(&full) * 0.5, "part {} full {}", away(&part), away(&full));
        assert!(rolls(&radius, &Value::Number(24.0), 0.0, 10).iter().all(|v| *v == Value::Number(24.0)));
    }

    #[test]
    fn no_amount_changes_nothing_and_a_little_stays_near_a_value_outside_the_dice_range() {
        let reg = registry();
        let velocity = def(&reg, "balls", "velocity");
        let radius = def(&reg, "balls", "radius");
        let still = Value::Vec2([0.0, 0.0]);
        let tiny = Value::Number(3.0);
        assert!(rolls(&velocity, &still, 0.0, 20).is_empty(), "nothing rolls at no amount");
        assert!(rolls(&radius, &tiny, 0.0, 20).is_empty());
        // A little moves part of the way from where the setting is, not into the dice range first.
        for v in rolls(&velocity, &still, 0.3, 200) {
            let Value::Vec2([x, y]) = v else { unreachable!() };
            let speed = (x * x + y * y).sqrt();
            assert!(speed <= 0.3 * 900.0 + 1e-3, "resting balls move at most 30% of the way: {speed}");
        }
        for v in rolls(&radius, &tiny, 0.3, 200) {
            let Value::Number(r) = v else { unreachable!() };
            assert!((3.0..=3.0 + 0.3 * (60.0 - 3.0) + 1e-9).contains(&r), "{r}");
        }
        // A value inside the setting's own range but outside the dice range stays in the setting's range.
        for v in rolls(&radius, &tiny, 1.0, 50) {
            let Value::Number(r) = v else { unreachable!() };
            assert!((6.0..=60.0).contains(&r), "a full roll picks from the dice range: {r}");
        }
    }

    #[test]
    fn targets_text_and_unflagged_settings_never_roll() {
        let reg = registry();
        let target = def(&reg, "emitter", "target");
        assert!(rolls(&target, &Value::Target(bpe_core::Target::Anything), 1.0, 5).is_empty());
        let text = def(&reg, "text", "text");
        assert!(rolls(&text, &Value::Text("Hi".into()), 1.0, 5).is_empty());
        let duration = def(&reg, "scene", "duration");
        assert!(rolls(&duration, &Value::Number(10.0), 1.0, 5).is_empty());
    }

    #[test]
    fn palette_makes_n_distinct_bright_colours() {
        for seed in 0..20 {
            let colours = palette(&mut Rng::new(seed, 0), 5);
            assert_eq!(colours.len(), 5);
            for c in &colours {
                let (_, s, v) = c.to_hsv();
                assert!((0.52..=0.93).contains(&s), "saturation {s}");
                assert!((0.82..=1.0).contains(&v), "value {v}");
            }
            let distinct: std::collections::BTreeSet<_> = colours.iter().map(|c| (c.r, c.g, c.b)).collect();
            assert!(distinct.len() >= 2, "{colours:?}");
        }
    }

    fn own(id: ThingId, key: &str) -> Address {
        Address { owner: Owner::Thing(id), slot: None, key: key.into() }
    }

    #[test]
    fn scene_scope_rolls_things_modules_and_overrides_but_not_locks_keys_or_inherited_slots() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        let a = add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
        let b = add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
        let neon = scene.add_module(&reg, "look.classic", "Neon").unwrap();
        scene.link(&reg, a, SlotId::Look, neon).unwrap();
        let glow = Address { owner: Owner::Thing(a), slot: Some(SlotId::Look), key: "glow".into() };
        scene.set_at(&reg, &glow, Value::Number(1.5)).unwrap();
        scene.set_locked(own(b, "radius"), true);
        let bloom = own(ThingId::ROOT, "bloom");
        scene.set_key(&reg, &bloom, 1.0, Value::Number(0.7)).unwrap();
        scene.things.get_mut(&b).unwrap().slots.insert(SlotId::Motion, bpe_core::SlotLink::Inherit);
        let neon_colour = Address { owner: Owner::Module(neon), slot: None, key: "colours".into() };
        let before = scene.clone();

        let rolled = roll(&mut scene, &reg, &Scope::Scene, Roll { seed: 5, amount: 1.0 });

        assert!(rolled.changed.contains(&own(a, "radius")), "a's own size rolled");
        assert!(rolled.changed.contains(&neon_colour), "the library module rolled");
        assert!(rolled.changed.contains(&glow), "a's own change on top of Neon rolled");
        assert_ne!(scene.value_at(&reg, &glow), Some(Value::Number(1.5)));
        assert!(!rolled.changed.contains(&own(b, "radius")), "locked");
        assert_eq!(scene.value_at(&reg, &own(b, "radius")), before.value_at(&reg, &own(b, "radius")));
        assert!(!rolled.changed.contains(&bloom), "keyed");
        assert_eq!(scene.things[&b].slots[&SlotId::Motion], bpe_core::SlotLink::Inherit, "no override appears");
        assert!(!rolled.changed.iter().any(|c| c.owner == Owner::Thing(b) && c.slot.is_some()));
    }

    #[test]
    fn card_scope_follows_the_write_policy() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        let a = add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
        let neon = scene.add_module(&reg, "look.classic", "Neon").unwrap();
        scene.link(&reg, a, SlotId::Look, neon).unwrap();
        let params_before = scene.library.modules[&neon].params.clone();
        let rolled =
            roll(&mut scene, &reg, &Scope::Card { thing: a, slot: Some(SlotId::Look) }, Roll { seed: 3, amount: 1.0 });
        assert!(rolled.changed.iter().all(|c| c.owner == Owner::Module(neon)), "the only user edits the module");
        assert_ne!(scene.library.modules[&neon].params, params_before);
        let overrides = |scene: &Scene, id: ThingId| match &scene.things[&id].slots[&SlotId::Look] {
            bpe_core::SlotLink::Linked { overrides, .. } => overrides.clone(),
            other => panic!("{other:?}"),
        };
        assert!(overrides(&scene, a).is_empty());

        let c = add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
        scene.link(&reg, c, SlotId::Look, neon).unwrap();
        let params_before = scene.library.modules[&neon].params.clone();
        let rolled =
            roll(&mut scene, &reg, &Scope::Card { thing: c, slot: Some(SlotId::Look) }, Roll { seed: 3, amount: 1.0 });
        assert!(!rolled.changed.is_empty());
        assert!(rolled.changed.iter().all(|c2| c2.owner == Owner::Thing(c) && c2.slot == Some(SlotId::Look)));
        assert_eq!(scene.library.modules[&neon].params, params_before, "a shared module is left alone");
        assert!(!overrides(&scene, c).is_empty());
        assert!(overrides(&scene, a).is_empty());

        let d = add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
        scene.things.get_mut(&d).unwrap().slots.insert(SlotId::Look, bpe_core::SlotLink::Inherit);
        let rolled =
            roll(&mut scene, &reg, &Scope::Card { thing: d, slot: Some(SlotId::Look) }, Roll { seed: 3, amount: 1.0 });
        assert!(rolled.changed.is_empty());
        assert_eq!(scene.things[&d].slots[&SlotId::Look], bpe_core::SlotLink::Inherit);

        let rolled = roll(&mut scene, &reg, &Scope::Card { thing: d, slot: None }, Roll { seed: 3, amount: 1.0 });
        assert!(rolled.changed.contains(&own(d, "radius")));
    }

    #[test]
    fn same_seed_same_result_and_locks_do_not_shift_other_settings() {
        let reg = registry();
        let mut base = new_scene(&reg);
        let a = add_thing(&mut base, &reg, "balls", ThingId::ROOT).unwrap();
        let mut one = base.clone();
        let mut two = base.clone();
        let r1 = roll(&mut one, &reg, &Scope::Scene, Roll { seed: 7, amount: 1.0 });
        roll(&mut two, &reg, &Scope::Scene, Roll { seed: 7, amount: 1.0 });
        assert_eq!(one, two);
        assert!(r1.changed.len() > 5, "{}", r1.changed.len());

        let mut locked = base.clone();
        locked.set_locked(own(a, "radius"), true);
        let r3 = roll(&mut locked, &reg, &Scope::Scene, Roll { seed: 7, amount: 1.0 });
        assert_eq!(r3.changed.len(), r1.changed.len() - 1);
        for c in &r3.changed {
            assert_eq!(locked.value_at(&reg, c), one.value_at(&reg, c), "{c:?}");
        }
        assert_eq!(locked.value_at(&reg, &own(a, "radius")), base.value_at(&reg, &own(a, "radius")));
        assert_ne!(fresh_seed(), 0);
        assert_eq!(salt(&own(a, "radius")), salt(&own(a, "radius")));
        assert_ne!(salt(&own(a, "radius")), salt(&own(a, "count")));
    }

    #[test]
    fn dice_fit_the_starting_shot() {
        let reg = crate::registry();
        let mut scene = crate::new_studio_scene(&reg);
        let cam = crate::camera_of(&scene).unwrap();
        scene.things.get_mut(&cam).unwrap().params.set("zoom", Value::Number(2.0));
        for seed in 0..50 {
            let mut rolled = scene.clone();
            roll(&mut rolled, &reg, &Scope::Scene, Roll { seed, amount: 1.0 });
            let Some(Value::Number(radius)) = rolled.things[&ThingId(1)].params.get("radius").cloned() else {
                continue;
            };
            // The dice roll 150 to 500 for a shot at Zoom 1; at Zoom 2 the shot is half as wide.
            assert!(radius <= 250.0 + 1e-3, "seed {seed}: radius {radius}");
        }
    }
}
