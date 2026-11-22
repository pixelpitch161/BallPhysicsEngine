# The same physics maths on every platform

**Goal:** fix `library::every_scene_plays_its_full_length_and_lands_its_payoff` on the owner's PC (stem-battle's
payoff at 30.90 s on Windows, 26.52 s on Linux) at its cause, and take it off the nextest `gpu` profile's
"KNOWN ON THE CARD" list. Card-only bug 4 of `ballphysics/gpu-checks/card-only-bugs.md`.

## Cause

`f32::sin`/`cos`/`exp`/`powf`/`atan2`/`mul_add`... call the platform maths library (glibc on Linux, MinGW's on
Windows). Their last bits differ, and the chaotic arena fight grows a one-ulp difference into a 4 s different winner.
Confirmed: switching the Linux build to the `libm` crate moves the payoff to exactly the 30.90 s the PC got.

## Decisions

- Not a retune of stem-battle (it would drift again, and every other chaotic scene and golden stays split).
- A trait `bpe_core::maths::Maths` on f32/f64 with `det_`-prefixed methods (inherent methods win over a trait's of
  the same name, so the names must differ); free functions were rejected because rewriting `a.b().sin()` receivers
  by script is fragile, while a method rename is a one-token edit placed from clippy's own spans.
- glam's `libm` feature for `from_angle`, rotations and the like.
- Enforced by `disallowed-methods` in `clippy.toml` of bpe-core, bpe-runtime and bpe-kit (tests included).
  bpe-gfx, bpe-sound, bpe-dsp and bpe-studio keep std maths: they don't step the physics.
- Left as is: `sqrt`, `abs`, `floor`, `round`, `%` (exact by IEEE 754) and `powi` (compiler-builtins, same code).
- Not done: the sound engine's DSP (sound goldens stay Linux-only; a setting that Follows sound can still differ).

## Steps and checks

1. `crates/bpe-core/src/maths.rs` + unit test (agrees with std within 1e-6). Workspace `libm = "0.2"`.
2. `clippy.toml` in the three crates; rewrite every reported call (`.sin(` -> `.det_sin(`, `f32::sin` ->
   `Maths::det_sin`) and import the trait. Clippy `--workspace --all-targets -D warnings` clean.
3. Re-bless the physics goldens in debug and release (`BLESS_GOLDEN=1`); only physics lines move. Sound goldens
   don't move.
4. stem-battle's `WON` to 30.90; the library test passes on Linux.
5. `.config/nextest.toml` `gpu` profile: drop the library test from KNOWN ON THE CARD, and stop leaving out the CPU
   physics goldens (they should now match on Windows). CI on the owner's PC proves it.
6. Notes: `docs/notes/engine.md`, `checks.md`, CLAUDE.md pitfall.

## As built

As above. 345 call sites in 90 files. Twelve golden files moved (golden, moths, particles, particles-meeting,
particles-motion, rules; debug and release); health, text and sound goldens did not.
