//! The three buses: shapes play into them, each runs its effects, its fader, pan, mute and solo,
//! sends on to other buses after its fader, and they all end at the master.
//!
//! Buses run in send order (a bus that sends to another runs first); a send that would make a
//! loop is dropped. A bus whose effects delay the sound (a limiter's lookahead) is lined up with
//! the others, so a shape heard on two buses never smears.

// The bus graph walks small arrays by bus number, which reads clearest by index.
#![allow(clippy::needless_range_loop)]

use crate::fx_chain::{FxChain, FxCx};
use crate::hits::BlockHit;
use crate::lag::{Lag, MAX_LAG};
use crate::voices::knobs::track_value;
use crate::{BLOCK, BusReading, Meters, TempoMap};
use bpe_core::{Address, Registry, Scene, SlotId, ThingId, Track};
use bpe_dsp::{Smoother, db_to_gain, gain_to_db};
use bpe_kit::sound::{BUSES, BusSettings, MixerSettings, SILENT_DB};

/// How long turning a bus on or off, muting or soloing it fades.
const SWITCH_MS: f32 = 10.0;

/// Whether a send at `db` sends anything.
pub fn sends(db: f32) -> bool {
    db > SILENT_DB
}

/// Which bus sends to which: `edges[from][to]`.
pub type Edges = [[bool; BUSES]; BUSES];

/// The sends `mixer` has turned up (a bus never sends to itself).
pub fn edges_of(mixer: &MixerSettings) -> Edges {
    let mut e = [[false; BUSES]; BUSES];
    for (from, row) in e.iter_mut().enumerate() {
        for (to, edge) in row.iter_mut().enumerate() {
            *edge = from != to && sends(mixer.bus(from as u8 + 1).send(to as u8 + 1));
        }
    }
    e
}

/// Whether `to` can be reached from `from` along `edges`.
fn reaches(edges: &Edges, from: usize, to: usize) -> bool {
    let mut seen = [false; BUSES];
    let mut stack = [0usize; BUSES * BUSES];
    let mut n = 1;
    stack[0] = from;
    while n > 0 {
        n -= 1;
        let b = stack[n];
        if b == to {
            return true;
        }
        if std::mem::replace(&mut seen[b], true) {
            continue;
        }
        for (next, &e) in edges[b].iter().enumerate() {
            if e && !seen[next] {
                stack[n] = next;
                n += 1;
            }
        }
    }
    false
}

/// Whether turning up bus `from`'s send to bus `to` (both 1..=3) would make the sound go round
/// in a loop, given the other sends `mixer` has up.
pub fn would_loop(mixer: &MixerSettings, from: u8, to: u8) -> bool {
    let (f, t) = (from as usize - 1, to as usize - 1);
    if f == t {
        return true;
    }
    let mut e = edges_of(mixer);
    e[f][t] = false;
    reaches(&e, t, f)
}

/// The sends kept (those that would close a loop are dropped, first come first kept, in bus
/// order) and the order the buses run in so every bus runs after those sending to it.
pub fn send_order(edges: &Edges) -> ([usize; BUSES], Edges) {
    let mut kept = [[false; BUSES]; BUSES];
    for from in 0..BUSES {
        for to in 0..BUSES {
            if edges[from][to] && from != to && !reaches(&kept, to, from) {
                kept[from][to] = true;
            }
        }
    }
    // Kahn's order, lowest bus first among the ready ones.
    let mut order = [0usize; BUSES];
    let mut done = [false; BUSES];
    for slot in &mut order {
        let next = (0..BUSES)
            .find(|&b| !done[b] && (0..BUSES).all(|s| done[s] || !kept[s][b]))
            .expect("the kept sends have no loop");
        done[next] = true;
        *slot = next;
    }
    (order, kept)
}

/// A bus's settings with keys, read once a block.
#[derive(Clone, Default)]
struct BusKeys {
    fader: Option<Track>,
    pan: Option<Track>,
    sends: [Option<Track>; BUSES],
}

/// Everything one bus is built from.
pub struct BusSpec {
    pub settings: BusSettings,
    keys: BusKeys,
    pub chain: FxChain,
}

