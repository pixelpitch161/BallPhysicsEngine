# Modular Ball Studio: design

Date: 2026-10-29. Status: written for review.

## 1. Why

Ball Studio has grown feature by feature. Each feature added its own tab, section or checkbox, so
related things sit in unrelated places: liquid and swarm are checkboxes at the bottom of the Balls
tab, Look, Bounces and Sound are global tabs with per-ball sections added later, effects reach
single objects only through an "Applies to" combo, and Rules and Shader graphs exist only once
per scene.

The goal is a rewrite around one idea: **a scene is a tree of things, and each thing carries its
own modules** (look, motion, bounce, sound, particles, rules, shader). Global settings still
exist, as the Scene defaults at the root of the tree, but deep, per-thing customisation is the
focus. The editor opens simple and lets you dig as deep as you like.

## 2. Agreed decisions

These were settled with Aedan during brainstorming and are not open in this spec.

| Topic | Decision |
|---|---|
| Old scenes and gallery presets | Clean break. Old `.ron` scenes do not open. Gallery presets are not needed now. |
| Video output | Unchanged: vertical mp4 with audio, same sizes and quality settings. |
| New scene | Starts very simple. |
| Approach | Rewrite the engine around a modular core. Proven pieces (collision maths, ball shaders, instruments, node compilers) are ported in, not reused in place. |
| Sharing | Modules live in a scene library. Things link to them. A thing can override single settings or make the module its own. |
| Interactions | Set up on each thing. Rules and bounce settings target other things by name or by tag. |
| Depth | Every module has three depths: Simple, More, Nodes. Nodes can expose sliders back to Simple and More. |
| Hierarchy | Folders plus attaching: a thing can ride on another and inherits its modules. |
| 3D | 2D first. The data model leaves room for 3D, which is a later stage. |
| Layout | Scene tree on the left, inspector on the right, node graph slides up under the preview, timeline along the bottom ([mockup](https://claude.ai/artifact/Bj8fLuNzBCnsGdK9GwmHoe), option A). |
| Delivery | The new editor ships as a second program, `ball-studio-next.exe`, until it replaces `ball-studio.exe` at the last stage. |

## 3. Concepts

### 3.1 Things

A **thing** is anything you add to a scene. Every thing has:

- `id`: a `ThingId(u32)`, stable for the life of the scene and never reused.
- `name`, shown in the tree and used to refer to it in rules.
- `kind`: which plugin it is (balls, arena, emitter...).
- `tags`: free-form words such as `drums` or `hitters`.
- `parent` and `attach` (see 3.4).
- `transform`: position and rotation relative to its parent.
- `enabled`.
- `params`: the kind's own settings (a block of balls has columns, rows, spacing).
- `slots`: one link per module slot the kind offers (see 3.2).

Kinds in the first release (2D):

| Kind | What it is | Default slots |
|---|---|---|
| Scene defaults | The root. Video size, fps and length, background, gravity, steps per second, the song, chords, beat sync and MIDI, picture-wide effects, and the default module for every slot. Exactly one, cannot be removed. | all |
| Arena | Walls: circle, box, polygon or custom outline. Can spin and shrink. More than one allowed. | Look, Bounce, Sound, Shader |
| Balls | A set of balls placed as a block, a single ball or a ring. | Look, Trail, Motion, Bounce, Sound |
| Emitter | Releases balls, liquid or swarm points into a **target** thing, over time or on an event. What it releases uses the target's modules. | Rules |
| Liquid | A body of liquid. The engine uses the GPU solver above 4,000 particles and the CPU solver below (the limit is a More setting). | Look, Motion, Bounce, Shader |
| Swarm | A cloud of points that flock and follow. GPU only. | Look, Motion |
| Obstacle | Peg, bar, peg grid or shape. Can move on a path or spin. | Look, Bounce, Sound, Shader |
| Soft body | Jelly, rope or cloth. | Look, Motion, Bounce, Sound |
| Field | Invisible zone. Pushes (wind, gravity well, magnet, vortex) and fires "entered" and "left" events. | Rules |
| Text | A caption. | Look, Shader |
| Folder | Groups things. Can hold any slot, which its children inherit. | none until added |

Every kind can also have Particles and Rules added. "Default slots" are the slots a new thing
shows; the inspector's "+ Add slot" adds any other slot the kind supports.

### 3.2 Modules and slots

A **slot** is a role: Look, Trail, Motion, Bounce, Sound, Particles, Rules, Shader. A **module** is
a named set of settings for one slot, kept in the scene's **library**. A thing's slot holds one of:

```rust
pub enum SlotLink {
    /// Use whatever the parent (or Scene defaults) uses for this slot.
    Inherit,
    /// Use this library module, with these settings changed for this thing only.
    Linked { module: ModuleId, overrides: Params },
    /// This slot does nothing on this thing (no sound, no trail).
    Off,
}
```

"Make unique" duplicates the module into the library under a new name, links only this thing to
it, and clears the overrides. "Push to module" writes this thing's overrides into the shared
module, so every thing using it changes. "Reset" drops an override.

A module records which **module kind** it is (for example the Look slot has `look.classic` and
`look.styled`, ported from today's two ball looks) plus its params and, once edited as nodes, its
own graph (see 3.5).

### 3.3 Where a setting's value comes from

For a thing T, slot S, setting K, the first of these that has a value wins:

1. T's override for K in slot S.
2. The module T links to in slot S.
3. If T's slot S is `Inherit`: repeat from step 1 with T's parent, up to Scene defaults.
4. The default in the setting's schema.

If a slot is `Off` anywhere on the way up, it stays off. A child's `Linked` always beats its
parent. Resolution happens once per edit in a **resolve** pass, not per frame (see 5.1).

### 3.4 Folders and attaching

Every thing except Scene defaults has a parent: Scene defaults, a folder, or another thing.

```rust
pub enum Attach {
    /// Grouped under the parent: inherits its modules, ignores its motion.
    Group,
    /// Rides on the parent: inherits its modules and moves with it.
    Ride { offset: Vec2, turn_with_parent: bool, item: Option<u32> },
}
```

- Things with a scripted transform (arena, obstacle, emitter, field, text) follow a parent they
  ride on every step.
- Free bodies (balls, liquid, soft bodies) take the parent's transform only when they spawn, and
  add the parent's velocity when "carry speed" is on. After that they move freely.
- `item` picks one item of the parent when the parent is a set, such as one ball of a Balls thing,
  so a small arena can ride inside a single ball. One copy per item of a set is out of scope.
- The editor refuses to attach a thing to itself or to one of its own descendants.

### 3.5 Three depths: Simple, More, Nodes

Each module kind is defined by a **template graph**: blocks wired together, whose exposed inputs
are the module's settings. Each exposed input is marked Simple or More.

- **Simple** shows the inputs marked Simple, usually three or four.
- **More** shows every exposed input, grouped.
- **Nodes** opens the graph in the node panel under the preview.

As long as a module's graph is unedited, it runs on a hand-written **native fast path** for its
module kind, so stock modules cost nothing extra. Editing the graph marks the module
**custom**: it keeps its exposed inputs as sliders, can expose new ones, and runs through the
compiled graph. "Revert to template" goes back.

Graphs compile into one of three targets, decided by the slot:

| Target | Slots | Runs |
|---|---|---|
| GPU (WGSL) | Look, Trail, Shader, particle looks | per pixel, compiled to WGSL like today's shader graphs |
| CPU per event | Bounce response, Sound, Particles bursts, Rules | once per event, in the graph VM |
| CPU per step | Motion (custom forces) | per item per step, in the graph VM |

The GPU compiler is ported from `bpe-render/src/effects/graph`. The CPU compiler and VM grow out
of today's rule compiler in `bpe-sim/src/rules.rs` into a crate of their own (see 6).

### 3.6 Tags and targets

Anywhere a setting points at other things (rules, bounce overrides, emitter targets, sound
filters), it holds a `Target`:

```rust
pub enum Target { Me, Thing(ThingId), Tag(String), Kind(KindId), Anything }
```

Renaming a thing doesn't break targets because they store the id. Removing a thing turns its
targets into a red badge (see 5.6).

### 3.7 Settings schema

Every setting is described once, in Rust, with a derive macro on a plain struct:

```rust
#[derive(Settings)]
pub struct ClassicLook {
    #[setting(label = "Colour", simple, keyframe, random)]
    pub color: Color,
    #[setting(label = "Glow", range = 0.0..=2.0, simple, keyframe, random, pulse)]
    pub glow: f32,
    #[setting(label = "Size", unit = "px", range = 0.5..=200.0, simple, keyframe, random)]
    pub radius: f32,
    // ...
}
```

The macro produces a `&'static [SettingDef]` (key, label, help text, value type, default, range,
unit, depth, group, flags for keyframe, randomize, lock and pulse) and a
`from_params(&Params) -> Self` that fills the struct from resolved values.

The scene stores **sparse** `Params` maps (only values that differ from the default), so overrides
are naturally small and saved files stay short. The runtime only ever sees the filled-in typed
structs.

Panels, the timeline, randomize, locks, pulse with music, copy and paste, search and saving all
read `SettingDef`s, so a new setting gets all of them with no extra code.

### 3.8 Addresses

A setting is named by its **address**:

```rust
pub struct Address { pub owner: Owner, pub slot: Option<SlotId>, pub key: SettingKey }
pub enum Owner { Thing(ThingId), Module(ModuleId) }
```

`Owner::Thing` with a slot means "this thing's override in that slot". Keyframe tracks, pulse
modulators, randomize locks and rule "set setting" actions all store addresses, so they keep
pointing at the right setting when things are reordered or renamed.

### 3.9 Plugins

Every thing kind, module kind and node block is a plugin. In this release a plugin is a Rust
module compiled into the program and listed in one registry; loading plugins from separate
files is out of scope. A plugin lives in one folder of `bpe-kit` (see 6) and has up to four
parts, each implementing a trait from a lower crate:

| Part | Trait (crate) | Job |
|---|---|---|
| Data | `ThingKind` / `ModuleKind` (`bpe-core`) | id, name, icon, help, settings struct, slots offered, defaults, template graph |
| Simulation | `ThingSim` / `ModuleSim` (`bpe-runtime`) | build its tables, add work to stages, emit events |
| Drawing | `ThingDraw` / `ModuleDraw` (`bpe-gfx`) | GPU resources and draw calls, WGSL for its graph blocks |
| Sound | `ModuleMix` (`bpe-mix`) | turn events into notes |

A **contract test** runs over every registered plugin automatically (see 8).

## 4. The editor

### 4.1 Layout

```
┌───────────────────────────── toolbar ──────────────────────────────┐
│ New Open Save Undo Redo   [scene name]   Library Randomize Export  │
├──────────┬────────────────────────────────────────┬────────────────┤
│ Scene    │                                        │ Inspector      │
│ tree     │            preview (9:16)              │  header: name, │
│          │                                        │  kind, tags    │
│ + Add    ├────────────────────────────────────────┤  slot cards    │
│  thing   │  node panel (hidden until opened)      │  + Add slot    │
├──────────┴────────────────────────────────────────┴────────────────┤
│ timeline: play, time, lanes for the selected thing                 │
└────────────────────────────────────────────────────────────────────┘
```

Every panel can collapse. The node panel is hidden until a slot's "Nodes" button opens it, and its
divider can be dragged up to fill the window.

### 4.2 Scene tree

- Scene defaults always sits at the top.
- Rows show icon, name and tags. Riding things are indented under their parent with a ↳ mark.
  Folders have a ▸ mark.
- Drag to reorder and to re-parent. Hold Alt while dropping to ride instead of group.
- Eye toggle (enabled), right-click menu (rename, duplicate, delete, make folder from selection,
  select things using the same module).
- A filter box at the top filters by name, kind or tag.
- "+ Add thing" opens a menu of kinds with an icon and one-line description each.
- Ctrl and Shift click select several things. Edits in the inspector then apply to all selected
  things that have that setting, as ball cards do today.

### 4.3 Inspector

- **Header:** name, kind (with its own Simple and More settings, like columns and rows for a block of balls),
  tags, and the parent and attach settings.
- **Slot cards**, one per slot in use. Each card's top row holds the slot name, the module picker
  (module name, "shared ×N" when used by several things, "from Folder / from Scene defaults" when
  inherited), and the Simple / More / Nodes switch.
- Overridden settings show an orange dot. Their right-click menu adds "Reset to module" and
  "Push to module", next to the existing keyframe, pulse and lock items.
- "+ Add slot" lists the slots the kind supports that aren't shown yet.
- Scene defaults uses the same inspector, with its own settings (video, background, gravity,
  steps per second, music) followed by its default slot cards and its picture effects list.

### 4.4 Library

A window or side tab listing modules grouped by slot. Each module shows how many things use it.
It supports rename, duplicate, delete (refused while in use, with "select users"), and drag onto
a thing in the tree to link it. Saved presets ("My presets") become modules saved outside the
scene, and they show here under a separate heading.

### 4.5 Preview

The preview works as it does today: drag things to move them, drag arrows to set velocity, drag
ring edges to resize. Clicking a thing in the preview selects it in the tree. Invisible things
(fields, folders) draw a dashed outline while selected.

### 4.6 Node panel

The node panel is the existing node editor (`bpe-app/src/node_editor`), ported to show any
module's graph. Its title shows the path, for example "Red balls › Rules". It keeps sticky notes,
frames and live block previews. Blocks offered depend on the slot's target (GPU, CPU per event,
CPU per step).

### 4.7 Timeline

The timeline works as today (play, scrub, keyframes, eases, own curves, hit and beat lanes), but
lanes follow the selection: the selected things' keyframed settings, then Scene defaults'.

### 4.8 What happens to today's tabs and workspaces

| Today | New home |
|---|---|
| Balls tab | Balls things |
| Arena tab | Arena things, plus Scene defaults for gravity and steps per second |
| Bounces tab | Bounce slot |
| Look tab | Look and Trail slots, Scene defaults for background |
| Effects tab | Shader slot on things, picture effects on Scene defaults |
| Sound tab | Sound slot, Scene defaults for song, chords and beat sync |
| Rules tab and Rules workspace | Rules slot on any thing, and the node panel |
| Text tab | Text things |
| Shaders workspace | Node panel |
| Batch workspace, Explorer, Randomize | Kept as windows, reading the new scene |
| Command palette | Kept. Searches things, modules and settings. |

### 4.9 A new scene

A new scene contains Scene defaults, a circle Arena and one Balls thing holding a single ball.
The ball uses a plain Look module, a default Bounce module and a default Sound module, all in
the library. Nothing else is shown until you add it.

## 5. The engine

### 5.1 From edit to frame

```
Scene (sparse, saved)  --resolve-->  Resolved scene (typed, per thing)  --build-->  World
                                                                                  |
                         event log  <--  step loop (240 Hz, staged)  <------------+
                             |
               +-------------+-------------+
               v             v             v
             gfx           mix          editor lanes
```

- **Resolve** runs after each edit. It walks the tree, applies 3.3, fills each plugin's typed
  settings structs, compiles changed graphs (cached by content hash, so shared modules compile
  once), and checks targets.
- **Build** turns the resolved scene into a `World` at time 0.
- **Step** advances the world. The editor keeps checkpoints for seeking, as `Playback` does
  today.

An edit that only changes how things look (the resolve pass knows which settings are visual from
their slot) rebuilds drawing data without restarting the simulation, as colour edits do today.

### 5.2 World storage

Each thing owns packed tables for its items, as columns (`positions: Vec<Vec2>`,
`velocities: Vec<Vec2>`, `radii: Vec<f32>`...). Items keep a stable index within their thing
(new items append; removed items are tombstoned and compacted only between steps in a fixed order).
Iteration is always in `ThingId` order, then item order, so results don't depend on hash order
or thread timing.

### 5.3 Step stages

Each step runs these stages in order. Plugins add work to stages. Within a stage, work runs in
`ThingId` order.

1. **Spawn:** emitters, splits and rule spawns from the previous step.
2. **Scripted motion:** arenas, obstacles, fields, riders follow their paths and parents.
3. **Forces:** gravity (Scene defaults times the thing's gravity scale), drag, fields, custom
   Motion graphs.
4. **Integrate.**
5. **Collide:** one shared broadphase over every colliding thing, then contact solving (ported
   from `bpe-sim` arena, contacts, broadphase and bodies).
6. **Respond:** each contact asks the Bounce modules involved what happens (5.4).
7. **Events:** hits, spawns, removals, zone entries and leaves, beats and custom events are
   appended to the event log.
8. **Rules:** Rules modules run on this step's events and timers. Their actions (set a setting,
   spawn, remove, fire an event) are queued and all apply at the start of the next step, in the
   order they were queued.

GPU liquid and swarm keep today's approach: they step at 120 Hz from a private copy of the world
with fixed-point sums, and they never feed back into the CPU physics.

### 5.4 Who decides a bounce

- A moving thing hitting a fixed one (arena, obstacle): the mover's Bounce module decides,
  using its "when I hit [target]" overrides when one matches. If the fixed thing's Bounce module
  has "takes over" on, the fixed thing decides instead (a trampoline that's bouncy for
  everyone).
- Two moving things: bounciness and friction are the average of both sides. Each side applies its
  own effects (grow, recolour, speed gain, split) to itself.

### 5.5 Events

```rust
pub struct Event {
    pub step: u64,
    pub kind: EventKind, // Hit, Spawn, Remove, Enter, Leave, Beat, Note, Custom(name)
    pub a: Party,        // thing id and item index
    pub b: Option<Party>,
    pub point: Vec2,
    pub normal: Vec2,
    pub speed: f32,
}
```

Sound, particles, drawing (flashes) and the timeline read the log after the step. They never write
back, so a scene always produces the same video. Physics settings may pulse only with the song,
never with the bounce sounds, as today.

### 5.6 Errors

| Problem | What you see | What happens |
|---|---|---|
| A graph fails to compile | Red badge on the slot card and the tree row. Hover shows the compiler message. | The thing falls back to its parent's module for that slot. The preview keeps playing. |
| A module or target thing was removed | Red badge naming what's missing | The link falls back to Inherit. The target matches nothing. |
| Attach would make a loop | Drop is refused, with a short message | Nothing changes |
| A file from the old editor or a newer version | Message: "This scene was made with the old Ball Studio" or "...with a newer version" | Nothing opens |
| A setting out of its range in a file | Clamped on load | Value is kept within range |

## 6. Crates

New crates are added next to the old ones. The old crates stay untouched until the final
stage, then they are removed.

| Crate | New or ported | Holds |
|---|---|---|
| `bpe-core` | new | ids, things, modules, library, `Params` and values, schema types, addresses, resolution, registry traits for data, save and load (RON with `format: 2`), targets |
| `bpe-settings-derive` | new | the `#[derive(Settings)]` macro |
| `bpe-graph` | extended | graph data. Adds typed ports, exposed inputs with depth, template id and custom flag |
| `bpe-vm` | new, from `bpe-sim/src/rules.rs` | CPU graph compiler and VM for per-event and per-step graphs |
| `bpe-runtime` | new, ports `bpe-sim` | world, tables, stages, collision, soft bodies, CPU liquid, event log, playback and checkpoints |
| `bpe-gfx` | new, ports `bpe-render` | renderer, WGSL graph compiler, GPU liquid and swarm, particles, text, bloom, picture effects |
| `bpe-mix` | new, ports `bpe-audio` | mixer reading the event log. Instruments are moved into a module both old and new mixers use, until the old one is removed |
| `bpe-music` | kept | song decoding, beats, MIDI |
| `bpe-kit` | new | every built-in plugin, one folder each (`things/balls/{mod,sim,draw}.rs`, `modules/sound/{mod,mix}.rs`...) |
| `bpe-studio` | new, ports `bpe-app` | the editor, `ball-studio-next` |
| `bpe-cli` | new | `bpe-render-next` command line export. The ffmpeg pipe and loudness code move out of `bpe-export` into a shared module |

Dependencies point one way: `core` ← `vm`, `runtime`, `gfx`, `mix` ← `kit` ← `studio`, `cli`.

## 7. Stages

Each stage ends with a Windows build of `ball-studio-next.exe` that Aedan can try, and nothing in
the old editor changes. Each stage gets its own implementation plan.

1. **Core:** `bpe-core`, `bpe-settings-derive`, graph extensions. Tests only.
2. **First video:** runtime with Scene defaults, Arena and Balls, the Look, Motion, Bounce and
   Sound modules (Simple and More), renderer and mixer for those, the editor shell (tree,
   inspector, preview, toolbar, undo, save, open), and mp4 export.
3. **Building blocks:** Obstacles, folders, attaching, tags and targets, the Library, overrides,
   multi-select.
4. **Going deep:** `bpe-vm`, the Rules slot, the node panel, and Nodes for every module (template
   graphs and custom modules).
5. **Visuals:** Shader slot, picture effects, Particles and Trail slots.
6. **Flowing things:** Emitter, Liquid (CPU and GPU), Swarm, Field.
7. **The rest:** soft bodies, Text, timeline and keyframes, pulse with music, song, beat sync, MIDI.
8. **Finish:** randomize and locks, explorer and scorer, batch, My presets as library modules, and
   a few starter scenes. `ball-studio-next` becomes `ball-studio`, and the old crates are removed.
9. **Later, separate spec:** 3D.

## 8. Testing

- **Golden event logs:** each stage adds five test scenes, each stepped for 20 seconds, and the
  hash of their event log is checked, as `bpe-sim` does today. Running a scene twice, or seeking and
  playing back, must give the same hash.
- **Plugin contract test,** run over every registered plugin: its defaults resolve; its params
  save and load unchanged; every setting's default lies in its range; every keyframe-flagged setting
  can be keyed and read back; randomize stays within ranges; a template graph run through the
  compiled path matches the native fast path (event log hash for CPU targets, pixels for GPU
  targets).
- **Resolution tests:** override, linked, inherited, off, folder and ride cases, and missing
  modules.
- **Picture tests** on the software GPU, saving PNGs when `BPE_SAVE_RENDERS` is set, as today.
- **Editor tests** with `egui_kittest` for tree drag, add thing, slot switching, overrides, and
  screenshots under Xvfb.
- **Export check:** every stage's build exports a short clip from its stage scenes with
  `bpe-render-next`, and the clip's length, size and audio are checked.
- **Speed check:** a 10,000-ball scene must step and draw at least as fast in the new engine as
  the same setup in the old one, measured with the existing profile bench.

## 9. Out of scope

- Opening old scenes, and gallery presets (a few starter scenes come in stage 8).
- 3D (stage 9, separate spec).
- Plugins loaded from separate files.
- One copy of an attached thing per item of a set.
- Changes to the old editor.
