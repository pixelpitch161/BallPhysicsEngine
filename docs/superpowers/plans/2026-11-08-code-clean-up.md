# W16: code clean-up

Workstream W16 of the 2026-11-08 improvement review (report "Ball Studio code review: what to improve", findings C5,
C6, C10 and C11). Aedan picked it on 2026-11-08 ("everything besides the Size section"); A9/C5's dropping of old scene
formats is covered by that pick. It is a clean-up: **no physics, sound or picture result may change**. Every golden
stays byte-identical, and picture tests keep their thresholds (a test moved to another drawing path may get new
numbers only where the old path drew something the new one draws differently, said in the commit).

One branch (`claude/project-thread-42e4gy`), one pull request, one commit per finding so each can be read and
reverted alone. Order: C10, C11, C6, C5 (smallest risk first; C5 deletes the most).

## C10: Particles starting looks as one function each

`bpe-kit/src/things/particles/looks.rs::recipe` is one 1,312-line `match`; `looks_motion.rs::recipe` is the same
for the eleven motion looks.

- Each arm becomes a function named after its look (`fn rain(cx: &Cx) -> Recipe`, `fn snow`, ...), in the same file
  and order. `Cx` is a small struct holding what the arms share today: `d` (the default `ParticlesSettings`), the
  `riding` closure's input (`rides`), `top` and `vanish`. `riding` becomes `Cx::riding(&self, s)`.
- `recipe(look, rides, fed_by)` stays as the one entry point: the fed Empty case, then a `match` of one line per look
  that calls its function. `looks_motion::recipe` the same way with its own functions.
- No value changes. The bodies are moved, not edited, apart from `d`, `top`, `vanish` and `riding(...)` becoming
  `cx.d`, `cx.top`, `cx.vanish` and `cx.riding(...)`.
- **Verify**: before the change, a throw-away test writes `save(scene)` of a scene with each look added (riding and
  not, fed and not) to a file; after it the same test must write identical text (diff empty), then it is deleted.
  `golden_particles*` goldens unchanged; `cargo test -p bpe-kit particles` green.

## C11: clear names for the "what does this edit restart" hooks, and a test over every setting

The `ThingSim` hooks (`bpe-runtime/src/plugins.rs`) that feed the fingerprints get names that say what they answer.
Pure renames (every implementation, every caller, docs and the CLAUDE.md pitfall line):

| Today | New | Feeds |
| --- | --- | --- |
| `affects_physics(key)` | `key_restarts_physics(key)` | `fingerprint` |
| `affects_physics_in(scene, thing, key)` | `key_restarts_physics_here(scene, thing, key)` | `fingerprint`, the timeline |
| `fingerprint_more(scene, thing, r)` | `physics_print_extra(scene, thing, r)` | `fingerprint` (and `beside_fingerprint`) |
| `more_fingerprint(r)` | `beside_print_extra(r)` | `beside_fingerprint` (and `fingerprint` for two-way things) |
| `affects_beside(key)` | `key_restarts_beside(key)` | `beside_fingerprint` |
| `beside_card_key(slot, key)` | `card_key_restarts_beside(slot, key)` | `beside_fingerprint` |
| `own_cards()` | `steps_own_cards()` | `fingerprint` hashes its Motion/Bounce cards whole |
| `needs_fingerprint(r)` | `needs_physics_print(r)` | `World::build` |

`fingerprint`, `sound_fingerprint`, `beside_fingerprint`, `heard` and `in_main` keep their names (they are the
fingerprints themselves or already plain). Each renamed hook's doc comment starts with the question it answers and
names the fingerprint it feeds.

**The test** (`crates/bpe-kit/tests/restarts.rs`): for every thing kind the kit registers (except Scene settings,
which is the root and is covered through its cards, and Folder), build the test start scene (`new_scene`: a hollow
circle and one ball) plus one thing of that kind added with `starters::add_thing` (Balls get a second group, an
Emitter its starting round, a Particles thing sits under the ball group so it can ride). Then for every setting the
thing holds (its kind's settings and each of its cards' settings, layers included) change the value:

- number: to a value inside its range that differs from the current one (the far end of the range from it, or
  current × 1.5 + 1 with no range), bool: flipped, choice: the next option, colour: another colour; other value types
  (paths, gradients, curves, text) are skipped and counted.

and check, after running both scenes for 1.5 s on their main playbacks:

1. If the balls moved differently (log hash or any body position differs), `fingerprint` must differ (else the
   editor would keep playing a stale preview).
2. For a thing stepped beside the physics (`is_beside`): if its own run differs (a particle playback built the way
   the editor builds `ParticleJob`, compared by the state hash the particles thing exposes), `beside_fingerprint`
   must differ.

The failure message names the thing kind, card and key. The test does not check the other way (a fingerprint that
moves for a visual setting only costs a restart). It also asserts that it changed at least a few hundred settings,
so a bug that skips everything can't pass. Run time is measured; it must stay under about a minute in debug, else
the run time is cut (fewer seconds, or one scene per kind run once with all its baseline shared).

Any hole the test finds is a real bug: fix it in the hook (that is the point of the test), say so in the PR.

## C6: one shared dice module and one shared shader-helper file

