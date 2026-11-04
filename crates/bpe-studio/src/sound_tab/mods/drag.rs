//! Dragging a modulator's handle: the payload, the ghost under the pointer, and what a knob
//! or another control says when one is held over it.

use super::super::TabCx;
use crate::style;
use bpe_core::{LayerId, SlotId, ThingId, split_layered};
use eframe::egui::{self, Response, Ui};

/// What dragging a tile's handle carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModDrag {
    pub thing: ThingId,
    pub layer: LayerId,
}

/// The hover note on a control a modulator can't move.
pub const CANT: &str = "Can't modulate this";
/// The hover note on a knob the modulator already moves.
pub const ALREADY: &str = "It already moves this";

/// Whether `drag` may move the sound card's knob `key`: not one of its own knobs, a number knob
/// the voice reads, and not one it already moves.
pub fn check(cx: &TabCx, drag: &ModDrag, key: &str) -> Result<(), &'static str> {
    let own = split_layered(key).is_some_and(|(l, _)| l == drag.layer);
    let card = cx.scene.card(cx.id, SlotId::Sound);
    let routable = card.is_some_and(|c| bpe_sound::patch::routable(c, key));
    if drag.thing != cx.id || own || !routable {
        return Err(CANT);
    }
    if cx.routes().iter().any(|r| r.source == drag.layer && r.target == key) {
        return Err(ALREADY);
    }
    Ok(())
}

/// A held modulator over a control that takes no modulation (a switch, chips): the hover note.
pub fn refuse(response: &Response) {
    if response.dnd_hover_payload::<ModDrag>().is_some() {
        note(&response.ctx, CANT.to_owned());
    }
}

fn note_id() -> egui::Id {
    egui::Id::unique("mod drag note")
}

/// What the held modulator would do where it is: shown under its name by [`ghost`].
fn note(ctx: &egui::Context, text: String) {
    let frame = ctx.cumulative_frame_nr();
    ctx.data_mut(|d| d.insert_temp(note_id(), (frame, text)));
}

/// The note under the held modulator now (set this frame or the one before), if any.
pub fn current_note(ctx: &egui::Context) -> Option<String> {
    let frame = ctx.cumulative_frame_nr();
    let (at, text): (u64, String) = ctx.data(|d| d.get_temp(note_id()))?;
    (at + 1 >= frame).then_some(text)
}

/// The knob `key`'s side of a drag: a note while one is held over it, a route when one is let go
/// on it.
pub fn on_knob(cx: &mut TabCx, response: &Response, key: &str) {
    if let Some(held) = response.dnd_hover_payload::<ModDrag>() {
        match check(cx, &held, key) {
            Ok(()) => note(&response.ctx, format!("Modulate {}", super::routes::target_words(cx, key))),
            Err(why) => note(&response.ctx, why.to_owned()),
        }
    }
    if let Some(dropped) = response.dnd_release_payload::<ModDrag>()
        && check(cx, &dropped, key).is_ok()
    {
        super::add_route(cx, dropped.layer, key);
    }
}

/// While a handle is dragged: its modulator's name follows the pointer.
pub fn ghost(ui: &Ui, cx: &TabCx) {
    let Some(held) = egui::DragAndDrop::payload::<ModDrag>(ui.ctx()) else { return };
    let Some(p) = ui.ctx().pointer_interact_pos() else { return };
    // The note from where it is now (set while drawing the controls under it), or none.
    let note = current_note(ui.ctx());
    let name = match &note {
        Some(n) => format!("{}: {n}", super::name_of(cx, held.layer)),
        None => super::name_of(cx, held.layer),
    };
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("mod drag ghost")));
    let galley = painter.layout_no_wrap(name, style::semibold(12.0), style::TEXT_STRONG);
    let rect = egui::Rect::from_min_size(p + egui::vec2(12.0, 8.0), galley.size() + egui::vec2(12.0, 6.0));
    painter.rect_filled(rect, style::RADIUS, style::RAISED);
    painter.rect_stroke(rect, style::RADIUS, egui::Stroke::new(1.0, cx.colour), egui::StrokeKind::Inside);
    painter.galley(rect.min + egui::vec2(6.0, 3.0), galley, style::TEXT_STRONG);
}
