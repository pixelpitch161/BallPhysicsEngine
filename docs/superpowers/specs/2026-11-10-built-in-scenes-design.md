# Built-in scenes: design

Date: 2026-11-10. Asked for by Aedan in the project chat ("extensive built-in scenes, ranging from simple starter
scenes to advanced and unique simulations/visual spectacles"). This is the explicit ask the old "no built-in scenes"
rule waited for (memory `no-builtin-scenes`), so it overrides that rule for this feature only: the editor still opens
to a blank project, and nothing is added to a scene unless the person opens a built-in one.

Mockups and the draft list: https://claude.ai/artifact/1aQeHBDp2bg868rCJQZc4M (copies in the project files,
`ballphysics/builtin-scenes/mockups/`). Aedan's answers, 2026-11-10:

| Question | Answer |
| --- | --- |
| What are they for? | **Both, by level.** Starters are clean templates to build on; spectacles are polished showpieces that can be posted as they are, and are still fully editable. |
| Today's features only? | **Mostly today's features, plus small missing pieces** added along the way. Big new ideas go on the ideas backlog. |
| Sound? | **By level.** Starters get one clean, simple sound that is easy to swap; spectacles get full sound design (scales, chords, effects, Follow sound tied to the picture). |
| How many? | **About 40**: 14 starters, 14 in-between, 12 spectacles, built in batches. |
| Where are they picked? | **A: the Scenes window** from the toolbar. Not B (a page in the left panel) and not C (a start strip on a blank project). |

## What success looks like

- A person who has never built a scene can open Ball Studio, press **Scenes**, watch a few play with sound, open one
  and export a video worth posting, in under a minute.
- The same person can open a starter, see from its tree and names how it works, and change it into their own video
  without reading anything else.
- Every built-in scene opens, plays and exports on every build: a test fails if one breaks.

## The rule that keeps "own settings" true

Opening a built-in scene makes a **new project** holding an ordinary scene, exactly as if it had been built by hand:
every thing owns its settings, nothing stays linked to the built-in copy, there's no "reset to the original", no
library entry in the saved file and no marker that it came from the library. The new project has no file yet
(`Studio.path` is `None`), so Save asks for a name, offering the scene's name. Ready-made looks
(`docs/superpowers/specs/2026-11-08-ready-made-looks-design.md`) follow the same rule.

## The Scenes window

A **Scenes** toolbar button, right after Recent (`app/panels.rs::toolbar_left`, `ToolbarClick::Scenes`), opens the
Scenes window: an `egui::Modal` over the editor, 92% of the window's width and height, at most 1320 x 820 points,
laid out as mockup A. Escape, the close button and Cancel close it.

