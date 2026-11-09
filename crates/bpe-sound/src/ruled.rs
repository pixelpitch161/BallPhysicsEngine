//! What rules change on the Sound and Mixer cards (W13). The sound engine's own physics runs the
//! scene's rules, so its world holds every Change setting a rule made; each block a number knob is
//! as set, then as its timeline key has it, then folded with those changes at the block's time,
//! with the same maths the physics uses. Nothing here allocates: the keys are made when the patch
//! is built.

use bpe_core::{SlotId, ThingId, Value};
use bpe_runtime::{Change, ChangeKey, RulesState};

/// `base` with the changes in `list` on top at `time` (a change made on a whole-number setting
/// read as a number, as the knobs are).
pub fn number(list: &[Change], base: f32, time: f64) -> f32 {
    let mut v = Value::Number(base as f64);
    for c in list {
        v = match c.value {
            Value::Int(x) => Change { value: Value::Number(x as f64), ..c.clone() }.on(&v, time),
            _ => c.on(&v, time),
        };
    }
    match v {
        Value::Number(x) if x.is_finite() => x as f32,
        _ => base,
    }
}

/// `base` with the rules' changes on `key` (whole thing) at `time`.
pub fn at(rules: Option<&RulesState>, key: &ChangeKey, base: f32, time: f64) -> f32 {
    match rules.map(|r| r.changes_on(key)) {
        Some(list) if !list.is_empty() => number(list, base, time),
        _ => base,
    }
}

/// The key of setting `key` of `thing`'s card in `slot`, for the whole thing.
pub fn key(thing: ThingId, slot: SlotId, key: &str) -> ChangeKey {
    ChangeKey { thing, slot: Some(slot), row: None, key: key.to_owned() }
}

/// The key of setting `key` of the scene's Mixer card.
pub fn mixer_key(key: &str) -> ChangeKey {
    self::key(ThingId::ROOT, SlotId::Mixer, key)
}

/// The settings of `thing`'s Sound card a rule's change is heard on: every knob its voice reads
/// once a block, and its Sends (what the editor's Change setting menu offers). Empty when its
/// sound is off.
pub fn voice_rule_keys(scene: &bpe_core::Scene, resolved: &bpe_core::Resolved, thing: ThingId) -> Vec<String> {
    let Some(spec) = crate::patch::spec_of(scene, resolved, thing) else { return Vec::new() };
    let mut keys: Vec<String> = spec.knob_keys.into_iter().map(|(k, _)| k).collect();
    keys.extend((1..=bpe_kit::sound::BUSES).map(|n| format!("out_send{n}")));
    keys.sort();
    keys.dedup();
    keys
}

/// The settings of the scene's Mixer card a rule's change is heard on: each bus's Fader, Pan and
/// Sends, the Master's Input and Output, and every number of the bus effects that takes keys.
pub fn mixer_rule_keys(scene: &bpe_core::Scene, registry: &bpe_core::Registry) -> Vec<String> {
    use bpe_kit::sound::MixerSettings;
    let mut keys = vec!["master_input".to_owned(), "master_output".to_owned()];
    for n in 1..=bpe_kit::sound::BUSES as u8 {
        for k in ["fader", "pan", "send1", "send2", "send3"] {
            keys.push(MixerSettings::bus_key(n, k));
        }
    }
    if let Some(card) = scene.card(ThingId::ROOT, SlotId::Mixer) {
        for layer in card.layers.iter().filter(|l| bpe_kit::sound::fx::is_fx(&l.kind)) {
            let Some(kind) = registry.layer(&layer.kind) else { continue };
            for d in kind.settings() {
                if d.flags.keyframe && matches!(d.ty, bpe_core::ValueType::Number | bpe_core::ValueType::Int) {
                    keys.push(bpe_core::layered_key(layer.id, d.key));
                }
            }
        }
    }
    keys.sort();
    keys.dedup();
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::Ease;
    use bpe_runtime::How;

    fn change(how: How, value: Value) -> Change {
        Change { how, value, start: 1.0, glide: 0.0, ease: Ease::Linear, lasts: None, range: None }
    }

    #[test]
    fn numbers_fold_as_the_physics_folds_them() {
        let set = change(How::Set, Value::Number(-60.0));
        assert_eq!(number(std::slice::from_ref(&set), 0.0, 2.0), -60.0);
        assert_eq!(number(std::slice::from_ref(&set), 0.0, 0.5), 0.0, "not before it starts");
        let add_int = change(How::Add, Value::Int(3));
        assert_eq!(number(&[add_int], 2.0, 2.0), 5.0, "a whole-number change adds");
        let times = change(How::Multiply, Value::Number(0.5));
        assert_eq!(number(&[times], 1000.0, 2.0), 500.0);
    }

    #[test]
    fn without_rules_or_changes_the_value_stays() {
        let k = mixer_key("bus1_fader");
        assert_eq!(at(None, &k, -6.0, 3.0), -6.0);
    }
}
