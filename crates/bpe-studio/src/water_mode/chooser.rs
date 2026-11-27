//! The New scene chooser: Balls or Water, shown when the app opens and by New. A Water scene
//! then picks one of the six looks or Plain; the looks themselves arrive in a later part, so the
//! pick is only recorded on the scene's Water card.

use bpe_core::SimKind;
use bpe_kit::things::water::{NEW_NAME, ReadyLook};
use eframe::egui::{self, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};

use crate::style;

/// The name a new Balls scene has until it is given one.
pub const BALLS_NAME: &str = "New scene";

/// What a new scene is to be.
#[derive(Clone, Debug, PartialEq)]
pub struct NewScene {
    pub sim: SimKind,
    /// A Water scene's starting look (Plain for a Balls scene).
    pub look: ReadyLook,
    pub name: String,
}

impl NewScene {
    /// The blank Balls project.
    #[cfg(test)]
    pub fn balls() -> Self {
        Self { sim: SimKind::Balls, look: ReadyLook::Plain, name: BALLS_NAME.into() }
    }

    /// A Water scene with `look`.
    #[cfg(test)]
    pub fn water(look: ReadyLook) -> Self {
        Self { sim: SimKind::Water, look, name: NEW_NAME.into() }
    }
}

/// Which page of the chooser shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Balls or Water.
    Kind,
    /// A Water scene's look.
    Look,
}

/// The chooser while it is open.
#[derive(Clone, Debug, PartialEq)]
pub struct Chooser {
    pub step: Step,
    pub sim: SimKind,
    pub look: ReadyLook,
    pub name: String,
    /// The name was typed, so picking the other kind leaves it alone.
    pub named: bool,
    /// Shown as the app opens: there is nothing to go back to, so no Cancel.
    pub at_start: bool,
}

impl Chooser {
    /// The chooser the app opens on.
    pub fn at_start() -> Self {
        Self {
            step: Step::Kind,
            sim: SimKind::Balls,
            look: ReadyLook::Plain,
            name: BALLS_NAME.into(),
            named: false,
            at_start: true,
        }
    }

    /// The chooser New shows over the open scene.
    pub fn for_new() -> Self {
        Self { at_start: false, ..Self::at_start() }
    }

    fn pick(&mut self, sim: SimKind) {
        self.sim = sim;
        if !self.named {
            self.name = default_name(sim).into();
        }
    }

    fn made(&self) -> NewScene {
        let look = if self.sim == SimKind::Water { self.look } else { ReadyLook::Plain };
        let name = self.name.trim();
        let name = if name.is_empty() { default_name(self.sim) } else { name };
        NewScene { sim: self.sim, look, name: name.into() }
    }
}

fn default_name(sim: SimKind) -> &'static str {
    match sim {
        SimKind::Balls => BALLS_NAME,
        SimKind::Water => NEW_NAME,
    }
}

/// What the chooser was answered with.
#[derive(Clone, Debug, PartialEq)]
pub enum ChooserOutcome {
    Create(NewScene),
    /// Open a scene…
    Open,
    Cancel,
}

const KIND_TILE: Vec2 = Vec2::new(330.0, 236.0);
const KIND_PICTURE: f32 = 132.0;
const LOOK_TILE: Vec2 = Vec2::new(162.0, 150.0);
const LOOK_PICTURE: f32 = 78.0;
const WIDTH: f32 = 2.0 * KIND_TILE.x + 12.0;

/// Shows the chooser as a dialog and says how it was answered, if it was.
pub fn show(ctx: &egui::Context, chooser: &mut Chooser) -> Option<ChooserOutcome> {
    let mut outcome = None;
    let modal = egui::Modal::new(egui::Id::unique("new-scene")).show(ctx, |ui| {
        ui.set_width(WIDTH);
        outcome = match chooser.step {
            Step::Kind => kind_page(ui, chooser),
            Step::Look => look_page(ui, chooser),
        };
    });
    if outcome.is_none() && !chooser.at_start && modal.should_close() {
        outcome = Some(ChooserOutcome::Cancel);
    }
    outcome
}

