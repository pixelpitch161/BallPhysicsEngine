use super::*;
use crate::{load, save, testkit};

fn scene() -> (Registry, Scene) {
    let r = testkit::registry();
    let s = Scene::empty(&r);
    (r, s)
}

#[test]
fn a_new_scene_has_no_rules_and_saves_without_them() {
    let (_, s) = scene();
    assert!(s.rules.is_empty());
    assert!(!save(&s).contains("rules"), "{}", save(&s));
}

#[test]
fn rules_round_trip_through_saving() {
    let (r, mut s) = scene();
    let b = testkit::push(&mut s, "balls", ThingId::ROOT);
    let rule = s.add_rule("Bricks");
    let when = s.add_block(&r, rule, "when.test", None).unwrap();
    let iff = s.add_block(&r, rule, "if.test", Some(when)).unwrap();
    let act = s.add_block(&r, rule, "do.test", Some(iff)).unwrap();
    s.set_block_param(&r, rule, act, "amount", Value::Number(50.0)).unwrap();
    s.set_block_param(&r, rule, act, "who", Value::Who(Who::Thing { thing: b, ball: Some(1) })).unwrap();
    s.set_block_param(&r, rule, act, "amount~from", Value::Text("strength".into())).unwrap();
    s.set_block_param(&r, rule, act, "amount~times", Value::Number(0.5)).unwrap();
    s.set_rule_enabled(rule, false).unwrap();
    let text = save(&s);
    assert!(text.contains("format: 12"));
    let back = load(&text, &r).unwrap();
    assert_eq!(back, s);
    assert_eq!(back.rules.list[0].blocks[2].inputs, vec![iff]);
}

#[test]
fn load_mends_rules() {
    let (r, mut s) = scene();
    let rule = s.add_rule("A");
    let when = s.add_block(&r, rule, "when.test", None).unwrap();
    let act = s.add_block(&r, rule, "do.test", Some(when)).unwrap();
    {
        let rl = s.rules.rule_mut(rule).unwrap();
        let b = rl.block_mut(act).unwrap();
        b.inputs.push(BlockId(40));
        b.params.set("amount", Value::Number(500.0));
        b.params.set("wobble", Value::Number(1.0));
        b.params.set("amount~times", Value::Text("no".into()));
        rl.blocks.push(Block { id: when, kind: "do.test".into(), ..Block::default() });
        rl.blocks.push(Block { id: BlockId(9), kind: "do.mystery".into(), params: Params::default(), inputs: vec![] });
        rl.next_block = 0;
        rl.blocks[3].params.set("anything", Value::Bool(true));
    }
    s.rules.next_rule = 0;
    let back = load(&save(&s), &r).unwrap();
    let rl = &back.rules.list[0];
    assert_eq!(rl.blocks.len(), 3, "a repeated id goes");
    let b = rl.block(act).unwrap();
    assert_eq!(b.inputs, vec![when], "a link from a block that isn't there goes");
    assert_eq!(b.params.get("amount"), Some(&Value::Number(100.0)), "clamped");
    assert_eq!(b.params.get("wobble"), None);
    assert_eq!(b.params.get("amount~times"), None, "wrong type");
    assert_eq!(rl.block(BlockId(9)).unwrap().params.get("anything"), Some(&Value::Bool(true)), "unknown kinds keep");
    assert_eq!(rl.next_block, 10);
    assert_eq!(back.rules.next_rule, 1);
}

#[test]
fn ids_are_never_given_out_twice() {
    let (r, mut s) = scene();
    let a = s.add_rule("A");
    let b = s.add_rule("B");
    s.remove_rule(b).unwrap();
    let c = s.add_rule("C");
    assert_ne!(b, c);
    assert_eq!(c, RuleId(2));
    let w = s.add_block(&r, a, "when.test", None).unwrap();
    let d = s.add_block(&r, a, "do.test", Some(w)).unwrap();
    s.remove_block(&r, a, d).unwrap();
    let e = s.add_block(&r, a, "do.test", Some(w)).unwrap();
    assert_ne!(d, e);
}

#[test]
fn rules_move_duplicate_rename_and_switch() {
    let (r, mut s) = scene();
    let a = s.add_rule("A");
    let b = s.add_rule("B");
    s.add_block(&r, a, "when.test", None).unwrap();
    s.move_rule(b, 0).unwrap();
    assert_eq!(s.rules.list.iter().map(|r| r.id).collect::<Vec<_>>(), [b, a]);
    let c = s.duplicate_rule(a).unwrap();
    assert_eq!(s.rules.list[2].id, c);
    assert_eq!(s.rules.list[2].name, "A copy");
    assert_eq!(s.rules.list[2].blocks, s.rules.list[1].blocks);
    s.rename_rule(c, "Copy").unwrap();
    s.set_rule_enabled(c, false).unwrap();
    assert!(!s.rules.rule(c).unwrap().enabled && s.rules.rule(c).unwrap().name == "Copy");
    assert_eq!(s.rename_rule(RuleId(99), "x"), Err(EditError::NoRule(RuleId(99))));
}