/// The three buses' settings, ready for the engine.
pub struct BusesSpec {
    pub buses: [BusSpec; BUSES],
}

impl BusesSpec {
    /// The buses of the scene's mixer card (`card` its resolved card, when it has one).
    pub fn build(scene: &Scene, mixer: &MixerSettings, reg: &Registry, tempo: &TempoMap) -> BusesSpec {
        let track = |key: String| {
            let a = Address { thing: ThingId::ROOT, slot: Some(SlotId::Mixer), key };
            scene.tracks.get(&a).filter(|t| !t.keys.is_empty()).cloned()
        };
        let buses = std::array::from_fn(|i| {
            let n = i as u8 + 1;
            let key = |k: &str| MixerSettings::bus_key(n, k);
            BusSpec {
                settings: mixer.bus(n).clone(),
                keys: BusKeys {
                    fader: track(key("fader")),
                    pan: track(key("pan")),
                    sends: std::array::from_fn(|t| track(key(&format!("send{}", t + 1)))),
                },
                chain: FxChain::build(scene, i as u8, reg, tempo),
            }
        });
        BusesSpec { buses }
    }
}

/// One bus while it plays.
struct Bus {
    settings: BusSettings,
    keys: BusKeys,
    chain: FxChain,
    fader: Smoother,
    pan: Smoother,
    /// 1 while it plays (on, not muted, not left out by a solo), else 0.
    audible: Smoother,
    sends: [Smoother; BUSES],
    /// The bus is off and silent: its effects rest and are cleared before it plays again.
    asleep: bool,
    /// Its sound from the shapes, and from each other bus (by that bus's place).
    from_shapes: [[f32; BLOCK]; 2],
    from_buses: [[[f32; BLOCK]; 2]; BUSES],
    /// Lines up what reaches it: the shapes' sound, and each bus's.
    shapes_lag: Lag,
    bus_lags: [Lag; BUSES],
    /// Lines its output up at the master.
    out_lag: Lag,
}

impl Bus {
    fn new(spec: BusSpec) -> Bus {
        let mut b = Bus {
            settings: spec.settings,
            keys: spec.keys,
            chain: spec.chain,
            fader: Smoother::new(Smoother::KNOB_MS),
            pan: Smoother::new(Smoother::KNOB_MS),
            audible: Smoother::new(SWITCH_MS),
            sends: std::array::from_fn(|_| Smoother::new(Smoother::KNOB_MS)),
            asleep: false,
            from_shapes: [[0.0; BLOCK]; 2],
            from_buses: [[[0.0; BLOCK]; 2]; BUSES],
            shapes_lag: Lag::new(),
            bus_lags: std::array::from_fn(|_| Lag::new()),
            out_lag: Lag::new(),
        };
        b.at(0.0, false);
        b.snap();
        b
    }

    /// Heads the smoothers for where the settings and keys are at `time`.
    fn at(&mut self, time: f64, any_solo: bool) {
        let key = |t: &Option<Track>, base: f32| t.as_ref().and_then(|t| track_value(t, time)).unwrap_or(base);
        let s = &self.settings;
        self.fader.set_target(fader_gain(key(&self.keys.fader, s.fader)));
        self.pan.set_target((key(&self.keys.pan, s.pan) / 100.0).clamp(-1.0, 1.0));
        let plays = s.on && !s.mute && (!any_solo || s.solo);
        self.audible.set_target(if plays { 1.0 } else { 0.0 });
        for (t, send) in self.sends.iter_mut().enumerate() {
            send.set_target(fader_gain(key(&self.keys.sends[t], s.send(t as u8 + 1))));
        }
    }

    /// Jumps every smoother to where it is headed.
    fn snap(&mut self) {
        for s in [&mut self.fader, &mut self.pan, &mut self.audible].into_iter().chain(self.sends.iter_mut()) {
            let t = s.target();
            s.reset(t);
        }
    }

    fn clear_inputs(&mut self) {
        self.from_shapes = [[0.0; BLOCK]; 2];
        self.from_buses = [[[0.0; BLOCK]; 2]; BUSES];
    }
}

