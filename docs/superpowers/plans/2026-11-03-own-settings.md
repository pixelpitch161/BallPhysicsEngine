# Each thing owns its settings, right-click setting menu, no randomize (rework step 2)

Requested by Aedan on 2026-11-03:

> Ditch the module system. Each thing's settings will be unique to itself. Later we can add a copy
> all settings button, but that can wait until the settings are redesigned.
> When right clicking on a setting, do not change the value and instead open a popup menu with:
> Copy Value / Paste Value (display currently copied value in parentheses) / Create Keyframe.
> Then, remove the keyframe icon beside each setting so that way it's less cluttered.
> Remove the randomize feature.

This is mostly removal plus one small menu, so there is no spec; the decisions are below.

## Decisions

### Data model (bpe-core)

- **Gone:** `Library`, `Module`, `library::Stash`, `ModuleId`, `SlotLink` (Inherit/Linked/Off),
  `Scene::library`, `Scene::next_module`, `Scene::users_of`, `Module.owner` and `starter_name`,
  `ThingKind::passes_down`, `ThingKind::accepts`, `Registry::carry_over` (moved into the slot kind
  switch), `ProblemKind::{MissingModule, WrongSlot}`, every library/link/override/follow edit
  (`add_module`, `add_starter`, `own_copy`, `own_linked`, `make_own`, `copy_owned_starters`,
  `link`, `set_slot_off`, `set_slot_inherit`, `make_unique`, `push_to_module`, `reset_override`,
  `rename_module`, `duplicate_module`, `set_override`, `remove_module`, `kind_suits_users`).
- **New:** `Thing.slots: BTreeMap<SlotId, Slot>` where
  `Slot { kind: KindId, params: Params, stash: BTreeMap<KindId, SlotStash> }`. A slot that is
  not in the map simply isn't there (the old `Off`). `params` holds only values that differ from
  the kind's defaults (sparse, like a thing's own params). `SlotStash { params, tracks, pulses }`
  keeps what a type switch set aside so switching back restores it (only Effects has several types).
- `Address { thing: ThingId, slot: Option<SlotId>, key }` (the `Owner` enum goes; every setting
  belongs to a thing now). `def_at`/`value_at`/`set_at` read the slot's own kind.
- Edits: `add_slot(reg, id, slot, kind)` (refuses slots the kind lacks and unknown/wrong-slot
  kinds), `remove_slot(id, slot)` (drops that slot's tracks and pulses), `set_slot_kind(reg, id,
  slot, kind)` (keeps settings both kinds share with the same type; stashes the rest with their
  tracks and pulses; restores a stash when switching back, newer shared values win).
- `ModuleKind` keeps its name: it is now "the kind of one of a thing's cards" (Look, Trail…). Renaming
  it touches every plugin for no user-visible gain; docs and CLAUDE.md say what it means now.
- Folders no longer have slots (they only existed to pass modules down). Scene settings keep their
  Effects slot. `resolve` reads only each thing's own slots; `ResolvedSlot { kind, params }` (no
  `module`, `from`). `resolve_slot`/`find_slot` go.
- `Scene::FORMAT` becomes 3. Old files are not migrated (no backward compatibility, per Aedan).
- **Locks go** (`Scene::locks`, `is_locked`, `set_locked`): they only existed for the dice.

### Randomize

- Delete `bpe-kit/src/random.rs` (roll, scopes, dice) except the colour palette generator and
  `fresh_seed`, which the gradient bar's "Random palette" button uses; that button is a colour tool,
  not the dice, so it stays (moved to `paint/palette_gen.rs`). Delete `bpe-studio/src/dice.rs`, the
  toolbar dice, the Randomize window, every card's dice button, the "Lock when randomizing" menu
  and padlocks.
- `#[setting(random)]` goes. `random(lo, hi)` becomes `span(lo, hi)`: the usual span a slider covers
  when the setting has no hard `range` (it already set the slider's feel through
  `range.or(dice)`). On settings that also have `range`, it is simply dropped. `SettingDef.dice` →
  `span`; `Flags.random` and `dice_range` go. The contract check keeps "span sits inside range".

### Inspector

- One card per slot the thing has: fold arrow, title, the Type menu when several kinds fit (Effects),
  and a "Remove" button. No module picker, no "shared ×n", no Follow folder / Make own / Make
  unique / Push / Reset all / Open in Library, no override dots. "+ Add slot" lists slots the kind
  can have and doesn't; adding fills it with the kind's starter (kit `fill_slot`).
- **No keyframe diamonds.** The diamond column is removed from every settings grid (and the
  header/placement grids that mimicked it). A keyed setting's label is drawn in the accent colour
  so a key isn't invisible; nothing else marks it.
- **Right-click menu** on every setting row (label and widget; colour buttons and gradient bars
  only on the label, because their own popups/right-click already use those widgets):
  - **Copy Value** — puts the value in the studio's value clipboard (egui context data, shared by
    every card and thing, lives until the editor closes).
  - **Paste Value (…)** — shows the copied value in brackets (number with unit, colour as hex,
    choice as its words, On/Off, "3 colours"…); disabled when nothing is copied or the type doesn't
    fit. Pasting goes through the same path as an edit (writes a key at the playhead when keyed,
    reaches every selected thing that has the setting, clamped to range).
  - **Create Keyframe** — a key at the playhead holding the current value (replaces one already
    there); disabled with a hover reason on settings that can't have keys.
  - Nothing else. Aedan chose (decision card, 14:41Z) NOT to move the diamond's pulse items into
    this menu: pulses stay in the engine, files and timeline, but a setting can't be given a new
    pulse from the inspector for now.
- **Right-click never changes a value.** egui sliders and drag values move on any mouse button;
  the setting widgets ignore a secondary press (checked by a kittest that right-clicks a slider and
  asserts the value is unchanged and the menu is open).
- Double-click / Alt+click on a label still resets to the default.

### Elsewhere

- Studio: delete `library.rs` (Library window), `presets.rs` (My presets were modules), the tree's
  module bits, the timeline lanes' module naming. `tree.rs` duplicate copies the thing's slots
  (they're in the `Thing`) plus tracks/pulses at its addresses.
- Kit: `starters.rs` keeps `starter_kind`, `fill_slot`, `add_thing`; `keep_followers`,
  `move_thing`'s copying and `make_folder`'s fix-ups go (nothing follows a folder any more).
  `scenes.rs` builder writes slot params straight into each thing; `own_everything` goes.
- Goldens: physics must not move. Every golden line must be byte-identical to main (debug and
  release). The scene files in `scenes/next/` and `crates/bpe-kit/tests/data` are rewritten in the
  new format.

## Steps and checks

1. Core: model, resolve, address, edit, animation, save, contract, testkit + their tests.
   `cargo test -p bpe-core`.
2. Settings derive/`SettingDef`: `random` → `span`. Kit: plugins, starters, scenes, random.rs,
   tests. `cargo test -p bpe-kit` (goldens unchanged).
3. Runtime/gfx/cli compile and tests.
4. Studio: remove library/presets/dice; inspector cards; widgets menu + no diamonds + right-click
   guard; kittests for the menu, paste label, keyframe, right-click-doesn't-change.
5. `cargo clippy --workspace --all-targets`, `cargo fmt`, `cargo test --workspace`, goldens re-blessed
   debug + release and diffed against main (no change expected).
6. Update CLAUDE.md; one Opus review over the diff; PR. After "merge": Windows exe to F: bin.

## Not doing

- No "copy all settings" button (Aedan: later, after the settings redesign).
- No migration of old scene files.
- No new way to share settings between things.
