//! Typing a Text's words straight into the preview (Text part 1): a double-click on a Text opens
//! a box over it, holding its words all selected, so typing replaces them; Enter starts a new
//! line. Every key shows at once, and the whole session is one undo step. Escape or a click
//! elsewhere ends it.

use super::View;
use crate::app::Studio;
use crate::style;
use bpe_core::{Scene, ThingId, Value};
use eframe::egui::{self, Key, Ui};

/// A Text being typed into.
pub struct Typing {
    pub id: ThingId,
    /// The scene before the first key, what one undo goes back to.
    pub before: Scene,
    /// The words as typed so far.
    pub buffer: String,
    /// The box has had the keyboard (it takes it as soon as it can).
    focused: bool,
}

fn edit_id() -> egui::Id {
    egui::Id::unique("type the words")
}

/// Starts typing into Text `id`, with its words as they are.
pub fn start(studio: &mut Studio, id: ThingId) {
    let Some(thing) = studio.scene.things.get(&id).filter(|t| t.kind == bpe_kit::things::text::KIND) else {
        return;
    };
    // The words at the playhead when they have keys.
    let a = bpe_core::Address::own(id, "words");
    let words = match studio.scene.value_at_time(&studio.kit.registry, &a, studio.time) {
        Some(Value::Text(w)) if studio.scene.keyed(&a) => w,
        _ => <bpe_kit::things::text::TextSettings as bpe_core::Settings>::from_params(&thing.params).words,
    };
    studio.typing = Some(Typing { id, before: studio.scene.clone(), buffer: words, focused: false });
}

/// Draws the box over the Text being typed into, writes what is typed into its words and ends the
/// session on Escape or a click elsewhere.
pub fn show(ui: &mut Ui, studio: &mut Studio, view: &View) {
    let Some(typing) = studio.typing.as_mut() else { return };
    let id = typing.id;
    // The text went (deleted, or another scene opened): nothing to type into.
    if !studio.scene.things.get(&id).is_some_and(|t| t.kind == bpe_kit::things::text::KIND) {
        studio.typing = None;
        return;
    }
    // Over the text's box on the screen, or the middle of the preview when it shows nothing.
    let (reg, world, time) = (&studio.kit.registry, studio.playback.world(), studio.sim_time());
    let corner =
        super::text_handles::now_of(&studio.scene, reg, world, super::text_handles::Seen::of(studio), id, time)
            .and_then(|(now, _)| {
                let v = view.of(studio, id)?;
                let (lo, _) = now.bounds()?;
                Some(v.to_screen(now.to_world(lo)))
            });
    // Kept on the preview, as far as it has room.
    let most = (view.rect.max - egui::vec2(220.0, 80.0)).max(view.rect.min);
    let at = corner.unwrap_or(view.rect.center()).max(view.rect.min).min(most);
    let typing = studio.typing.as_mut().expect("looked up above");
    let first = !typing.focused;
    let area =
        egui::Area::new(egui::Id::unique("typing over the preview")).fixed_pos(at).order(egui::Order::Foreground);
    let response = area
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style())
                .show(ui, |ui| {
                    // An area's first frame only sizes it: the box takes the keyboard once it can.
                    if first && ui.is_enabled() {
                        // Everything selected, so typing replaces the words.
                        let mut state = egui::text_edit::TextEditState::default();
                        let end = egui::text::CCursor::new(typing.buffer.chars().count());
                        state
                            .cursor
                            .set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), end)));
                        state.store(ui.ctx(), edit_id());
                        ui.memory_mut(|m| m.request_focus(edit_id()));
                    }
                    let edit = egui::TextEdit::multiline(&mut typing.buffer)
                        .id(edit_id())
                        .desired_rows(2)
                        .desired_width(220.0);
                    let r = ui.add(edit);
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, "Type the words"));
                    ui.label(egui::RichText::new("Esc or click away to finish").small().color(style::MUTED));
                    r
                })
                .inner
        })
        .inner;
    typing.focused |= response.has_focus();
    if response.changed() {
        let words = typing.buffer.clone();
        // A key at the playhead when the words have keys.
        let kit = studio.kit.clone();
        let a = bpe_core::Address::own(id, "words");
        let mut keyed = false;
        crate::inspector::set_or_key(&mut studio.scene, &kit.registry, &a, Value::Text(words), studio.time, &mut keyed);
        // Shown at once; the history hears of it when typing ends.
        studio.scene_changed();
        studio.recorded = true;
    }
    let escape = ui.input(|i| i.key_pressed(Key::Escape));
    if escape || response.lost_focus() {
        finish(studio);
    }
}

