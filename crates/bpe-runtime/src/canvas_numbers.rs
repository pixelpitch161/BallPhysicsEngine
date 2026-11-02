//! The live numbers a canvas reports: how much of it is covered, revealed or wet, and in a colour
//! war each team's share of it. The canvas's sim works them out and hands them to the world with
//! [`World::report_canvas`]; captions, rules, the synth and the Meter read them back by the
//! canvas's name with a [`CanvasRef`], written the way a caption writes it:
//! `Canvas.revealed`, `Canvas.share Red`, `Canvas.leader`.
//!
//! Nothing here changes how anything moves: the numbers stay out of the log's hash.

use bpe_core::ThingId;

use crate::{Log, World};

/// The numbers a canvas reports, by the names captions, rules and the synth use for them.
/// `leader` is a team's name, not a number, so only captions and the Meter read it.
pub const CANVAS_NUMBERS: [&str; 5] = ["covered", "revealed", "wet", "share", "leader"];

/// One side in a colour war on one canvas.
#[derive(Clone, Debug, PartialEq)]
pub struct Team {
    /// What captions, rules and the synth call it: the claiming thing's name (team By thing) or
    /// its colour's name (team By colour), made unique on its canvas with " 2", " 3" and so on.
    pub name: String,
    /// Linear light, alpha 1: the colour its ground is drawn in.
    pub colour: [f32; 4],
}

/// What a canvas reports at one moment. Every number is 0 to 1.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CanvasNumbers {
    /// Paint canvases: the share of the canvas with paint on it. Covers, powder and fog: the
    /// share still there (1 − revealed).
    pub covered: f32,
    /// Covers, powder and fog: the share scraped away. Paint canvases: 0.
    pub revealed: f32,
    /// Paint canvases: the share of the painted cells whose paint is still wet. Others: 0.
    pub wet: f32,
    /// Per team, in the order the teams first claimed ground (the order of
    /// [`World::teams`]): its share of the whole canvas. They add up to at most 1; the rest is
    /// unclaimed ([`CanvasNumbers::unclaimed`]).
    pub shares: Vec<f32>,
}

impl CanvasNumbers {
    /// Team `team`'s share, 0 for a team with no entry yet.
    pub fn share(&self, team: usize) -> f32 {
        self.shares.get(team).copied().unwrap_or(0.0)
    }

    /// The share of the canvas no team holds.
    pub fn unclaimed(&self) -> f32 {
        (1.0 - self.shares.iter().sum::<f32>()).clamp(0.0, 1.0)
    }

    /// The index of the leading team: the largest share above 0; on a tie the team that claimed
    /// first. `None` while nothing is claimed.
    pub fn leader(&self) -> Option<usize> {
        let mut best: Option<usize> = None;
        for (i, &s) in self.shares.iter().enumerate() {
            if s > 0.0 && best.is_none_or(|b| s > self.shares[b]) {
                best = Some(i);
            }
        }
        best
    }
}

/// One moment of one canvas's numbers in the log.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasSample {
    pub step: u64,
    pub canvas: ThingId,
    pub numbers: CanvasNumbers,
}

/// A number of a named canvas, as a caption token, a rule or a synth input names it:
/// `<canvas name>.<number>` or `<canvas name>.share <team name>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanvasRef {
    /// The canvas's name, trimmed, as written (matched without caring about case).
    pub canvas: String,
    /// One of [`CANVAS_NUMBERS`], lowercase.
    pub number: String,
    /// For `share`: the team's name, trimmed (matched without caring about case). Empty
    /// otherwise.
    pub team: String,
}

impl CanvasRef {
    /// Reads `Canvas.revealed`, `My canvas.share Red team` or `Canvas.leader`: the canvas's name
    /// is everything before the last `.`, then a number from [`CANVAS_NUMBERS`] (any case), then
    /// for `share` a space and the team's name. `None` when the text is not one of these: no
    /// `.`, an empty canvas name, an unknown number, a `share` without a team, or a team after
    /// another number.
    pub fn parse(text: &str) -> Option<CanvasRef> {
        let (canvas, rest) = text.rsplit_once('.')?;
        let canvas = canvas.trim();
        if canvas.is_empty() {
            return None;
        }
        let rest = rest.trim();
        let (number, team) = match rest.split_once(char::is_whitespace) {
            Some((number, team)) => (number, team.trim()),
            None => (rest, ""),
        };
        let number = number.to_lowercase();
        if !CANVAS_NUMBERS.contains(&number.as_str()) || (number == "share") == team.is_empty() {
            return None;
        }
        Some(CanvasRef { canvas: canvas.to_string(), number, team: team.to_string() })
    }

    /// The number this names in `numbers`, 0 to 1, with `teams` the canvas's teams. `None` for
    /// `leader` and for a team the canvas doesn't have (yet).
    pub fn value(&self, numbers: &CanvasNumbers, teams: &[Team]) -> Option<f32> {
        match self.number.as_str() {
            "covered" => Some(numbers.covered),
            "revealed" => Some(numbers.revealed),
            "wet" => Some(numbers.wet),
            "share" => team_index(teams, &self.team).map(|i| numbers.share(i)),
            _ => None,
        }
    }

