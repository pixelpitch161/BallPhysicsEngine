# Fix: knob modulation works like Serum

Aedan, 2026-11-05: "When a modulator is assigned to a knob, adjusting the knob should behave like Serum. It seems
like currently the amount of modulation is a % based on the current value, rather than a linear range of the knob.
When adjusting the knob value, the modulation range changes in a weird way."

## Root causes (found, not guessed)

1. **Hz knobs.** Cutoff, a Downsample's Rate and an LFO's Rate are drawn turning in ratios (`TabCx::log_knob`,
   `Knob::log`), but `voices/knobs.rs::targets` gives every target `log: false`, so the engine adds
   `amount × (hi − lo)` Hz, and `modmatrix::route_ends` (the ring and the "a → b" text) works in Hz too. Drawn on a
   log dial, a fixed number of Hz is a huge arc low down and a sliver high up: the ring's size changes as the knob
   turns. `mods::amount_for` divides Hz by the Hz span as well.
2. **Level knobs.** Gain, Level, Sub level and Volume (`Combine::Scale`) take every route as a VCA:
   `gain × (1 − a·(1 − m))`, a percentage of the current level. Amount 1 always reaches silence, whatever the knob,
   so the ring stretches as the knob turns, and the amount isn't a share of the knob.

## The rule after the fix (Serum's)

A route's Amount is a share of the knob's own travel (−100..+100 %): at modulator value m the knob sits at
`turn(base) + amount × m` (× (2m − 1) both ways), clamped to the knob's ends, then read back as a value. `turn` is
linear for most knobs and even in ratios for Hz knobs (`modmatrix::turns_in_ratios(def)`: unit "Hz" and a range
above 0), the same function the editor's knob uses to draw. Turning the knob slides the range without changing its
size; the ring stops at the knob's end.

**Exception (decision card, recommended option, built first):** an ADSR on Gain, Level, Sub level or Volume stays a
volume shaper (`Combine::Scale`, unchanged maths): the knob sets the peak and the envelope shapes it. That keeps the
starter patch, the note-ending envelope (`level_envs`) and Follow sound's Level as they are. LFO, Distance and Hit
routes on those knobs add in dB like any knob, the bottom (−60 dB) being silence.

## Changes

- `bpe-sound/src/modmatrix.rs`
  - `turns_in_ratios(def: &SettingDef) -> bool`.
  - `is_level_key(key)`; `combine_of(key, from_envelope: bool)`: Scale only when both.
  - `ModMatrix::build(routes, targets, envelopes: &[LayerId])`.
  - `ModMatrix::adds(target) -> bool` and `offset_db(target, base_db, sources) -> f32` (Add routes only, clamped),
    plus `adding(target)` iterator for per-sample work.
  - `route_ends(r, base, range, log, combine)`.
- `bpe-sound/src/voices/knobs.rs::targets`: `log: turns_in_ratios(def)`.
- `bpe-sound/src/patch.rs`: pass the ADSR ids to `build`.
- `bpe-sound/src/voices/shape.rs`: per gain knob with Add routes, `offset_by_routes` works the gain sample by
  sample from the smoothed knob (back to dB, the routes added, clamped, `knob_gain`), before the VCA routes.
- `bpe-sound/src/control.rs::level`: `knob_gain(offset_db(...)) * factor(...)`.
- `bpe-studio`: `TabCx::knob_with` takes `log` from `turns_in_ratios(def)` (no `log_knob`); `TabCx::combine(route)`
  asks whether the source is an ADSR; `mods::ring`/`amount_for` work in turns (`amount = turn(to) − turn(base)`);
  `routes.rs` and `playing.rs` pass the new arguments.

## Not to do

- Don't change the VCA maths, the starter patch or the sound goldens' starter patch (they must not move).
- Don't touch the Mixer's knobs (they take no routes).

## Verify

- modmatrix unit tests: a Hz route adds evenly in octaves; the ring keeps its size in turns as the base moves and
  clamps at the end; an LFO on Gain adds dB; an ADSR on Gain still scales.
- shape voice test: an LFO on Gain at base −inf is heard (offset up from silence); at base 0 dB with amount −0.5 it is
  quieter.
- editor tests: `amount_for` round-trips for linear, log and dB knobs; ring size constant in turns as base moves.
- `cargo test --workspace` (debug), sound goldens unchanged except where a golden patch routes a Hz knob.
