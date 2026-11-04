//! The chain's filters: the state-variable types (low, high, band, notch, peak, shelves; 12 or
//! 24 dB), the ladder, the comb and the formant filter, two channels each, with their settings
//! glided sample by sample over a block so a moving cutoff never clicks.

pub mod comb;
pub mod formant;
pub mod ladder;
pub mod response;
pub mod svf;

pub use comb::{COMB_LEN, CombLine};
pub use ladder::{LadderCoefs, LadderState};
pub use response::response_db;
pub use svf::{SvfCoefs, SvfMode, SvfState};

use crate::{BLOCK, SAMPLE_RATE};

/// A filter's type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FilterType {
    #[default]
    LowPass,
    HighPass,
    BandPass,
    Notch,
    Peak,
    LowShelf,
    HighShelf,
    Ladder,
    Comb,
    Formant,
}

impl FilterType {
    pub const ALL: [FilterType; 10] = [
        FilterType::LowPass,
        FilterType::HighPass,
        FilterType::BandPass,
        FilterType::Notch,
        FilterType::Peak,
        FilterType::LowShelf,
        FilterType::HighShelf,
        FilterType::Ladder,
        FilterType::Comb,
        FilterType::Formant,
    ];

    /// The state-variable mode it is made of (none for the ladder, comb and formant).
    pub fn svf_mode(self) -> Option<SvfMode> {
        Some(match self {
            FilterType::LowPass => SvfMode::Low,
            FilterType::HighPass => SvfMode::High,
            FilterType::BandPass => SvfMode::Band,
            FilterType::Notch => SvfMode::Notch,
            FilterType::Peak => SvfMode::Bell,
            FilterType::LowShelf => SvfMode::LowShelf,
            FilterType::HighShelf => SvfMode::HighShelf,
            _ => return None,
        })
    }
}

/// A filter's settings at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FilterParams {
    pub kind: FilterType,
    /// Two stages (24 dB) for the state-variable types.
    pub slope24: bool,
    /// Where it cuts (a comb's pitch); the caller has applied key tracking.
    pub cutoff_hz: f32,
    /// 0..1.
    pub reso: f32,
    /// 0..1.
    pub drive: f32,
    /// Peak and shelves.
    pub gain_db: f32,
    /// Formant: 0..4 (A E I O U).
    pub vowel: f32,
    /// Formant: semitones.
    pub shift: f32,
}

impl Default for FilterParams {
    fn default() -> Self {
        FilterParams {
            kind: FilterType::LowPass,
            slope24: false,
            cutoff_hz: 2000.0,
            reso: 0.0,
            drive: 0.0,
            gain_db: 0.0,
            vowel: 0.0,
            shift: 0.0,
        }
    }
}

/// Q of the two stages of a fourth-order Butterworth.
const BUTTER4: [f64; 2] = [0.541_196_1, 1.306_563];
/// The least damping resonance reaches.
const K_MIN: f64 = 0.025;

/// How many numbers a filter's tuning takes (glided together).
pub(crate) const NC: usize = 12;
/// Where the drive sits among them.
const DRIVE: usize = NC - 1;

/// `tan(π·hz/rate)` with the cutoff kept where the filter can reach.
pub fn tan_w(hz: f64) -> f64 {
    let hz = if hz.is_finite() { hz.clamp(10.0, 0.48 * SAMPLE_RATE as f64) } else { 1000.0 };
    (std::f64::consts::PI * hz / SAMPLE_RATE as f64).tan()
}

/// The damping of a stage at resonance `r` (0..1), from `k0` (no resonance) to [`K_MIN`].
fn damping(k0: f64, r: f64) -> f64 {
    let r = r.clamp(0.0, 1.0);
    k0 * (1.0 - r) + K_MIN * r
}

/// The stages of a state-variable filter (one or two).
pub(crate) fn svf_stages(p: &FilterParams) -> Option<([SvfCoefs; 2], usize)> {
    let mode = p.kind.svf_mode()?;
    let r = p.reso as f64;
    let w = tan_w(p.cutoff_hz as f64);
    let shelf_like = matches!(mode, SvfMode::Bell | SvfMode::LowShelf | SvfMode::HighShelf);
    let gain = if p.gain_db.is_finite() { p.gain_db.clamp(-48.0, 48.0) as f64 } else { 0.0 };
    if p.slope24 {
        let a = 10f64.powf(gain / 2.0 / 40.0);
        let k1 = 1.0 / BUTTER4[0];
        let k2 = if shelf_like { damping(1.0 / BUTTER4[1], r * 0.8) } else { damping(1.0 / BUTTER4[1], r) };
        Some(([SvfCoefs::new(mode, w, k1, a), SvfCoefs::new(mode, w, k2, a)], 2))
    } else {
        let a = 10f64.powf(gain / 40.0);
        let k =
            if shelf_like { damping(std::f64::consts::SQRT_2, r * 0.8) } else { damping(std::f64::consts::SQRT_2, r) };
        Some(([SvfCoefs::new(mode, w, k, a), SvfCoefs::default()], 1))
    }
}

