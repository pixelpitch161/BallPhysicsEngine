# Text thing, part 5: experimental effects — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans (inline). Steps use checkbox (`- [ ]`)
> syntax. Project rule: one Opus review of the whole part before the PR, no review after every task.

**Goal:** the last part of the Text design: five experimental effects on a Text's Look card, pure in time and
drawing only. **Echo** (copies stacked toward a moving point, each smaller and turned round the colour wheel),
**Mirror** (2 to 12 ways round the text's middle, the picture's middle or a point, optionally flipped and cut to
wedges: a kaleidoscope), **Colour split** (red and blue copies pulled apart, harder on hits), **Glitch** (torn
horizontal bands of the picture, harder on hits, with their own colour split) and **Melt** (each column of the letters
drips and stretches down, then starts again). Four new looks: Glitch (#19), Melting text (#20), Kaleidoscope words
(#21), Echo stack (#23).

**Architecture:** the effects are Text-only layers of the `look.shape` card's Reaction section, kinds `text.fx.*`
(beside part 4's `text.move.*` letter moves, the same `LayerKind::fits`, `only` by letter number, on switch,
keyframes), listed by the editor in their own **Experimental** list. Melt changes each letter's contours on the CPU
before they are pushed. The other four work on the Text's finished shape instances (`PathInstance`s, letters and
shattered pieces alike) after they are pushed (`things/text/fx.rs`): copies that move, turn, scale, flip and fade
(sharing the letters' points where they can), clips that cut an instance to a band or a wedge (the convex `clip` part
1 and rules part 3 already draw through), and a per-instance **style** in the spare `PathInstance.clip[3]`: a channel
(red light only / green covering / blue light only), a hue turn and a colourise amount, read by one new branch at the
end of `paths.wgsl::shade_path`. Instances whose style is 0 (every shape and every Text without these effects) draw
byte for byte as before.

**Tech stack:** Rust, wgpu/WGSL, egui 0.37 + egui_kittest. No new crates.

**Spec:** `docs/superpowers/specs/2026-11-07-text-thing-design.md`, "Experimental (part 5)" and "Looks by part" (5).
Read parts 1 to 4's "As built" first.

## Global constraints

- Nothing moves for scenes without a Text: physics goldens (debug and release), sound goldens, rules goldens,
  `unlit_pictures`, `paths_tests::shapes_draw_as_before`.
- A Text without effects draws exactly as in part 4 (the same instances, points and records; part 1 to 4 tests stay).
- Pure in time: every effect is worked out at the moment drawn from its settings, the clock, the camera and the log.
- Drawing only: walls, parts, the light probe, the physics fingerprint and the editor's handles never see the effects.
- Plain words in every label and help; no colours made up in the editor; scene format stays 12.

## Decisions (each settles something the spec left open)

1. **Layers, not card settings.** The spec calls each effect "a fold on the Text's Look card". They are made as
   Text-only layers (`EffectKind`, section Reaction, card `look.shape`, `repeats`, no sub-layers, `fits` only `text`)
   shown in their own **Experimental** list with **+ Add effect**, under Letter moves. Why: they are off until added
   (no clutter on every Text), each has its on switch, Amount, keyframes and **Only some letters…** for free, the
   picked effect's settings show grouped like any layer's, and a shape's Look card never sees them. The editor splits
   the Reaction layers by id: `text.move.*` in Letter moves, `text.fx.*` in Experimental (`EFFECTS`, `is_effect`).
2. **Order.** Melt always acts first (it reshapes the outlines). Echo, Mirror, Colour split and Glitch act in list order
   on what the effects above them made (Echo then Mirror: a kaleidoscope of echoes; Mirror then Echo: echoes of the
   kaleidoscope). Each effect acts only on the instances of letters that wear it. A switched-off effect does nothing.
   An effect may be added twice (two Mirrors multiply).
3. **Every effect has Amount** (0 … 1, keyframe, pulse) first: Echo and Mirror scale their copies' opacity (the
   letters themselves stay whole), Colour split scales its distance, Glitch scales the share of bands torn and how far,
   Melt scales how far the drips have run.
