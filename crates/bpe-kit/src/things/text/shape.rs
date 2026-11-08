//! A Text's letters as a shape other things are made of (Text part 4): a group of balls whose Word
//! is made of a Text thing, and a Particles thing made of one. Read from the Text's saved settings
//! (its keys and pulses aren't followed), its words with every value in braces left out, laid out
//! in its font and placed on its path as at 0 s (Along a shape reads as Straight), in its own frame
//! (middle at 0), not turned by its Rotation.

use bpe_core::{Scene, Settings, Target, ThingId};
use glam::Vec2;

use super::{KIND, TextPath, TextSettings};
use crate::text::live::{self, Piece};
use crate::text::{PathAt, arrange, fonts, layout, placed};

/// The first Text `target` names in `tree` (not `me`).
pub fn text_named(tree: &Scene, target: &Target, me: ThingId) -> Option<ThingId> {
    let is_text = |id: &ThingId| *id != me && tree.things.get(id).is_some_and(|t| t.kind == KIND);
    match target {
        Target::Me => None,
        Target::Thing(id) => is_text(id).then_some(*id),
        Target::Tag(tag) => tree.things.values().filter(|t| t.tags.contains(tag)).map(|t| t.id).find(is_text),
        Target::Kind(k) if k.as_str() != KIND => None,
        Target::Kind(_) | Target::Anything => tree.things.keys().copied().find(is_text),
    }
}

/// The words of a Text with every value in braces left out.
fn plain_words(words: &str) -> String {
    if !words.contains('{') {
        return words.to_string();
    }
    live::parse(words)
        .into_iter()
        .filter_map(|p| match p {
            Piece::Text(t) => Some(t),
            Piece::Value(..) => None,
        })
        .collect()
}

/// Every contour of Text `id`'s letters in `tree` (closed, in its own frame); empty for anything
/// that isn't a Text or has no letters.
pub fn letters_of(tree: &Scene, id: ThingId) -> Vec<Vec<Vec2>> {
    let Some(thing) = tree.things.get(&id).filter(|t| t.kind == KIND) else { return Vec::new() };
    let s = TextSettings::from_params(&thing.params);
    let words = plain_words(&s.words);
    let found = fonts::find(&tree.fonts, &s.font, s.weight);
    let laid = layout(found.font(), &s.layout_spec_of(&words));
    let path = match s.path {
        TextPath::Straight | TextPath::AlongAShape => PathAt::Straight,
        TextPath::RoundACircle => {
            PathAt::Circle { radius: s.radius, start: s.start_angle.to_radians(), inside: s.inside }
        }
        TextPath::AlongAWave => PathAt::Wave {
            height: s.wave_height,
            wavelength: s.wavelength,
            shift: s.wave_shift,
            follow: s.follow_slope,
        },
    };
    let at = arrange(&laid, &path);
    laid.letters.iter().zip(at).flat_map(|(l, p)| placed(l, p)).filter(|c| c.len() >= 3).collect()
}

/// A hash of everything [`letters_of`] reads of Text `id` in `tree`: its saved settings and its
/// scene font. 0 when there is no such Text.
pub fn letters_hash(tree: &Scene, id: ThingId) -> u64 {
    let Some(thing) = tree.things.get(&id).filter(|t| t.kind == KIND) else { return 0 };
    let mut h = 0xcbf2_9ce4_8422_2325u64 ^ id.0 as u64;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            h = (h ^ *b as u64).wrapping_mul(0x100_0000_01b3);
        }
    };
    eat(format!("{:?}", thing.params).as_bytes());
    let s = TextSettings::from_params(&thing.params);
    if let Some(f) = tree.fonts.get(&s.font) {
        eat(&(f.bytes.len() as u64).to_le_bytes());
        eat(&f.bytes[..f.bytes.len().min(64)]);
        eat(&f.bytes[f.bytes.len().saturating_sub(64)..]);
    }
    h.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::Value;

    #[test]
    fn a_texts_letters_are_its_words_contours_round_its_middle() {
        let reg = crate::registry();
        let mut scene = Scene::empty(&reg);
        let id = crate::add_thing(&mut scene, &reg, KIND, ThingId::ROOT).unwrap();
        scene.things.get_mut(&id).unwrap().params.set("words", Value::Text("O{time}".into()));
        let o = letters_of(&scene, id);
        assert_eq!(o.len(), 2, "an O's outside and its hole; the value left out");
        let (lo, hi) = o.iter().flatten().fold((Vec2::MAX, Vec2::MIN), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        assert!((lo + hi).length() < 0.2 * (hi - lo).length(), "about its middle: {lo} {hi}");
        assert_eq!(text_named(&scene, &Target::Anything, ThingId::ROOT), Some(id));
        assert_eq!(text_named(&scene, &Target::Anything, id), None, "not itself");
        let before = letters_hash(&scene, id);
        scene.things.get_mut(&id).unwrap().params.set("words", Value::Text("OK".into()));
        assert_ne!(letters_hash(&scene, id), before);
        assert!(letters_of(&scene, ThingId::ROOT).is_empty());
    }
}