/// A fader or send in dB as a gain; all the way down is silence.
fn fader_gain(db: f32) -> f32 {
    if db <= SILENT_DB { 0.0 } else { db_to_gain(db) }
}

/// The three buses while they play.
pub struct Buses {
    buses: [Bus; BUSES],
    order: [usize; BUSES],
    kept: Edges,
    any_solo: bool,
    /// Frames from a shape to the master, the same through every bus.
    latency: usize,
    /// Each bus's output from the last block (after its fader, before lining up), for
    /// sidechains.
    last: [[[f32; BLOCK]; 2]; BUSES],
    /// Each bus's output in the last block, lined up as the master hears it (the stems).
    stems: [[[f32; BLOCK]; 2]; BUSES],
}

impl Buses {
    pub fn new(spec: BusesSpec) -> Buses {
        let settings: [BusSettings; BUSES] = std::array::from_fn(|i| spec.buses[i].settings.clone());
        let mixer = MixerSettings {
            bus1: settings[0].clone(),
            bus2: settings[1].clone(),
            bus3: settings[2].clone(),
            ..MixerSettings::default()
        };
        let mut edges = edges_of(&mixer);
        // A send with keys counts as up, so the order holds whatever the keys do.
        for (from, b) in spec.buses.iter().enumerate() {
            for to in 0..BUSES {
                edges[from][to] |= from != to && b.keys.sends[to].is_some();
            }
        }
        let (order, kept) = send_order(&edges);
        let any_solo = settings.iter().any(|s| s.on && s.solo);
        let [a, b, c] = spec.buses;
        let mut buses = Buses {
            buses: [Bus::new(a), Bus::new(b), Bus::new(c)],
            order,
            kept,
            any_solo,
            latency: 0,
            last: [[[0.0; BLOCK]; 2]; BUSES],
            stems: [[[0.0; BLOCK]; 2]; BUSES],
        };
        for bus in &mut buses.buses {
            bus.at(0.0, any_solo);
            bus.snap();
            bus.asleep = bus.audible.target() == 0.0;
        }
        buses.line_up();
        buses
    }

    /// Sets every lag so all paths from a shape to the master take the same time.
    fn line_up(&mut self) {
        let mut out_lat = [0usize; BUSES];
        for &b in &self.order {
            let input = (0..BUSES).filter(|&s| self.kept[s][b]).map(|s| out_lat[s]).max().unwrap_or(0);
            let bus = &mut self.buses[b];
            bus.shapes_lag.set(input);
            for s in 0..BUSES {
                bus.bus_lags[s].set(if self.kept[s][b] { input - out_lat[s] } else { 0 });
            }
            out_lat[b] = input + bus.chain.latency();
        }
        self.latency = out_lat.iter().copied().max().unwrap_or(0).min(MAX_LAG);
        for (b, bus) in self.buses.iter_mut().enumerate() {
            bus.out_lag.set(self.latency.saturating_sub(out_lat[b]));
        }
    }

    /// Frames the buses delay the sound by.
    pub fn latency(&self) -> usize {
        self.latency
    }

    /// The order the buses run in (by place, 0..2).
    pub fn order(&self) -> [usize; BUSES] {
        self.order
    }

    /// Silence, as new, with every setting where it is at `time`.
    pub fn reset(&mut self, time: f64) {
        for bus in &mut self.buses {
            bus.chain.reset(time);
            bus.shapes_lag.reset();
            bus.bus_lags.iter_mut().for_each(Lag::reset);
            bus.out_lag.reset();
            bus.clear_inputs();
            bus.at(time, self.any_solo);
            bus.snap();
            bus.asleep = bus.audible.target() == 0.0;
        }
        self.last = [[[0.0; BLOCK]; 2]; BUSES];
        self.stems = [[[0.0; BLOCK]; 2]; BUSES];
    }

