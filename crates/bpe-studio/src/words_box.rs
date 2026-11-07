//! A Text's words box (Text part 2): the words in a multi-line box where every value in braces
//! (`{hits}`, `{countdown 30|m:ss}` …) shows on a tinted chip, and an **Insert value** menu under it
//! that puts one in where the cursor last was (the end when it never was in the box).

use bpe_core::ThingId;
use eframe::egui::{self, Response, Ui, text::LayoutJob};
use std::sync::Arc;

use crate::style;
use crate::widgets::TargetChoices;

/// The byte ranges of `words` that are values: braces holding something the words can read.
pub fn token_ranges(words: &str) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(open) = words[from..].find('{').map(|i| from + i) {
        let Some(close) = words[open..].find('}').map(|i| open + i) else { break };
        // The innermost braces: a `{` before the `}` starts again from there.
        if let Some(again) = words[open + 1..close].rfind('{') {
            from = open + 1 + again;
            continue;
        }
        if bpe_kit::text::live::has_tokens(&words[open..=close]) {
            out.push(open..close + 1);
        }
        from = close + 1;
    }
    out
}

/// `words` laid out with each value on a chip.
fn job_of(words: &str, wrap: f32) -> LayoutJob {
    let font = egui::FontId::proportional(13.0);
    let plain = egui::TextFormat { font_id: font.clone(), color: style::TEXT, ..Default::default() };
    let token = egui::TextFormat {
        font_id: font,
        color: style::TEXT_STRONG,
        background: style::TOKEN_BG,
        ..Default::default()
    };
    let mut job = LayoutJob::default();
    let mut at = 0;
    for r in token_ranges(words) {
        job.append(&words[at..r.start], 0.0, plain.clone());
        job.append(&words[r.clone()], 0.0, token.clone());
        at = r.end;
    }
    job.append(&words[at..], 0.0, plain);
    job.wrap.max_width = wrap;
    job
}

/// The words box for `text`, named by `name`: changed when typed into or a value was inserted.
pub fn words_box(ui: &mut Ui, text: &mut String, choices: &TargetChoices, name: egui::Id) -> Response {
    let id = ui.make_persistent_id(("words box", name));
    let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap: f32| -> Arc<egui::Galley> {
        ui.fonts_mut(|f| f.layout_job(job_of(buf.as_str(), wrap)))
    };
    let output = egui::TextEdit::multiline(text)
        .id(id)
        .desired_rows(3)
        .desired_width(f32::INFINITY)
        .layouter(&mut layouter)
        .show(ui);
    let mut response = output.response.response.labelled_by(name);
    // Where the cursor last was, in characters, kept while the menu is open.
    let cursor_id = id.with("cursor");
    if let Some(range) = output.cursor_range {
        ui.data_mut(|d| d.insert_temp(cursor_id, range.primary.index));
    }
    let mut picked = None;
    let menu = ui.menu_button("Insert value", |ui| insert_menu(ui, choices, &mut picked));
    menu.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Insert value"));
    if let Some(token) = picked {
        let chars = text.chars().count();
        let at = ui.data(|d| d.get_temp::<usize>(cursor_id)).unwrap_or(chars).min(chars);
        let byte = text.char_indices().nth(at).map_or(text.len(), |(b, _)| b);
        text.insert_str(byte, &token);
        ui.data_mut(|d| d.insert_temp(cursor_id, at + token.chars().count()));
        response.mark_changed();
    }
    response
}

/// One menu item: `label` inserts `token`.
fn item(ui: &mut Ui, label: &str, token: String, picked: &mut Option<String>) {
    if ui.button(label).clicked() {
        *picked = Some(token);
        ui.close();
    }
}

/// The things a submenu lists: each of `ids` by name.
fn named<'a>(choices: &'a TargetChoices, ids: impl Fn(ThingId) -> bool + 'a) -> impl Iterator<Item = &'a str> + 'a {
    choices.things.iter().filter(move |(id, name)| ids(*id) && !name.is_empty()).map(|(_, n)| n.as_str())
}

/// The Insert value menu: sets `picked` to the token chosen.
fn insert_menu(ui: &mut Ui, choices: &TargetChoices, picked: &mut Option<String>) {
    let balls = |id: ThingId| choices.ball_things.contains(&id);
    ui.menu_button("Hits", |ui| {
        item(ui, "Everything", "{hits}".into(), picked);
        for name in named(choices, |_| true) {
            item(ui, name, format!("{{hits of {name}}}"), picked);
        }
    });
    ui.menu_button("Balls left", |ui| {
        item(ui, "All", "{balls left}".into(), picked);
        for name in named(choices, balls) {
            item(ui, name, format!("{{balls left of {name}}}"), picked);
        }
    });
    item(ui, "Time", "{time}".into(), picked);
    item(ui, "Time as a clock", "{time|m:ss}".into(), picked);
    item(ui, "Countdown", "{countdown 30}".into(), picked);
    item(ui, "Countdown as a clock", "{countdown 30|m:ss}".into(), picked);
    item(ui, "Beat", "{beat}".into(), picked);
    item(ui, "Bar", "{bar}".into(), picked);
    ui.menu_button("Counter", |ui| {
        if choices.counters.is_empty() {
            ui.label(egui::RichText::new("No rule counts anything yet").color(style::MUTED));
        }
        for c in &choices.counters {
            item(ui, c, format!("{{counter {c}}}"), picked);
        }
    });
    ui.menu_button("Health", |ui| {
        item(ui, "Its ball or parent", "{health}".into(), picked);
        item(ui, "Its ball or parent, percent", "{health percent}".into(), picked);
        for name in named(choices, |_| true) {
            item(ui, name, format!("{{health of {name}}}"), picked);
        }
    });
    item(ui, "Ball number", "{ball number}".into(), picked);
    item(ui, "Ball name", "{ball name}".into(), picked);
    ui.menu_button("Standings", |ui| {
        for (label, by) in [
            ("Furthest down", "down"),
            ("Most hits", "hits"),
            ("Health", "health"),
            ("Size", "size"),
            ("Speed", "speed"),
        ] {
            item(ui, label, format!("{{standings {by} 5}}"), picked);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_found_in_the_words() {
        let w = "HITS {hits} {nope} {{time}} {countdown 30|m:ss}";
        let found: Vec<&str> = token_ranges(w).into_iter().map(|r| &w[r]).collect();
        assert_eq!(found, ["{hits}", "{time}", "{countdown 30|m:ss}"]);
        assert!(token_ranges("no braces {").is_empty());
    }

    #[test]
    fn a_layout_keeps_every_letter() {
        let w = "A {hits} B";
        let job = job_of(w, 100.0);
        assert_eq!(job.text, w);
        assert_eq!(job.sections.len(), 3);
        assert_eq!(job.sections[1].format.background, style::TOKEN_BG);
    }
}
