//! Physics input blocks: a value from the run, mapped from its physics range onto 0 to 1,
//! curved and smoothed.

use crate::inputs::{HIT_INPUTS, LIVE_INPUTS, SCENE_INPUTS, Sources};
use crate::{BLOCK, Block, Buf, Ctx, Knobs};

/// Which values the block reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reads {
    /// Fixed when the note starts.
    Hit,
    /// The ball's track while the voice sounds.
    Live,
    /// The whole scene, on the video clock.
    Scene,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Curve {
    Linear,
    Exponential,
    S,
}

pub struct PhysicsIn {
    reads: Reads,
    /// The input's place in its table.
    index: usize,
    input: String,
    tag: String,
    channel: String,
    low: f32,
    high: f32,
    curve: Curve,
    smoothing: f32,
    sources: Sources,
    /// The value read once at the hit.
    fixed: f32,
    smoothed: Option<f32>,
}

impl PhysicsIn {
    pub fn new(reads: Reads, k: &Knobs) -> Self {
        let table = match reads {
            Reads::Hit => HIT_INPUTS,
            Reads::Live => LIVE_INPUTS,
            Reads::Scene => SCENE_INPUTS,
        };
        let input = k.choice("input", table[0].name).to_string();
        let index = table.iter().position(|d| d.name == input).unwrap_or(0);
        let curve = match k.choice("curve", "linear") {
            "exponential" => Curve::Exponential,
            "S" => Curve::S,
            _ => Curve::Linear,
        };
        Self {
            reads,
            index,
            input: table[index].name.to_string(),
            tag: k.text("tag").to_string(),
            channel: k.text("channel").to_string(),
            low: k.number("low", 0.0),
            high: k.number("high", 1.0),
            curve,
            smoothing: k.number("smoothing", 0.0).max(0.0),
            sources: Sources::default(),
            fixed: 0.0,
            smoothed: None,
        }
    }

    fn raw(&self, cx: &Ctx) -> f32 {
        match self.reads {
            Reads::Hit => self.fixed,
            Reads::Live => {
                self.sources.hit.as_ref().and_then(|h| h.track.as_ref()).map_or(0.0, |t| t.at(cx.since_on)[self.index])
            }
            Reads::Scene => self.sources.scene.as_ref().map_or(0.0, |s| s.value(&self.input, &self.channel, cx.time)),
        }
    }

    fn map(&self, raw: f32) -> f32 {
        let span = self.high - self.low;
        let t = if span.abs() < 1e-12 || !raw.is_finite() { 0.0 } else { ((raw - self.low) / span).clamp(0.0, 1.0) };
        match self.curve {
            Curve::Linear => t,
            Curve::Exponential => t * t,
            Curve::S => t * t * (3.0 - 2.0 * t),
        }
    }
}

impl Block for PhysicsIn {
    fn reset(&mut self, _cx: &Ctx) {
        self.smoothed = None;
    }

    fn bind(&mut self, sources: &Sources) {
        self.sources = sources.clone();
        self.fixed = match &sources.hit {
            Some(h) if self.input == "tag" => h.tags.contains(&self.tag) as u8 as f32,
            Some(h) => h.values.get(self.index).copied().unwrap_or(0.0),
            None => 0.0,
        };
        self.smoothed = None;
    }