4. **The style word** (`bpe_gfx::PathStyle`, `PathInstance::style`/`set_style`, `clip[3]`): `channel + 4·hue_q +
   4096·colourise_q`, `channel` 0 normal, 1 red light only, 2 green covering, 3 blue light only; `hue_q` the hue turn
   in 1024ths of a turn; `colourise_q` 0 … 15. At most 65535, exact in f32. 0 is every existing instance. The shader
   (`path_styled`, called only when `clip.w > 0.5`):
   - colour passes (0 the picture, 1 lit colour, 3 light given off, 7 the behind picture) turn the hue (Rodrigues
     rotation about the grey axis, linear so it works on premultiplied colour, clamped at 0) and colourise (mixed toward
     `rainbow(hue) × max(r, g, b)`, `rainbow` a full hue in linear light; white becomes the pure hue);
   - passes 0, 1 and 7 apply the channel: 1 → `(r, 0, 0, 0)`, 2 → `(0, g, 0, a)`, 3 → `(0, 0, b, 0)`. Alpha 0 adds
     light (premultiplied over), so a green covering copy drawn first and red and blue copies added after give the
     whole colour where all three overlap and coloured fringes where they part;
   - every other pass (surface, tint, tags, glass) and the object picture: channels 1 and 3 discard, 2 draws whole,
     so the light sees one letter.
5. **Draw order** (`fx::finish`): every instance carries a depth (0 the letters; Echo copy `j` −`j`, the furthest first)
   and whether it adds light. A stable sort by (depth, adds light) puts the copies behind the letters and every
   covering copy before the light added onto it.
