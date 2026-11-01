# Scene settings and per-thing starter modules

Date: 2026-10-31. Requested by Aedan. Path: architectural (spec, then plan).

## Why

Scene defaults is the root of every scene. It holds two unrelated things: settings for the
whole video, and four shared modules (Plain look, Default motion, Default bounce, Default
sound) that every thing falls back to. Since any module can already be shared by picking it in
a slot, those fallbacks add little. They also cost a step, because an inherited card is
read-only until "Change here". On top of that, one generic module doesn't fit every kind:
walls read only Colour and Glow from a look, and liquids and swarms already skip the fallback
by getting their own starters.

## What we agreed

- **Approach 1.** The root stays at the top of the tree. It is renamed **Scene settings**, it
  holds only settings for the whole scene, and its slots no longer pass anything down.
- **Every new thing gets its own fresh starter modules**, made for its kind. Starters are not
  shared per kind and not copied from siblings.
- **The Library stays tidy.** Starters are listed in the Library named after their thing, are
  renamed with it, and are deleted with it when nothing else uses them.
- **Folders still pass modules down.** A thing added inside a folder that fills a slot follows
  the folder for that slot. Anywhere else it gets its own starter.
- **Chords leave Scene settings.** Each sound module picks its own progression from the
  Library, the way it already picks an instrument.
- **Scene-wide modules stay on Scene settings** (Aedan chose this 22:24Z). These
  are Effects (whole-picture shaders such as kaleidoscope, echo and flare), Music (drums or a
  melody playing on the beat) and Rules. They act on the scene and are never passed down.
- Old scene files need not open (standing project rule). The built-in scenes are rebuilt.

## Design

### 1. The root: Scene settings

- The kind id stays `"scene"`, so keyframes, randomise, the timeline and `ThingId::ROOT` keep
  working. Only `name()` changes to "Scene settings" and the help text is updated.
- `SceneSettings` loses `chords`. Every other setting stays: length, gravity, background,
  bloom, exposure, grade, video size and rates, master, seed, song, beat and off-shot fade.
- The root's `slots()` become `[Sound, Shader, Rules]` (shown as Music, Effects and Rules on
  this card only). Its `default_slots()` is empty, so a new scene starts with none of them
  filled.
