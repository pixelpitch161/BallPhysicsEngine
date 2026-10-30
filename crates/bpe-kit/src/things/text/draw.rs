//! Laying a Text thing out for the frame being drawn.

use bpe_core::{Ease, Settings, ThingId, Value};
use bpe_gfx::text::{TextStyle, layout_text};
use bpe_gfx::{DrawCx, Frame, ThingDraw, blend_alpha, hit_strength};
use bpe_runtime::EventKind;
use glam::Vec2;

use super::fill::{Values, fill};
use super::{Appear, TextSettings};
use crate::modules::look::draw::look_of;
use crate::paint::{Follow, Inputs, Inside, Paint, beats_at, colour_at, paint_gpu};

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
        // The colours are painted on each glyph below.
        let style = TextStyle { color: [1.0; 4], outline: s.outline, outline_color: [1.0; 4], glow: look.glow };
        let (px, anchor, align, fit) = (
            s.size * scale,
            cx.thing.transform.position + offset,
            s.align.text_align(),
            s.max_width * frame.width as f32,
        );
        let face = s.font.face();
        let first = frame.glyphs.len();
        layout_text(&text, face, px, anchor, align, fit, visible, &style, &mut frame.glyphs);
        // Every letter of the text, typed in yet or not, so the colours stay put while it types.
        let all: Vec<[f32; 4]> = if visible == usize::MAX {
            frame.glyphs[first..].iter().map(|g| g.rect).collect()
        } else {
            let mut every = Vec::new();
            layout_text(&text, face, px, anchor, align, fit, usize::MAX, &style, &mut every);
            every.iter().map(|g| g.rect).collect()
        };
        let fill = for_text(look.fill);
        let outline = for_text(s.outline_paint);
        let whole = all.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, r| {
            [b[0].min(r[0]), b[1].min(r[1]), b[2].max(r[2]), b[3].max(r[3])]
        });
        let centre = |r: [f32; 4]| Vec2::new((r[0] + r[2]) * 0.5, (r[1] + r[3]) * 0.5);
        let steps_per_second = cx.world.steps_per_second;
        let beats = if fill.follow == Follow::Beat || outline.follow == Follow::Beat {
            beats_at(cx.log, cx.video_time, steps_per_second)
        } else {
            0.0
        };
        let count = if s.paint_per_letter { all.len() } else { 1 };
        let (screen, camera) = (cx.resolved.screen, cx.world.camera);
        for (k, g) in frame.glyphs[first..].iter_mut().enumerate() {
            let (index, local) = if s.paint_per_letter { (k, g.rect) } else { (0, whole) };
            let place = centre(local);
            let inputs = Inputs {
                index,
                count,
                place,
                place_screen: if screen { place } else { camera.to_screen(place) },
                start: place,
                beats,
                time: cx.time as f32,
                ..Inputs::default()
            };
            g.color = blend_alpha(faded(colour_at(&fill, &inputs)));
            g.outline_color = blend_alpha(faded(colour_at(&outline, &inputs)));
            g.local = local;
            if fill.inside != Inside::None {
                g.paint = paint_gpu(&fill, &inputs, &mut frame.gradients, cx.time as f32);
            }
        }
    }
}

/// `p` as text reads it: a letter never spawns and its place along the line is its order, so
/// Start reads as Place and Path as Order. Speed, hits, steps and life all read 0.
fn for_text(mut p: Paint) -> Paint {
    p.follow = match p.follow {
        Follow::Start => Follow::Place,
        Follow::Path => Follow::Order,
        f => f,
    };
    p
}

#[cfg(test)]
mod tests {
    use crate::{kit, new_scene};
    use bpe_core::{Color, Gradient, Params, SlotId, ThingId, Value, resolve};
    use bpe_gfx::{Frame, GlyphInstance, PaintGpu, build_frame_at};
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

    const RED: Color = Color::rgb(255, 0, 0);
    const GREEN: Color = Color::rgb(0, 255, 0);
    const BLUE: Color = Color::rgb(0, 0, 255);

