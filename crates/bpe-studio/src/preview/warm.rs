//! Effect warm-up. Effects that remember earlier frames (Feedback tunnel, Thermal vision, Rainbow
//! echoes…) build their picture up frame by frame, so a frame drawn straight after a jump in time
//! has no memory yet, while the export, which runs every frame from the start, has it. After a
//! jump the preview shows the frame at once, then replays the effects' remembered seconds on a
//! second renderer, a few frames per editor frame, and swaps that renderer in when it reaches
//! the playhead, so the paused picture matches the export.

use bpe_gfx::{Frame, Renderer, build_frame_with, wgpu};
use bpe_runtime::{CameraView, Playback};

use super::{Drawn, Preview, shared_programs};
use crate::app::Studio;

/// Frames replayed each editor frame.
pub const FRAMES_PER_UPDATE: usize = 6;
/// A forward step in time longer than this is a jump (the effects' own MAX_STEP).
const LONGEST_STEP: f64 = 0.25;

/// A replay in progress.
pub struct Warmup {
    playback: Playback,
    frame: Frame,
    /// The time it started from, the next frame's time and the playhead it warms up to.
    from: f64,
    at: f64,
    target: f64,
    /// Seconds between video frames.
    every: f64,
    revision: u64,
    size: (u32, u32),
    look: Option<(CameraView, (u32, u32))>,
}

impl Warmup {
    /// How far it has got, 0 to 1.
    pub fn done(&self) -> f32 {
        let span = self.target - self.from;
        if span <= 0.0 { 1.0 } else { ((self.at - self.from) / span).clamp(0.0, 1.0) as f32 }
    }
}

/// The video's frame rate.
fn fps(studio: &Studio) -> f64 {
    studio
        .resolved
        .things
        .get(&bpe_core::ThingId::ROOT)
        .map(|r| <bpe_kit::SceneSettings as bpe_core::Settings>::from_params(&r.params).fps.max(1) as f64)
        .unwrap_or(60.0)
}

/// Draws the frame at `time` from `playback` into `renderer`, as the preview does.
#[allow(clippy::too_many_arguments)]
fn draw_at(
    studio: &Studio,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut Renderer,
    frame: &mut Frame,
    playback: &mut Playback,
    time: f64,
    look: Option<(CameraView, (u32, u32))>,
) {
    playback.seek(time);
    build_frame_with(
        &studio.scene,
        &studio.resolved,
        playback.world(),
        playback.log(),
        time,
        &studio.kit.draws,
        studio.follow.as_deref().map(|f| f as &dyn bpe_runtime::SoundAt),
        frame,
    );
    if let Some((camera, size)) = look {
        super::world_look(frame, camera, size);
    }
    frame.cull();
    renderer.render(device, queue, frame);
}

/// A picture as shown: its time, the scene's revision, the renderer's size and the view.
type Seen = (f64, u64, (u32, u32), Option<(CameraView, (u32, u32))>);

/// What the preview showed last, and whether the effects' memory on screen is short of what
/// the export would have there (after a jump in time, an edit or a resize).
#[derive(Default)]
pub struct Shown {
    last: Option<Seen>,
    cold: bool,
}

/// Whether the preview should keep drawing: a warm-up runs or is about to start.
pub fn busy(preview: &Preview) -> bool {
    preview.warm.is_some() || preview.shown.cold
}

/// After the preview has drawn: notes a jump in time (or an edit) on a picture whose effects
/// remember, starts a warm-up once the playhead has rested a frame, moves one along, or drops
/// it when it no longer fits. Returns whether the main renderer was swapped (its texture must be
/// pointed at the new one).
pub fn update(
    preview: &mut Preview,
    studio: &Studio,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    catching_up: bool,
    look: Option<(CameraView, (u32, u32))>,
) -> bool {
    let Some((time, revision)) = preview.drawn.as_ref().map(|d: &Drawn| (d.time, d.revision)) else { return false };
    let size = preview.renderer.size();
    let memory = preview.frame.memory() as f64;
    if studio.playing {
        // Playing draws every frame in turn; what it shows now is where it carries on from.
        preview.warm = None;
        preview.shown = Shown { last: Some((time, revision, size, look)), cold: false };
        return false;
    }
    if catching_up {
        // The playback is still on its way to the playhead: a jump, whatever the last step.
        preview.warm = None;
        preview.shown.cold = memory > 0.0;
        return false;
    }
    let now = (time, revision, size, look);
    let rested = preview.shown.last == Some(now);
    if let Some((t, r, z, l)) = preview.shown.last
        && (time < t - 1e-9 || time - t > LONGEST_STEP || r != revision || z != size || l != look)
    {
        preview.shown.cold = true;
    }
    preview.shown.last = Some(now);
    if memory <= 0.0 || time <= 0.0 {
        // Nothing remembers, or nothing to remember yet: what is drawn is what the export draws.
        preview.warm = None;
        preview.shown.cold = false;
        if memory <= 0.0 {
            preview.spare = None;
        }
        return false;
    }
    if let Some(w) = &preview.warm
        && (w.target != time || w.revision != revision || w.size != size || w.look != look)
    {
        preview.warm = None;
    }
    if preview.warm.is_none() {
        // Wait for the playhead to rest a frame, so scrubbing and dragging don't replay every frame.
        if !(preview.shown.cold && rested) {
            return false;
        }
        let every = 1.0 / fps(studio);
        let from = (time - memory).max(0.0);
        // The video's frames from there, as the export draws them.
        let first = ((from / every).ceil() * every).min(time);
        preview.warm = Some(Warmup {
            playback: studio.playback.rewound(first),
            frame: Frame::default(),
            from: first,
            at: first,
            target: time,
            every,
            revision,
            size,
            look,
        });
        let spare = preview.spare.get_or_insert_with(|| {
            let mut r = Renderer::new(device, size.0, size.1);
            r.share_programs(shared_programs());
            r
        });
        if spare.size() != size {
            spare.resize(device, size.0, size.1);
        }
        // It may hold the effects' memory of another stretch: start them afresh.
        spare.forget_effect_history();
    }
    let (Some(warm), Some(spare)) = (preview.warm.as_mut(), preview.spare.as_mut()) else { return false };
    for _ in 0..FRAMES_PER_UPDATE {
        if warm.at >= warm.target - 1e-9 {
            break;
        }
        let at = warm.at;
        draw_at(studio, device, queue, spare, &mut warm.frame, &mut warm.playback, at, warm.look);
        warm.at = ((at / warm.every).round() + 1.0) * warm.every;
    }
    if warm.at < warm.target - 1e-9 {
        return false;
    }
    // The playhead's own frame, then the warm renderer takes the place of the cold one.
    let target = warm.target;
    draw_at(studio, device, queue, spare, &mut warm.frame, &mut warm.playback, target, warm.look);
    std::mem::swap(&mut preview.renderer, spare);
    preview.warm = None;
    preview.shown.cold = false;
    true
}