fn kind_page(ui: &mut Ui, chooser: &mut Chooser) -> Option<ChooserOutcome> {
    let mut outcome = None;
    ui.heading("New scene");
    ui.label(
        RichText::new(
            "What does this scene simulate? Each kind has its own editor, physics and lighting, built for that \
             kind alone.",
        )
        .color(style::MUTED),
    );
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        let balls = "Bouncing balls, rings, emitters, rules and hit sounds. The editor you know: things in a tree \
                     with their cards.";
        let water = "Liquids running through courses: streams, pools, slopes and openings. Its own course \
                     builder, water physics and water lighting.";
        for (sim, title, words) in [(SimKind::Balls, "Balls", balls), (SimKind::Water, "Water", water)] {
            let on = chooser.sim == sim;
            let tile = tile(ui, KIND_TILE, KIND_PICTURE, title, words, on, |ui, rect| match sim {
                SimKind::Balls => balls_picture(ui, rect),
                SimKind::Water => water_picture(ui, rect, ReadyLook::Plain),
            });
            if tile.clicked() {
                chooser.pick(sim);
            }
            if tile.double_clicked() && sim == SimKind::Balls {
                outcome = Some(ChooserOutcome::Create(chooser.made()));
            }
        }
    });
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        let label = ui.label("Name");
        let name = ui.add(egui::TextEdit::singleline(&mut chooser.name).desired_width(260.0)).labelled_by(label.id);
        if name.changed() {
            chooser.named = true;
        }
    });
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        if ui.button("Open a scene…").on_hover_text("Open a scene you saved before, instead of starting one").clicked()
        {
            outcome = Some(ChooserOutcome::Open);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match chooser.sim {
            SimKind::Balls => {
                if ui.add(style::primary_button("Create Balls scene")).clicked() {
                    outcome = Some(ChooserOutcome::Create(chooser.made()));
                }
                cancel(ui, chooser, &mut outcome);
            }
            SimKind::Water => {
                if ui.add(style::primary_button("Next")).on_hover_text("Pick how the water looks").clicked() {
                    chooser.step = Step::Look;
                }
                cancel(ui, chooser, &mut outcome);
            }
        });
    });
    outcome
}

fn cancel(ui: &mut Ui, chooser: &Chooser, outcome: &mut Option<ChooserOutcome>) {
    if !chooser.at_start && ui.button("Cancel").clicked() {
        *outcome = Some(ChooserOutcome::Cancel);
    }
}

fn look_page(ui: &mut Ui, chooser: &mut Chooser) -> Option<ChooserOutcome> {
    let mut outcome = None;
    ui.heading("New Water scene");
    ui.label(
        RichText::new(
            "Pick a look to start from, or Plain. For now this only remembers your pick: the looks themselves \
             arrive in a later update, and you can change everything afterwards.",
        )
        .color(style::MUTED),
    );
    ui.add_space(10.0);
    for row in ReadyLook::ALL.chunks(4) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for &look in row {
                let on = chooser.look == look;
                let tile = tile(ui, LOOK_TILE, LOOK_PICTURE, look.title(), look.words(), on, |ui, rect| {
                    water_picture(ui, rect, look);
                });
                if tile.clicked() {
                    chooser.look = look;
                }
                if tile.double_clicked() {
                    outcome = Some(ChooserOutcome::Create(chooser.made()));
                }
            }
        });
        ui.add_space(8.0);
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui.button("Back").clicked() {
            chooser.step = Step::Kind;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(style::primary_button("Create Water scene")).clicked() {
                outcome = Some(ChooserOutcome::Create(chooser.made()));
            }
            cancel(ui, chooser, &mut outcome);
        });
    });
    outcome
}