- **Resolving never walks into the root.** `find_slot` and `resolve_with` stop at the
  thing's last ancestor below the root. The root's own slots resolve only for the root. This
  removes the special case in `shader/draw.rs` ("the root's shader ... already covers every
  thing below it"), because nothing inherits it any more.
- The root's Sound slot accepts the music module kinds (drums, melody) and plays on Beat
  events, as today. Which sound kinds the root accepts is decided by the existing module-kind
  check. If hit-driven `sound.notes` turns out to be pointless on the root, it is hidden from
  that slot's picker (decided during planning, after reading the mix code).

### 2. Starter modules

`bpe_kit::add_thing` becomes the only way the editor adds things. It replaces the current
liquid and swarm `starter_modules` table with one starter list per kind:

| Kind | Starters (each the thing's own) |
|---|---|
| Balls, Soft body | Look (`look.classic`), Motion (`motion.basic`), Bounce (`bounce.basic`), Sound (`sound.notes`) |
| Arena, Obstacle | Look (`look.classic`, wall values), Bounce (`bounce.basic`, Takes over off), Sound (`sound.notes`) |
| Liquid | Look (`look.liquid`), Motion (`motion.liquid`), Bounce (`bounce.basic`, bounciness 0) |
| Swarm | Look (`look.swarm`), Motion (`motion.flock`) |
| Text | Look (`look.classic`) |
| Emitter, Field, Camera, Folder | none |

- The slots each kind starts with are its existing `default_slots()`. Optional slots (Trail,
  Particles, Rules, Shader) start empty and are added from the card picker, as today.
- **Sound starters** use one shared Library instrument (Pluck) and one shared Library
  progression ("Starter"). Both are made the first time something needs them and reused after
  that. They are shared library items, not owned starters, so deleting a thing never removes
  them.
- **The folder rule.** For each default slot, if a folder above the new thing fills that slot,
  the thing's link is `SlotLink::Inherit` (Follow folder). Otherwise a fresh starter module is
  made and linked.
- `Scene::add_thing` in bpe-core stays plain (no modules). It is used by tests and by
  `add_thing` itself.

### 3. Owned starters

- `Module` gets `owner: Option<ThingId>` (serde default `None`, skipped when `None`). A
  starter's owner is the thing it was made for. Modules made in the Library, duplicated, or
  made by "Make unique" have no owner.
- **Naming.** An owned starter is called "<thing name> <slot>", for example "Ball look" or
  "Ball 2 sound". Renaming a thing renames its owned starters, unless the module has been
  renamed by hand (its name no longer matches the pattern for the old thing name).
- **Deleting a thing.** After the delete, every module whose owner was a deleted thing and
  that nothing links any more is removed from the Library. History snapshots the whole scene,
  so undo brings them back.
- **Sharing.** Linking an owned starter on another thing is allowed and keeps the owner. The
  module survives its owner's deletion while someone still uses it, and its owner is then
  cleared.
- **"Make own"** (see section 5) makes a new owned starter, copying the followed module's kind,
  params, graph, macros and channel strip.

### 4. Moving things between folders

When a thing is moved (`reparent`, tree drag, Ride or Group), every slot that is
`SlotLink::Inherit` and resolved to a module before the move is checked again afterwards. If
it would now resolve to a different module, or to nothing, the thing gets an owned copy of the
module it used before. Nothing changes look or sound just because it moved.

### 5. Inspector

- **Scene settings card.** Shows only its settings, grouped as today (Video, Grade, Master,
  Song, Beat, Camera sound), plus the buses list. The Chords row is removed, along with "Plays
  Scene defaults' chords/instrument" notes and the "Scene's" picker entries for chords and
  instruments. Below the settings are three optional cards: Effects, Music and Rules.
- **A thing's slot card picker** offers:
  - the thing's current module, and **Make unique** when it is shared (unchanged);
  - every Library module of a fitting kind (this is how you share, unchanged);
  - **Follow folder**, shown only when a folder above fills that slot (replaces "Inherit");
  - **Remove** (unchanged, `SlotLink::Off`);
  - the "Inherit (nothing above)" entry is gone.
- **Followed cards** are read-only, and their header says which folder they follow. The
  "Change here" button becomes **Make own**, which makes an owned copy (section 3).
- An owned starter used only by its own thing is edited directly, under the existing
  `write_target` rule (the sole user with no overrides edits the module).
- **Sound module rows.** Chords and Instrument pick a Library item, "New progression" / "New
  patch", or Make unique. `None` stays valid in the data: it means the built-in starter
  progression or Pluck. Starters never leave it `None`.

### 6. Timeline and mix

- The timeline's Chords track shows the chords of the selected thing's sound. With nothing
  selected, or a selection with no sound, it shows the first thing in tree order that has one.
  It no longer reads the root's `chords`.
- `bpe-mix`'s `scene_instrument` and "Scene's" fallbacks are removed. `InstrumentRef(None)`
  means built-in Pluck. `ProgressionRef(None)` means `Progression::starter()`, which
  `ChordClock` already uses for an empty list.
- Randomise: the root's `chords` lock and roll disappear. Sound modules' own `chords` is
  already `random`.

### 7. Built-in scenes and new scene

- `new_scene` makes Scene settings, an Arena and a Ball through `add_thing`, so each gets its
  starters. The ball's look is set to the pink it has today.
- The scene builder helpers (`root_look`, `motion`, `bounce`, `sound` in `scenes.rs`) change
  to set values on every relevant thing's starter instead of the root's module. The aim is
  scenes that look and sound the same as now.
- Stage 5 scenes keep their whole-picture shaders on the root's Effects slot.

## Testing

- bpe-core: resolving never inherits from the root; the root's own slots still resolve for
  the root; a followed folder slot resolves; Follow folder with nothing above resolves to
  nothing; owner survives save and load.
- bpe-kit: each kind's starters (kinds, names, values, owner); the second ball gets fresh
  modules; sound starters share one instrument and one progression; the folder rule when
  adding; rename; delete removes owned and unused starters only; moving out of a folder keeps
  the module through a copy; the root's shader is drawn once, as a picture effect.
- bpe-studio (kittest): Scene settings has no Chords row; no "Change here"; Make own; Follow
  folder only offered inside a filled folder; the timeline chord track follows the selection.
- Goldens: a render of the built-in scenes before and after. Differences are explained or
  fixed, then re-blessed (once in debug and once with `--release`).
- `cargo test --workspace` and `cargo clippy` clean.

## Out of scope

- Per-kind templates that new things copy (option 3, deferred).
- Hiding owned starters from the Library.
- 3D.