1. **Header**: "Scenes", "40 scenes, from simple starters to full spectacles" (the real count), the **Find a scene**
   box (every typed word must be in the scene's name, kind words or description, ignoring case) and the close
   button.
2. **Left column, filters**: **Level**: All, Starters, In-between, Spectacles, each with its count. **Kind**: Bounce
   and drop, Escape, Music, Battles and races, Hypnotic, Light and glass, Liquid, smoke and sparks, Words, each with
   its count. One level and at most one kind are on at a time; clicking the kind that is on turns it off. Counts
   are for the current level and find words, so a 0 shows a filter will empty the grid.
3. **Middle, the grid**: a line saying what is shown ("Showing 5 scenes for "escape"."), then one tile per scene in
   list order: a 9:16 still picture of the scene at its **poster moment**, a level badge in its top left corner, the
   name and its kinds ("Escape · Music"). Hovering a tile shows that scene in the preview; clicking picks it (the
   picked tile has the 1.5 px `text` ring); double-clicking opens it. Nothing matching shows "No scenes match. Clear
   the filters" with a Clear button.
4. **Right column, the preview**: the hovered scene, else the picked one, else the first shown.
   - The scene **playing** at 9:16 (236 x 420 points), looping over its first `preview_seconds` (at most 15), with
     its **sound** while Sound is on. Until the clip is ready it shows the poster picture and "Getting the preview
     ready…" with a progress bar.
   - **Playing / Paused** and **Sound on / off** buttons and the time ("0:12 / 0:34", the second number the scene's
     full length).
   - The name (`title` style), "Level · Kinds · 34 s · sound designed" (or "· one simple sound" for a starter, "·
     no sound" if it has none), the description (one or two sentences), "Uses: …" (the systems in it, from its info)
     and **Good things to change first** (one sentence).
   - "Opens as a new project, yours to change. Your current project asks to be saved first." Then **Open scene**
     (the primary button) and **Cancel**.

**Opening**: Open scene (or double-click) goes through `Studio::guard(Pending::OpenBuiltIn(id), ctx)`, so unsaved
work asks to be saved first, exactly like Open and New. `perform` builds the scene fresh with the current registry,
calls `replace_scene(scene, None)` and sets the status to "Opened the built-in scene <name>. Save it to keep your
changes." The window closes. Opening is not an undo step (like opening a file).

**While the window is open** the project is paused (the editor's own playing state is remembered and given back on
Cancel), and the project's sound job is paused, so only the preview is heard.

### Pictures for tiles and the preview

Tiles and the preview are drawn the way export draws (`bpe_cli::FrameMaker`, the same lighting, particles, effects
and grade), so what the window shows is what the scene exports.

- **Posters**: one background thread with the editor's graphics device renders each scene's poster frame at 180 x
  320 pixels (video scale), the shown tiles first, then the rest, and hands each picture to the window as a texture.
  Posters are kept for the rest of the session (they don't change while the program runs). A tile whose poster isn't
  ready shows its two swatch colours (`SceneInfo::swatch`) as a vertical gradient.
- **Preview clips**: picking or hovering a scene for 0.3 s asks the same thread for its clip: 216 x 384 pixels at 30
  frames a second for `preview_seconds`, kept as textures for the last 4 scenes asked for (the oldest is dropped
  first). A newer ask goes first; a clip half made is kept and finished later.
- **Preview sound**: a second `SoundJob` (`sound_job.rs`) built for the previewed scene, on the same speakers, its
  clock driving which clip frame shows. It is stopped and dropped when the preview changes scene or the window
  closes.
- No graphics device (tests without one): tiles show swatches and the preview shows the poster area empty with "No
  preview on this computer".

## The scenes, as code in the program

The scenes are built into the program as code, not shipped as files. A scene built from code uses the current
settings and file format every time, so a format change can't break it, and the tests below build and play every one
on every pull request.

- New module **`bpe-kit/src/library/`** (always compiled; it is not the `test-scenes` feature): `mod.rs` holds
  `SceneInfo`, `Level`, `Kind`, `Entry { info: SceneInfo, build: fn(&Registry) -> Scene }` and `pub fn all() ->
  &'static [Entry]` (in list order) and `pub fn find(id: &str) -> Option<&'static Entry>`; `build.rs` holds small
  helpers that make the scene code short and readable (add a thing under a parent with a name and track colour, set a
  setting, add a layer, add a rule, add a Text with live words); then one file per scene, grouped in folders by level:
  `library/starters/first_bounce.rs`, `library/middle/will_it_escape.rs`, `library/spectacles/diamond_fire.rs`.
- `SceneInfo { id: &'static str, name, level: Level, kinds: &'static [Kind] (one or two), about: &'static str,
  uses: &'static [&'static str], change_first: &'static str, sound: SoundLevel (None / Simple / Designed),
  poster_seconds: f64, preview_seconds: f64, swatch: [Rgba; 2], payoff: Option<Payoff> }`. `id` is kebab case and
  never changes once shipped.
- A scene's `build` starts from `bpe_kit::new_studio_scene(reg)` (Scene settings and Camera, the defaults a new
  project has) and adds things with `starters::add_thing`, so it gets today's starting cards, then sets only what the
  scene needs. It never uses the test-only `classic_*` helpers. The scene's `name` is the info's `name`; its length
  (Scene settings) is the scene's full length.
- **Tidy scenes are part of the job**, above all for starters: every thing has a name that says what it is ("Ring",
  "Golden ball", "Title", "Escape counter"), related things sit in a Folder ("Rings"), track colours differ between
  neighbours, rules are named for what they do ("Ring breaks when the ball gets out"), and no card, layer or rule is
  there that does nothing.
- **Scenes use only what comes with the program**: built-in synth sounds (no sample files), the fonts in `fonts\`
  beside the exe, and no pictures or MIDI files on disk. A melody is written into the scene as notes.
- **Melodies are public domain** (Ode to Joy, Für Elise, Twinkle Twinkle, Greensleeves, Canon in D and the like), or
  written for the scene. No copyrighted songs.
- Spectacles are 25 to 40 seconds long and open with a hook title, keep a live number on screen and build to one
  payoff (an escape, a winner, a reveal), because that is what works on TikTok and Shorts (research in the ideas
  backlog, https://claude.ai/artifact/3RUuJ8kgnYFeY19CXvce4K).
- **Payoffs that must land on time** (an escape on the drop, a winner before the end) are stated in
  `SceneInfo.payoff` (`Payoff { what: PayoffEvent, at_seconds, within_seconds }`, the event being a rule firing,
  a thing breaking or a counter reaching a number), and a test checks the payoff happens at that moment, so a
  physics change that moves it fails the build and the scene gets retuned in the same pull request.

## The 40 scenes

The first draft, agreed 2026-11-10. Each build part may tune details (sizes, colours, notes) but keeps the idea. A
part that finds an idea doesn't work says so in its pull request and swaps in a scene of the same level and kind.

### Starters (14): clean templates, one simple sound

| # | id | Name | Kind | The idea |
| --- | --- | --- | --- | --- |
| 1 | first-bounce | First bounce | Bounce and drop | One ball, one floor, a soft pluck on every hit. The simplest scene to pull apart. |
| 2 | ball-in-a-circle | Ball in a circle | Bounce and drop | A ball in a ring; each hit climbs a scale. |
| 3 | grow-every-bounce | Grow every bounce | Bounce and drop | The ball grows on every hit until it fills the ring, with a size counter. |
| 4 | shrinking-box | Shrinking box | Bounce and drop | The square shrinks a little on each bounce, so the hits come faster and faster. |
| 5 | every-bounce-makes-two | Every bounce makes two | Bounce and drop | Each hit splits the ball in two, with a live ball count. |
| 6 | rainbow-drop | Rainbow drop | Bounce and drop | A grid of balls wearing one rainbow across the group pours into a bowl. |
| 7 | fountain | Fountain | Bounce and drop | An emitter fires fans of balls that bounce and fade away. |
| 8 | string-art | String art | Hypnotic | Two balls in a circle leave lines that stay, building a pattern. |
| 9 | spinning-triangle | Spinning triangle | Hypnotic | A ball inside a slowly spinning triangle, ricocheting off the corners. |
| 10 | neon-tube | Neon tube | Light and glass | A neon ball in a glowing hexagon, in a dark room with bloom. |
| 11 | lit-room | Lit room | Light and glass | One lamp, a few balls and real soft shadows on the walls. |
| 12 | breakable-bricks | Breakable bricks | Bounce and drop | A wall of bricks with health; each shatters when it runs out. |
| 13 | bounce-off-words | Bounce off words | Words | Balls bounce around inside the letters of a big word. |
| 14 | sparks-on-hit | Sparks on hit | Liquid, smoke and sparks | Every bounce throws sparks and a puff of smoke. |

### In-between (14): two or three systems together, fuller sound

| # | id | Name | Kind | The idea | New piece |
| --- | --- | --- | --- | --- | --- |
| 15 | will-it-escape | Will it escape? | Escape | A spinning ring with a gap, and an escape counter. | Gap |
| 16 | escape-layers | Escape layers | Escape | Three rings with gaps; each shatters with a zoom punch and a step up the scale when the ball gets out. | Gap |
| 17 | melody-per-bounce | Melody per bounce | Music | Every bounce plays the next note of a public-domain tune. "Guess the song". | Melody |
| 18 | polyrhythm-rings | Polyrhythm rings | Music | Three rings spin at 3:4:5, each ball plays its own note, and the rhythm lines up every few bars. | |
| 19 | all-meet-at-once | All meet at once | Hypnotic | An emitter at one focus of an ellipse; every ball lands on the other focus at the same moment, in a chord. | |
| 20 | hypnotic-petals | Hypnotic petals | Hypnotic | Hundreds of balls released into a flower outline, with a slowly shifting gradient. | |
| 21 | red-vs-blue | Red vs Blue | Battles and races | Two balls with health bars; harder hits do more damage; a winner title at the end. | |
| 22 | colour-wars | Colour wars | Battles and races | Two teams; a ball that gets hit joins the hitter's team, with live team counts. | |
| 23 | marble-race | Marble race | Battles and races | Balls race down pegs and spinners to a finish line, with live standings and the camera on the leader. | |
| 24 | merge-and-grow | Merge and grow | Bounce and drop | "Every pop makes a bigger pop": same-size balls merge, and the sound drops in pitch as they grow. | Merge |
| 25 | stained-glass | Stained glass | Light and glass | Balls bounce between coloured glass panes that throw coloured shadows. | |
| 26 | prism-rainbow | Prism rainbow | Light and glass | A beam of light through a bouncing glass prism spreads into a rainbow. | |
| 27 | club-night | Club night | Music | A light rig chases and strobes on the beat while balls bounce to a four-on-the-floor. | |
| 28 | splash-glass | Splash glass | Liquid, smoke and sparks | Balls drop into a glass of liquid and splash, with lamp light bent through the water. | |

### Spectacles (12): post-ready, full sound design

| # | id | Name | Kind | The idea | New piece |
| --- | --- | --- | --- | --- | --- |
| 29 | the-simulation-is-the-song | The simulation is the song | Music | Balls arrive one by one (kick, hat, bass, chords), and random plinks turn into a finished beat. | |
| 30 | escape-on-the-drop | Escape on the drop | Escape · Music | Nested rings; the last escape lands on the drop, with slow motion, a flash and a change of light. | Gap |
| 31 | one-to-a-million | One to a million | Escape · Liquid, smoke and sparks | Each escape doubles the balls until a swarm of millions pours out. | Gap |
| 32 | diamond-fire | Diamond fire | Light and glass | A spinning diamond in the dark throws rainbow fire as balls hit it. | |
| 33 | mirror-maze | Mirror maze | Light and glass | Mirrors and light pipes steer a beam; breaking a mirror sends the light somewhere new. | |
| 34 | fire-tornado | Fire tornado | Liquid, smoke and sparks | Swirling air lifts flames and embers that pulse with the bass. | |
| 35 | word-from-chaos | Word from chaos | Words | Thousands of specks swirl, snap into a word on the drop, and scatter again. | |
| 36 | liquid-escape | Liquid escape | Escape · Liquid, smoke and sparks | A ring slowly fills with liquid until it spills out through the gap. | Gap |
| 37 | letter-smash | Letter smash | Words | A big word is the wall; balls break it letter by letter while the letters glitch and melt. | |
| 38 | stem-battle | Stem battle | Battles and races · Music | Four balls each carry one part of a song; when one is knocked out its part goes silent (a rule changes its Mixer bus, which rules can already do since W13). | |
| 39 | gravity-flips-every-bar | Gravity flips every bar | Music | Gravity, colours and lights change on every bar of the beat. | |
| 40 | golden-hour | Golden hour | Light and glass | A slow, filmic scene: lit balls in warm light, full colour grading, soft piano. | |

## Three small new pieces

Each is a normal engine feature, available to everyone's own scenes, with its own notes in the area's file.

### Gap in a shape (part 2)

A Shape's body settings gain a **Gap** group: **Gaps** (0 to 8; 0, the default, is today's closed shape), **Gap
size** (a share of the way round, 0.01 to 0.5) and **Gap at** (where the first gap's middle sits, as a share of the
way round from the shape's first corner clockwise, the same measure Place uses; more gaps are spread evenly). Gaps
turn and shrink with the shape's Spin and Shrink.

- **Physics**: a ball meets no wall where the outline is open; the wall's ends are round caps (a ball can hit an end
  of the opening). Every outline form takes gaps (Ring, Box, Ellipse, Path), by measuring the way round along the
  outline. A shape with no gaps steps exactly as before (goldens unchanged).
- **Drawing**: the outline is cut at the gaps, with the same round ends; glow, outline and gradient layers follow the
  cut.
- **Rules**: Enters / Leaves on a shape already watches balls going in and out, so "When a ball leaves the ring" is
  an escape; the plan checks that a ball leaving through a gap counts (and fixes it if not).
- Keyframes and Follow sound work on all three settings.

### Merge on touch (part 2)

A new rule action **Merge** (`do.merge`, Do section): when two balls touch (fired by When hit with a ball as Other),
they become one ball: at their area-weighted middle, with their areas added (two equal balls make one 1.41 times as
wide), their momentum kept, and That's look, number and tags. The Other ball is removed (a Removed event, so its
trail runs out). Its settings: **Only same size** (on by default, within **Size match** percent, default 5%) and
**Grow** (Add areas / Keep That's size × an amount). Merging counts against the 4096 actions a step limit like any
action. If a ball's hit sound can't yet follow its size, part 2 also adds **Pitch follows size** (bigger is lower,
by an amount in semitones per doubling) to the Balls/Emitter sound.

### Melody list (part 2)

The Notes settings of a sound gain **Plays: Melody**: a list of notes (each a pitch, and optionally a chord on top),
up to 1024 long, where each hit plays the next one, then **At the end** wraps or holds the last. It is written into
the scene (no file). The Sound tab shows the list as one row of note names with Add / Remove and a piano-roll strip,
and **Import MIDI…** gains **As a melody** (the notes in time order, a chord counting as one step). This lifts the
16-slot limit for melodies only; slots and chords work as before.

## Checks

- **`bpe-kit/tests/library.rs`**, for every entry in `library::all()`:
  - ids are unique, kebab case and match the file name; names are unique; every scene has a description, uses,
    change-first line and one or two kinds; the level counts are 14 / 14 / 12 once all parts are in (each part checks
    the count it has so far).
  - the scene builds with no unknown kinds or problems, its Scene settings length equals the info's length, and
    `save` then `load` gives the same scene;
  - its physics runs its full length with no rule problems and no "too busy" rules, every ball stays finite, and at
    least one hit happens;
  - frames drawn (through `test_device()`) at 3 moments are not one flat colour;
  - a scene with sound renders 4 seconds of sound that isn't silent and peaks under 0 dBFS;
  - a payoff, where there is one, happens within its time.
- **Full-size look check**: `full_size_look.rs` gains a `library` set (`LOOK_ONLY=library` or a scene id) that
  renders every built-in scene at its poster moment and two other moments at 1080 x 1920. Each build part looks at its
  new scenes' full-size pictures and 1:1 crops before the pull request.
- **Editor tests** (kittest, `bpe-studio/src/scenes_window/tests.rs`): the Scenes button opens the window; level,
  kind and find filters narrow the tiles and show the counts; the preview shows the picked scene's info; Open scene
  with unsaved work asks to save first, then replaces the scene (path `None`, the scene's name, the right things);
  Cancel and Escape close it and leave the project as it was, playing again if it was.
- No physics goldens per scene (they would move on every physics change); the payoff checks and full-length runs
  catch what matters.

## Build parts

Five parts, each its own pull request, merged by the thread when checks and the one Opus review pass. The exe is
built and installed only when Aedan asks in the project chat.

1. **The Scenes window and the first 8 starters** (1 to 8): `library/` with `SceneInfo`, the scene helpers, the
   window, posters, preview clips and sound, opening, the library tests and look-check set.
2. **The other 6 starters (9 to 14) and the three new pieces**: Gap in a shape, Merge on touch, Melody list.
3. **In-between 15 to 24.**
4. **In-between 25 to 28 and spectacles 29 to 33.**
5. **Spectacles 34 to 40.**

Each part saves a handover note for the next (`/mnt/project-files/ballphysics/builtin-scenes/partN-handover.md`) and
adds its "as built" summary to `docs/notes/library.md` (a new area file, with one line in `CLAUDE.md`'s area index).

## Not doing

- No start strip on a blank project and no Scenes page in the left panel (Aedan picked A only).
- No scene files shipped beside the exe, no "save as my own built-in scene" and no online sharing.
- No user templates with exposed settings forms (on the ideas backlog as "Templates from any scene").
- No sample files, pictures or copyrighted melodies inside built-in scenes.
- No big new engine features for a scene (soft bodies, 3D, split screen); ideas that need one go on the backlog.
