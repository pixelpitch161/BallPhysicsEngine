//! The DSP blocks a patch is built from.

pub mod adsr;
pub mod amp;
pub mod chorus;
pub mod crush;
pub mod delay;
pub mod drive;
pub mod dynamics;
pub mod eq;
pub mod filter;
pub mod ladder;
pub mod lfo;
pub mod maths;
pub mod noise;
pub mod osc;
pub mod physics;
pub mod reverb;
pub mod sampler;
pub mod space;
pub mod tables;

pub use adsr::Adsr;
pub use amp::Amp;
pub use chorus::Chorus;
pub use crush::Bitcrush;
pub use delay::Delay;
pub use drive::Drive;
pub use dynamics::{Clipper, Compressor, Limiter, OVERSAMPLE_DELAY, Oversampler4, TruePeakLimiter, true_peak};
pub use eq::Equaliser;
pub use filter::Svf;
pub use ladder::Ladder;
pub use lfo::{Hold, Lfo};
pub use maths::{Arith, Crossfade, Macro, Mix, NoteIn, PitchToHz, Scale, VoiceOut};
pub use noise::Noise;
pub use osc::Osc;
pub use physics::{PhysicsIn, Reads};
pub use reverb::Reverb;
pub use sampler::{Sampler, missing_samples};
pub use space::{Follower, Width};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{run, sine};
    use crate::{BLOCK, Block, Ctx, Knobs};
    use bpe_graph::Param;

    #[test]
    fn extreme_knobs_stay_finite() {
        let osc = Knobs::default()
            .with("wave", Param::Choice("saw".into()))
            .with("unison", Param::Number(8.0))
            .with("detune", Param::Number(100.0))
            .with("fm", Param::Number(10.0));
        let filter = Knobs::default().with("cutoff", Param::Number(30_000.0)).with("resonance", Param::Number(1.0));
        let ladder = filter.clone().with("drive", Param::Number(1.0));
        let mut source = Osc::new(&osc);
        let mut svf = Svf::new(&filter);
        let mut lad = Ladder::new(&ladder);
        let mut cx = Ctx { note: 120.0, ..Ctx::default() };
        source.reset(&cx);
        let mut checked = 0;
        for b in 0..1500 {
            let fm = sine(3_000.0, 48_000.0, b * BLOCK);
            let mut a = [crate::SILENT];
            source.process(&cx, &[None, Some(&fm), None], &mut a);
            let mut f = [crate::SILENT];
            svf.process(&cx, &[Some(&a[0]), Some(&fm)], &mut f);
            let mut l = [crate::SILENT];
            lad.process(&cx, &[Some(&f[0]), Some(&fm)], &mut l);
            for buf in [&a[0], &f[0], &l[0]] {
                for side in buf {
                    for &v in side {
                        assert!(v.is_finite() && v.abs() <= 4.0, "{v} at block {b}");
                        checked += 1;
                    }
                }
            }
            cx.advance();
        }
        assert!(checked > 0);
        // And on a plain sine at the cutoff, the resonant ladder still holds.
        let out = run(&mut Ladder::new(&ladder), &mut Ctx::default(), 1500, |b, _| {
            vec![Some(sine(1_000.0, 48_000.0, b * BLOCK))]
        });
        assert!(out.iter().all(|f| f[0].is_finite() && f[0].abs() <= 4.0));
    }
}