#[cfg(test)]
mod tests {
    use super::super::draw_picture;
    use super::*;
    use bpe_core::{SlotId, ThingId};

    /// Draws the playhead's moment the way the preview does, until it has caught up.
    fn draw_now(studio: &mut Studio, gpu: &(wgpu::Device, wgpu::Queue), preview: &mut Preview) -> bool {
        let mut catching_up = true;
        for _ in 0..200 {
            catching_up = draw_picture(
                studio,
                &gpu.0,
                &gpu.1,
                &mut preview.renderer,
                &mut preview.frame,
                &mut preview.drawn,
                None,
            );
            if !catching_up {
                break;
            }
        }
        catching_up
    }

    fn preview(device: &wgpu::Device) -> Preview {
        Preview {
            renderer: Renderer::new(device, 135, 240),
            texture: eframe::egui::TextureId::default(),
            frame: Frame::default(),
            drag: None,
            menu: None,
            drawn: None,
            warm: None,
            spare: None,
            shown: Shown::default(),
        }
    }

    #[test]
    fn a_jump_with_a_feedback_effect_warms_up_to_what_a_straight_run_draws() {
        let Ok(gpu) = bpe_gfx::headless_device() else { return };
        let mut studio = Studio::headless();
        let mut scene = studio.scene.clone();
        let reg = &studio.kit.registry;
        bpe_kit::add_thing(&mut scene, reg, "balls", ThingId::ROOT).unwrap();
        scene.add_slot(reg, ThingId::ROOT, SlotId::Shader, bpe_kit::modules::effects::EFFECTS).unwrap();
        scene.add_layer(reg, ThingId::ROOT, SlotId::Shader, "picture.echo").unwrap();
        studio.replace_scene(scene, None);
        let target = 1.0;

        // A straight run: every video frame from the start, as the export draws them.
        let mut straight = preview(&gpu.0);
        for k in 0..=60 {
            studio.seek(k as f64 / 60.0);
            draw_now(&mut studio, &gpu, &mut straight);
        }
        let want = straight.renderer.read_output(&gpu.0, &gpu.1).unwrap();

        // A jump from the start to the target: cold at first, then warmed up.
        let mut jumped = preview(&gpu.0);
        studio.seek(0.0);
        let still = draw_now(&mut studio, &gpu, &mut jumped);
        assert!(!update(&mut jumped, &studio, &gpu.0, &gpu.1, still, None));
        studio.seek(target);
        let still = draw_now(&mut studio, &gpu, &mut jumped);
        let cold = jumped.renderer.read_output(&gpu.0, &gpu.1).unwrap();
        let mut swapped = false;
        assert!(!update(&mut jumped, &studio, &gpu.0, &gpu.1, still, None));
        assert!(busy(&jumped), "the jump is noted, and the warm-up starts once the playhead rests");
        for _ in 0..40 {
            if update(&mut jumped, &studio, &gpu.0, &gpu.1, still, None) {
                swapped = true;
                break;
            }
            assert!(jumped.warm.is_some(), "a warm-up runs after the jump");
        }
        assert!(swapped, "the warm-up finished");
        let warm = jumped.renderer.read_output(&gpu.0, &gpu.1).unwrap();
        let off = |a: &[u8], b: &[u8]| a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0);
        assert!(off(&cold, &want) > 8, "the cold frame lacks the echoes");
        assert!(off(&warm, &want) <= 3, "warmed up it draws what a straight run does: {}", off(&warm, &want));
        // Drawing it again doesn't start another.
        assert!(!update(&mut jumped, &studio, &gpu.0, &gpu.1, false, None));
        assert!(!update(&mut jumped, &studio, &gpu.0, &gpu.1, false, None));
        assert!(jumped.warm.is_none() && !busy(&jumped));
    }
}