    /// The text a caption shows for it: a whole percent (`37`, no `%` sign) for a number, the
    /// leading team's name for `leader` (empty while nothing is claimed). `None` for a team the
    /// canvas doesn't have (yet).
    pub fn text(&self, numbers: &CanvasNumbers, teams: &[Team]) -> Option<String> {
        if self.number == "leader" {
            return Some(numbers.leader().and_then(|i| teams.get(i)).map_or_else(String::new, |t| t.name.clone()));
        }
        self.value(numbers, teams).map(|v| format!("{}", (v.clamp(0.0, 1.0) * 100.0).round() as u32))
    }
}

/// The index of the team called `name` (trimmed, any case).
pub fn team_index(teams: &[Team], name: &str) -> Option<usize> {
    let name = name.trim();
    teams.iter().position(|t| t.name.eq_ignore_ascii_case(name))
}

impl World {
    /// The first canvas thing in the tree called `name` (trimmed, any case).
    pub fn canvas_named(&self, name: &str) -> Option<ThingId> {
        let name = name.trim();
        self.order().iter().copied().find(|id| {
            self.kind_of(*id) == Some("canvas")
                && self.names.get(id).is_some_and(|n| n.trim().eq_ignore_ascii_case(name))
        })
    }

    /// The numbers `canvas` reported last; `None` before its first report.
    pub fn canvas_numbers_of(&self, canvas: ThingId) -> Option<&CanvasNumbers> {
        self.canvas_numbers.get(&canvas)
    }

    /// The teams of `canvas`'s colour war, in the order they first claimed; empty without one.
    pub fn teams_of(&self, canvas: ThingId) -> &[Team] {
        self.teams.get(&canvas).map_or(&[], |t| t.as_slice())
    }

    /// The value `r` names right now, 0 to 1; `None` when its canvas or team is unknown or it
    /// names `leader`.
    pub fn canvas_value(&self, r: &CanvasRef) -> Option<f32> {
        let canvas = self.canvas_named(&r.canvas)?;
        r.value(self.canvas_numbers_of(canvas)?, self.teams_of(canvas))
    }

    /// What a caption shows for `r` right now (see [`CanvasRef::text`]); `None` when its canvas
    /// or team is unknown.
    pub fn canvas_text(&self, r: &CanvasRef) -> Option<String> {
        let canvas = self.canvas_named(&r.canvas)?;
        r.text(self.canvas_numbers_of(canvas)?, self.teams_of(canvas))
    }

    /// A canvas's sim hands in its numbers. They become the canvas's current numbers, and on
    /// every step that is a multiple of `steps_per_second / 60` they are also written into the
    /// log as a [`CanvasSample`]. Call it once per step per canvas, in the Events stage.
    pub fn report_canvas(&mut self, canvas: ThingId, numbers: CanvasNumbers, log: &mut Log) {
        let every = (self.steps_per_second / crate::MOTION_RATE).max(1) as u64;
        if self.step.is_multiple_of(every) {
            log.push_canvas(CanvasSample { step: self.step, canvas, numbers: numbers.clone() });
        }
        self.canvas_numbers.insert(canvas, numbers);
    }

    /// A canvas's sim hands in its teams whenever one is added: they become the canvas's teams
    /// here and in the log.
    pub fn set_teams(&mut self, canvas: ThingId, teams: Vec<Team>, log: &mut Log) {
        log.set_teams(canvas, teams.clone());
        self.teams.insert(canvas, teams);
    }
}

impl Log {
    /// Adds a moment of a canvas's numbers. Samples arrive in step order.
    pub fn push_canvas(&mut self, s: CanvasSample) {
        self.canvas.push(s);
    }

    /// Every canvas's numbers, 60 times a second, in step order (canvases in tree order within a
    /// step).
    pub fn canvas_samples(&self) -> &[CanvasSample] {
        &self.canvas
    }

    /// The samples of `canvas`, in step order.
    pub fn canvas_samples_of(&self, canvas: ThingId) -> impl Iterator<Item = &CanvasSample> {
        self.canvas.iter().filter(move |s| s.canvas == canvas)
    }

    /// The latest sample of `canvas` at or before `step`.
    pub fn canvas_at(&self, canvas: ThingId, step: u64) -> Option<&CanvasNumbers> {
        let end = self.canvas.partition_point(|s| s.step <= step);
        self.canvas[..end].iter().rev().find(|s| s.canvas == canvas).map(|s| &s.numbers)
    }

    /// Notes `canvas`'s teams (the whole list, which only ever grows during a run).
    pub fn set_teams(&mut self, canvas: ThingId, teams: Vec<Team>) {
        self.teams.insert(canvas, teams);
    }

