//! The dice: roll a scene, a thing, one card or one module from a seed, leaving locked, keyed
//! and never-random settings alone.

use crate::rng::Rng;
use bpe_core::{Address, Color, ModuleId, Owner, Registry, Scene, SettingDef, SlotId, SlotLink, ThingId, Value};

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
    for a in addresses(scene, reg, scope) {
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
    rolled
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
            if names.is_empty() || dice.unit() >= amount {
                return None;
            }
            Some(Value::Choice(names[dice.next_u32() as usize % names.len()].into()))
        }
        Value::Color(c) => {
            let rolled = if amount >= 1.0 { palette(dice, 1)[0] } else { c.shift_hue(turn(dice, amount)) };
            Some(Value::Color(Color { a: c.a, ..rolled }))
        }
        Value::Colors(list) => {
            if list.is_empty() {
                return None;
            }
            if amount >= 1.0 {
                return Some(Value::Colors(palette(dice, list.len())));
            }
            let t = turn(dice, amount);
            Some(Value::Colors(list.iter().map(|c| c.shift_hue(t)).collect()))
        }
        Value::Vec2([x, y]) => Some(Value::Vec2(roll_vec2([*x, *y], def.dice_range(), def.range, dice, amount))),
        Value::Text(_) | Value::Target(_) => None,
    }
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
    use bpe_core::{Owner, SlotId, ThingId, Value, ValueType};

    fn def(reg: &bpe_core::Registry, kind: &str, key: &str) -> SettingDef {
        let defs = reg.thing(kind).map(|k| k.settings()).or_else(|| reg.module(kind).map(|k| k.settings()));
        defs.unwrap_or_else(|| panic!("no kind {kind}")).iter().find(|d| d.key == key).unwrap().clone()
    }

    fn rolls(def: &SettingDef, current: &Value, amount: f32, n: u32) -> Vec<Value> {
        (0..n).filter_map(|i| roll_value(def, current, &mut Rng::new(i, 1), amount)).collect()
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
        let color = def(&reg, "look.classic", "color");
        let red = Color::rgb(255, 0, 0);
        let colours = rolls(&color, &Value::Color(red), 1.0, 20);
        assert_eq!(colours.len(), 20);
        assert!(colours.iter().any(|v| *v != Value::Color(red)), "a new colour comes up");
        let gradient = def(&reg, "look.classic", "gradient");
        let three = vec![red, Color::rgb(0, 255, 0), Color::rgb(0, 0, 255)];
        for v in rolls(&gradient, &Value::Colors(three.clone()), 1.0, 20) {
            match v {
                Value::Colors(list) => assert_eq!(list.len(), 3),
                other => panic!("{other:?}"),
            }
        }
        assert!(rolls(&gradient, &Value::Colors(Vec::new()), 1.0, 5).is_empty(), "no colours stay no colours");
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
        let neon_colour = Address { owner: Owner::Module(neon), slot: None, key: "color".into() };
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
}