/// Ends typing: the whole session is one undo step.
pub fn finish(studio: &mut Studio) {
    if let Some(typing) = studio.typing.take() {
        studio.after_separate_edit(typing.before);
        studio.recorded = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::Settings;
    use egui_kittest::{Harness, kittest::Queryable};

    fn view() -> View {
        View {
            rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(270.0, 480.0)),
            scale: 0.25,
            camera: bpe_runtime::CameraView::default(),
            world: false,
            stage: None,
        }
    }

    fn words(studio: &Studio, id: ThingId) -> String {
        bpe_kit::things::text::TextSettings::from_params(&studio.scene.things[&id].params).words
    }

    #[test]
    fn double_clicking_a_text_types_into_it() {
        let mut studio = Studio::headless();
        let kit = studio.kit.clone();
        let reg = &kit.registry;
        let before = studio.scene.clone();
        let id = bpe_kit::add_thing(&mut studio.scene, reg, bpe_kit::things::text::KIND, ThingId::ROOT).unwrap();
        studio.after_separate_edit(before);
        start(&mut studio, id);
        let mut h = Harness::builder()
            .with_size(egui::vec2(270.0, 480.0))
            .with_max_steps(60)
            .allow_missing_glyphs()
            .with_context_setup(crate::style::apply)
            .build_ui_state(|ui, s: &mut Studio| show(ui, s, &view()), studio);
        h.run();
        h.get_by_label("Type the words").type_text("NEW");
        h.run();
        assert_eq!(words(h.state(), id), "NEW", "typing replaced the words, and shows at once");
        h.key_press(egui::Key::Enter);
        h.get_by_label("Type the words").type_text("LINE");
        h.run();
        assert_eq!(words(h.state(), id), "NEW\nLINE", "Enter starts a new line");
        h.key_press(egui::Key::Escape);
        h.run();
        assert!(h.state().typing.is_none(), "Escape ends it");
        let studio = h.state_mut();
        assert!(studio.undo());
        assert_eq!(words(studio, id), "TEXT", "one undo brings back the old words");
    }

    #[test]
    fn typing_into_keyed_words_keys_them_at_the_playhead() {
        let mut studio = Studio::headless();
        let kit = studio.kit.clone();
        let reg = &kit.registry;
        let id = bpe_kit::add_thing(&mut studio.scene, reg, bpe_kit::things::text::KIND, ThingId::ROOT).unwrap();
        let a = bpe_core::Address::own(id, "words");
        studio.scene.set_key(reg, &a, 0.0, Value::Text("ONE".into())).unwrap();
        studio.scene.set_key(reg, &a, 2.0, Value::Text("TWO".into())).unwrap();
        studio.time = 2.5;
        studio.scene_changed();
        start(&mut studio, id);
        assert_eq!(studio.typing.as_ref().unwrap().buffer, "TWO", "the words at the playhead");
        let mut h = Harness::builder()
            .with_size(egui::vec2(270.0, 480.0))
            .with_max_steps(60)
            .allow_missing_glyphs()
            .with_context_setup(crate::style::apply)
            .build_ui_state(|ui, s: &mut Studio| show(ui, s, &view()), studio);
        h.run();
        h.get_by_label("Type the words").type_text("THREE");
        h.run();
        let s = h.state();
        assert_eq!(s.scene.value_at_time(reg, &a, 2.5), Some(Value::Text("THREE".into())), "a key at 2.5 s");
        assert_eq!(s.scene.value_at_time(reg, &a, 0.5), Some(Value::Text("ONE".into())), "the first key kept");
    }

    #[test]
    fn a_small_preview_still_takes_the_box() {
        let mut studio = Studio::headless();
        let kit = studio.kit.clone();
        let id =
            bpe_kit::add_thing(&mut studio.scene, &kit.registry, bpe_kit::things::text::KIND, ThingId::ROOT).unwrap();
        studio.scene_changed();
        start(&mut studio, id);
        let small = View { rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(120.0, 60.0)), ..view() };
        let mut h = Harness::builder()
            .with_size(egui::vec2(300.0, 300.0))
            .with_max_steps(60)
            .allow_missing_glyphs()
            .build_ui_state(move |ui, s: &mut Studio| show(ui, s, &small), studio);
        h.run();
        assert!(h.state().typing.is_some());
    }

    #[test]
    fn clicking_away_ends_typing() {
        let mut studio = Studio::headless();
        let kit = studio.kit.clone();
        let reg = &kit.registry;
        let id = bpe_kit::add_thing(&mut studio.scene, reg, bpe_kit::things::text::KIND, ThingId::ROOT).unwrap();
        studio.scene_changed();
        start(&mut studio, id);
        let mut h = Harness::builder()
            .with_size(egui::vec2(270.0, 480.0))
            .with_max_steps(60)
            .allow_missing_glyphs()
            .with_context_setup(crate::style::apply)
            .build_ui_state(|ui, s: &mut Studio| show(ui, s, &view()), studio);
        h.run();
        assert!(h.state().typing.is_some());
        let at = egui::pos2(260.0, 470.0);
        h.hover_at(at);
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            h.step();
        }
        h.run();
        assert!(h.state().typing.is_none());
    }
}