    /// The glyphs of a text saying `text` in a classic look set up by `look`, its own settings
    /// set up by `f`, one second in.
    fn painted_glyphs(
        text: &str,
        look: impl FnOnce(&mut Params),
        f: impl FnOnce(&mut Params),
    ) -> (Vec<GlyphInstance>, usize) {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let id = scene.add_thing(&kit.registry, "text", ThingId::ROOT).unwrap();
        let params = &mut scene.things.get_mut(&id).unwrap().params;
        params.set("text", Value::Text(text.into()));
        f(params);
        let module = scene.add_module(&kit.registry, "look.classic", "Text colours").unwrap();
        look(&mut scene.library.modules.get_mut(&module).unwrap().params);
        scene.link(&kit.registry, id, SlotId::Look, module).unwrap();
        let resolved = resolve(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame_at(&scene, &resolved, &world, &Log::default(), 1.0, 1.0, &kit.draws, &mut frame);
        let o = frame.overlay.unwrap();
        (o.glyphs.clone(), o.gradients.len())
    }

    fn rgb_order(p: &mut Params) {
        p.set("colours", Value::Gradient(Gradient::even(&[RED, GREEN, BLUE])));
        p.set("follow", Value::Choice("order".into()));
    }

    #[test]
    fn text_per_letter_order_colours_each_letter() {
        let (glyphs, rows) = painted_glyphs("ABC", rgb_order, |_| {});
        let colours: Vec<[f32; 4]> = glyphs.iter().map(|g| g.color).collect();
        assert_eq!(colours, [RED.to_linear(), GREEN.to_linear(), BLUE.to_linear()]);
        assert!(glyphs.iter().all(|g| g.paint == PaintGpu::default()), "no inside pattern");
        assert_eq!(rows, 0);
        // Each letter's own box.
        assert!(glyphs.iter().all(|g| g.local == g.rect));
        // All one colour, and one box over the line, when the letters share it.
        let whole = |p: &mut Params| p.set("paint_per_letter", Value::Bool(false));
        let (glyphs, _) = painted_glyphs("ABC", rgb_order, whole);
        assert!(glyphs.iter().all(|g| g.color == RED.to_linear()));
        let line = [glyphs[0].rect[0], glyphs[0].rect[1].min(glyphs[2].rect[1]), glyphs[2].rect[2], glyphs[0].rect[3]];
        assert!(glyphs.iter().all(|g| g.local[0] == line[0] && g.local[2] == line[2]), "{glyphs:?}");
    }

    #[test]
    fn typing_in_keeps_each_letter_s_colour() {
        let typing = |p: &mut Params| {
            p.set("appear", Value::Choice("typewriter".into()));
            p.set("appear_seconds", Value::Number(2.0));
        };
        let (glyphs, _) = painted_glyphs("ABC", rgb_order, typing);
        let colours: Vec<[f32; 4]> = glyphs.iter().map(|g| g.color).collect();
        assert_eq!(colours, [RED.to_linear(), GREEN.to_linear()], "half way: two of the three letters");
    }

    #[test]
    fn text_outline_and_inside_pattern_paint_too() {
        let striped = |p: &mut Params| {
            rgb_order(p);
            p.set("inside", Value::Choice("stripes".into()));
        };
        let outlined = |p: &mut Params| {
            p.set("outline_colours", Value::Gradient(Gradient::even(&[BLUE, RED])));
            p.set("outline_follow", Value::Choice("order".into()));
        };
        let (glyphs, rows) = painted_glyphs("AB", striped, outlined);
        assert_eq!(rows, 1);
        assert!(glyphs.iter().all(|g| g.paint.row == 0.0 && g.paint.pattern == 1.0));
        assert_eq!((glyphs[0].paint.t0, glyphs[1].paint.t0), (0.0, 1.0), "each letter starts at its own place");
        assert_eq!(glyphs[0].outline_color, BLUE.to_linear());
        assert_eq!(glyphs[1].outline_color, RED.to_linear());
    }

    #[test]
    fn empty_text_draws_nothing() {
        assert_eq!(glyphs_at(1.0, |p| p.set("text", Value::Text("   ".into()))), 0);
        assert_eq!(glyphs_at(1.0, |p| p.set("text", Value::Text("{hits} hits".into()))), 5, "0 h i t s");
    }
}