/// One tile: a picture, a title and a line or two of words, ringed when picked.
fn tile(
    ui: &mut Ui,
    size: Vec2,
    picture_height: f32,
    title: &str,
    words: &str,
    on: bool,
    picture: impl FnOnce(&Ui, Rect),
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, title));
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let fill = if on { style::SELECTED } else { style::RAISED };
    ui.painter().rect_filled(rect, style::RADIUS, fill);
    let top = Rect::from_min_size(rect.min, Vec2::new(rect.width(), picture_height));
    picture(ui, top);
    let ring = if on {
        Stroke::new(1.5, style::TEXT)
    } else if response.hovered() {
        Stroke::new(1.0, style::MUTED)
    } else {
        Stroke::new(1.0, style::LINE)
    };
    ui.painter().rect_stroke(rect, style::RADIUS, ring, egui::StrokeKind::Inside);
    let ink = if on { style::TEXT_STRONG } else { style::TEXT };
    let left = rect.left() + 10.0;
    let width = rect.width() - 20.0;
    let big = size.x > 200.0;
    let name = ui.painter().layout_no_wrap(title.to_owned(), style::semibold(if big { 15.0 } else { 12.5 }), ink);
    let under = top.bottom() + 8.0 + name.size().y + 4.0;
    ui.painter().galley(Pos2::new(left, top.bottom() + 8.0), name, ink);
    let small = FontId::proportional(if big { 12.0 } else { 10.5 });
    let text = ui.painter().layout(words.to_owned(), small, style::MUTED, width);
    ui.painter().galley(Pos2::new(left, under), text, style::MUTED);
    response
}

/// A Balls scene in small: a ring on a dark ground with three balls in it.
fn balls_picture(ui: &Ui, rect: Rect) {
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, style::chooser::BALLS_GROUND);
    let c = rect.center();
    let r = rect.height() * 0.4;
    painter.circle_stroke(c, r, Stroke::new(2.0, style::chooser::BALLS_RING));
    let spots = [(-0.35, 0.2, 0.2), (0.3, -0.25, 0.14), (0.15, 0.45, 0.1)];
    for ((x, y, size), colour) in spots.into_iter().zip(style::chooser::BALLS) {
        painter.circle_filled(c + Vec2::new(x, y) * r, size * r, colour);
    }
}

