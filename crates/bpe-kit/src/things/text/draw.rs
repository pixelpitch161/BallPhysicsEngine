//! Laying a Text thing out for the frame being drawn.

use bpe_core::{Ease, Settings, ThingId, Value};
use bpe_gfx::text::{TextStyle, layout_text};
use bpe_gfx::{DrawCx, Frame, ThingDraw, hit_strength};
use bpe_runtime::EventKind;
use glam::Vec2;

use super::fill::{Values, fill};
use super::{Appear, TextSettings};
use crate::modules::look::draw::look_of;

pub struct TextDraw;

impl ThingDraw for TextDraw {
    fn kind(&self) -> &'static str {
        "text"
    }

    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let s = TextSettings::from_params(&cx.resolved.params);
        if s.text.trim().is_empty() {
            return;
        }
        let Some(look) = look_of(cx) else { return };
        let duration = match cx.scene.things.get(&ThingId::ROOT).and_then(|r| r.params.get("duration")) {
            Some(Value::Number(d)) => *d as f32,
            _ => 0.0,
        };
        let from = s.show_from as f64;
        let to = if s.show_to > 0.0 {
            s.show_to as f64
        } else if duration > 0.0 {
            duration as f64
        } else {
            f64::INFINITY
        };
        // Clocks, counters and show windows are the video's; the run's log is read at its own time.
        let time = cx.video_time;
        if time < from || time > to {
            return;
        }
        let appear = s.appear_seconds.max(0.05) as f64;
        let coming = ((time - from) / appear).clamp(0.0, 1.0) as f32;
        let going = if to.is_finite() { ((to - time) / appear).clamp(0.0, 1.0) as f32 } else { 1.0 };
        let progress = coming.min(going);
        let chars = s.text.chars().count();
        let (mut alpha, mut scale, mut offset, mut visible) = (1.0f32, 1.0f32, Vec2::ZERO, usize::MAX);
        match s.appear {
            Appear::None => {}
            Appear::Fade => alpha = progress,
            Appear::Pop => {
                scale = 0.6 + 0.4 * Ease::Overshoot.apply(coming);
                alpha = (coming * 3.0).min(1.0) * going;
            }
            Appear::Rise => {
                offset.y = (1.0 - Ease::EaseOut.apply(coming)) * 0.3 * s.size;
                alpha = progress;
            }
            Appear::Typewriter => {
                visible = (coming * chars as f32).ceil() as usize;
                alpha = going;
            }
        }
        if s.jump > 0.0 {
            scale += s.jump * 0.35 * hit_strength(cx.log, cx.time, cx.world.steps_per_second);
        }
        let rate = cx.world.steps_per_second.max(1);
        let last_step = (cx.time * rate as f64 + 1e-6).floor().max(0.0) as u64;
        let hits = cx.log.events()[..cx.log.events().partition_point(|e| e.step <= last_step)]
            .iter()
            .filter(|e| e.kind == EventKind::Hit)
            .count() as u32;
        let balls = cx.world.bodies.values().map(|b| b.alive_count() as u32).sum();
        let text = fill(&s.text, &Values { time, duration, hits, balls });
        let faded = |c: [f32; 4]| [c[0], c[1], c[2], c[3] * alpha];
        let style = TextStyle {
            color: faded(look.color.to_linear()),
            outline: s.outline,
            outline_color: faded(s.outline_color.to_linear()),
            glow: look.glow,
        };
        layout_text(
            &text,
            s.font.face(),
            s.size * scale,
            cx.thing.transform.position + offset,
            s.align.text_align(),
            s.max_width * frame.width as f32,
            visible,
            &style,
            &mut frame.glyphs,
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::{kit, new_scene};
    use bpe_core::{Params, ThingId, Value, resolve};
    use bpe_gfx::{Frame, build_frame_at};
    use bpe_runtime::{Log, World};

    fn glyphs_at(time: f64, f: impl FnOnce(&mut Params)) -> usize {
        glyphs_at_both(time, time, f)
    }

    /// The glyphs drawn at a moment `time` of the run shown at `video_time` of the video.
    fn glyphs_at_both(time: f64, video_time: f64, f: impl FnOnce(&mut Params)) -> usize {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let text = scene.add_thing(&kit.registry, "text", ThingId::ROOT).unwrap();
        f(&mut scene.things.get_mut(&text).unwrap().params);
        let resolved = resolve(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame_at(&scene, &resolved, &world, &Log::default(), time, video_time, &kit.draws, &mut frame);
        // Text sticks to the screen, so it draws into the overlay.
        assert!(frame.glyphs.is_empty());
        frame.overlay.map_or(0, |o| o.glyphs.len())
    }

    #[test]
    fn a_caption_shows_by_the_video_clock_under_beat_sync() {
        let window = |p: &mut Params| p.set("show_from", Value::Number(1.5));
        assert_eq!(glyphs_at_both(1.0, 1.0, window), 0, "not yet on either clock");
        assert_eq!(glyphs_at_both(1.0, 2.0, window), 5, "the video has reached the window");
        assert_eq!(glyphs_at_both(2.0, 1.0, window), 0, "the run has, but the video hasn't");
        let clock = |p: &mut Params| p.set("text", Value::Text("{seconds}".into()));
        assert_eq!(glyphs_at_both(3.0, 12.0, clock), 2, "1 2: the video's seconds, not the run's");
    }

    #[test]
    fn a_text_draws_glyphs_inside_its_window_only() {
        let window = |p: &mut Params| {
            p.set("show_from", Value::Number(1.0));
            p.set("show_to", Value::Number(2.0));
        };
        assert_eq!(glyphs_at(0.5, window), 0);
        assert_eq!(glyphs_at(1.5, window), 5, "H e l l o");
        assert_eq!(glyphs_at(2.5, window), 0);
        // With no end it shows to the video's end, and not after.
        assert_eq!(glyphs_at(19.0, |_| {}), 5);
        assert_eq!(glyphs_at(21.0, |_| {}), 0);
    }

    #[test]
    fn typewriter_reveals_letters_over_time() {
        let typing = |p: &mut Params| {
            p.set("appear", Value::Choice("typewriter".into()));
            p.set("appear_seconds", Value::Number(1.0));
        };
        assert_eq!(glyphs_at(0.2, typing), 1);
        assert_eq!(glyphs_at(0.5, typing), 3);
        assert_eq!(glyphs_at(1.0, typing), 5);
        assert_eq!(glyphs_at(5.0, typing), 5);
    }

    #[test]
    fn empty_text_draws_nothing() {
        assert_eq!(glyphs_at(1.0, |p| p.set("text", Value::Text("   ".into()))), 0);
        assert_eq!(glyphs_at(1.0, |p| p.set("text", Value::Text("{hits} hits".into()))), 5, "0 h i t s");
    }
}
