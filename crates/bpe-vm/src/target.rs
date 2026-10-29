//! Targets written as text, so a graph's `Param::Text` can hold one.
//!
//! The grammar is `me`, `anything`, `thing:<id>`, `tag:<name>` or `kind:<id>`. Anything else
//! reads as `anything`, so an old or mistyped target never breaks a graph.

use bpe_core::{Target, ThingId};

pub fn target_from_text(text: &str) -> Target {
    let text = text.trim();
    if text == "me" {
        return Target::Me;
    }
    if let Some(id) = text.strip_prefix("thing:") {
        return id.parse().map(|id| Target::Thing(ThingId(id))).unwrap_or(Target::Anything);
    }
    if let Some(tag) = text.strip_prefix("tag:") {
        return Target::Tag(tag.to_owned());
    }
    if let Some(kind) = text.strip_prefix("kind:") {
        return Target::Kind(kind.to_owned());
    }
    Target::Anything
}

pub fn target_to_text(target: &Target) -> String {
    match target {
        Target::Me => "me".into(),
        Target::Anything => "anything".into(),
        Target::Thing(id) => format!("thing:{}", id.0),
        Target::Tag(tag) => format!("tag:{tag}"),
        Target::Kind(kind) => format!("kind:{kind}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::ThingId;

    #[test]
    fn targets_round_trip_through_text() {
        for t in [
            Target::Me,
            Target::Anything,
            Target::Thing(ThingId(7)),
            Target::Tag("red".into()),
            Target::Kind("balls".into()),
        ] {
            assert_eq!(target_from_text(&target_to_text(&t)), t);
        }
        assert_eq!(target_from_text("thing:x"), Target::Anything);
        assert_eq!(target_from_text(""), Target::Anything);
        assert_eq!(target_to_text(&Target::Thing(ThingId(7))), "thing:7");
    }
}
