//! Do: Break, and how a broken thing goes (its look: plan `docs/superpowers/plans/rules/part3.md`).
//! The thing stops simulating at once; its look is drawn only (`rules::remains`).

use std::sync::Arc;

use bpe_core::{BlockSection, Choice, Params, Settings, ThingId, Who};
use bpe_runtime::{Action, BlockSim, BreakShow, Firing, PrepareCx, StageCx, ThingState, state};

use super::block_kind;
use super::who::targets;

/// How a broken thing goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum BreakLook {
    /// Gone at once.
    #[default]
    Vanish,
    /// Fades to nothing.
    FadeOut,
    /// Swells, then fades.
    Pop,
    /// Cut into pieces that fly apart from the hit, fall, bounce off other shapes, spin and fade.
    Shatter,
}

pub struct Break;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct BreakSettings {
    #[setting(label = "Who", simple, group = "Rule")]
    pub who: Who,
    #[setting(label = "Look", simple, group = "Rule")]
    pub look: BreakLook,
    #[setting(label = "Time", when = "look=fade_out", range(0, 1e6), span(0, 3), unit = "s", group = "Rule")]
    pub fade_time: f64,
    #[setting(label = "Time", when = "look=pop", range(0, 1e6), span(0, 3), unit = "s", group = "Rule")]
    pub pop_time: f64,
    #[setting(label = "Swell", when = "look=pop", range(0, 1e3), span(1, 4), unit = "×", group = "Rule")]
    pub swell: f32,
    #[setting(label = "Pieces", when = "look=shatter", range(2, 256), span(2, 64), group = "Rule")]
    pub pieces: u32,
    #[setting(label = "Spread", when = "look=shatter", range(0, 1e6), span(0, 2000), unit = "px/s", group = "Rule")]
    pub spread: f32,
    #[setting(label = "Spin", when = "look=shatter", range(-1e6, 1e6), span(0, 1440), unit = "°/s", group = "Rule")]
    pub spin: f32,
    #[setting(label = "Gravity", when = "look=shatter", range(-1e3, 1e3), span(0, 3), unit = "×", group = "Rule")]
    pub gravity: f32,
    #[setting(label = "Hit shapes", when = "look=shatter", group = "Rule")]
    pub hit_shapes: bool,
    #[setting(label = "Bounce", when = "look=shatter and hit_shapes", range(0, 1), group = "Rule")]
    pub bounce: f32,
    #[setting(label = "Fade after", when = "look=shatter", range(0, 1e6), span(0, 5), unit = "s", group = "Rule")]
    pub fade_after: f64,
    #[setting(label = "Fade over", when = "look=shatter", range(0, 1e6), span(0, 3), unit = "s", group = "Rule")]
    pub fade_over: f64,
}

impl Default for BreakSettings {
    fn default() -> Self {
        let l = LookSettings::default();
        BreakSettings {
            who: Who::That,
            look: l.look,
            fade_time: l.fade_time,
            pop_time: l.pop_time,
            swell: l.swell,
            pieces: l.pieces,
            spread: l.spread,
            spin: l.spin,
            gravity: l.gravity,
            hit_shapes: l.hit_shapes,
            bounce: l.bounce,
            fade_after: l.fade_after,
            fade_over: l.fade_over,
        }
    }
}

impl BreakSettings {
    /// Its look's settings.
    pub fn look_settings(&self) -> LookSettings {
        LookSettings {
            look: self.look,
            fade_time: self.fade_time,
            pop_time: self.pop_time,
            swell: self.swell,
            pieces: self.pieces,
            spread: self.spread,
            spin: self.spin,
            gravity: self.gravity,
            hit_shapes: self.hit_shapes,
            bounce: self.bounce,
            fade_after: self.fade_after,
            fade_over: self.fade_over,
        }
    }
}

/// A Break look's settings, under the same keys as the Break block's: what a break's
/// [`BreakShow::look`] holds and the drawing reads (a Health card's At zero can hand its own params
/// the same way).
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct LookSettings {
    #[setting(label = "Look", group = "Rule")]
    pub look: BreakLook,
    #[setting(label = "Time", range(0, 1e6), unit = "s", group = "Rule")]
    pub fade_time: f64,
    #[setting(label = "Time", range(0, 1e6), unit = "s", group = "Rule")]
    pub pop_time: f64,
    #[setting(label = "Swell", range(0, 1e3), group = "Rule")]
    pub swell: f32,
    #[setting(label = "Pieces", range(2, 256), group = "Rule")]
    pub pieces: u32,
    #[setting(label = "Spread", range(0, 1e6), group = "Rule")]
    pub spread: f32,
    #[setting(label = "Spin", range(-1e6, 1e6), group = "Rule")]
    pub spin: f32,
    #[setting(label = "Gravity", range(-1e3, 1e3), group = "Rule")]
    pub gravity: f32,
    #[setting(label = "Hit shapes", group = "Rule")]
    pub hit_shapes: bool,
    #[setting(label = "Bounce", range(0, 1), group = "Rule")]
    pub bounce: f32,
    #[setting(label = "Fade after", range(0, 1e6), unit = "s", group = "Rule")]
    pub fade_after: f64,
    #[setting(label = "Fade over", range(0, 1e6), unit = "s", group = "Rule")]
    pub fade_over: f64,
}