    fn process(&mut self, cx: &Ctx, _inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        let target = self.map(self.raw(cx));
        let v = match self.smoothed {
            Some(last) if self.smoothing > 0.0 => {
                let a = 1.0 - (-(BLOCK as f32 / cx.rate) / (self.smoothing / 1000.0)).exp();
                last + (target - last) * a
            }
            _ => target,
        };
        self.smoothed = Some(v);
        outputs[0] = [[v; BLOCK]; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inputs::{HitPhysics, SceneSignals, Track};
    use bpe_graph::Param;
    use std::sync::Arc;

    fn knobs(input: &str, low: f32, high: f32) -> Knobs {
        Knobs::default()
            .with("input", Param::Choice(input.into()))
            .with("low", Param::Number(low))
            .with("high", Param::Number(high))
    }

    fn hit(values: &[(usize, f32)]) -> Sources {
        let mut h = HitPhysics::default();
        for &(i, v) in values {
            h.values[i] = v;
        }
        Sources { hit: Some(Arc::new(h)), scene: None }
    }

    /// The block's first output after `blocks` blocks from `cx`.
    fn read(block: &mut PhysicsIn, cx: &mut Ctx, blocks: usize) -> f32 {
        let mut out = [crate::SILENT];
        let mut v = 0.0;
        for _ in 0..blocks.max(1) {
            block.process(cx, &[], &mut out);
            v = out[0][0][0];
            cx.advance();
        }
        v
    }

    fn once(k: Knobs, reads: Reads, sources: &Sources, cx: Ctx) -> f32 {
        let mut b = PhysicsIn::new(reads, &k);
        b.reset(&cx);
        b.bind(sources);
        read(&mut b, &mut cx.clone(), 1)
    }

    #[test]
    fn hit_input_maps_its_range() {
        let at = |raw: f32, curve: &str| {
            once(
                knobs("speed", 0.0, 1200.0).with("curve", Param::Choice(curve.into())),
                Reads::Hit,
                &hit(&[(0, raw)]),
                Ctx::default(),
            )
        };
        assert!((at(600.0, "linear") - 0.5).abs() < 1e-6);
        assert!((at(600.0, "exponential") - 0.25).abs() < 1e-6);
        assert!((at(600.0, "S") - 0.5).abs() < 1e-6);
        assert_eq!(at(2000.0, "linear"), 1.0);
        assert_eq!(at(-5.0, "linear"), 0.0);
    }

    #[test]
    fn smoothing_starts_at_the_value() {
        let k = knobs("speed", 0.0, 1200.0).with("smoothing", Param::Number(500.0));
        assert_eq!(once(k, Reads::Hit, &hit(&[(0, 1200.0)]), Ctx::default()), 1.0);
    }

    #[test]
    fn live_input_follows_the_track_and_holds_the_last_value() {
        let track = |heights: &[f32]| {
            let samples = heights.iter().map(|&h| [0.0, h, 0.0, 0.0]).collect();
            let h = HitPhysics { track: Some(Arc::new(Track { rate: 60.0, samples })), ..HitPhysics::default() };
            Sources { hit: Some(Arc::new(h)), scene: None }
        };
        let falling = track(&[1.0, 0.5, 0.0]);
        let at = |sources: &Sources, since_on: f32| {
            once(knobs("height", 0.0, 1.0), Reads::Live, sources, Ctx { since_on, ..Ctx::default() })
        };
        assert_eq!(at(&falling, 0.0), 1.0);
        assert!((at(&falling, 1.0 / 120.0) - 0.75).abs() < 0.01);
        assert_eq!(at(&falling, 1.0), 0.0);
        assert_eq!(at(&falling, 5.0), 0.0);
        assert!((at(&track(&[1.0, 0.3]), 20.0) - 0.3).abs() < 1e-6);
    }

    #[test]
    fn scene_input_reads_the_video_clock() {
        let scene = SceneSignals {
            rate: 60.0,
            balls: vec![0.0, 10.0, 20.0],
            beats: vec![1.0, 1.5, 2.0],
            ..SceneSignals::default()
        };
        let sources = Sources { hit: None, scene: Some(Arc::new(scene)) };
        let at = |k: Knobs, time: f64| once(k, Reads::Scene, &sources, Ctx { time, ..Ctx::default() });
        assert!((at(knobs("balls", 0.0, 20.0), 1.0 / 60.0) - 0.5).abs() < 1e-5);
        assert!((at(knobs("beat", 0.0, 1.0), 1.25) - 0.5).abs() < 1e-5);
        assert!((at(knobs("bar", 0.0, 1.0), 1.25) - 0.125).abs() < 1e-5);
    }

    #[test]
    fn tag_input_matches_the_other_things_tags() {
        let h = HitPhysics { tags: vec!["pegs".into()], ..HitPhysics::default() };
        let sources = Sources { hit: Some(Arc::new(h)), scene: None };
        let with = |tag: &str| {
            once(knobs("tag", 0.0, 1.0).with("tag", Param::Text(tag.into())), Reads::Hit, &sources, Ctx::default())
        };
        assert_eq!(with("pegs"), 1.0);
        assert_eq!(with("walls"), 0.0);
    }

    #[test]
    fn channel_level_reads_its_channel() {
        let scene =
            SceneSignals { levels: [("Kick".to_string(), Arc::new(vec![-6.0; 10]))].into(), ..SceneSignals::default() };
        let sources = Sources { hit: None, scene: Some(Arc::new(scene)) };
        let with = |channel: &str| {
            let k = knobs("channel", -48.0, -6.0).with("channel", Param::Text(channel.into()));
            once(k, Reads::Scene, &sources, Ctx::default())
        };
        assert_eq!(with("Kick"), 1.0);
        assert_eq!(with("Snare"), 0.0);
    }
}
