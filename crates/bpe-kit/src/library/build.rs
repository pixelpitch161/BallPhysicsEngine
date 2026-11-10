//! Small helpers that keep each built-in scene's code short and readable. They only call the same edits
//! the editor makes, so a built-in scene is exactly what a person could have built by hand.

use bpe_core::{
    BlockId, Color, LayerId, NoteSlot, Params, Registry, RuleId, Scene, Settings, SlotId, ThingId, Value, Who,
    layered_key,
};
use glam::Vec2;

use crate::modules::look::surface::{SurfaceKind, SurfaceSettings, preset_values};
use crate::ready_looks::{self, Colours, How};
use crate::sound::{Plays, Root, Scale, VoiceSettings};
use crate::things::text::TextSettings;
use crate::things::text::looks::{Look, add_text};
use crate::{BallsSettings, BasicMotion, SceneSettings, ShapeSettings, add_thing};

/// A colour from 0xRRGGBB.
pub(crate) fn hex(rgb: u32) -> Color {
    Color::rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

pub(crate) fn n(x: f64) -> Value {
    Value::Number(x)
}

pub(crate) fn int(x: i64) -> Value {
    Value::Int(x)
}

pub(crate) fn choice(name: &str) -> Value {
    Value::Choice(name.into())
}

pub(crate) fn text(words: &str) -> Value {
    Value::Text(words.into())
}

/// Copies the `prefix`ed keys of `params` into their own set, without the prefix.
fn strip(params: &Params, prefix: &str) -> Params {
    let mut out = Params::default();
    for (k, v) in &params.0 {
        if let Some(rest) = k.strip_prefix(prefix) {
            out.set(rest, v.clone());
        }
    }
    out
}

/// Puts `own` back into `params` under `prefix`, dropping what was there (keys at their default stay out).
fn put_back(params: &mut Params, prefix: &str, own: Params) {
    params.0.retain(|k, _| !k.starts_with(prefix));
    for (k, v) in own.0 {
        params.set(&format!("{prefix}{k}"), v);
    }
}

/// Reads `params` as `S`, changes it and writes back: the keys `S` had away from their default are
/// replaced by the new ones; every other key stays.
fn edit_as<S: Settings>(params: &mut Params, f: impl FnOnce(&mut S)) {
    let mut s = S::from_params(params);
    for k in s.to_params().0.keys() {
        params.remove(k);
    }
    f(&mut s);
    for (k, v) in s.to_params().0 {
        params.set(&k, v);
    }
}

/// Where a ball of `radius` falling at `g` px/s² must be at 0 s, and how fast it goes, so that it lands on the
/// floor every `every` seconds, first at `first` seconds (as long as nothing else touches it).
pub(crate) fn on_the_beat(floor_top: f32, radius: f32, g: f32, every: f64, first: f64) -> (f32, f32) {
    let up = g * every as f32 / 2.0;
    // How long since it last left the floor.
    let since = (every - first.rem_euclid(every)).rem_euclid(every) as f32;
    let height = up * since - g * since * since / 2.0;
    (floor_top - radius - height, -(up - g * since))
}

/// A rule block: its kind and the settings it changes.
pub(crate) type Block<'k> = (&'k str, Vec<(&'k str, Value)>);

/// A ready voice for a scene's sound ([`Lib::tone`]).
#[derive(Clone, Copy, Debug)]
pub(crate) enum Tone {
    /// Two sines an octave apart that ring on.
    Bell,
    /// A bright, short saw with a square under it.
    Pluck,
    /// A soft wooden knock.
    Mallet,
    /// A soft, long note.
    Pad,
    /// A low saw with a sine under it.
    Bass,
    /// A kick drum: a sine whose pitch drops fast.
    Kick,
    /// A closed hi-hat: a short tick of noise.
    Hat,
    /// A clap: a burst of noise with a little tone under it.
    Clap,
}

/// A built-in scene being made.
pub(crate) struct Lib<'a> {
    pub reg: &'a Registry,
    pub scene: Scene,
}

impl<'a> Lib<'a> {
    /// A blank project (Scene settings and Camera, as the editor starts) `seconds` long.
    pub fn new(reg: &'a Registry, seconds: f64) -> Lib<'a> {
        let mut lib = Lib { reg, scene: crate::new_studio_scene(reg) };
        lib.scene_settings(|s| s.duration = seconds as f32);
        lib
    }

    /// Changes the Scene settings.
    pub fn scene_settings(&mut self, f: impl FnOnce(&mut SceneSettings)) {
        self.params(ThingId::ROOT, f);
    }

    /// A new thing of `kind` named `name` under `parent`, with its kind's starting cards.
    pub fn add(&mut self, kind: &str, parent: ThingId, name: &str) -> ThingId {
        let id = add_thing(&mut self.scene, self.reg, kind, parent).unwrap_or_else(|e| panic!("{kind}: {e}"));
        self.scene.rename_thing(id, name);
        id
    }

    /// Puts `id` at `at`.
    pub fn at(&mut self, id: ThingId, at: Vec2) {
        self.scene.things.get_mut(&id).expect("a thing").transform.position = at;
    }

    /// Changes a thing's own settings, read as `S`. Only settings away from their default are kept, and keys
    /// `S` doesn't know stay as they are.
    pub fn params<S: Settings>(&mut self, id: ThingId, f: impl FnOnce(&mut S)) {
        let thing = self.scene.things.get_mut(&id).expect("a thing");
        edit_as(&mut thing.params, f);
    }

    /// Changes a card's own settings, read as `S` (its layers' settings and keys `S` doesn't know stay).
    pub fn card<S: Settings>(&mut self, id: ThingId, slot: SlotId, f: impl FnOnce(&mut S)) {
        let card = self.scene.card_mut(id, slot).unwrap_or_else(|| panic!("no {slot:?} card"));
        edit_as(&mut card.params, f);
    }

    /// A new layer of `kind` on the card.
    pub fn layer(&mut self, id: ThingId, slot: SlotId, kind: &str) -> LayerId {
        self.scene.add_layer(self.reg, id, slot, kind).unwrap_or_else(|e| panic!("{kind}: {e}"))
    }

    /// Changes a layer's settings, read as `S`.
    pub fn layer_params<S: Settings>(&mut self, id: ThingId, slot: SlotId, layer: LayerId, f: impl FnOnce(&mut S)) {
        let prefix = layered_key(layer, "");
        let card = self.scene.card_mut(id, slot).unwrap_or_else(|| panic!("no {slot:?} card"));
        let mut own = strip(&card.params, &prefix);
        edit_as(&mut own, f);
        put_back(&mut card.params, &prefix, own);
    }

    /// Keeps every layer of a thing's Look in its own colours, whatever the scene's light (each layer's Lit at 0),
    /// so words stay bright in a dark lit scene.
    pub fn unlit(&mut self, id: ThingId) {
        let reg = self.reg;
        let card = self.scene.card_mut(id, SlotId::Look).unwrap_or_else(|| panic!("no Look card"));
        let lit = |kind: &str| reg.layer(kind).is_some_and(|k| k.light_defs().iter().any(|d| d.key == "lit"));
        let layers: Vec<LayerId> = card.layers.iter().filter(|l| lit(&l.kind)).map(|l| l.id).collect();
        for layer in layers {
            card.params.set(&layered_key(layer, "lit"), Value::Number(0.0));
        }
    }

    /// Dresses a ball or shape in a ready-made look (its own colours replaced by the look's).
    pub fn look(&mut self, id: ThingId, look: &str) {
        let ready = ready_looks::find(look).unwrap_or_else(|| panic!("no ready-made look {look}"));
        ready_looks::apply(&mut self.scene, self.reg, id, ready, Colours::Look, How::Replace)
            .unwrap_or_else(|e| panic!("{look}: {e}"));
    }

    /// Dresses a ball or shape in a ready-made look in `colour`: as the editor's "Keep mine" does, the colour
    /// goes where the look's main colour goes (a neon tube's tube and glow, a glow orb's halo). A look that takes
    /// no colour keeps its own, with only its Colour layer (a ball's Body) set to `colour`.
    pub fn look_in(&mut self, id: ThingId, look: &str, colour: Color) {
        self.look(id, look);
        self.own_colour(id, colour);
        let ready = ready_looks::find(look).expect("found above");
        if ready.takes_colour {
            ready_looks::apply(&mut self.scene, self.reg, id, ready, Colours::Mine, How::Replace)
                .unwrap_or_else(|e| panic!("{look}: {e}"));
        }
    }

    /// Sets a thing's own colours, the ones "Keep mine" reads: a ball's Body, or a shape's Colour layer.
    fn own_colour(&mut self, id: ThingId, colour: Color) {
        use crate::modules::look::layers::BODY;
        let card = self.scene.card_mut(id, SlotId::Look).expect("a Look card");
        if let Some(body) = card.layers.iter().find(|x| x.kind == BODY) {
            let key = layered_key(body.id, "colours");
            card.params.set(&key, Value::Gradient(bpe_core::Gradient::solid(colour)));
        } else {
            self.colour_in(id, colour);
        }
    }

    /// Sets a shape's or a Text's Colour layer to one colour.
    pub fn colour_in(&mut self, id: ThingId, colour: Color) {
        use crate::modules::look::shape::PAINT;
        let card = self.scene.card_mut(id, SlotId::Look).expect("a Look card");
        let paint = card.layers.iter().find(|x| x.kind == PAINT && x.parent.is_none()).expect("a Colour layer").id;
        card.params.set(&layered_key(paint, "colours"), Value::Gradient(bpe_core::Gradient::solid(colour)));
    }

    /// Gives `id` the sound "Enable sound" adds (the starter voice).
    pub fn sound(&mut self, id: ThingId) {
        let card = crate::sound::starter_voice(self.reg);
        self.scene.things.get_mut(&id).expect("a thing").slots.insert(SlotId::Sound, card);
    }

    /// A rule named `name` whose blocks follow one another: a When, Ifs and one Do at the end (a Do passes
    /// nothing on; for several Dos use `rule_doing`).
    pub fn rule(&mut self, name: &str, blocks: &[Block]) -> RuleId {
        self.rule_doing(name, blocks, &[])
    }

    /// A rule named `name`: the `chain` (a When, then Ifs) follows one another, and each of `dos` comes
    /// after the chain's last block. A Do block passes nothing on, so a rule that does several things
    /// hangs each one off the same block.
    pub fn rule_doing(&mut self, name: &str, chain: &[Block], dos: &[Block]) -> RuleId {
        let rule = self.scene.add_rule(name);
        let mut last: Option<BlockId> = None;
        for (kind, set) in chain {
            last = Some(self.block(name, rule, kind, set, last));
        }
        for (kind, set) in dos {
            self.block(name, rule, kind, set, last);
        }
        rule
    }

    fn block(
        &mut self,
        name: &str,
        rule: RuleId,
        kind: &str,
        set: &[(&str, Value)],
        after: Option<BlockId>,
    ) -> BlockId {
        let id = self.scene.add_block(self.reg, rule, kind, after).unwrap_or_else(|e| panic!("{kind}: {e}"));
        for (k, v) in set {
            self.scene
                .set_block_param(self.reg, rule, id, k, v.clone())
                .unwrap_or_else(|e| panic!("{name}: {kind} {k}: {e}"));
        }
        id
    }

    /// A Text made from one of the Text's own looks (a hook title, a hit counter), named and with its words.
    pub fn text(&mut self, look: Look, name: &str, words: &str) -> ThingId {
        let id = add_text(&mut self.scene, self.reg, look, ThingId::ROOT).unwrap_or_else(|e| panic!("{name}: {e}"));
        self.scene.rename_thing(id, name);
        self.params(id, |s: &mut TextSettings| s.words = words.into());
        id
    }

    /// A Text made from one of the Text's own looks with its own words in place of the look's, laid out (and
    /// its balls placed) for them.
    pub fn text_saying(&mut self, look: Look, name: &str, words: &str) -> ThingId {
        let id =
            crate::things::text::looks::add_text_saying(&mut self.scene, self.reg, look, ThingId::ROOT, Some(words))
                .unwrap_or_else(|e| panic!("{name}: {e}"));
        self.scene.rename_thing(id, name);
        id
    }

    /// A hollow ring of `radius` at `at`, wearing the ready-made shape look `look`.
    pub fn ring(&mut self, name: &str, at: Vec2, radius: f32, thickness: f32, look: &str) -> ThingId {
        let id = self.add("shape", ThingId::ROOT, name);
        self.at(id, at);
        self.params(id, |s: &mut ShapeSettings| *s = ShapeSettings::circle(radius, thickness));
        self.look(id, look);
        id
    }

    /// One ball of `radius` at `at` setting off at `velocity`, pulled by the scene's gravity, wearing the
    /// ready-made look `look`.
    pub fn ball(&mut self, name: &str, at: Vec2, radius: f32, velocity: Vec2, look: &str) -> ThingId {
        let id = self.add("balls", ThingId::ROOT, name);
        self.at(id, at);
        self.params(id, |s: &mut BallsSettings| {
            s.radius = radius;
            s.velocity = velocity;
        });
        self.card(id, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 1.0);
        self.look(id, look);
        id
    }

    /// Gives `id` the starter sound playing `notes` (MIDI numbers, middle C 60) as slot 1, picked by
    /// `plays`, in the key of `root` `scale`.
    pub fn notes(&mut self, id: ThingId, notes: &[u8], plays: Plays, root: Root, scale: Scale) {
        if self.scene.card(id, SlotId::Sound).is_none() {
            self.sound(id);
        }
        let mut sorted = notes.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        self.card(id, SlotId::Sound, |v: &mut VoiceSettings| {
            v.notes_slots = vec![NoteSlot { notes: sorted, order_clicked: Vec::new() }];
            v.notes_plays = plays;
            v.notes_root = root;
            v.notes_scale = scale;
        });
    }

    /// Gives `id` the starter sound playing `steps` one after another, a step a hit (a step of several
    /// notes is a chord), starting again after the last.
    pub fn melody(&mut self, id: ThingId, steps: &[&[u8]]) {
        if self.scene.card(id, SlotId::Sound).is_none() {
            self.sound(id);
        }
        self.card(id, SlotId::Sound, |v: &mut VoiceSettings| {
            v.notes_melody = steps
                .iter()
                .map(|n| {
                    // A step's notes low to high, as the editor keeps them (and the file wants them).
                    let mut notes = n.to_vec();
                    notes.sort_unstable();
                    notes.dedup();
                    NoteSlot { notes, order_clicked: Vec::new() }
                })
                .collect();
            v.notes_plays = Plays::Melody;
        });
    }

    /// Makes `id`'s sound (made if missing) `tone`: its oscillators and envelope.
    pub fn tone(&mut self, id: ThingId, tone: Tone) {
        use crate::sound::{AdsrSettings, Wave};
        if self.scene.card(id, SlotId::Sound).is_none() {
            self.sound(id);
        }
        // (Osc A wave, unison, Osc B: wave, octave, level in dB, envelope: attack, decay, release in ms, sustain in %)
        let (a, unison, b, (attack, decay, sustain, release)) = match tone {
            Tone::Bell => (Wave::Sine, 1, Some((Wave::Sine, 1, -10.0)), (1.0, 1400.0, 0.0, 900.0)),
            Tone::Pluck => (Wave::Saw, 3, Some((Wave::Square, -1, -14.0)), (1.0, 260.0, 0.0, 220.0)),
            Tone::Mallet => (Wave::Sine, 1, Some((Wave::Triangle, 2, -16.0)), (1.0, 450.0, 0.0, 300.0)),
            Tone::Pad => (Wave::Triangle, 2, Some((Wave::Sine, -1, -8.0)), (25.0, 1600.0, 0.0, 1200.0)),
            Tone::Bass => (Wave::Saw, 1, Some((Wave::Sine, -1, -4.0)), (2.0, 380.0, 0.0, 200.0)),
            Tone::Kick => (Wave::Sine, 1, Some((Wave::Triangle, 0, -18.0)), (0.5, 320.0, 0.0, 120.0)),
            Tone::Hat => (Wave::Noise, 1, None, (0.5, 45.0, 0.0, 30.0)),
            Tone::Clap => (Wave::Noise, 1, Some((Wave::Triangle, 1, -14.0)), (1.0, 150.0, 0.0, 120.0)),
        };
        self.card(id, SlotId::Sound, |v: &mut VoiceSettings| {
            v.a.wave = a;
            v.a.unison = unison;
            if let Some((wave, octave, level)) = b {
                v.b.on = true;
                v.b.wave = wave;
                v.b.octave = octave;
                v.b.level = level;
                // The envelope shapes Osc B as it does Osc A.
                let route = bpe_core::ModRoute { target: "b_gain".into(), ..v.routes[0].clone() };
                if !v.routes.contains(&route) {
                    v.routes.push(route);
                }
            }
        });
        self.layer_params(id, SlotId::Sound, LayerId(0), |e: &mut AdsrSettings| {
            e.attack = attack;
            e.decay = decay;
            e.sustain = sustain;
            e.release = release;
        });
        if let Tone::Kick = tone {
            // A second, fast envelope drops the pitch two octaves into the note: the kick's thump.
            let drop = self.layer(id, SlotId::Sound, crate::sound::ADSR);
            self.layer_params(id, SlotId::Sound, drop, |e: &mut AdsrSettings| {
                e.attack = 0.0;
                e.decay = 45.0;
                e.sustain = 0.0;
                e.release = 20.0;
            });
            self.card(id, SlotId::Sound, |v: &mut VoiceSettings| {
                for target in ["a_semi", "b_semi"] {
                    let route = bpe_core::ModRoute { source: drop, target: target.into(), ..v.routes[0].clone() };
                    v.routes.push(bpe_core::ModRoute { amount: 0.5, ..route });
                }
            });
        }
    }

    /// A reverb on the Mixer's first bus, where every sound goes unless sent elsewhere.
    pub fn reverb(&mut self, decay: f32, mix: f32) {
        use crate::sound::fx::ReverbSettings;
        crate::ensure_mixer(&mut self.scene, self.reg);
        let layer = self
            .scene
            .add_layer_in(self.reg, ThingId::ROOT, SlotId::Mixer, crate::sound::fx::REVERB, Some(0))
            .expect("the kit has the reverb");
        self.layer_params(ThingId::ROOT, SlotId::Mixer, layer, |r: &mut ReverbSettings| {
            r.decay = decay;
            r.mix = mix;
        });
    }

    /// An emitter named `name` at `at`, its settings changed by `f`.
    pub fn emitter(
        &mut self,
        name: &str,
        at: Vec2,
        f: impl FnOnce(&mut crate::things::emitter::EmitterSettings),
    ) -> ThingId {
        let id = self.add(crate::things::emitter::KIND, ThingId::ROOT, name);
        self.at(id, at);
        self.params(id, f);
        id
    }

    /// Keyframes the setting `key` (on `slot`, or the thing's own settings for `None`) through `keys`
    /// (time, value), moving evenly from one to the next.
    pub fn keys(&mut self, id: ThingId, slot: Option<SlotId>, key: &str, keys: &[(f64, Value)]) {
        let a = bpe_core::Address { thing: id, slot, key: key.into() };
        for (t, v) in keys {
            let i = self.scene.set_key(self.reg, &a, *t, v.clone()).unwrap_or_else(|e| panic!("{key}: {e}"));
            self.scene.tracks.get_mut(&a).expect("keyed").keys[i].ease = bpe_core::Ease::Linear;
        }
    }

    /// Gives `id` the card the editor's Add card puts in `slot` (a Health card, say).
    pub fn fill(&mut self, id: ThingId, slot: SlotId) {
        crate::fill_slot(&mut self.scene, self.reg, id, slot).unwrap_or_else(|e| panic!("{slot:?}: {e}"));
    }

    /// A light of `kind` named `name` at `at`, its settings changed by `f`. The scene gets its Lighting card
    /// as the editor gives it with the first light.
    pub fn light(
        &mut self,
        name: &str,
        kind: crate::things::light::LightType,
        at: Vec2,
        f: impl FnOnce(&mut crate::things::light::LightSettings),
    ) -> ThingId {
        self.light_in(ThingId::ROOT, name, kind, at, f)
    }

    /// A light as [`Lib::light`], inside `parent`: inside a Balls thing it rides the ball.
    pub fn light_in(
        &mut self,
        parent: ThingId,
        name: &str,
        kind: crate::things::light::LightType,
        at: Vec2,
        f: impl FnOnce(&mut crate::things::light::LightSettings),
    ) -> ThingId {
        let id = crate::things::light::add_light(&mut self.scene, self.reg, kind, parent)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        self.scene.rename_thing(id, name);
        self.params(id, |l: &mut crate::things::light::LightSettings| {
            l.position = at;
            f(l);
        });
        id
    }

    /// Sets the Look card's Surface to the chip `kind` with its numbers, as picking the chip in the editor does,
    /// then changes them with `f`.
    pub fn surface(&mut self, id: ThingId, kind: SurfaceKind, f: impl FnOnce(&mut SurfaceSettings)) {
        let card = self.scene.card_mut(id, SlotId::Look).expect("a Look card");
        card.params.set("surface", Value::Choice(bpe_core::Choice::name(kind).into()));
        for (k, v) in preset_values(kind) {
            card.params.set(k, v);
        }
        self.card(id, SlotId::Look, f);
    }

    /// A Particles thing from the ready look `look`, named `name`, under `parent`.
    pub fn particles(&mut self, look: crate::things::particles::looks::Look, parent: ThingId, name: &str) -> ThingId {
        let id = crate::things::particles::looks::add_particles(&mut self.scene, self.reg, look, parent)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        self.scene.rename_thing(id, name);
        id
    }

    /// Makes the setting `key` (on `slot`, or the thing's own for `None`) follow how loud `shape` is (Follow
    /// sound): `amount` is added at full loudness, and the setting stays within `range`.
    pub fn follow_sound(&mut self, id: ThingId, slot: Option<SlotId>, key: &str, shape: ThingId, amount: f64, range: (f64, f64)) {
        let address = bpe_core::Address { thing: id, slot, key: key.into() };
        let pulse = bpe_core::Pulse {
            source: bpe_core::PulseSource::Sound { shape, source: bpe_core::SoundSource::Level },
            amount: Value::Number(amount),
            range: Some(range),
            ..Default::default()
        };
        self.scene.pulses.insert(address, pulse);
    }

    /// The thing as a rule names it.
    pub fn who(&self, id: ThingId) -> Value {
        Value::Who(Who::Thing { thing: id, ball: None })
    }

    pub fn finish(self) -> Scene {
        self.scene
    }
}
