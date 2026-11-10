//! Effect warm-up. Effects that remember earlier frames (Feedback tunnel, Thermal vision, Rainbow
//! echoes…) build their picture up frame by frame, so a frame drawn straight after a jump in time
//! has no memory yet, while the export, which runs every frame from the start, has it. After a
//! jump the preview shows the frame at once, then replays the effects' remembered seconds on a
//! second renderer, as many frames as fit in [`WARM_BUDGET`] each editor frame, and swaps that
//! renderer in when it reaches the playhead, so the paused picture matches the export. Every
//! replayed frame is built as the preview builds its own ([`super::compose`]): the soloed lights,
//! the Show bar's light show, the light and sound the picture reads, and the particle playback's
//! particles where it has them (the playhead's own moment). The spare renderer is let go once
//! no warm-up has needed it for [`SPARE_KEEP`].

use std::time::{Duration, Instant};

use bpe_gfx::{Frame, LightShow, Renderer, wgpu};
use bpe_runtime::{CameraView, Playback};

use super::{Drawn, Preview};
use crate::app::Studio;
use crate::lights_panel::LightSource;

/// How long the replay may draw each editor frame (at least one frame is drawn).
pub const WARM_BUDGET: Duration = Duration::from_millis(8);
/// How long the spare renderer is kept after a warm-up, for the next one.
pub const SPARE_KEEP: Duration = Duration::from_secs(2);
/// A forward step in time longer than this is a jump (the effects' own MAX_STEP).
const LONGEST_STEP: f64 = 0.25;

/// What a lit picture shows, and the soloed lights.
type Light = (LightShow, Vec<LightSource>);

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
    light: Light,
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

/// What a lit picture shows now, and the soloed lights.
fn light_of(studio: &Studio) -> Light {
    (studio.light_show, studio.light_solo.iter().copied().collect())
}

/// Draws the frame at `time` from `playback` into `renderer`, as the preview does: through
/// [`super::compose`], with the particle playback's particles when they are of this step.
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
    let particles = studio.particle_job.as_ref().and_then(|job| job.latest());
    let lent = particles.filter(|p| p.step == playback.world().step).map(|p| playback.lend_states(p.lent()));
    // The handles read the grade the preview shows, not a warm-up moment's.
    let shown = studio.grade.shown.take();
    super::compose(studio, playback.world(), playback.log(), time, look, frame);
    studio.grade.shown.replace(shown);
    if let Some(lent) = lent {
        playback.take_back(lent);
    }
    frame.cull();
    renderer.render(device, queue, frame);
}

/// A picture as shown: its time, the scene's revision, the renderer's size, the view and the
/// light shown.
type Seen = (f64, u64, (u32, u32), Option<(CameraView, (u32, u32))>, Light);

/// What the preview showed last, and whether the effects' memory on screen is short of what
/// the export would have there (after a jump in time, an edit or a resize).
#[derive(Default)]
pub struct Shown {
    last: Option<Seen>,
    cold: bool,
    /// When the spare renderer was last used, while it is kept.
    spare_used: Option<Instant>,
}

/// Whether the preview should keep drawing: a warm-up runs or is about to start.
pub fn busy(preview: &Preview) -> bool {
    preview.warm.is_some() || preview.shown.cold
}

/// How long until the kept spare renderer is let go, so the preview looks again then.
pub fn spare_due(preview: &Preview) -> Option<Duration> {
    let used = preview.shown.spare_used.filter(|_| preview.spare.is_some() && preview.warm.is_none())?;
    Some(SPARE_KEEP.saturating_sub(used.elapsed()))
}