6. **Copies of instances** (`fx::Inst { p, copy, letter, depth }`). Moving, turning and scaling a copy changes only its
   `a` (centre, angle, scale) and scales its band `b[0]` and reach past `c[2]`: its points and clip are shared. A
   flipped copy (Mirror's Mirrored copies) gets new points: every local point's y negated, its clip corners flipped and
   reversed (still clockwise); fast letters can't flip, so a Text with a flipping Mirror (or a Melt) draws its letters
   from outlines (`fx::needs_outlines`). Fading multiplies what shows: `clip[2] = 1 − (1 − clip[2]) × opacity`. At most
   `MOST_FX_INSTANCES` (16384) instances and `MOST_FX_POINTS` (1 000 000) new points a Text a frame: past that an
   effect makes no more copies (what it made stays).
7. **Clipping to a band or wedge** (`fx::clip_to`): an instance's convex clip (every letter, glyph and piece has one)
   cut by half-planes given in the frame's space, turned into its own units (`local_half_plane`), Sutherland–Hodgman,
   pushed as new corners; fewer than 3 corners drops the instance. An instance without a clip is left whole.
8. **Echo** `text.fx.echo` (`EchoEffect`): Amount (1), **Copies** (u32 1 … 64, 12), **Toward** (px pair from the
   text's middle, (0, −260)), **Wander** (px, 0: the point circles this far round Toward), **Wander speed** (turns/s,
   0.2), **Reach** (0 … 1, 0.8: how far toward the point the last copy goes), **Last size** (×, 0.5), **Hue each**
   (°, 15), **Hue spin** (°/s, 0), **Colourise** (0 … 1, 0), **Last opacity** (0 … 1, 0.4). Copy `j` of `n` (f = j/n)
   sits at `M + (P − M)·f·reach` scaled `1 + (last − 1)·f` about the text's middle M, opacity
   `amount·(1 + (last − 1)·f)`, hue `j·each + spin·t`. M is each copy's (riding) middle, in the frame's space (the
   screen for a screen Text).
9. **Mirror** `text.fx.mirror` (`MirrorEffect`): Amount (1), **Ways** (u32 2 … 12, 6), **Round** The text's middle /
   The middle of the picture / A point, **Centre** (px pair, for A point: the screen for a screen Text, the world
   otherwise), **Mirrored copies** (off), **Cut to wedges** (off), **Turn** (°, 0), **Spin** (°/s, 0), **Hue each copy**
   (°, 0), **Colourise** (0). Copy `k` (0 the letters) is the letters turned `β_k = turn + spin·t + k·360°/ways` round
   the centre C, odd copies first flipped across the line through C along x when mirrored (the particles' maths). Cut
   to wedges clips copy `k` to the wedge of half-angle 180°/ways about direction `β_k` from C (the letters too), so a
   flipped kaleidoscope joins at the wedges' edges. Copies `k ≥ 1` fade by Amount.
10. **Colour split** `text.fx.split` (`SplitEffect`): Amount (1), **Distance** (px, 8), **Angle** (°, 0: red to the
    right on the screen), **Wobble** (px, 0) with **Wobble speed** (per s, 6) and **Seed**, **Harder on hits** (0 … 1, 0),
    **Hits of** (`Target`, Anything), **Settles over** (s, 0.4). Distance d = amount·(distance + wobble·noise(t)) ×
    (1 + 3·harder·e), e = e^(−age/settles) of the latest hit of Hits of (0 with none). Each instance becomes a green
    covering copy in place, a red copy moved +d and a blue copy moved −d, both adding light; shifts are in screen units
    (a world Text's mapped back through the camera).
11. **Glitch** `text.fx.glitch` (`GlitchEffect`): Amount (0.2: the share of bands torn), **Band height** (px, 16),
    **Tear** (px, 40), **Changes a second** (10), **Tear colour split** (px, 6), **Seed**, **Harder on hits** (0.6),
    **Hits of** (Anything), **Settles over** (0.4). Bands are rows of the screen `band` high; band b at slot
    `floor(t·rate)` is torn when `dice(seed, b, slot) < amount + (1 − amount)·harder·e`, by
    `signed dice · tear · (1 + 2·harder·e)` along the screen's x. An instance spanning only untorn bands stays as it
    is; else it is cut into runs: each untorn run one piece in place, each torn band its own piece moved by its tear,
    split into green covering, red (+split) and blue (−split) when Tear colour split > 0. At most 64 bands an
    instance (taller instances keep the bands past that whole).
12. **Melt** `text.fx.melt` (`MeltEffect`): Amount (1), **Drip** (px, 160: the longest drip), **Speed** (share a
    second, 0.2), **Starts at** (s, 0), **Starts again every** (s, 0 = melts once), **Drip width** (px, 12), **Sag**
    (0 … 1, 0.25), **Seed**. Progress u = clamp((t − start)·speed) with t taken modulo Starts again every; drip
    length at x is `amount·drip·u²·(0.1 + 0.9·n(x)²)`, n a seeded smooth noise over columns Drip width wide across the
    text. Each point of a letter moves down (the picture's down, in the text's frame) by `len(x)·(sag + (1 − sag)·w)`,
    w its height share in its letter (0 its top, 1 its bottom). Contours are first cut into pieces at most a third of
    the Drip width long (at least 1.5 px, at most `MOST_POINTS` 2048 a letter), so flat bottoms drip. With Starts again
    every, the letters fade out over the last `min(0.5 s, a tenth)` of each round. A melting Text lays its gradient over
    its letters as laid out (it doesn't stretch). Shattering letters melt as they break.
13. **Hits** (`fx::hit_age`): the age of the latest Hit event before the moment that involves a thing Hits of names
    (`World::matching`, Me = the Text), read back at most 8 × Settles over; none is no boost.
14. **Looks** (`LOOKS` grows to 24):
    - **Glitch**: "GLITCH" in Bungee 150 px on the screen, white paint; Colour split (Distance 4, Harder on hits 0.8)
      and Glitch (Amount 0.12, Band 14, Tear 30, Tear colour split 8, Harder on hits 0.8).
    - **Melting text**: "MELT" in Bowlby One 220 px on the screen, a straight gradient warm yellow at the top to pink at
      the bottom (turn 90); Melt (Drip 260, Speed 0.2, Starts at 1, Starts again every 9, Drip width 10).
    - **Kaleidoscope words**: "WOW" in Bungee 80 px on the screen at (150, 0), spinning 70°/s, a cycling rainbow
      paint; Mirror 8 ways round the middle of the picture, mirrored, cut to wedges, Spin 17°/s.
    - **Echo stack**: "DEEP" in Anton 200 px on the screen, white paint with a dark outline; Echo (Copies 24, Toward
      (0, 0), Wander 300, Wander speed 0.12, Reach 0.8, Last size 0.45, Hue each 12, Hue spin 40, Colourise 1, Last
      opacity 0.9).
15. **Editor.** The Look editor of a Text shows **Experimental** ("+ Add effect", "None yet: add one to echo, mirror,
    split, tear or melt the letters.") after Letter moves; Letter moves lists and adds only `text.move.*`
    (`list_header` takes a kind filter). Rows, row menus (Only some letters…) and the picked effect's settings are the
    letter moves' own.

## Files

| File | What |
| --- | --- |
| `crates/bpe-gfx/src/frame.rs`, `shaders/paths.wgsl` | `PathStyle`, `style`/`set_style`, `path_styled` |
| `crates/bpe-gfx/src/paths_tests.rs` | style round trip; a styled channel split picture |
| `crates/bpe-kit/src/modules/look/shape/effects.rs` | the five effect kinds and settings, `EFFECTS`, `is_effect` |
| `crates/bpe-kit/src/modules/look/shape/mod.rs`, `lib.rs` | module, registration |
| `crates/bpe-kit/src/things/text/fx.rs` (+ `fx_tests.rs`) | `Fx::of`, melt, the instance effects, finish |
| `crates/bpe-kit/src/things/text/draw.rs` | hooks: no tiles, melted outlines, opacity, the instance pass |
| `crates/bpe-kit/src/things/text/moves.rs` | `wobble`, `dice` shared (pub(crate)) |
| `crates/bpe-kit/src/things/text/looks.rs` | the four looks |
| `crates/bpe-kit/tests/text_pictures.rs` | split, mirror, echo hue pictures |
| `crates/bpe-studio/src/look_editor/mod.rs` | the Experimental list, filtered headers, kittests |
| `CLAUDE.md` | Text part 5 paragraph |

## Tasks

### Task 1: the style word
- [ ] `PathStyle { channel, hue, colourise }`, `Channel`, encode/decode; `path_styled` in paths.wgsl (shade_path and
  the object entry).
- Verify: unit test round trip (0 is the default; every channel, hue 0.999, colourise 1); `shapes_draw_as_before`
  unchanged; a GPU test: a white square drawn as green covering + red and blue adds at the same place reads white,
  and moved apart shows red on one side and blue on the other.

### Task 2: effect kinds
- [ ] `effects.rs` settings and `EffectKind`s, registered after the moves; `is_effect`.
- Verify: registry check passes; the kinds fit only a Text; `moves_of` ignores them.

### Task 3: Melt
- [ ] `fx::Melt` (progress, fade, `melt(contours, letter box)`), subdivision; draw.rs: outlines, full, shatter, opacity,
  gradient laid over placed letters, no tiles.
- Verify: unit tests (bottom moves more than top; columns differ; before Starts at nothing moves; starts again; fades
  before it restarts; a letter not wearing it stays).

### Task 4: the instance effects
- [ ] `Inst`, `clip_to`, `local_half_plane`, Echo, Mirror (turn, flip, wedges), Colour split, Glitch, `hit_age`,
  `finish` (sort, caps); draw.rs runs them over the Text's instances.
- Verify: unit tests (Echo: n copies before the letters, smaller and further each, hue codes; Mirror: ways × letters,
  centres turned round C, flipped copies' points mirrored, wedge clips inside the wedge; Colour split: three a letter,
  coverings before adds, moved ±d, harder right after a hit; Glitch: no torn band leaves the instance as it was, a
  torn band makes pieces whose clips tile the letter's, moved sideways; only some letters; no effects: identical
  frame).

### Task 5: looks
- [ ] The four looks.
- Verify: every look resolves cleanly (`every_look_adds_a_text_that_resolves_cleanly`); each draws more instances than
  letters at a moment it acts.

### Task 6: pictures
- [ ] `text_pictures`: Colour split shows red right and blue left of a white letter; a 2-way Mirror of an off-centre
  word shows letters on both sides; an Echo copy with Colourise isn't white.

### Task 7: editor
- [ ] Experimental list; Letter moves list filtered.
- Verify: kittest: a Text's Look shows "Experimental"; "+ Add effect" adds Echo (and lists no moves); "+ Add move"
  lists no effects; a shape's Look shows neither.

### Task 8: finish
- [ ] `cargo fmt`, `cargo clippy --workspace`, tests crate by crate (bpe-gfx `--test-threads=2`), goldens unchanged in
  debug and release.
- [ ] CLAUDE.md paragraph; this plan's "As built".
- [ ] One Opus review, fixes, PR, merge. Then (all five Text parts merged) install: build from main, strip, replace
  `bin\ball-studio.exe` keeping a backup, and copy `assets/fonts/text/` to `bin\fonts\`.

## Not doing (this part)

- Effects on the walls or parts, on Shapes, or read by the light probe (drawing only, like the moves).
- Flipping a gradient with a Mirror's flipped copy (the copy's letters flip, the gradient across them doesn't).
- Echo copies of earlier moments (copies are of the text now).
- Scanlines (a Shader layer or picture effect does them).

## As built

- **Style word** (`bpe_gfx::PathStyle`, `PathChannel`, `PathInstance::style`/`set_style` in `clip[3]`) and
  `paths.wgsl::path_styled` (hue turn about the grey axis, `path_rainbow` colourise, channels in passes 0, 1 and 7;
  red/blue copies discard in the blocker passes and the object picture). `shade_path` is the old body renamed
  `shade_path_unstyled` plus the one branch; `shapes_draw_as_before` holds.
- **Kinds** (`modules/look/shape/effects.rs`): `EchoEffect`, `MirrorEffect` (`MirrorRound`), `SplitEffect`,
  `GlitchEffect`, `MeltEffect`, registered as `MoveKind`s after the moves.
- **Effects** (`things/text/fx.rs`): `Fx::of`/`melt`/`opacity`/`needs_outlines`, `MeltNow`, `melt_contours`
  (`finer`), `FxCx`, `hit_age`, `clip_to`/`cut`/`local_half_plane`, `flipped`, `split`, `echo`, `mirror`,
  `colour_split`, `glitch`, `apply`. `draw.rs` turns fast letters off for Melt or a flipping Mirror, melts outlines,
  `full` contours and shattering letters, multiplies Melt's fade into each letter's opacity, lays a melting Text's
  gradient over its letters as laid out, notes which letter owns every pushed instance (pieces too) and runs `apply`.
- **Looks** (24): Glitch, Melting text, Kaleidoscope words (WOW 80 px at (150, 0)), Echo stack.
- **Editor**: the Experimental list (`list_header_where`), Letter moves listing only moves.
- **Tests**: `fx_tests.rs` (11), `effects.rs` (1), `paths_tests` (2), `text_pictures` (3), looks
  `the_part_5_looks_copy_and_reshape_their_letters`, kittest `a_texts_look_has_experimental_effects_apart_from_its_moves`.