/// The ladder's tuning.
pub(crate) fn ladder_coefs(p: &FilterParams) -> LadderCoefs {
    LadderCoefs {
        g: tan_w(p.cutoff_hz as f64) as f32,
        // Past 4 it sings on its own.
        k: 4.3 * p.reso.clamp(0.0, 1.0),
        d: 1.0 + 9.0 * p.drive.clamp(0.0, 1.0),
    }
}

/// The comb's period (samples), feedback and output scale.
pub(crate) fn comb_coefs(p: &FilterParams) -> (f32, f32, f32) {
    let hz = if p.cutoff_hz.is_finite() { p.cutoff_hz.clamp(20.0, 0.45 * SAMPLE_RATE) } else { 440.0 };
    let fb = 0.5 + 0.49 * p.reso.clamp(0.0, 1.0);
    (SAMPLE_RATE / hz, fb, (1.0 - fb).sqrt())
}

/// The formant bands: each (g, k, level).
pub(crate) fn formant_bands(p: &FilterParams) -> [(f32, f32, f32); 3] {
    let shift = if p.shift.is_finite() { (p.shift.clamp(-24.0, 24.0) as f64 / 12.0).exp2() } else { 1.0 };
    let width = 2.0 - 1.5 * p.reso.clamp(0.0, 1.0) as f64;
    formant::formants(p.vowel as f64).map(|(f, db, bw)| {
        let f = f * shift;
        let k = (bw * width / f).clamp(0.01, 2.0);
        (tan_w(f) as f32, k as f32, 10f64.powf(db / 20.0) as f32)
    })
}

/// The tuning as one list of numbers, glided as one.
fn coefs(p: &FilterParams) -> [f32; NC] {
    let mut c = [0.0; NC];
    match p.kind {
        FilterType::Ladder => {
            let l = ladder_coefs(p);
            c[0] = l.g;
            c[1] = l.k;
            c[2] = l.d;
        }
        FilterType::Comb => {
            let (d, fb, norm) = comb_coefs(p);
            c[0] = d;
            c[1] = fb;
            c[2] = norm;
        }
        FilterType::Formant => {
            for (i, (g, k, a)) in formant_bands(p).into_iter().enumerate() {
                c[3 * i] = g;
                c[3 * i + 1] = k;
                c[3 * i + 2] = a;
            }
        }
        _ => {
            if let Some((stages, _)) = svf_stages(p) {
                for (i, s) in stages.iter().enumerate() {
                    c[5 * i..5 * i + 5].copy_from_slice(&[s.g, s.k, s.m0, s.m1, s.m2]);
                }
            }
        }
    }
    c[DRIVE] = p.drive.clamp(0.0, 1.0);
    c
}

fn svf_at(c: &[f32; NC], stage: usize) -> SvfCoefs {
    let s = &c[5 * stage..5 * stage + 5];
    SvfCoefs { g: s[0], k: s[1], m0: s[2], m1: s[3], m2: s[4] }
}

/// The drive into a state-variable, comb or formant filter: none at 0, a tanh up to +24 dB.
#[inline]
fn drive_in(x: f32, t: f32) -> f32 {
    if t <= 0.0 {
        return x;
    }
    let d = 1.0 + 15.0 * t;
    x + t * ((d * x).tanh() / d.sqrt() - x)
}

/// One channel's memory.
#[derive(Clone, Debug, Default)]
struct Channel {
    svf: [SvfState; 3],
    ladder: LadderState,
}

/// A filter of one type, two channels. Its type and slope are fixed when it is made (a comb
/// allocates its delay lines then); everything else is set each block.
#[derive(Clone, Debug)]
pub struct StereoFilter {
    kind: FilterType,
    slope24: bool,
    cur: [f32; NC],
    step: [f32; NC],
    left: u32,
    fresh: bool,
    ch: [Channel; 2],
    comb: Option<Box<[CombLine; 2]>>,
}

