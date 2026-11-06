//! Do: Play a note (rules part 6). A shape's voice plays a note, as the sound engine hears it in
//! the editor and in export: a set note, or the shape's own next note as a hit would pick it.

use bpe_core::{BlockSection, Choice, Params, Settings, ThingId, Who};
use bpe_runtime::rules::AS_IF_HIT;
use bpe_runtime::{Action, BlockSim, Event, EventKind, Firing, Party, PrepareCx, StageCx, ThingState, state};
use glam::Vec2;

use super::amount::Amount;
use super::block_kind;
use super::who::{is_ball, targets};

/// The shape thing's kind.
const SHAPE: &str = "shape";

/// Which note it plays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum PitchFrom {
    /// The note set here.
    #[default]
    ANote,
    /// The shape's own next note, picked as a hit picks it.
    TheShapesNextNote,
}

pub struct PlayNote;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct PlayNoteSettings {
    /// The shape whose voice plays it.
    #[setting(label = "Shape", simple, group = "Rule")]
    pub who: Who,
    #[setting(label = "Plays", simple, group = "Rule")]
    pub pitch_from: PitchFrom,
    /// The note, as MIDI numbers it: 60 is C5.
    #[setting(label = "Note", when = "pitch_from=a_note", range(0, 127), simple, group = "Rule")]
    pub note: u32,
    #[setting(label = "Velocity", range(0, 1), simple, group = "Rule")]
    pub velocity: f32,
    /// Seconds before the note lets go; 0 leaves it to the shape's envelopes.
    #[setting(label = "Length", range(0, 1e9), span(0, 4), unit = "s", group = "Rule")]
    pub length: f32,
}

impl Default for PlayNoteSettings {
    fn default() -> Self {
        PlayNoteSettings { who: Who::Other, pitch_from: PitchFrom::ANote, note: 60, velocity: 0.8, length: 0.0 }
    }
}

block_kind!(
    PlayNote,
    "do.note",
    "Play a note",
    Do,
    PlayNoteSettings,
    "A shape's voice plays a note: one set here, or its own next note as if hit."
);

#[derive(Clone, Debug)]
struct NoteState {
    s: PlayNoteSettings,
    note: Amount,
    velocity: Amount,
    length: Amount,
}

impl BlockSim for PlayNote {
    fn kind(&self) -> &'static str {
        "do.note"
    }
    fn section(&self) -> BlockSection {
        BlockSection::Do
    }
    fn prepare(&self, params: &Params, cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = PlayNoteSettings::from_params(params);
        if let Some(t) = s.who.names_thing()
            && let Some(thing) = cx.scene.things.get(&t)
            && thing.kind != SHAPE
        {
            return Err(format!("{} isn't a shape, so it has no voice", thing.name));
        }
        let note = Amount::read(params, "note", s.note as f64);
        let velocity = Amount::read(params, "velocity", s.velocity as f64);
        let length = Amount::read(params, "length", s.length as f64);
        Ok(Box::new(NoteState { s, note, velocity, length }))
    }
    fn act(&self, st: &mut dyn ThingState, firing: &Firing, cx: &StageCx, out: &mut Vec<Action>) {
        let m = state::<NoteState>(st);
        let pitch = match m.s.pitch_from {
            PitchFrom::ANote => m.note.or_fixed(firing).round().clamp(0.0, 127.0) as u32,
            PitchFrom::TheShapesNextNote => AS_IF_HIT,
        };
        let velocity = (m.velocity.or_fixed(firing) as f32).clamp(0.0, 1.0);
        let length = (m.length.or_fixed(firing) as f32).max(0.0);
        // The ball the note is about (for Plays By ball, its Hit modulators), else the shape.
        let ball = firing.subject.filter(|p| is_ball(cx.world, *p));
        for (thing, _) in targets(cx, &m.s.who, firing) {
            if cx.world.kind_of(thing) != Some(SHAPE) || thing == ThingId::ROOT {
                continue;
            }
            out.push(Action::Note(Event {
                step: 0,
                kind: EventKind::Note,
                a: ball.unwrap_or(Party { thing, item: 0 }),
                b: Some(Party { thing, item: pitch }),
                point: firing.point,
                normal: Vec2::new(length, 0.0),
                speed: velocity,
                at: 0.0,
            }));
        }
    }
}
