//! A block's face in its lane: its section, its kind's name and two or three of its settings as
//! small fields ("Who Ball 1", "Hits Tagged breakable").

use bpe_core::rules::{from_key, plus_key, times_key};
use bpe_core::{Block, BlockSection, Choice, RuleBlockKind, Scene, Value, Who};
use bpe_kit::Kit;
use bpe_kit::rules::change::ChangeSettingSettings;
use eframe::egui::{Color32, Painter, Pos2, Rect, Stroke, StrokeKind, text::LayoutJob, text::TextWrapping};

use super::setting_menu;
use crate::style;
use crate::widgets::{TargetChoices, value_words, who_words};

/// One small field on a face.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    /// The setting's label, muted ("" for none).
    pub label: String,
    pub words: String,
    /// The track colour of the thing it names.
    pub dot: Option<Color32>,
}

/// The most fields a face shows.
pub const MOST_FIELDS: usize = 3;

/// A firing's value in plain words: "Hit strength", "Hits so far".
pub fn value_name(name: &str) -> String {
    match name {
        "strength" => "Hit strength".into(),
        "speed" => "Speed".into(),
        "size" => "Size".into(),
        "hits" => "Hits so far".into(),
        "ball" => "Ball number".into(),
        "time" => "Time".into(),
        "count" => "Count".into(),
        "bar" => "Bar".into(),
        "beat" => "Beat".into(),
        other => crate::widgets::pretty(other),
    }
}

fn number(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(x)) => Some(*x),
        Some(Value::Int(x)) => Some(*x as f64),
        _ => None,
    }
}

/// A number's words: "0.5" without trailing zeros.
fn short(x: f64) -> String {
    value_words(&Value::Number(x), "", &TargetChoices::default())
}

/// What a from-the-firing number reads: "Hit strength ×2 +10".
pub fn from_words(block: &Block, key: &str) -> Option<String> {
    let Some(Value::Text(name)) = block.params.get(&from_key(key)) else { return None };
    if name.is_empty() {
        return None;
    }
    let times = number(block.params.get(&times_key(key))).unwrap_or(1.0);
    let plus = number(block.params.get(&plus_key(key))).unwrap_or(0.0);
    let mut words = value_name(name);
    if times != 1.0 {
        words.push_str(&format!(" ×{}", short(times)));
    }
    if plus != 0.0 {
        words.push_str(&format!(" {}{}", if plus < 0.0 { "−" } else { "+" }, short(plus.abs())));
    }
    Some(words)
}

fn who_field(scene: &Scene, choices: &TargetChoices, label: &str, who: &Who) -> Field {
    Field {
        label: label.to_owned(),
        words: who_words(who, choices),
        dot: who.names_thing().filter(|t| scene.things.contains_key(t)).map(|t| style::colour_of(scene, t)),
    }
}

/// The fields of `block` (of `kind`): the kind's own summary when it gives one, Change setting's
/// who, setting and how, else its first simple settings that apply.
pub fn fields(
    scene: &Scene,
    kit: &Kit,
    choices: &TargetChoices,
    block: &Block,
    kind: &dyn RuleBlockKind,
) -> Vec<Field> {
    let get = |k: &str| -> Option<Value> {
        block.params.get(k).cloned().or_else(|| kind.settings().iter().find(|d| d.key == k).map(|d| d.default.clone()))
    };
    let summary = kind.summary(&get);
    if summary != kind.name() {
        return vec![Field { label: String::new(), words: summary, dot: None }];
    }
    if kind.id() == "do.change" {
        return change_fields(scene, kit, choices, block);
    }
    let mut out = Vec::new();
    for def in kind.settings().iter().filter(|d| d.depth == bpe_core::Depth::Simple) {
        if out.len() >= MOST_FIELDS {
            break;
        }
        if !def.applies(None, &get) {
            continue;
        }
        let value = get(def.key).unwrap_or_else(|| def.default.clone());
        let field = match &value {
            Value::Who(w) => who_field(scene, choices, def.label, w),
            Value::Text(name) if kind.id() == "if.compare" && def.key == "value" => {
                Field { label: def.label.to_owned(), words: value_name(name), dot: None }
            }
            _ => {
                let words = from_words(block, def.key)
                    .unwrap_or_else(|| value_words(&value, crate::widgets::unit_for(def, &get), choices));
                Field { label: def.label.to_owned(), words, dot: None }
            }
        };
        out.push(field);
    }
    out
}