    /// `canvas`'s teams as last noted; empty without a colour war.
    pub fn teams_of(&self, canvas: ThingId) -> &[Team] {
        self.teams.get(&canvas).map_or(&[], |t| t.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn team(name: &str) -> Team {
        Team { name: name.into(), colour: [1.0, 0.0, 0.0, 1.0] }
    }

    #[test]
    fn refs_parse_the_way_captions_write_them() {
        let r = CanvasRef::parse("Canvas.revealed").unwrap();
        assert_eq!((r.canvas.as_str(), r.number.as_str(), r.team.as_str()), ("Canvas", "revealed", ""));
        let r = CanvasRef::parse(" My card.Share  Red team ").unwrap();
        assert_eq!((r.canvas.as_str(), r.number.as_str(), r.team.as_str()), ("My card", "share", "Red team"));
        // The name is everything before the last dot.
        assert_eq!(CanvasRef::parse("v1.2.covered").unwrap().canvas, "v1.2");
        assert_eq!(CanvasRef::parse("Arena.leader").unwrap().number, "leader");
        for bad in ["revealed", ".revealed", "Canvas.", "Canvas.size", "Canvas.share", "Canvas.covered Red", "time"] {
            assert_eq!(CanvasRef::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn values_and_texts() {
        let n = CanvasNumbers { covered: 0.25, revealed: 0.756, wet: 0.5, shares: vec![0.1, 0.3, 0.3] };
        let teams = [team("Red"), team("Blue"), team("Gold")];
        let get = |t: &str| CanvasRef::parse(t).unwrap();
        assert_eq!(get("C.covered").value(&n, &teams), Some(0.25));
        assert_eq!(get("C.share blue").value(&n, &teams), Some(0.3));
        assert_eq!(get("C.share Green").value(&n, &teams), None);
        assert_eq!(get("C.leader").value(&n, &teams), None);
        assert_eq!(get("C.revealed").text(&n, &teams).as_deref(), Some("76"));
        // A tie goes to the team that claimed first.
        assert_eq!(get("C.leader").text(&n, &teams).as_deref(), Some("Blue"));
        assert_eq!(get("C.leader").text(&CanvasNumbers::default(), &teams).as_deref(), Some(""));
        assert!((n.unclaimed() - 0.3).abs() < 1e-6);
        assert_eq!(n.share(7), 0.0);
    }

    #[test]
    fn the_log_keeps_samples_and_teams_out_of_its_hash() {
        let mut log = Log::default();
        let before = log.hash();
        let n = |c: f32| CanvasNumbers { covered: c, ..Default::default() };
        log.push_canvas(CanvasSample { step: 4, canvas: ThingId(3), numbers: n(0.1) });
        log.push_canvas(CanvasSample { step: 4, canvas: ThingId(5), numbers: n(0.9) });
        log.push_canvas(CanvasSample { step: 8, canvas: ThingId(3), numbers: n(0.2) });
        log.set_teams(ThingId(3), vec![team("Red")]);
        assert_eq!(log.hash(), before);
        assert_eq!(log.canvas_at(ThingId(3), 7).map(|n| n.covered), Some(0.1));
        assert_eq!(log.canvas_at(ThingId(3), 8).map(|n| n.covered), Some(0.2));
        assert_eq!(log.canvas_at(ThingId(5), 3), None);
        assert_eq!(log.canvas_samples_of(ThingId(3)).count(), 2);
        assert_eq!(log.teams_of(ThingId(3))[0].name, "Red");
        log.truncate_motion(5);
        assert_eq!(log.canvas_samples().len(), 2);
    }

    #[test]
    fn a_world_reports_at_60_hz_and_finds_canvases_by_name() {
        let mut w = World::build(&bpe_core::Scene::default(), &bpe_core::Resolved::default(), &crate::Sims::default());
        let (canvas, other) = (ThingId(7), ThingId(8));
        for (id, kind, name) in [(canvas, "canvas", "Scratch Card"), (other, "text", "Scratch card")] {
            w.kinds.insert(id, kind.into());
            w.names.insert(id, name.into());
        }
        w.order.extend([other, canvas]);
        assert_eq!(w.canvas_named(" scratch card "), Some(canvas));
        assert_eq!(w.canvas_named("Nope"), None);
        let mut log = Log::default();
        let n = CanvasNumbers { revealed: 0.5, shares: vec![0.2], ..Default::default() };
        // 240 steps a second: a sample every 4th step.
        for step in 1..=8 {
            w.step = step;
            w.report_canvas(canvas, n.clone(), &mut log);
        }
        assert_eq!(log.canvas_samples().iter().map(|s| s.step).collect::<Vec<_>>(), [4, 8]);
        w.set_teams(canvas, vec![team("Red")], &mut log);
        let r = CanvasRef::parse("scratch card.share red").unwrap();
        assert_eq!(w.canvas_value(&r), Some(0.2));
        assert_eq!(w.canvas_text(&CanvasRef::parse("Scratch Card.revealed").unwrap()).as_deref(), Some("50"));
        assert_eq!(log.teams_of(canvas).len(), 1);
    }
}