#[test]
fn blocks_add_after_and_whens_stack() {
    let (r, mut s) = scene();
    let a = s.add_rule("A");
    let w1 = s.add_block(&r, a, "when.test", None).unwrap();
    let w2 = s.add_when(&r, a, "when.test").unwrap();
    assert_eq!(s.add_when(&r, a, "do.test"), Err(EditError::WrongType));
    let w3 = s.add_block(&r, a, "when.test", Some(w1)).unwrap();
    let l = s.add_block(&r, a, "logic.test", Some(w1)).unwrap();
    s.link(&r, a, w2, l).unwrap();
    let rule = s.rules.rule(a).unwrap();
    assert!(rule.block(w3).unwrap().inputs.is_empty(), "a When ignores after");
    assert_eq!(rule.block(l).unwrap().inputs, vec![w1, w2]);
    assert_eq!(s.add_block(&r, a, "do.nope", None), Err(EditError::UnknownKind("do.nope".into())));
    assert_eq!(s.add_block(&r, a, "do.test", Some(BlockId(77))), Err(EditError::NoBlock(BlockId(77))));
}

#[test]
fn links_replace_add_and_refuse_loops() {
    let (r, mut s) = scene();
    let a = s.add_rule("A");
    let w = s.add_block(&r, a, "when.test", None).unwrap();
    let i1 = s.add_block(&r, a, "if.test", Some(w)).unwrap();
    let i2 = s.add_block(&r, a, "if.test", Some(i1)).unwrap();
    // Onto an If: replaces.
    s.link(&r, a, w, i2).unwrap();
    assert_eq!(s.rules.rule(a).unwrap().block(i2).unwrap().inputs, vec![w]);
    // i2 -> i1 is fine now; then i1 -> i2 would close a circle.
    s.link(&r, a, i2, i1).unwrap();
    assert_eq!(s.link(&r, a, i1, i2), Err(EditError::RuleLoop));
    assert_eq!(s.link(&r, a, i1, i1), Err(EditError::RuleLoop));
    assert_eq!(s.link(&r, a, i1, w), Err(EditError::WrongType), "a When takes no input");
    s.unlink(a, i2, i1).unwrap();
    assert!(s.rules.rule(a).unwrap().block(i1).unwrap().inputs.is_empty());
}

#[test]
fn removing_a_block_closes_the_links_up() {
    let (r, mut s) = scene();
    let a = s.add_rule("A");
    let w = s.add_block(&r, a, "when.test", None).unwrap();
    let i = s.add_block(&r, a, "if.test", Some(w)).unwrap();
    let d = s.add_block(&r, a, "do.test", Some(i)).unwrap();
    let w2 = s.add_when(&r, a, "when.test").unwrap();
    let l = s.add_block(&r, a, "logic.test", Some(i)).unwrap();
    s.link(&r, a, w2, l).unwrap();
    s.remove_block(&r, a, i).unwrap();
    let rule = s.rules.rule(a).unwrap();
    assert_eq!(rule.block(d).unwrap().inputs, vec![w], "the Do takes the If's input");
    assert_eq!(rule.block(l).unwrap().inputs, vec![w, w2], "the Logic takes it in the If's place");
    assert!(rule.problems(&r).is_empty(), "{:?}", rule.problems(&r));
}

