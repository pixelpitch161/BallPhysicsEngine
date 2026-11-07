//! **Show air** (particles part 2): faint arrows over the preview showing which way the air
//! goes at the playhead and how fast, read from the particle playback (the editor's own world
//! has no air grid). Never in the video.

use super::View;
use crate::app::Studio;
use crate::style;
use bpe_core::ThingId;
use bpe_kit::air::Air;
use eframe::egui::{self, Pos2, Stroke};

/// Screen points between the arrows.
pub const SPACING: f32 = 36.0;
/// The longest an arrow is drawn, in screen points.
pub const LONGEST: f32 = 30.0;
/// An arrow's length for its speed: where the air goes in this many seconds.
const SECONDS: f32 = 0.08;
/// Air slower than this (px/s) shows no arrow.
const STILL: f32 = 2.0;

/// One arrow: from where to where on the screen, and how strongly it shows (0 to 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arrow {
    pub from: Pos2,
    pub to: Pos2,
    pub strength: f32,
}

/// The arrows of `air` over `view`, on a lattice [`SPACING`] apart.
pub fn arrows(air: &Air, view: &View) -> Vec<Arrow> {
    let mut out = Vec::new();
    if !air.awake() {
        return out;
    }
    let rect = view.rect;
    let mut y = rect.top() + SPACING * 0.5;
    while y < rect.bottom() {
        let mut x = rect.left() + SPACING * 0.5;
        while x < rect.right() {
            let from = egui::pos2(x, y);
            let at = view.to_world(from);
            let v = air.at(at);
            let speed = v.length();
            if speed.is_finite() && speed >= STILL {
                let tip = view.to_screen(at + v * SECONDS);
                let mut d = tip - from;
                if !(d.x.is_finite() && d.y.is_finite()) {
                    d = egui::Vec2::ZERO;
                }
                if d.length() > LONGEST {
                    d *= LONGEST / d.length();
                }
                if d.length() >= 1.0 {
                    out.push(Arrow { from, to: from + d, strength: (speed / 300.0).clamp(0.2, 1.0) });
                }
            }
            x += SPACING;
        }
        y += SPACING;
    }
    out
}

/// The air in the particle playback's latest snapshot, when it is of the step shown.
fn shown_air(studio: &Studio) -> Option<std::sync::Arc<crate::particle_job::Snapshot>> {
    let snapshot = studio.particle_job.as_ref()?.latest()?;
    (snapshot.step == studio.playback.world().step).then_some(snapshot)
}

/// Draws the air's arrows while Show air is on.
pub fn draw(painter: &egui::Painter, studio: &Studio, view: &View) {
    if !studio.show_air {
        return;
    }
    let Some(snapshot) = shown_air(studio) else { return };
    let Some(air) = snapshot
        .states
        .iter()
        .find(|(id, _)| *id == ThingId::ROOT)
        .and_then(|(_, s)| (**s).as_any().downcast_ref::<Air>())
    else {
        return;
    };
    for a in arrows(air, view) {
        let colour = style::MUTED.gamma_multiply(0.25 + 0.5 * a.strength);
        let stroke = Stroke::new(1.0, colour);
        painter.line_segment([a.from, a.to], stroke);
        let d = (a.to - a.from).normalized();
        let side = egui::vec2(-d.y, d.x);
        let head = 4.0f32.min((a.to - a.from).length() * 0.5);
        painter.line_segment([a.to, a.to - d * head + side * head * 0.6], stroke);
        painter.line_segment([a.to, a.to - d * head - side * head * 0.6], stroke);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_kit::air::AirSpec;
    use bpe_runtime::CameraView;
    use eframe::egui::Rect;
    use glam::Vec2;

    fn view() -> View {
        View {
            rect: Rect::from_min_size(Pos2::ZERO, egui::vec2(270.0, 480.0)),
            scale: 0.25,
            camera: CameraView::default(),
            world: false,
            stage: None,
        }
    }

    fn world() -> bpe_runtime::World {
        let kit = bpe_kit::kit();
        let scene = bpe_kit::new_studio_scene(&kit.registry);
        bpe_runtime::World::build(&scene, &bpe_kit::resolve_scene(&scene, &kit.registry), &kit.sims)
    }

    #[test]
    fn windy_air_shows_arrows_its_way_and_still_air_none() {
        let w = world();
        let mut air = Air::new(AirSpec { wind_speed: 200.0, ..AirSpec::default() });
        assert!(arrows(&air, &view()).is_empty(), "asleep: nothing to show");
        air.wake(&w);
        let shown = arrows(&air, &view());
        assert!(shown.len() > 50, "{}", shown.len());
        assert!(shown.iter().all(|a| a.to.x > a.from.x && (a.to.y - a.from.y).abs() < 0.5), "to the right");
        assert!(shown.iter().all(|a| (a.to - a.from).length() <= LONGEST + 1e-3));
        let mut still = Air::new(AirSpec::default());
        still.wake(&w);
        assert!(arrows(&still, &view()).is_empty());
        let _ = Vec2::ZERO;
    }
}
