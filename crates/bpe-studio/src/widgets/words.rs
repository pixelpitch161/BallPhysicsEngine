//! Settings in words: values, targets, whos, paths and units, for menus, tooltips and summaries.

use super::*;

/// A target in plain words, as a rule's who reads: "Anything", "Itself", "Ball", "Tagged walls",
/// "Any Shape".
pub fn target_words(t: &Target, choices: &TargetChoices) -> String {
    match t {
        Target::Anything => "Anything".to_owned(),
        Target::Me => "Itself".to_owned(),
        Target::Thing(id) => match choices.things.iter().find(|(t, _)| t == id) {
            Some((_, name)) => name.clone(),
            None => format!("A missing thing (id {})", id.0),
        },
        Target::Tag(tag) => format!("Tagged {tag}"),
        Target::Kind(kind) => {
            let name = choices.kinds.iter().find(|(id, _)| id == kind).map(|(_, n)| n.as_str()).unwrap_or(kind);
            format!("Any {name}")
        }
    }
}

/// A value in a few words for the menu's Paste Value: a number with its unit, a colour as hex,
/// a choice as its words, On or Off, a gradient's colours.
pub fn value_words(value: &Value, unit: &str, choices: &TargetChoices) -> String {
    let number = |x: f64| {
        let text = format!("{x:.3}");
        let text = text.trim_end_matches('0').trim_end_matches('.');
        if text == "-0" { "0".to_owned() } else { text.to_owned() }
    };
    let hex = |c: &Color| {
        if c.a == 255 {
            format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", c.r, c.g, c.b, c.a)
        }
    };
    match value {
        Value::Number(x) => format!("{}{}", number(*x), suffix(unit)),
        Value::Int(i) => format!("{i}{}", suffix(unit)),
        Value::Bool(b) => if *b { "On" } else { "Off" }.to_owned(),
        Value::Vec2(v) if unit == VELOCITY => {
            let (speed, angle) = speed_angle(*v);
            format!("{} px/s at {}°", number(speed.round()), number(angle.round()))
        }
        Value::Vec2([x, y]) => format!("{}, {}{}", number(f64::from(*x)), number(f64::from(*y)), suffix(unit)),
        Value::Color(c) => hex(c),
        Value::Gradient(g) => match g.stops.as_slice() {
            [one] => hex(&one.color),
            stops => format!("{} colours", stops.len()),
        },
        Value::Choice(c) => pretty(c),
        Value::Text(t) if t.chars().count() > 24 => format!("\"{}…\"", t.chars().take(23).collect::<String>()),
        Value::Text(t) => format!("\"{t}\""),
        Value::Target(t) => target_words(t, choices),
        Value::Strokes(s) => strokes_words(s),
        Value::Points(ps) if ps.len() == 1 => "1 point".to_owned(),
        Value::Points(ps) => format!("{} points", ps.len()),
        Value::Path(p) => path_words(p),
        Value::Routes(r) if r.len() == 1 => "1 route".to_owned(),
        Value::Routes(r) => format!("{} routes", r.len()),
        Value::Slots(n) if n.len() == 1 => "1 slot".to_owned(),
        Value::Slots(n) => format!("{} slots", n.len()),
        Value::Curve(c) => format!("{} points", c.len()),
        Value::LightScenes(l) => scenes_words(l.len()),
        Value::Who(w) => who_words(w, choices),
    }
}

/// A rule block's Who in a few words, naming things and kinds as the scene does: "That ball",
/// "Bricks", "Balls, ball 3", "Tagged breakable", "Any Shape".
pub fn who_words(w: &bpe_core::Who, choices: &TargetChoices) -> String {
    use bpe_core::Who;
    let thing = |id: &ThingId| match choices.things.iter().find(|(t, _)| t == id) {
        Some((_, name)) => name.clone(),
        None => format!("A missing thing (id {})", id.0),
    };
    match w {
        Who::That => "That ball".to_owned(),
        Who::Other => "The thing it hit".to_owned(),
        Who::Group => "That ball's group".to_owned(),
        Who::Thing { thing: id, ball: Some(n) } if choices.letter_things.contains(id) => {
            format!("{}, letter {n}", thing(id))
        }
        Who::Thing { thing: id, ball: Some(n) } => format!("{}, ball {n}", thing(id)),
        Who::Thing { thing: id, ball: None } => thing(id),
        Who::Tag(t) => format!("Tagged {t}"),
        Who::Kind(k) => {
            let name = choices.kinds.iter().find(|(id, _)| id == k).map(|(_, n)| n.as_str()).unwrap_or(k);
            format!("Any {name}")
        }
        Who::Anything => "Anything".to_owned(),
    }
}

/// "Even" for a shape that isn't moulded, else its corners and curves: "7 points, 1 curve".
pub fn path_words(p: &bpe_core::ShapePath) -> String {
    if p.is_empty() { "Even".to_owned() } else { p.words() }
}

/// "No strokes", "1 stroke", "3 strokes".
pub fn strokes_words(s: &bpe_core::Strokes) -> String {
    match s.list.len() {
        0 => "No strokes".to_owned(),
        1 => "1 stroke".to_owned(),
        n => format!("{n} strokes"),
    }
}

/// "No scenes", "1 scene", "3 scenes".
pub fn scenes_words(n: usize) -> String {
    match n {
        0 => "No scenes".to_owned(),
        1 => "1 scene".to_owned(),
        n => format!("{n} scenes"),
    }
}

pub fn suffix(unit: &str) -> String {
    if unit.is_empty() { String::new() } else { format!(" {unit}") }
}