fn change_fields(scene: &Scene, kit: &Kit, choices: &TargetChoices, block: &Block) -> Vec<Field> {
    use bpe_core::Settings;
    let s = ChangeSettingSettings::from_params(&block.params);
    let mut out = vec![who_field(scene, choices, "Who", &s.who)];
    let words = setting_menu::setting_words(scene, kit, &s.who, s.card, s.setting.trim());
    out.push(Field { label: String::new(), words, dot: None });
    let def = setting_menu::picked_def(scene, kit, &s.who, s.card, s.setting.trim()).map(|(_, d)| d);
    let numeric = def.is_some_and(|d| {
        matches!(d.ty, bpe_core::ValueType::Number | bpe_core::ValueType::Int | bpe_core::ValueType::Vec2)
    });
    let how = if numeric { s.how.name() } else { "set" };
    let to = match def.map(|d| &d.ty) {
        Some(_) if how == "multiply" => {
            format!("×{}", from_words(block, "times").unwrap_or_else(|| short(s.times as f64)))
        }
        Some(bpe_core::ValueType::Number | bpe_core::ValueType::Int) => {
            from_words(block, "to_number").unwrap_or_else(|| short(s.to_number as f64))
        }
        Some(bpe_core::ValueType::Vec2) => value_words(&Value::Vec2(s.to_pair.into()), "", choices),
        Some(bpe_core::ValueType::Bool) => value_words(&Value::Bool(s.to_switch), "", choices),
        Some(bpe_core::ValueType::Color) => value_words(&Value::Color(s.to_colour), "", choices),
        Some(bpe_core::ValueType::Gradient) => value_words(&Value::Gradient(s.to_gradient.clone()), "", choices),
        Some(bpe_core::ValueType::Choice(_)) => crate::widgets::pretty(&s.to_choice),
        _ => return out,
    };
    out.push(Field { label: crate::widgets::pretty(how), words: to, dot: None });
    out
}

/// Block faces' size.
pub const BLOCK_W: f32 = 156.0;
pub const BLOCK_H: f32 = 64.0;

/// How a face is drawn: picked, just fired, hovered.
#[derive(Clone, Copy, Debug, Default)]
pub struct Looks {
    pub picked: bool,
    pub fired: bool,
    pub hovered: bool,
}

/// Paints a block's face in `rect`.
pub fn paint(painter: &Painter, rect: Rect, section: BlockSection, name: &str, fields: &[Field], looks: Looks) {
    let fill = if looks.picked {
        style::SELECTED
    } else if looks.hovered {
        style::HOVER
    } else {
        style::RAISED
    };
    painter.rect_filled(rect, style::RADIUS, fill);
    let edge = if looks.picked { style::TEXT } else { style::LINE };
    painter.rect_stroke(rect, style::RADIUS, Stroke::new(1.0, edge), StrokeKind::Inside);
    // When blocks wear a bright top edge, Logic blocks a muted one.
    let top = match section {
        BlockSection::When => Some(style::TEXT),
        BlockSection::Logic => Some(style::MUTED),
        _ => None,
    };
    if let Some(ink) = top {
        let edge = Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.top() + 2.0));
        painter.rect_filled(edge, style::RADIUS, ink);
    }
    if looks.fired {
        painter.rect_stroke(
            rect.expand(2.0),
            style::RADIUS + 1,
            Stroke::new(1.5, style::TEXT_STRONG),
            StrokeKind::Outside,
        );
    }
    let inner = rect.shrink2(eframe::egui::vec2(7.0, 5.0));
    let line = |text: &str, font, ink, at: Pos2, width: f32| {
        let mut job = LayoutJob::simple_singleline(text.to_owned(), font, ink);
        job.wrap = TextWrapping::truncate_at_width(width);
        let galley = painter.layout_job(job);
        let h = galley.size().y;
        painter.galley(at, galley, ink);
        h
    };
    let mut y = inner.top() + 1.0;
    y +=
        line(&section.name().to_uppercase(), style::mono(8.5), style::FAINT, Pos2::new(inner.left(), y), inner.width());
    y += line(name, style::semibold(12.0), style::TEXT, Pos2::new(inner.left(), y), inner.width()) + 1.0;
    for field in fields.iter().take(MOST_FIELDS) {
        if y + 10.0 > rect.bottom() {
            break;
        }
        let mut x = inner.left();
        if let Some(dot) = field.dot {
            painter.circle_filled(Pos2::new(x + 3.0, y + 6.0), 3.0, dot);
            x += 9.0;
        }
        let mut job = LayoutJob::default();
        if !field.label.is_empty() {
            job.append(&format!("{} ", field.label), 0.0, plain(style::FAINT));
        }
        job.append(&field.words, 0.0, plain(style::MUTED));
        job.wrap = TextWrapping::truncate_at_width(inner.right() - x);
        let galley = painter.layout_job(job);
        let h = galley.size().y;
        painter.galley(Pos2::new(x, y), galley, style::MUTED);
        y += h;
    }
}

fn plain(ink: Color32) -> eframe::egui::TextFormat {
    eframe::egui::TextFormat { font_id: eframe::egui::FontId::proportional(10.5), color: ink, ..Default::default() }
}