Rust: `bpe-core/src/dice.rs` (bpe-core is the one crate every scene crate depends on; bpe-dsp's noise stays its own,
it is the sound's signal generator and depends on nothing):

- `pub fn splitmix64(z: u64) -> u64` (the SplitMix64 finaliser), `pub fn fmix32(h: u32) -> u32` (MurmurHash3's
  finaliser), `pub fn pcg32(v: u32) -> u32` (the PCG hash), `pub fn unit24(x: u32) -> f32` (top 24 bits as `[0, 1)`),
  and `pub struct Rng` (moved from `bpe-kit/src/rng.rs`, which becomes `pub use bpe_core::dice::Rng`).
- Every copy of one of these bodies is replaced by a call. A site keeps its own seeding and mixing of inputs (that
  is what makes its numbers its own); only the shared primitive goes. Copies that are a different formula stay (they
  are not copies; changing them would move goldens) but are listed in the module doc so nobody adds another.
- **Verify**: each replaced function gets checked with a throw-away test against its old body on 10,000 inputs before
  the old body is deleted; all goldens unchanged.

Shaders: `bpe-gfx/src/shaders/common.wgsl` holds the helpers that are the same maths in several files today
(brightness, heat colour, value-noise hash and noise, where their maths really is identical; differing weights stay
apart). Each shader source that used a copy gets `common.wgsl` prepended once (where sources are concatenated, it is
put in front once, and the copies' names become the shared one). **Verify**: every shader parses (`naga` tests), and
the picture tests and full picture hashes pass unchanged.

## C5: remove dead code

1. **The three old drawing paths.** `Frame::balls` (`BallInstance`, `vs_ball`/`fs_ball*` in scene.wgsl, the ball and
   ball-object pipelines), `Frame::looks` (`LookInstance`, styled.wgsl's `vs_look`/`fs_look*`, the look pipelines,
   `bpe-kit/src/modules/look/basic.rs::instance`) and `Frame::shapes` (`ShapeInstance`, `vs_shape`/`fs_shape*`, the
   shape pipelines). The kit never fills any of them: balls draw as `layered`, shapes and text as `paths`. Only tests
   do. The tests move first to the current paths through two helpers in `bpe-gfx/src/pictures.rs`: a plain layered
   ball (a body record, optionally a pattern record with a `PaintGpu`) and a path disc/ring. Then the old paths go,
   with `each.rs`'s kinds for them and `Frame::clear`/`has_thing` mentions. Helpers the layered or path shaders still
   call (`ball_shape`, `sd_*`, `sphere_light` if used) stay.
2. **Old scene formats.** `Scene::OLDEST_FORMAT` becomes 12; files of format 7 to 11 are refused with a new
   `LoadError::TooOld` ("This scene was made with an earlier version of Ball Studio that this one can't open").
   The registry's upgrade list (`add_upgrade`, `upgrades_for`, the `Upgrade` type) and the three upgrades go:
   `masks_to_cuts` (and `OLD_MASK`), `cards_to_layers`, `single_effect_to_stack`. The test scenes still build some
   scenes from old cards (`scenes.rs::classic_cards`, `put_old_card`), so `cards_to_layers.rs` and
   `effects/upgrade.rs::{stack_of, is_old}` move behind `#[cfg(any(test, feature = "test-scenes"))]` and stop
   shipping; the goldens are untouched because the test scenes are built exactly as before. Tests of loading old
   formats become tests that they are refused.
3. **Stray fonts.** `crates/bpe-gfx/fonts/` (Anton, Space Mono, Varela Round: nothing includes them; the Text
   thing's fonts are `assets/fonts/text/`).

**Verify**: `cargo build`, clippy (dead-code warnings show anything missed), all tests, goldens unchanged, the moved
picture tests pass, and the full-size look check renders and looks the same as main (spot check of the contact sheet
against a run on main).

## Not doing

- No changes to what anything looks like, sounds like or how it moves.
- No new features, no renames beyond the hooks above, no splitting of other giant files (that is W17).
- Not touching bpe-dsp's noise generators or the sound's random LFO.

## Notes

`docs/notes/engine.md` gets an "as built" paragraph (the new hook names, `bpe_core::dice`, `common.wgsl`, the
restarts test, format 12 as the oldest), `docs/notes/particles.md` the looks layout and hook names; CLAUDE.md's
pitfall line about the hooks gets the new names.

## As built

- **C10** as planned: each look's recipe is its own function; a script compared every old arm with its new
  function after normalising and found them identical.
- **C11** as planned, plus a job with Particles riding the start scene's balls (each of their switches and
  choices changed in turn). The sweep found no missing hook, so no hook changed; a deliberate break was caught.
- **C6**: `fmix32` wasn't needed (the particles' `hash` is the one MurmurHash-style fold in use), and
  `bpe-kit/src/rng.rs` was deleted rather than kept as a re-export: every user now names `bpe_core::dice::Rng`.
  The shader helpers are `effects/shared.wgsl` (what the prelude and the in-ball programs both had) and
  `shaders/common.wgsl` (`luma`). Goldens unchanged.
- **C5**: formats below 12 are refused (`LoadError::TooOld`); `cards_to_layers` and `effects/upgrade.rs::stack_of`
  stay for the test scenes only. The old ball, look and shape paths are gone with their shaders; picture tests
  moved to the layered path and paths. Tests of the old wall outlines' gradients went with them (paths have their
  own); the shapes test is now `pictures::every_ball_shape_draws_its_own_outline`. Two comparisons against the
  classic shiny ball had nothing left to compare with and were dropped. In `every_builtin_effect_changes_the_picture`
  ball A is now brighter than white, since a plain layered body has no highlight for Lens flare to catch.