#[test]
fn problems_name_what_is_wrong() {
    let (r, mut s) = scene();
    let b = testkit::push(&mut s, "balls", ThingId::ROOT);
    let a = s.add_rule("A");
    let w = s.add_block(&r, a, "when.test", None).unwrap();
    let d = s.add_block(&r, a, "do.test", None).unwrap();
    let l = s.add_block(&r, a, "logic.test", Some(w)).unwrap();
    s.set_block_param(&r, a, w, "who", Value::Who(Who::Thing { thing: b, ball: None })).unwrap();
    let kinds = |s: &Scene| s.rule_problems(&r, a).into_iter().map(|p| (p.block, p.what)).collect::<Vec<_>>();
    assert_eq!(kinds(&s), [(Some(d), RuleProblemKind::NeedsOneInput), (Some(l), RuleProblemKind::LogicNeedsTwo)],);
    s.remove_thing(b).unwrap();
    assert!(kinds(&s).contains(&(Some(w), RuleProblemKind::MissingThing(b))));
    // A circle, a When with an input and an unknown kind, made by hand.
    let rl = s.rules.rule_mut(a).unwrap();
    rl.block_mut(d).unwrap().inputs = vec![l];
    rl.block_mut(l).unwrap().inputs = vec![w, d];
    rl.block_mut(w).unwrap().inputs = vec![BlockId(50)];
    rl.blocks.push(Block { id: BlockId(60), kind: "when.gone".into(), ..Block::default() });
    let all = kinds(&s);
    assert!(all.contains(&(None, RuleProblemKind::Loop)), "{all:?}");
    assert!(all.contains(&(Some(w), RuleProblemKind::WhenWithInputs)));
    assert!(all.contains(&(Some(w), RuleProblemKind::MissingInput(BlockId(50)))));
    assert!(all.contains(&(Some(BlockId(60)), RuleProblemKind::UnknownKind("when.gone".into()))));
    assert!(s.rule_problems(&r, a).iter().all(|p| !p.words().is_empty()));
}

#[test]
fn flow_order_puts_inputs_first() {
    let (r, mut s) = scene();
    let a = s.add_rule("A");
    let w = s.add_block(&r, a, "when.test", None).unwrap();
    let d = s.add_block(&r, a, "do.test", None).unwrap();
    let i = s.add_block(&r, a, "if.test", Some(w)).unwrap();
    s.link(&r, a, i, d).unwrap();
    assert_eq!(s.rules.rule(a).unwrap().in_flow_order().unwrap(), [w, i, d]);
    assert_eq!(s.rules.rule(a).unwrap().outputs(w), [i]);
}

#[test]
fn copy_and_paste_give_fresh_ids() {
    let (r, mut s) = scene();
    let a = s.add_rule("A");
    let w = s.add_block(&r, a, "when.test", None).unwrap();
    let d = s.add_block(&r, a, "do.test", Some(w)).unwrap();
    s.set_block_param(&r, a, d, "amount", Value::Number(7.0)).unwrap();
    let copied = s.copy_rule(a).unwrap();
    let b = s.paste_rule(&copied);
    assert_ne!(a, b);
    assert_eq!(s.rules.rule(b).unwrap().blocks, s.rules.rule(a).unwrap().blocks);
    assert_eq!(s.rules.rule(b).unwrap().name, format!("{} copy", s.rules.rule(a).unwrap().name));
    let block = s.copy_block(a, d).unwrap();
    let e = s.paste_block(&r, a, &block, Some(w)).unwrap();
    let rule = s.rules.rule(a).unwrap();
    assert_eq!(rule.block(e).unwrap().params, rule.block(d).unwrap().params);
    assert_eq!(rule.block(e).unwrap().inputs, vec![w]);
}

#[test]
fn block_settings_are_typed_clamped_and_sparse() {
    let (r, mut s) = scene();
    let a = s.add_rule("A");
    let d = s.add_block(&r, a, "do.test", None).unwrap();
    s.set_block_param(&r, a, d, "amount", Value::Number(400.0)).unwrap();
    let block = |s: &Scene| s.rules.rule(a).unwrap().block(d).unwrap().params.clone();
    assert_eq!(block(&s).get("amount"), Some(&Value::Number(100.0)));
    s.set_block_param(&r, a, d, "amount", Value::Number(1.0)).unwrap();
    assert!(block(&s).is_empty(), "the default isn't stored");
    assert_eq!(s.set_block_param(&r, a, d, "amount", Value::Bool(true)), Err(EditError::WrongType));
    assert_eq!(s.set_block_param(&r, a, d, "nope", Value::Number(1.0)), Err(EditError::UnknownSetting("nope".into())));
    // A who has no from-parts; a number does.
    assert!(s.block_def_at(&r, a, d, "who~from").is_none());
    assert_eq!(s.block_def_at(&r, a, d, "amount~from").map(|d| d.ty), Some(ValueType::Text));
    assert_eq!(s.block_def_at(&r, a, d, "amount~plus").map(|d| d.default.clone()), Some(Value::Number(0.0)));
    assert_eq!(split_from("speed~times"), Some(("speed", FromPart::Times)));
    assert!(is_from_key("a~plus") && !is_from_key("a~to"));
}

#[test]
fn block_ids_share_the_plugin_id_space() {
    let mut r = testkit::registry();
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        r.add_block(testkit::TestBlockKind("balls", BlockSection::Do));
    }));
    assert!(caught.is_err());
    assert_eq!(r.blocks().count(), 4);
}