    /// Takes over the old buses' smoothers and effect memories (a patch swap), so nothing
    /// clicks and tails ring on. Lines that changed length start empty.
    pub fn adopt(&mut self, old: &mut Buses) {
        for (new, prev) in self.buses.iter_mut().zip(old.buses.iter_mut()) {
            new.chain.adopt(&mut prev.chain);
            for (n, p) in [&mut new.fader, &mut new.pan, &mut new.audible]
                .into_iter()
                .chain(new.sends.iter_mut())
                .zip([&prev.fader, &prev.pan, &prev.audible].into_iter().chain(prev.sends.iter()))
            {
                let t = n.target();
                n.reset(p.value());
                n.set_target(t);
            }
            new.asleep = prev.asleep;
            let same = |a: &Lag, b: &Lag| a.frames() == b.frames();
            if same(&new.shapes_lag, &prev.shapes_lag) {
                std::mem::swap(&mut new.shapes_lag, &mut prev.shapes_lag);
            }
            for s in 0..BUSES {
                if same(&new.bus_lags[s], &prev.bus_lags[s]) {
                    std::mem::swap(&mut new.bus_lags[s], &mut prev.bus_lags[s]);
                }
            }
            if same(&new.out_lag, &prev.out_lag) {
                std::mem::swap(&mut new.out_lag, &mut prev.out_lag);
            }
        }
        self.last = old.last;
    }

    /// Starts a block: clears what reaches each bus and reads the keys at `time`.
    pub fn begin(&mut self, time: f64) {
        for bus in &mut self.buses {
            bus.clear_inputs();
            bus.at(time, self.any_solo);
        }
    }

    /// Adds a shape's block into bus `bus` (1..=3), at full level or at a gain per sample.
    pub fn add(&mut self, bus: u8, gains: Option<&[f32; BLOCK]>, l: &[f32; BLOCK], r: &[f32; BLOCK]) {
        let Some(b) = (bus as usize).checked_sub(1).and_then(|i| self.buses.get_mut(i)) else { return };
        let [il, ir] = &mut b.from_shapes;
        match gains {
            None => {
                for i in 0..BLOCK {
                    il[i] += l[i];
                    ir[i] += r[i];
                }
            }
            Some(g) => {
                for i in 0..BLOCK {
                    il[i] += l[i] * g[i];
                    ir[i] += r[i] * g[i];
                }
            }
        }
    }

    /// Whether bus `bus` (1..=3) plays at all now (on, and not about to be silent).
    pub fn is_on(&self, bus: u8) -> bool {
        (bus as usize).checked_sub(1).and_then(|i| self.buses.get(i)).is_some_and(|b| b.settings.on)
    }

    /// Runs every bus and adds them into `master_l` and `master_r` (cleared first). Writes each
    /// bus's meter. Never allocates.
    #[allow(clippy::too_many_arguments)]
    pub fn process(
        &mut self,
        master_l: &mut [f32; BLOCK],
        master_r: &mut [f32; BLOCK],
        time: f64,
        tempo: &TempoMap,
        hits: &[BlockHit],
        meters: &Meters,
    ) {
        *master_l = [0.0; BLOCK];
        *master_r = [0.0; BLOCK];
        let mut outs = self.last;
        for &b in &self.order {
            let (mut l, mut r) = ([0.0f32; BLOCK], [0.0f32; BLOCK]);
            let mut reduction = 0.0;
            {
                let bus = &mut self.buses[b];
                let resting = bus.audible.target() == 0.0 && bus.audible.settled();
                if resting {
                    bus.asleep = true;
                } else {
                    if bus.asleep {
                        bus.chain.reset(time);
                        bus.shapes_lag.reset();
                        bus.bus_lags.iter_mut().for_each(Lag::reset);
                        bus.asleep = false;
                    }
                    let [sl, sr] = &mut bus.from_shapes;
                    bus.shapes_lag.process(sl, sr);
                    l = *sl;
                    r = *sr;
                    for s in 0..BUSES {
                        let [bl, br] = &mut bus.from_buses[s];
                        bus.bus_lags[s].process(bl, br);
                        for i in 0..BLOCK {
                            l[i] += bl[i];
                            r[i] += br[i];
                        }
                    }
                    let cx = FxCx { time, tempo, hits, buses: &self.last, meters };
                    reduction = bus.chain.process(&mut l, &mut r, &cx);
                    for i in 0..BLOCK {
                        let g = bus.fader.next() * bus.audible.next();
                        let p = bus.pan.next();
                        // Balance: the far side turns down, the near side stays.
                        l[i] *= g * (1.0 - p.max(0.0));
                        r[i] *= g * (1.0 + p.min(0.0));
                    }
                }
            }
            // Post-fader sends.
            for to in 0..BUSES {
                if !self.kept[b][to] {
                    continue;
                }
                let send = &mut self.buses[b].sends[to];
                let mut gains = [0.0f32; BLOCK];
                send.fill(&mut gains);
                let dest = &mut self.buses[to].from_buses[b];
                for i in 0..BLOCK {
                    dest[0][i] += l[i] * gains[i];
                    dest[1][i] += r[i] * gains[i];
                }
            }
            outs[b] = [l, r];
            let peak = |x: &[f32; BLOCK]| gain_to_db(x.iter().fold(0.0f32, |m, v| m.max(v.abs())));
            meters.write_bus(b, &BusReading { peak_l: peak(&l), peak_r: peak(&r), reduction });
            let bus = &mut self.buses[b];
            bus.out_lag.process(&mut l, &mut r);
            for i in 0..BLOCK {
                master_l[i] += l[i];
                master_r[i] += r[i];
            }
            self.stems[b] = [l, r];
        }
        self.last = outs;
    }

