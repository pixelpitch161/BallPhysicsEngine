//! "Preview final frame" (lighting part 3): the frame at the playhead drawn exactly as the export
//! will draw it (baked when the export bakes), on a thread with its own graphics device, then
//! shown in a window with how long it took and what the whole video would take.

use crate::export_window::minutes;
use crate::style;
use bpe_core::Scene;
use bpe_kit::Kit;
use eframe::egui::{self, RichText};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

/// What the window was asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ask {
    /// The moment, in video seconds.
    pub time: f64,
    /// The share of the video's size.
    pub scale: f32,
    /// Bake or not (`None`: as the Lighting card's On export says).
    pub bake: Option<bool>,
}

struct Shown {
    texture: egui::TextureHandle,
    size: (u32, u32),
    seconds: f32,
    baked: bool,
}

/// What the drawing thread sends back: the pixels, their size and the seconds it took.
type Drawn = anyhow::Result<(Vec<u8>, (u32, u32), f32)>;

#[derive(Default)]
pub struct FinalFrame {
    pub open: bool,
    asked: Option<Ask>,
    /// The frames of the video, for the estimate.
    frames: u32,
    working: Option<Receiver<Drawn>>,
    shown: Option<Result<Shown, String>>,
}

impl FinalFrame {
    /// Starts drawing `ask`'s frame of `scene` (a copy) in the background and opens the window.
    pub fn start(&mut self, ctx: &egui::Context, scene: &Scene, kit: &Arc<Kit>, frames: u32, ask: Ask) {
        let (sender, receiver) = channel();
        let (scene, kit, ctx2) = (scene.clone(), kit.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = bpe_gfx::headless_device().and_then(|(device, queue)| {
                bpe_cli::final_frame(&device, &queue, &scene, &kit, ask.time, ask.scale, ask.bake)
            });
            let _ = sender.send(result);
            ctx2.request_repaint();
        });
        self.open = true;
        self.asked = Some(ask);
        self.frames = frames;
        self.working = Some(receiver);
        self.shown = None;
    }

    #[cfg(test)]
    fn busy(&self) -> bool {
        self.working.is_some()
    }

    pub fn show(&mut self, ctx: &egui::Context, scene: &Scene, kit: &Kit) {
        if let Some(receiver) = &self.working
            && let Ok(result) = receiver.try_recv()
        {
            let baked = self.asked.is_some_and(|ask| bpe_cli::bake_for(scene, kit, ask.bake).is_some());
            self.shown = Some(match result {
                Ok((pixels, size, seconds)) => {
                    let size_px = [size.0 as usize, size.1 as usize];
                    let image = egui::ColorImage::from_rgba_unmultiplied(size_px, &pixels); // scene colour
                    let texture = ctx.load_texture("final frame", image, egui::TextureOptions::LINEAR);
                    Ok(Shown { texture, size, seconds, baked })
                }
                Err(error) => Err(format!("{error:#}")),
            });
            self.working = None;
        }
        if !self.open {
            return;
        }
        let mut open = self.open;
        egui::Window::new("Final frame")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| self.contents(ui, ctx));
        self.open = open;
    }

    fn contents(&self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let at = self.asked.map(|ask| ask.time).unwrap_or(0.0);
        let when = format!("The frame at {:.2} s, as the video will show it.", at);
        if self.working.is_some() {
            ui.label(RichText::new(when).color(style::MUTED));
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Drawing the frame…");
            });
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            return;
        }
        match &self.shown {
            Some(Ok(shown)) => {
                ui.label(RichText::new(when).color(style::MUTED));
                // Fits the screen: at most 70% of its height.
                let most = ctx.content_rect().height() * 0.7;
                let (w, h) = (shown.size.0 as f32, shown.size.1 as f32);
                let k = (most / h).min(1.0);
                ui.add(egui::Image::new(&shown.texture).fit_to_exact_size(egui::vec2(w * k, h * k)));
                let how = if shown.baked { "Baked" } else { "Drawn live" };
                ui.label(format!("{how} in {:.1} s.", shown.seconds));
                if self.frames > 1 {
                    let whole = shown.seconds * self.frames as f32;
                    ui.label(
                        RichText::new(format!(
                            "The whole video, {} frames at this size, would take about {}.",
                            self.frames,
                            minutes(whole)
                        ))
                        .color(style::MUTED),
                    );
                }
            }
            Some(Err(message)) => {
                ui.label(RichText::new(format!("The frame couldn't be drawn: {message}")).color(style::TEXT));
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_final_frame_comes_back_and_shows() {
        let kit = Arc::new(bpe_kit::kit());
        let scene = bpe_kit::new_scene(&kit.registry);
        let ctx = egui::Context::default();
        let mut window = FinalFrame::default();
        window.start(&ctx, &scene, &kit, 90, Ask { time: 0.5, scale: 0.25, bake: None });
        assert!(window.open && window.busy());
        let started = std::time::Instant::now();
        while window.busy() && started.elapsed() < std::time::Duration::from_secs(60) {
            let mut out = ctx.run_ui(Default::default(), |ui| window.show(ui.ctx(), &scene, &kit));
            out.textures_delta.clear();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        match &window.shown {
            Some(Ok(shown)) => {
                assert_eq!(shown.size, (270, 480));
                assert!(!shown.baked, "a scene without a Lighting card never bakes");
            }
            // No graphics card here: the window says so instead.
            Some(Err(message)) => eprintln!("skipped: {message}"),
            None => panic!("nothing came back"),
        }
    }
}
