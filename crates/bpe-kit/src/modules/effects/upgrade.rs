//! The test scenes are still written with the scene's one Shader card of a built-in effect
//! (`shader.<file>`), as scenes were before the Effects stack: it becomes an Effects card holding
//! that effect as its one layer, its Blend becoming the layer's Mix. Only tests build it.

use bpe_core::{Layer, LayerId, Params, Slot, layered_key};

use super::{EFFECTS, MIX, builtin_of, id_of};

/// The old one-effect card kind's prefix.
const OLD: &str = "shader.";

/// The Effects card an old one-effect card of `kind` with `params` becomes; `None` for a kind
/// that isn't an old effect card.
pub fn stack_of(kind: &str, params: &Params) -> Option<Slot> {
    let file = kind.strip_prefix(OLD)?;
    let builtin = builtin_of(&format!("picture.{file}"))?;
    let id = LayerId(0);
    let mut card = Slot::new(EFFECTS);
    card.layers.push(Layer {
        id,
        kind: id_of(builtin).into(),
        name: builtin.header().name.clone(),
        ..Layer::default()
    });
    for (k, v) in params.iter() {
        let key = if k == "blend" { MIX } else { k.as_str() };
        card.params.set(&layered_key(id, key), v.clone());
    }
    Some(card)
}

/// Whether `kind` is an old one-effect card.
pub fn is_old(kind: &str) -> bool {
    kind.starts_with(OLD)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::Value;

    #[test]
    fn an_old_kaleidoscope_card_becomes_a_one_layer_stack() {
        let mut old = Params::default();
        old.set("blend", Value::Number(0.5));
        old.set("slices", Value::Int(9));
        let card = &stack_of("shader.kaleidoscope", &old).unwrap();
        assert_eq!(card.kind, EFFECTS);
        assert_eq!(card.layers.len(), 1);
        assert_eq!(
            (card.layers[0].kind.as_str(), card.layers[0].name.as_str()),
            ("picture.kaleidoscope", "Kaleidoscope")
        );
        let id = card.layers[0].id;
        assert_eq!(card.params.get(&layered_key(id, MIX)), Some(&Value::Number(0.5)));
        assert_eq!(card.params.get(&layered_key(id, "slices")), Some(&Value::Int(9)));
        assert!(stack_of("picture.kaleidoscope", &old).is_none() && !is_old(EFFECTS));
    }
}