    /// Each bus's sound in the last block as the master heard it (after its fader, mute and
    /// solo, lined up): the stems.
    pub fn stems(&self) -> &[[[f32; BLOCK]; 2]; BUSES] {
        &self.stems
    }
}

/// Where one shape's sound goes: its main bus at full level, and a post-volume send to each
/// other bus.
pub struct ShapeRoute {
    pub main: u8,
    base: [f32; BUSES],
    keys: [Option<Track>; BUSES],
    gains: [Smoother; BUSES],
    scratch: [f32; BLOCK],
}

impl ShapeRoute {
    /// Straight into bus `main` (1..=3), no sends.
    pub fn main_only(main: u8) -> ShapeRoute {
        Self::new(main, [SILENT_DB; BUSES], [None, None, None])
    }

    /// The route of shape `id` from its voice settings.
    pub fn of(scene: &Scene, id: ThingId, v: &bpe_kit::sound::VoiceSettings) -> ShapeRoute {
        let track = |n: usize| {
            let a = Address { thing: id, slot: Some(SlotId::Sound), key: format!("out_send{n}") };
            scene.tracks.get(&a).filter(|t| !t.keys.is_empty()).cloned()
        };
        Self::new(
            v.out_bus.clamp(1, 3) as u8,
            [v.out_send1, v.out_send2, v.out_send3],
            std::array::from_fn(|i| track(i + 1)),
        )
    }

    fn new(main: u8, base: [f32; BUSES], keys: [Option<Track>; BUSES]) -> ShapeRoute {
        let mut r = ShapeRoute {
            main: main.clamp(1, 3),
            base,
            keys,
            gains: std::array::from_fn(|_| Smoother::new(Smoother::KNOB_MS)),
            scratch: [0.0; BLOCK],
        };
        r.at(0.0);
        for g in &mut r.gains {
            let t = g.target();
            g.reset(t);
        }
        r
    }

    /// Heads the sends for where their keys are at `time`.
    pub fn at(&mut self, time: f64) {
        for (i, g) in self.gains.iter_mut().enumerate() {
            let db = self.keys[i].as_ref().and_then(|t| track_value(t, time)).unwrap_or(self.base[i]);
            // A send to the main bus is not offered: it would only double the shape.
            g.set_target(if i + 1 == self.main as usize { 0.0 } else { fader_gain(db) });
        }
    }

    /// The sends glide on from where `old`'s were.
    pub fn adopt(&mut self, old: &ShapeRoute) {
        for (n, p) in self.gains.iter_mut().zip(&old.gains) {
            let t = n.target();
            n.reset(p.value());
            n.set_target(t);
        }
    }

