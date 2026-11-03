# Strip the engine down to a basic version (rework step 1)

Requested by Aedan on 2026-11-03. This is a removal only; nothing new is designed, so there is no
spec. The current engine is saved as **Version 1**: branch `version-1` on GitHub (commit f61951d)
and `F:\!Ai Stuff\BallPhysicsEngine\bin\version-1\` (ball-studio.exe + bpe-render-next.exe).
A `v1` tag could not be pushed from the cloud container (the proxy drops tag pushes); the branch is
the backup.

## What goes

1. Nodes and the rules nodes system.
2. The Explore and Batch sections.
3. The Paint canvas thing and the marks system.
4. The whole sound and chords system.
5. Things: Obstacle, Liquid, Soft body, Swarm, Field, Meter, Emitter, Text.

## What must keep working

Balls, walls/arena, colours (paint/gradients/palettes/blend/grade), looks, trails, hit particles,
built-in effects (shader modules: they are fixed effect files, not node graphs), the camera,
keyframes and the timeline, scene settings, folders, dice/locks/presets, the editor layout and
rendering to mp4. No backward compatibility for old scene files.

## Decisions

- **Crates deleted:** `bpe-vm` (graph compiler/VM), `bpe-synth` (node synth), `bpe-mix` (mixer).
- **Crates detached from the new engine:** `bpe-graph` and `bpe-music` stay only while the old
  frozen engine needs them. Nothing in `bpe-core`, `bpe-runtime`, `bpe-gfx`, `bpe-kit`, `bpe-cli`
  or `bpe-studio` depends on them after this change. (Whether the old engine is deleted too is
  Aedan's call; asked on a card. If yes: delete `bpe-scene`, `bpe-sim`, `bpe-audio`, `bpe-render`,
  `bpe-export`, `bpe-app`, `bpe-graph`, `bpe-music`, `scenes/*.ron`, `scenes/showcase/`, and the
  old engine's goldens, and trim README/CLAUDE.md.)
- **Song, beats, tempo, beat sync and bands are sound** and go: Scene settings lose the Mix and
  Music groups (keep `seed`, moved to the World group, since physics randomness reads it).
  `TimeWarp`/`warp_for` go; the video clock equals the simulation clock, so `video_time` plumbing
  goes too.
- **Pulses stay** (they are part of keyframes/timeline) but only the `Hits` source remains, and it
  becomes the default. `Bands`, `PulseInputs` beat/band fields go.
- **Events:** only `Hit` and `Remove` remain (Spawn/Enter/Leave were for emitters and fields, Beat
  and Note were sound, Custom was rules, marks go with the canvas). Tag numbers of Hit stay 0, so
  hashes of kept scenes are unchanged.
- **Runtime:** `actions.rs` (the rules action queue and the Rules stage), `canvas_numbers.rs`,
  `solids.rs` (only obstacles used it) go, plus `World::pictures`, spawn/enter/leave plumbing used
  only by emitters/fields. ThingSim hooks that only removed things implemented are removed when
  nothing kept uses them.
- **Core:** `instrument.rs`, `progression.rs`, `ModuleGraph`, `Bus`, `ChannelStrip`, library
  `progressions/instruments/buses/master_fx`, `Scene::next_progression/next_instrument`,
  `Module::graph/channel/macros`, `ModuleKind::template`, `ProblemKind::BrokenGraph`,
  `resolve_with`'s broken set. `SlotId` loses `Sound`, `Rules`, `Marks`.
- **Motion/Bounce** lose their graph templates; `MotionState`/`BounceState` keep `settings` only.
- **gfx:** `canvas/`, `flow/` (GPU swarms and liquids), `text/` (only Text, Meter and the canvas
  numbers drew text), `pictures.rs` if only the canvas used it, and their shaders and tests.
  `render_with_flow`/`FlowSource` go.
- **Kit:** things `canvas emitter field liquid meter obstacle soft swarm text`; modules
  `rules sound drums melody marks look_liquid look_swarm motion_liquid motion_flock`; files
  `graphs.rs` (its `resolve_scene` is kept, without graph compiling), `music/`, `pictures.rs`,
  `score.rs`; scenes that use any removed thing/module; mixer glue in `lib.rs`.
- **CLI:** `batch.rs` and the batch commands; export writes a silent AAC track (`anullsrc`) so the
  mp4 still has audio for upload apps; stems/midi/extras flags go.
- **Studio:** `nodes/`, `explorer.rs`, `batch_window.rs`, `chords/`, `timeline/chords.rs`,
  `audio_out.rs`, `sound_job.rs`, `instruments.rs`, the Mix/buses/master-effects parts of the
  inspector and library, sound "try it", NodesSwitch on cards, Start-window starters that use
  removed things, add-thing menu entries.
- **Tests/goldens:** tests that only cover removed features are deleted. Goldens are re-blessed in
  debug and release; every kept scene's golden line must be byte-identical to main (physics of
  kept things must not move).
- If a removal would break a kept feature, the shared part stays and the PR says so.

## Steps and checks

1. Delete crates/folders/files listed above; remove registrations in `bpe_kit::{registry,sims,draws}`.
2. `cargo check --workspace --all-targets` until clean, removing dead code that the removed
   features alone used (follow compiler warnings for private items; grep for pub items).
3. `cargo clippy --workspace --all-targets` clean, `cargo fmt`.
4. Re-bless goldens (debug + release), diff against main: only removed scenes' lines vanish.
5. `cargo test --workspace` (debug) passes.
6. Update CLAUDE.md crate map; one Opus review subagent over the diff; open PR.
7. After "merge": build `-p bpe-studio` and `-p bpe-cli` for Windows from main, copy to F: `bin`
   keeping `bin\ball-studio-backup.exe`.

## Not to do

- Don't change physics of kept things, the colour system, the camera or the timeline beyond
  removing sound/beat lanes.
- Don't add anything new.