/// Whether a spare renderer last used at `used` has been kept long enough at `now`.
fn spare_expired(used: Option<Instant>, now: Instant) -> bool {
    used.is_some_and(|t| now.saturating_duration_since(t) >= SPARE_KEEP)
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
    if preview.warm.is_none() && spare_expired(preview.shown.spare_used, Instant::now()) {
        // No warm-up has needed it for a while: its pictures go from video memory.
        preview.spare = None;
        preview.shown.spare_used = None;
    }
    let Some((time, revision)) = preview.drawn.as_ref().map(|d: &Drawn| (d.time, d.revision)) else { return false };
    let size = preview.renderer.size();
    let memory = preview.frame.memory() as f64;
    let light = light_of(studio);
    if studio.playing {
        // Playing draws every frame in turn; what it shows now is where it carries on from.
        preview.warm = None;
        preview.shown.last = Some((time, revision, size, look, light));
        preview.shown.cold = false;
        return false;
    }
    if catching_up {
        // The playback is still on its way to the playhead: a jump, whatever the last step.
        preview.warm = None;
        preview.shown.cold = memory > 0.0;
        return false;
    }
    let now = (time, revision, size, look, light.clone());
    let rested = preview.shown.last.as_ref() == Some(&now);
    if let Some((t, r, z, l, s)) = &preview.shown.last
        && (time < t - 1e-9 || time - t > LONGEST_STEP || *r != revision || *z != size || *l != look || *s != light)
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
            preview.shown.spare_used = None;
        }
        return false;
    }
    if let Some(w) = &preview.warm
        && (w.target != time || w.revision != revision || w.size != size || w.look != look || w.light != light)
    {
        preview.warm = None;
    }
    if preview.warm.is_none() {
        // Wait for the playhead to rest a frame, so scrubbing and dragging don't replay every frame,
        // and for the picture's shaders to be built: a replay without an effect remembers nothing.
        if !(preview.shown.cold && rested) || preview.renderer.programs_waiting() {
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
            light,
        });
        // It shares the preview's own shader store (the editor's shared one).
        let store = preview.renderer.programs().clone();
        let spare = preview.spare.get_or_insert_with(|| {
            let mut r = Renderer::new(device, size.0, size.1);
            r.share_programs(&store);
            r.preview_field_like_video(true);
            r
        });
        if spare.size() != size {
            spare.resize(device, size.0, size.1);
        }
        // It may hold the effects' memory of another stretch: start them afresh.
        spare.forget_effect_history();
    }
    let (Some(warm), Some(spare)) = (preview.warm.as_mut(), preview.spare.as_mut()) else { return false };
    let started = Instant::now();
    while warm.at < warm.target - 1e-9 {
        let at = warm.at;
        draw_at(studio, device, queue, spare, &mut warm.frame, &mut warm.playback, at, warm.look);
        if spare.programs_waiting() {
            // A shader it draws with is building after all: start again once it is built (the
            // picture stays cold, so the warm-up starts over).
            preview.warm = None;
            preview.shown.spare_used = Some(Instant::now());
            return false;
        }
        warm.at = ((at / warm.every).round() + 1.0) * warm.every;
        if started.elapsed() >= WARM_BUDGET {
            break;
        }
    }
    preview.shown.spare_used = Some(Instant::now());
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
            clipping: None,
            clipped: Default::default(),
        }
    }

    /// The test scene with an echo effect (it remembers earlier frames), lit by a spot light (its
    /// id returned, to solo) and a point light.
    fn echo_studio() -> (Studio, ThingId) {
        let mut studio = Studio::headless();
        let mut scene = studio.scene.clone();
        let reg = &studio.kit.registry;
        bpe_kit::add_thing(&mut scene, reg, "balls", ThingId::ROOT).unwrap();
        scene.add_slot(reg, ThingId::ROOT, SlotId::Shader, bpe_kit::modules::effects::EFFECTS).unwrap();
        scene.add_layer(reg, ThingId::ROOT, SlotId::Shader, "picture.echo").unwrap();
        use bpe_kit::things::light::{LightType, add_light};
        let spot = add_light(&mut scene, reg, LightType::Spot, ThingId::ROOT).unwrap();
        add_light(&mut scene, reg, LightType::Point, ThingId::ROOT).unwrap();
        studio.replace_scene(scene, None);
        (studio, spot)
    }

    /// Every video frame from the start to `target`, as the export draws them: the picture a
    /// warm-up must end on.
    fn straight_run(studio: &mut Studio, gpu: &(wgpu::Device, wgpu::Queue), target: f64) -> Vec<u8> {
        let mut straight = preview(&gpu.0);
        for k in 0..=(target * 60.0).round() as u32 {
            studio.seek(k as f64 / 60.0);
            draw_now(studio, gpu, &mut straight);
        }
        straight.renderer.read_output(&gpu.0, &gpu.1).unwrap()
    }

    /// A jump from the start to `target`: the cold picture and the one the warm-up swaps in.
    fn jump_and_warm(studio: &mut Studio, gpu: &(wgpu::Device, wgpu::Queue), target: f64) -> (Vec<u8>, Vec<u8>) {
        let mut jumped = preview(&gpu.0);
        studio.seek(0.0);
        let still = draw_now(studio, gpu, &mut jumped);
        assert!(!update(&mut jumped, studio, &gpu.0, &gpu.1, still, None));
        studio.seek(target);
        let still = draw_now(studio, gpu, &mut jumped);
        let cold = jumped.renderer.read_output(&gpu.0, &gpu.1).unwrap();
        assert!(!update(&mut jumped, studio, &gpu.0, &gpu.1, still, None));
        assert!(busy(&jumped), "the jump is noted, and the warm-up starts once the playhead rests");
        let mut swapped = false;
        for _ in 0..1000 {
            if update(&mut jumped, studio, &gpu.0, &gpu.1, still, None) {
                swapped = true;
                break;
            }
            assert!(jumped.warm.is_some(), "a warm-up runs after the jump");
        }
        assert!(swapped, "the warm-up finished");
        (cold, jumped.renderer.read_output(&gpu.0, &gpu.1).unwrap())
    }

    fn off(a: &[u8], b: &[u8]) -> u8 {
        a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0)
    }

    #[test]
    fn a_warm_up_swaps_in_the_soloed_picture_the_preview_shows() {
        let Ok(gpu) = bpe_gfx::test_device() else { return };
        let (mut studio, spot) = echo_studio();
        let both = straight_run(&mut studio, &gpu, 1.0);
        studio.light_solo.insert(LightSource::Light(spot));
        let want = straight_run(&mut studio, &gpu, 1.0);
        assert!(off(&both, &want) > 8, "the spot light alone looks different");
        let (_, warm) = jump_and_warm(&mut studio, &gpu, 1.0);
        assert!(off(&warm, &want) <= 3, "the warm-up keeps the solo: {}", off(&warm, &want));
    }

    #[test]
    fn the_spare_renderer_goes_once_no_warm_up_has_needed_it_for_a_while() {
        let now = Instant::now();
        assert!(!spare_expired(None, now));
        assert!(!spare_expired(Some(now), now + SPARE_KEEP / 2));
        assert!(spare_expired(Some(now), now + SPARE_KEEP));
    }

    #[test]
    fn a_jump_with_a_feedback_effect_warms_up_to_what_a_straight_run_draws() {
        let Ok(gpu) = bpe_gfx::test_device() else { return };
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
        for _ in 0..1000 {
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