    /// Adds the shape's block into its buses. Never allocates.
    pub fn send(&mut self, buses: &mut Buses, l: &[f32; BLOCK], r: &[f32; BLOCK]) {
        buses.add(self.main, None, l, r);
        for i in 0..BUSES {
            let g = &mut self.gains[i];
            if g.target() == 0.0 && g.settled() {
                continue;
            }
            g.fill(&mut self.scratch);
            buses.add(i as u8 + 1, Some(&self.scratch), l, r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mixer(sends: &[(u8, u8)]) -> MixerSettings {
        let mut m = MixerSettings::default();
        for &(from, to) in sends {
            let b = match from {
                1 => &mut m.bus1,
                2 => &mut m.bus2,
                _ => &mut m.bus3,
            };
            match to {
                1 => b.send1 = -6.0,
                2 => b.send2 = -6.0,
                _ => b.send3 = -6.0,
            }
        }
        m
    }

    #[test]
    fn send_loops_are_refused() {
        let m = mixer(&[(1, 2), (2, 3)]);
        assert!(would_loop(&m, 3, 1), "3 -> 1 closes 1 -> 2 -> 3");
        assert!(would_loop(&m, 2, 1));
        assert!(!would_loop(&m, 1, 3), "1 -> 3 alongside 1 -> 2 -> 3 is fine");
        assert!(would_loop(&m, 2, 2), "no bus sends to itself");
        // A loop in the settings (from a paste or keys) loses the send that closes it.
        let m = mixer(&[(1, 2), (2, 1)]);
        let (order, kept) = send_order(&edges_of(&m));
        assert!(kept[0][1] && !kept[1][0]);
        assert_eq!(order, [0, 1, 2]);
    }

    #[test]
    fn bus_order_is_topological() {
        let m = mixer(&[(3, 1), (2, 3)]);
        let (order, kept) = send_order(&edges_of(&m));
        assert_eq!(order, [1, 2, 0], "2 before 3 before 1");
        for from in 0..BUSES {
            for to in 0..BUSES {
                if kept[from][to] {
                    let p = |b| order.iter().position(|&x| x == b).unwrap();
                    assert!(p(from) < p(to));
                }
            }
        }
    }

    #[test]
    fn a_shape_on_two_buses_lines_up_when_one_has_a_limiter() {
        use bpe_core::{Settings, SlotId, Value};
        let kit = bpe_kit::kit();
        let reg = &kit.registry;
        let mut scene = bpe_kit::new_studio_scene(reg);
        let id = scene.add_layer_in(reg, ThingId::ROOT, SlotId::Mixer, "fx.limiter", Some(0)).unwrap();
        let card = scene.card_mut(ThingId::ROOT, SlotId::Mixer).unwrap();
        card.params.set(&bpe_core::layered_key(id, "ceiling"), Value::Number(0.0));
        card.params.set("bus2_on", Value::Bool(true));
        let mixer = MixerSettings::from_params(&card.params);
        let mut buses = Buses::new(BusesSpec::build(&scene, &mixer, reg, &TempoMap::default()));
        assert!(buses.latency() > 0, "the limiter looks ahead");
        let meters = Meters::default();
        let tempo = TempoMap::default();
        let mut out = Vec::new();
        for b in 0..8 {
            let mut x = [0.0f32; BLOCK];
            if b == 0 {
                x[3] = 0.25;
            }
            buses.begin(0.0);
            buses.add(1, None, &x, &x);
            buses.add(2, None, &x, &x);
            let (mut l, mut r) = ([0.0; BLOCK], [0.0; BLOCK]);
            buses.process(&mut l, &mut r, 0.0, &tempo, &[], &meters);
            out.extend_from_slice(&l);
        }
        let at: Vec<usize> = out.iter().enumerate().filter(|(_, v)| v.abs() > 1e-6).map(|(i, _)| i).collect();
        assert_eq!(at, [3 + buses.latency()], "one click, not two: {at:?}");
        assert!((out[at[0]] - 0.5).abs() < 1e-4);
    }
}