impl StereoFilter {
    /// Allocates (a comb's lines): build it off the audio path.
    pub fn new(kind: FilterType, slope24: bool) -> Self {
        StereoFilter {
            kind,
            slope24,
            cur: [0.0; NC],
            step: [0.0; NC],
            left: 0,
            fresh: true,
            ch: Default::default(),
            comb: (kind == FilterType::Comb).then(|| Box::new([CombLine::default(), CombLine::default()])),
        }
    }

    pub fn kind(&self) -> FilterType {
        self.kind
    }

    pub fn slope24(&self) -> bool {
        self.slope24
    }

    /// Silences its memory; the next [`StereoFilter::set`] takes its tuning at once.
    pub fn reset(&mut self) {
        for c in &mut self.ch {
            c.svf.iter_mut().for_each(SvfState::reset);
            c.ladder.reset();
        }
        if let Some(lines) = &mut self.comb {
            lines.iter_mut().for_each(CombLine::reset);
        }
        self.fresh = true;
        self.left = 0;
    }

    /// Takes on another filter's memory and tuning, without allocating (both of one type).
    pub fn copy_state_from(&mut self, other: &StereoFilter) {
        if other.kind != self.kind || other.slope24 != self.slope24 {
            self.reset();
            return;
        }
        self.cur = other.cur;
        self.step = other.step;
        self.left = other.left;
        self.fresh = other.fresh;
        self.ch = other.ch.clone();
        if let (Some(mine), Some(theirs)) = (&mut self.comb, &other.comb) {
            for (m, t) in mine.iter_mut().zip(theirs.iter()) {
                m.copy_from(t);
            }
        }
    }

    /// The settings for the next block: the tuning glides there over [`BLOCK`] samples (at once
    /// after a reset). `p.kind` and `p.slope24` are the filter's own.
    pub fn set(&mut self, p: &FilterParams) {
        let p = FilterParams { kind: self.kind, slope24: self.slope24, ..*p };
        let target = coefs(&p);
        if self.fresh {
            self.cur = target;
            self.step = [0.0; NC];
            self.left = 0;
            self.fresh = false;
            return;
        }
        for ((s, t), c) in self.step.iter_mut().zip(target).zip(self.cur) {
            *s = (t - c) / BLOCK as f32;
        }
        self.left = BLOCK as u32;
    }

    /// One sample of each channel.
    #[inline]
    pub fn tick(&mut self, l: f32, r: f32) -> (f32, f32) {
        if self.left > 0 {
            for (c, s) in self.cur.iter_mut().zip(self.step) {
                *c += s;
            }
            self.left -= 1;
        }
        let c = &self.cur;
        let drive = c[DRIVE];
        match self.kind {
            FilterType::Ladder => {
                let lc = LadderCoefs { g: c[0], k: c[1], d: c[2] };
                (self.ch[0].ladder.tick(&lc, l), self.ch[1].ladder.tick(&lc, r))
            }
            FilterType::Comb => {
                let Some(lines) = &mut self.comb else { return (l, r) };
                let (d, fb, norm) = (c[0], c[1], c[2]);
                (lines[0].tick(drive_in(l, drive), d, fb, norm), lines[1].tick(drive_in(r, drive), d, fb, norm))
            }
            FilterType::Formant => {
                let bands: [(SvfCoefs, f32); 3] = std::array::from_fn(|i| {
                    let (g, k, a) = (c[3 * i], c[3 * i + 1], c[3 * i + 2]);
                    (SvfCoefs { g, k, m0: 0.0, m1: k, m2: 0.0 }, a)
                });
                let mut out = [0.0f32; 2];
                for (ch, x) in [l, r].into_iter().enumerate() {
                    let x = drive_in(x, drive);
                    let st = &mut self.ch[ch].svf;
                    out[ch] = bands.iter().zip(st.iter_mut()).map(|((bc, a), s)| s.tick(bc, x) * a).sum();
                }
                (out[0], out[1])
            }
            _ => {
                let one = svf_at(c, 0);
                let two = svf_at(c, 1);
                let mut out = [0.0f32; 2];
                for (ch, x) in [l, r].into_iter().enumerate() {
                    let st = &mut self.ch[ch].svf;
                    let mut y = st[0].tick(&one, drive_in(x, drive));
                    if self.slope24 {
                        y = st[1].tick(&two, y);
                    }
                    out[ch] = y;
                }
                (out[0], out[1])
            }
        }
    }

    /// One block in place.
    pub fn process(&mut self, l: &mut [f32; BLOCK], r: &mut [f32; BLOCK]) {
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            (*a, *b) = self.tick(*a, *b);
        }
    }
}

#[cfg(test)]
mod tests;