/// A Water scene in small, in `look`'s colours: a course carved into a basin, a stream running
/// into the pool in it.
fn water_picture(ui: &Ui, rect: Rect, look: ReadyLook) {
    let (liquid, solid) = look.swatch();
    let (liquid, solid) = (style::video_bytes(liquid), style::video_bytes(solid));
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, solid);
    let (w, h) = (rect.width(), rect.height());
    let at = |x: f32, y: f32| rect.min + Vec2::new(x * w, y * h);
    // The carved space: a chute from the top left down into a basin.
    let carved = Rect::from_min_max(at(0.12, 0.12), at(0.88, 0.86));
    painter.rect_filled(carved, 6.0, style::chooser::WATER_BACK);
    // A ledge the stream runs off.
    painter.rect_filled(Rect::from_min_max(at(0.12, 0.38), at(0.46, 0.5)), 0.0, solid);
    // The stream along the ledge and down, then the pool.
    painter.rect_filled(Rect::from_min_max(at(0.2, 0.3), at(0.5, 0.38)), 3.0, liquid);
    painter.rect_filled(Rect::from_min_max(at(0.46, 0.3), at(0.53, 0.7)), 3.0, liquid);
    painter.rect_filled(Rect::from_min_max(at(0.12, 0.66), at(0.88, 0.86)), 6.0, liquid);
    // A tap where it comes in.
    let tap = vec![at(0.16, 0.17), at(0.26, 0.17), at(0.21, 0.27)];
    painter.add(egui::Shape::convex_polygon(tap, liquid, Stroke::NONE));
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    type State = (Chooser, Option<ChooserOutcome>);

    fn harness(chooser: Chooser) -> Harness<'static, State> {
        let mut h = Harness::builder()
            .with_size(egui::vec2(1000.0, 760.0))
            .allow_missing_glyphs()
            .with_context_setup(style::apply)
            .build_ui_state(
                |ui, (chooser, outcome): &mut State| {
                    if let Some(answer) = show(ui.ctx(), chooser) {
                        *outcome = Some(answer);
                    }
                },
                (chooser, None),
            );
        h.run();
        h
    }

    fn has(h: &Harness<State>, label: &str) -> bool {
        h.query_all_by_label(label).next().is_some()
    }

    fn click(h: &mut Harness<State>, label: &str) {
        h.query_all_by_label(label).next().unwrap_or_else(|| panic!("no {label}")).click();
        h.run();
    }

    #[test]
    fn balls_is_one_click_and_create() {
        let mut h = harness(Chooser::for_new());
        assert!(has(&h, "New scene") && has(&h, "Balls") && has(&h, "Water"));
        click(&mut h, "Balls");
        click(&mut h, "Create Balls scene");
        assert_eq!(h.state().1, Some(ChooserOutcome::Create(NewScene::balls())));
        assert_eq!(NewScene::balls().name, "New scene");
    }

    #[test]
    fn water_offers_plain_and_the_six_looks_then_creates() {
        let mut h = harness(Chooser::for_new());
        click(&mut h, "Water");
        assert_eq!(h.state().0.name, "Water course");
        assert!(!has(&h, "Create Balls scene"));
        click(&mut h, "Next");
        assert_eq!(h.state().0.step, Step::Look);
        for look in ReadyLook::ALL {
            assert!(has(&h, look.title()), "{}", look.title());
        }
        assert_eq!(h.state().0.look, ReadyLook::Plain, "Plain is picked until another is");
        click(&mut h, "Back");
        assert_eq!(h.state().0.step, Step::Kind);
        click(&mut h, "Next");
        click(&mut h, "Neon in the dark");
        click(&mut h, "Create Water scene");
        let want = NewScene { sim: SimKind::Water, look: ReadyLook::Neon, name: "Water course".into() };
        assert_eq!(h.state().1, Some(ChooserOutcome::Create(want)));
    }

    #[test]
    fn a_typed_name_is_kept_when_the_kind_changes_and_an_empty_one_gets_the_usual() {
        let mut chooser = Chooser::for_new();
        chooser.name = "Cave".into();
        chooser.named = true;
        let mut h = harness(chooser);
        click(&mut h, "Water");
        assert_eq!(h.state().0.name, "Cave");
        click(&mut h, "Balls");
        click(&mut h, "Create Balls scene");
        let made = |h: &Harness<State>| match &h.state().1 {
            Some(ChooserOutcome::Create(new)) => new.clone(),
            other => panic!("{other:?}"),
        };
        assert_eq!((made(&h).sim, made(&h).name.as_str()), (SimKind::Balls, "Cave"));
        // A Water pick's look never rides along on a Balls scene, and a blank name gets the usual one.
        let mut chooser = Chooser::for_new();
        chooser.look = ReadyLook::Molten;
        chooser.name = "  ".into();
        chooser.named = true;
        assert_eq!(chooser.made(), NewScene::balls());
        chooser.sim = SimKind::Water;
        assert_eq!(chooser.made(), NewScene::water(ReadyLook::Molten));
    }

    #[test]
    fn cancel_is_only_there_for_new() {
        let mut h = harness(Chooser::for_new());
        assert!(has(&h, "Open a scene…"));
        click(&mut h, "Cancel");
        assert_eq!(h.state().1, Some(ChooserOutcome::Cancel));
        let mut h = harness(Chooser::at_start());
        assert!(!has(&h, "Cancel") && has(&h, "Open a scene…"));
        click(&mut h, "Water");
        click(&mut h, "Next");
        assert!(!has(&h, "Cancel"), "nor on the looks page");
        let mut h = harness(Chooser::at_start());
        click(&mut h, "Open a scene…");
        assert_eq!(h.state().1, Some(ChooserOutcome::Open));
    }
}
