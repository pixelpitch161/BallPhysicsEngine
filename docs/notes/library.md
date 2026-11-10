# Built-in scenes (library, Scenes window)

Spec `docs/superpowers/specs/2026-11-10-built-in-scenes-design.md`; plans in `docs/superpowers/plans/built-in-scenes/`.
About 40 scenes in three levels (14 starters, 14 in-between, 12 spectacles), built over five parts. The editor still
opens to a blank project; the scenes are only reached through the toolbar's **Scenes** button.

## The library (`bpe-kit/src/library/`)

- Always compiled (not behind `test-scenes`, which holds the test scenes in `scenes.rs`).
- `mod.rs`: `Level` (Starter, InBetween, Spectacle), `Kind` (8, with `label()`), `SoundLevel` (None, Simple,
  Designed), `Payoff`/`PayoffEvent` (RuleFires, Breaks, Counter) for moments that must land on time, `SceneInfo` (id,
  name, level, kinds, about, uses, change_first, sound, seconds, poster_seconds, preview_seconds, swatch, payoff) with
  `matches(words)` and `kinds_label()`, `Entry { info, build }` with `scene(reg)` (sets the name, checks the length),
  `all()` (list order is the window's order) and `find(id)`.
- A scene is code: `build(&Registry) -> Scene` makes it from `new_studio_scene` with today's starting cards, through
  the same edits the editor makes, so a file-format change can't break one and an opened scene owns every setting.
  Nothing marks a scene as built-in.
- `build.rs`: `Lib` helpers that keep a scene file short (the full list is under Adding a scene). `params::<S>` and
  `card::<S>` edit settings read as `S` (only non-default keys are kept, unknown keys stay); `ball` sets Gravity × 1
  (the Motion card's default is 0); `notes` takes MIDI numbers (middle C 60). Value helpers `hex`, `n`, `int`,
  `choice`, `text`.
- `starters/`, `middle/` (the in-between scenes) and `spectacles/`: one file per scene, each with `pub(crate) const
  ENTRY`.

### Adding a scene

1. A file in its level's folder with `ENTRY` and `build`; add it to the folder's `mod.rs` and to `ENTRIES` in
   `library/mod.rs` in the window's order.
2. Bump `levels_so_far` in `bpe-kit/tests/library.rs`.
3. Run `cargo test -p bpe-kit --test library` and `cargo test -p bpe-sound --test library_sound`.
4. Look at it at full size: `LOOK_OUT=<dir> LOOK_ONLY=library cargo test --release -p bpe-kit --test full_size_look
   -- --ignored --nocapture` (three moments per scene: the poster, 40% and 80%).

Helpers in `library/build.rs`: `new`, `scene_settings`, `add`, `at`, `params`, `card`, `fill` (the card Add card puts
in a slot, a Health card say), `layer`, `layer_params`, `look`, `look_in` (a ready look in one colour, put on as the
editor's Keep mine does: the colour goes where the look's main colour goes), `colour_in` (a shape's or Text's
Colour layer set to one colour), `sound`, `notes`, `melody` (Plays Melody with these steps, each step's notes sorted), `tone` (a `Tone`:
Bell, Pluck, Mallet, Pad, Bass: the oscillators and envelope, with the envelope routed to Osc B too), `reverb` (a
Reverb on the Mixer's first bus), `keys` (keyframes, moving evenly), `rule`, `rule_doing` (a chain of When and Ifs,
then several Dos each hung off its last block), `text`, `text_saying`, `ring`, `ball`, `emitter`, `light` (a light at
a place; the scene gets its Lighting card; `light_in` puts it in a parent, so a lamp can ride a ball), `surface` (a
Look's Surface chip with its preset numbers, then any changes), `particles` (a Particles thing from a ready look,
named), `unlit` (every layer of a Look at Lit 0, so words stay bright in a dark lit scene), `who`, `finish`. `Tone`
also has the drum voices Kick (a sine with a fast pitch drop), Hat (short noise) and Clap (noise and a triangle).
`Block` is a rule block's kind and settings.

### Gotchas found while building

- A rule's Change setting Multiply compounds per firing, but a Balls `radius` is clamped to its range (200), so
  "grow until it fills the ring" needs a ring that a 200 ball nearly fills (Grow every bounce uses a 320 ring).
- A ball in a ring under gravity hits about once a second when it starts slow; give it speed (600+) for a busy
  scene.
- Emitter rounds that fade before they land never hit: keep `fade_time` longer than the fall.
- The Hit counter Text look is 260 big for a number alone; with a word beside it use about 120 to 140.
- A single ball with no gravity bounces on a repeating path, so it keeps hitting the same few bricks: Breakable
  bricks uses three balls fanned out. Time a poster on a hit (a throwaway test printing the Hit events' times
  helps) when the idea is what a hit throws, as in Sparks on hit.
- **A Do block passes nothing on**: only When, If and Logic blocks hand a firing to the blocks after them. A rule that
  does several things hangs every Do off the same last block (`Lib::rule_doing`); a Do after a Do never runs.
- **`when.hit` between two balls fires for both sides** (A hits B and B hits A). Colour wars makes the turned ball
  the rule's own (That, the Who side: the other team's ball) and holds it with a per-ball cooldown, so a ball hit by two at once turns once
  and the team counts stay true.
- An emitter's round fired **at 0 s** is fired while the world is built, before the rules watch, so `when.fired`
  never sees it: give the emitter After 0.1 s (Colour wars).
- A team that changes per ball must be **ball tags** (`do.tag` That/Other): a thing's tag can't come off one ball.
- A melody step's notes must be sorted low to high, or the saved file drops the melody (`NoteSlot` checks it):
  `Lib::melody` sorts them.
- **Chords where balls meet**: a ball passing near a focus on its own also enters a zone there; All meet at once
  keeps the zones tiny (4 px) and turns the round 2.5° off the long axis, so only the real meetings play.
- **Random wars**: a coin-toss scene (Colour wars) is decided by the Scene settings' Seed; it is set so a team takes
  every ball before the end, and the payoff test catches a physics change that undoes that.
- **A look's colour is more than its Colour layer**: Neon tube's tube and glow and Glow orb's halo take the look's
  main colour, and a dark fill sits in the Colour layer. Setting only the Colour layer (or a ball's Body) after
  `look` leaves the glow in the look's own colour; `Lib::look_in` puts the look on again with Keep mine.
- Live health of a broken ball reads "-": Red vs Blue sets the loser's number to "0" with Set text.
- A Race leaderboard block is centred on its place; at size 30 under a title, (-190, -470) keeps it on screen.
- A real-letter Text look's balls are placed for its own words: use `Lib::text_saying` (`add_text_saying`) to give
  it other words, never change Words afterwards. Balls inside a word glows on its own; keep the scene's bloom 0.

- **Gravity × is clamped to -5..5 on load** (and a shape's thickness 0 opens as 0.5): the save round trip catches
  it. Club night and The simulation is the song double the scene's Gravity instead of using Gravity × 6 or 8.
- **Beat balls**: a ball with bounciness 1 under gravity keeps exact time if it never starts touching the floor (a
  ball starting on it gains energy). `middle/club_night.rs::on_the_beat` works out a start height and speed for a
  landing every so many seconds; the first beat is 0.125 s in so no ball starts on the floor.
- A **Rig**'s defaults are Tail 1 and Off level 0.1: a Strobe rig needs both at 0 or it's always on.
- **Flat stained glass**: a glass shape bends light into caustics; at Bends light 1 its shadow is flat and tinted.
  Pale glass gives pale shadows: the panes use See-through 0.9, Tints 1 and the paint at 0.9.
- A **Light beam** bends through glass only when At shapes isn't Stop; a prism's rainbow shows only at some turns
  (Prism rainbow rocks its spin with keyframes). Two parallel sides cancel the split, so a square diamond makes no
  fan: Diamond fire's is a pentagon. A single-colour laser fans into copies of its own colour (Mirror maze turns
  its laser white at the end).
- An emitter's marker shows in the picture: `Marker::None` hides it. A Fan of one ball aims straight down the middle;
  Splash glass fires three a round in Random order so they land in different places.
- **Chaos and timing**: a ball with no gravity in spinning rings is chaotic: a velocity off by 0.03 px/s moved
  Escape on the drop's middle escape from 10 s to 31 s. Give the exact value the tuning used (it builds the velocity
  from the same angle and speed), and don't hang a musical moment on a chaotic one: the drop is a `when.beat` rule
  waiting for the counter the escape sets, so it always lands on a bar, even on Windows' slightly different maths.
- **A `when.beat` rule logs a Rule event on every beat** even when its Ifs stop it, so a RuleFires payoff on it fires
  at the first beat; use a Counter payoff on what its Dos set. A `when.count` rule logs only when the count crosses.
- A Counter payoff on a counter that starts at 0 and falls to 0 is met at once: Mirror maze checks its last rule
  (`when.count` Falls to 0) with RuleFires instead.
- **A Huge swarm from a rule**: a Particles thing riding an emitter's balls (Riding its parent, Each ball, Steady)
  makes nothing until the emitter has a ball, so a rule's Fire a round on an emitter set to By a rule starts it
  (One to a million). It needs a graphics card on the thread (`bpe_gfx::swarm::use_for_now`) to draw.
- Text in a dark lit scene takes the light: `Lib::unlit` keeps it bright. Neon sign text lights the room itself.

## Checks

- `bpe-kit/tests/library.rs`: `ids_names_and_info_are_tidy`, `levels_so_far`, `every_scene_builds_tidily` (known
  kinds incl. rule blocks, no resolve problems, length, unique thing names, no default rule names, save/load round
  trip), `every_scene_plays_its_full_length_and_lands_its_payoff` (one run each: no rule problems or busy rules,
  nothing goes to infinity, something hits, the payoff on time; a Counter payoff replays the scene in 1/60 s steps
  for the first moment the counter reaches its amount, as the log keeps no counters), `every_scene_draws_a_picture` (saves a PNG to the temp dir when one is flat).
- `bpe-sound/tests/library_sound.rs`: `every_scene_with_sound_sounds` (its sound level matches, peak between 0.001
  and 1.0 over the first seconds). Lives in bpe-sound because bpe-kit can't depend on it.
- `bpe-kit/tests/full_size_look.rs`: the `library` group.
- `bpe-cli/tests/preview_clip.rs`: `preview_clip_draws_every_frame_and_stops`.

## The Scenes window (`bpe-studio/src/scenes_window/`)

- `mod.rs`: `ScenesWindow { open, filter, picked, hovered, pictures, sound, .. }`, `open(was_playing, sound_on)`,
  `show(ctx, kit, volume) -> Option<WindowOutcome>` (`Open(id)` or `Closed { was_playing }`). An `egui::Modal` 92% of
  the editor, at most 1320 x 820: header (count, Find a scene, close cross), filters (Level then Kind, counts in
  mono, a kind that is on turns off), the tile grid (poster or swatch gradient, level badge, name, kinds; click
  picks, double-click opens), and the preview column (clip at the sound's clock, Playing/Paused, Sound on/off, m:ss
  time, facts line, about, Uses, Good things to change first, Open scene, Cancel). Escape closes (in Find a scene
  it only leaves the box). `take_keys` runs at the start of the editor's frame and takes Delete, Backspace, F, Space
  and Escape away from the panels behind the window. A tile only takes over the preview after 0.3 s of hover; a
  picked scene the filters hide isn't previewed; the first scene's clip waits for the posters unless asked for.
- `filter.rs`: `Filter { level, kind, words }` with `shown`, `level_count`, `kind_count`, `is_clear`.
- `pictures.rs`: `Pictures`, one thread with its own `headless_device` (FinalFrame's pattern), started on the first
  ask and ended when the window closes. Posters by `bpe_cli::final_frame` at 1/6 (180 x 320), unbaked; clips by
  `bpe_cli::preview_clip` at 0.2 (216 x 384), `CLIP_FPS` 24, the last `CLIPS_KEPT` 3 kept while the window is open
  (closing drops them; posters stay). A thread that dies isn't started again (the window says there's no preview). A newer clip ask cuts the
  one being drawn (tickets drop its late frames); a cut clip starts over when asked again. `Pictures::none()` draws
  nothing (tests, no device) and `with_device` takes a test device.
- `sound.rs`: `PreviewSound`, its own `SoundJob` on the speakers (the editor's is paused), started once the clip is
  ready so picture and sound start together, looped over `preview_seconds`; the wall clock runs it with no speakers.
- Studio side: `Pending::OpenBuiltIn(id)` through `guard` (unsaved work asks first), `Studio::open_built_in(id)` →
  `replace_scene(scene, None)` and the status "Opened the built-in scene <name>. Save it to keep your changes.",
  `ToolbarClick::Scenes` after Recent, `open_scenes_window` (pauses, remembers playing), `scenes_window(ctx)` each
  frame, `take_toolbar_click(ctx)`. Shortcuts are off while the window is open.
- Tests in `scenes_window/tests.rs` (kittest, names "Scenes", "Level <name>", "Kind <name>", "Scene <name>", "Find
  a scene", "Clear the filters", "Open scene", "Cancel", "Close scenes"), plus each file's own.

## As built

- Part 1 (2026-11-10): the library, starters 1 to 8 (First bounce, Ball in a circle, Grow every bounce, Shrinking
  box, Every bounce makes two, Rainbow drop, Fountain, String art), preview clips in bpe-cli and the Scenes window.
- Part 2 (2026-11-10): starters 9 to 14 (Spinning triangle, Neon tube, Lit room, Breakable bricks, Bounce off words,
  Sparks on hit) and three new pieces for later scenes: Gap in a shape (`engine.md`), Merge balls (`rules.md`) and
  Plays Melody with Pitch follows size (`sound.md`). Plan `docs/superpowers/plans/built-in-scenes/part2.md`.
- Part 3 (2026-11-10): the in-between scenes 15 to 24 in `library/middle/` (Will it escape?, Escape layers, Melody per
  bounce, Polyrhythm rings, All meet at once, Hypnotic petals, Red vs Blue, Colour wars, Marble race, Merge and grow),
  every one `SoundLevel::Designed` (`Lib::tone` voices and a reverb), payoffs on five of them (two counters, two
  breaks, one rule), and the counter payoff check. No engine change. Plan `docs/superpowers/plans/built-in-scenes/part3.md`.
- Part 4 (2026-11-10): the in-between scenes 25 to 28 in `library/middle/` (Stained glass, Prism rainbow, Club night,
  Splash glass) and the first five spectacles in `library/spectacles/` (The simulation is the song, Escape on the
  drop, One to a million, Diamond fire, Mirror maze), all `SoundLevel::Designed`, payoffs on the five spectacles
  (one rule, four counters). New helpers `surface`, `light_in`, `particles`, `unlit` and the drum tones. No engine
  change. Plan `docs/superpowers/plans/built-in-scenes/part4.md`.
