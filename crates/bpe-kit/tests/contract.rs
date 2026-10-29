//! The spec's contract for the dice: on every scene that ships, a roll stays inside every range,
//! leaves what it must alone, and gives a scene that still resolves and saves.

use bpe_core::{Address, Owner, SlotLink, Value};
use bpe_kit::random::{Roll, Scope, roll};
use bpe_kit::{registry, scenes};

/// Every setting address a scene holds a value for, with its definition.
fn every_address(scene: &bpe_core::Scene, reg: &bpe_core::Registry) -> Vec<Address> {
    let mut out = Vec::new();
    for (&id, thing) in &scene.things {
        if let Some(kind) = reg.thing(&thing.kind) {
            out.extend(kind.settings().iter().map(|d| Address {
                owner: Owner::Thing(id),
                slot: None,
                key: d.key.into(),
            }));
        }
        for (&slot, link) in &thing.slots {
            if let SlotLink::Linked { overrides, .. } = link {
                out.extend(overrides.iter().map(|(k, _)| Address {
                    owner: Owner::Thing(id),
                    slot: Some(slot),
                    key: k.clone(),
                }));
            }
        }
    }
    for (&m, module) in &scene.library.modules {
        if let Some(kind) = reg.module(&module.kind) {
            out.extend(kind.settings().iter().map(|d| Address {
                owner: Owner::Module(m),
                slot: None,
                key: d.key.into(),
            }));
        }
    }
    out
}

#[test]
fn randomize_stays_within_ranges_on_every_scene() {
    let reg = registry();
    for (name, base) in scenes::all(&reg) {
        for seed in 1..=3 {
            for amount in [1.0, 0.3] {
                let mut scene = base.clone();
                let rolled = roll(&mut scene, &reg, &Scope::Scene, Roll { seed, amount });
                assert!(!rolled.changed.is_empty(), "{name}: seed {seed} changed nothing");
                for a in every_address(&scene, &reg) {
                    let def = scene.def_at(&reg, &a).unwrap_or_else(|| panic!("{name}: {a:?} has no definition"));
                    let value = scene.value_at(&reg, &a).unwrap();
                    let number = match value {
                        Value::Number(x) => Some(x),
                        Value::Int(n) => Some(n as f64),
                        _ => None,
                    };
                    if let (Some(x), Some((min, max))) = (number, def.range) {
                        assert!((min..=max).contains(&x), "{name}: {a:?} = {x} outside {min}..{max}");
                    }
                    // A full roll lands in the dice range; a partial one between where the setting
                    // was and the dice range.
                    if let (Some(x), Some((lo, hi)), true) = (number, def.dice, rolled.changed.contains(&a)) {
                        let was = match base.value_at(&reg, &a) {
                            Some(Value::Number(x)) => x,
                            Some(Value::Int(n)) => n as f64,
                            _ => lo,
                        };
                        let (lo, hi) = if amount < 1.0 { (lo.min(was), hi.max(was)) } else { (lo, hi) };
                        let slack = 1e-9 * hi.abs().max(1.0);
                        assert!(
                            (lo - slack..=hi + slack).contains(&x),
                            "{name}: {a:?} = {x} outside {lo}..{hi} at amount {amount}"
                        );
                    }
                    if matches!(def.ty, bpe_core::ValueType::Target | bpe_core::ValueType::Text) {
                        assert_eq!(base.value_at(&reg, &a), Some(value), "{name}: {a:?} must not roll");
                    }
                }
                let resolved = bpe_kit::resolve_scene(&scene, &reg);
                assert!(resolved.problems.is_empty(), "{name}: {:?}", resolved.problems);
                let back = bpe_core::load(&bpe_core::save(&scene), &reg).unwrap();
                assert_eq!(back, scene, "{name}: save and load");
            }
        }
    }
}

#[test]
fn locks_hold_on_every_scene() {
    let reg = registry();
    for (name, base) in scenes::all(&reg) {
        let mut scene = base.clone();
        for a in every_address(&scene, &reg) {
            scene.set_locked(a, true);
        }
        let rolled = roll(&mut scene, &reg, &Scope::Scene, Roll { seed: 9, amount: 1.0 });
        assert!(rolled.changed.is_empty(), "{name}: {:?}", rolled.changed);
        scene.locks.clear();
        assert_eq!(scene, base, "{name}: nothing moved");
    }
}