impl Default for LookSettings {
    fn default() -> Self {
        LookSettings {
            look: BreakLook::Vanish,
            fade_time: 0.4,
            pop_time: 0.3,
            swell: 1.5,
            pieces: 12,
            spread: 300.0,
            spin: 360.0,
            gravity: 1.0,
            hit_shapes: true,
            bounce: 0.4,
            fade_after: 0.8,
            fade_over: 0.6,
        }
    }
}

/// A time setting as it plays: never below 0, and 0 when it isn't a number.
fn seconds(t: f64) -> f64 {
    if t.is_finite() { t.max(0.0) } else { 0.0 }
}

impl LookSettings {
    /// How long the look shows after the break, in seconds (0: gone at once).
    pub fn lasts(&self) -> f64 {
        match self.look {
            BreakLook::Vanish => 0.0,
            BreakLook::FadeOut => seconds(self.fade_time),
            BreakLook::Pop => seconds(self.pop_time),
            BreakLook::Shatter => seconds(self.fade_after) + seconds(self.fade_over),
        }
    }
}

/// Whether a thing of `kind` can show a Break look (else it vanishes whatever is picked): shapes,
/// balls, emitters (their balls), lights, Texts and Particles things (which fade, W12).
pub fn shows_looks(kind: &str) -> bool {
    matches!(
        kind,
        crate::things::shape::KIND
            | "balls"
            | crate::things::emitter::KIND
            | crate::things::light::LIGHT
            | crate::things::text::KIND
            | crate::things::particles::KIND
    )
}

/// How a break of a thing of `kind` with `look` plays out.
pub fn show_for(look: &LookSettings, kind: Option<&str>) -> BreakShow {
    let lasts = look.lasts();
    if lasts <= 0.0 || !kind.is_some_and(shows_looks) {
        return BreakShow::default();
    }
    BreakShow { look: look.to_params(), lasts }
}

/// One shared look a target kind, so every ball of a break shares it.
#[derive(Default)]
struct BreakLookShows(Vec<(Option<String>, Arc<BreakShow>)>);

impl BreakLookShows {
    fn of(&mut self, look: &LookSettings, kind: Option<&str>) -> Arc<BreakShow> {
        if let Some((_, s)) = self.0.iter().find(|(k, _)| k.as_deref() == kind) {
            return s.clone();
        }
        let s = Arc::new(show_for(look, kind));
        self.0.push((kind.map(str::to_owned), s.clone()));
        s
    }
}

block_kind!(
    Break,
    "do.break",
    "Break",
    Do,
    BreakSettings,
    "Breaks a ball or a whole thing: it is gone for the rest of the clip, vanishing, fading out, popping or \
     shattering."
);

impl BlockSim for Break {
    fn kind(&self) -> &'static str {
        "do.break"
    }
    fn section(&self) -> BlockSection {
        BlockSection::Do
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = BreakSettings::from_params(params);
        if s.who.names_thing() == Some(ThingId::ROOT) {
            return Err("The scene settings can't break".into());
        }
        Ok(Box::new(s))
    }
    /// A Particles thing it may break steps with the physics, so its Break look is known wherever
    /// the rule runs (W12): one its Who names by id, kind or tag, or any for Anything.
    fn keeps(&self, params: &Params, thing: ThingId, kind: &str, tags: &std::collections::BTreeSet<String>) -> bool {
        if kind != crate::things::particles::KIND {
            return false;
        }
        match BreakSettings::from_params(params).who {
            Who::Thing { thing: named, .. } => named == thing,
            Who::Kind(k) => k.as_str() == kind,
            Who::Tag(tag) => tags.contains(&tag),
            Who::Anything => true,
            // That, Other and Group are what a firing names: never a Particles thing.
            Who::That | Who::Other | Who::Group => false,
        }
    }

    fn act(&self, st: &mut dyn ThingState, firing: &Firing, cx: &StageCx, out: &mut Vec<Action>) {
        let s = state::<BreakSettings>(st);
        let mut each: Vec<(ThingId, Option<u32>)> = Vec::new();
        for (thing, row) in targets(cx, &s.who, firing) {
            match (&s.who, cx.world.bodies.get(&thing)) {
                // The group is its balls: each of them breaks, and the thing (an emitter, say)
                // carries on.
                (Who::Group, Some(b)) => {
                    each.extend((0..b.len() as u32).filter(|&r| b.alive[r as usize]).map(|r| (thing, Some(r))))
                }
                _ => each.push((thing, row)),
            }
        }
        let look = s.look_settings();
        let mut shows = BreakLookShows::default();
        for (thing, row) in each {
            if thing == ThingId::ROOT {
                continue;
            }
            out.push(Action::Break {
                thing,
                row,
                point: firing.point,
                normal: firing.normal,
                strength: firing.values.get("strength").unwrap_or(0.0) as f32,
                show: shows.of(&look, cx.world.kind(thing)),
            });
        }
    }
}
