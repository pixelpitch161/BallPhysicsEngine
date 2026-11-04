# Plan: one Physics card (part 3)

Spec: `docs/superpowers/specs/2026-11-04-sublayers-trails-physics-design.md`, "Part 3: Physics card". Owner's
decision (2026-11-04 16:09Z): a **straight merge** of Motion and Bounce into one Physics section, no new options.

**Design in one line:** the scene keeps its two cards (`SlotId::Motion` / `motion.basic`, `SlotId::Bounce` /
`bounce.basic`); only the inspector shows them as one card titled **Physics**. No data, runtime, save format or
physics change. Goldens cannot move, and must not.

Branch: the build thread's own branch, from `main`. Parallel with part 1 (sub-layers). The only file both touch is
`crates/bpe-studio/src/inspector.rs`: part 3 changes the per-slot card loop, the jump bar, the "+ Add slot" menu and
the `Card` enum; part 1 changes nothing there except possibly `Inspector` fields (adds `folded_pieces`). Whichever
merges second rebases; conflicts, if any, are in `Inspector`'s field list only.

## Things not to do

- Don't add `SlotId::Physics`, don't touch `bpe-core`'s ids, `bpe-runtime`, save/load, `starters.rs` slot lists, the
  test scenes or the goldens.
- Don't change any setting's key, range, default, `keyframe`/`pulse`/`when` flags. Only the two `group` names change
  (`"Basics"` → `"Motion"` on `BasicMotion`, `"Basics"` → `"Bounce"` on `BasicBounce`).
- Don't add a Type menu to the Physics card (each slot has one kind today; leave a `debug_assert!` that says so).
- Don't render screenshots or videos; kittests are the check.

## Tasks

### Task 1: group names

Files: `crates/bpe-kit/src/modules/motion/mod.rs`, `crates/bpe-kit/src/modules/bounce/mod.rs`,
`crates/bpe-core/src/settings.rs`.

1. In `BasicMotion`, change `group = "Basics"` to `group = "Motion"` on `gravity_scale`, `drag`, `wind`.
2. In `BasicBounce`, change every `group = "Basics"` to `group = "Bounce"` (bounciness, speed_gain, top_speed,
   friction, takes_over).
3. In `SECTION_ORDER` (`settings.rs`), insert `"Bounce"` right after `"Motion"`. `"Motion"` is already there.
   Check with `grep -rn '"Motion"\|"Bounce"' crates --include=*.rs` that no other card used `group = "Motion"` in a
   way the new order changes (the shape's Spin/Shrink settings use their own groups; verify).
4. Fix tests that look settings up by group `"Basics"` for these two kinds: `crates/bpe-kit/src/lib.rs` around
   line 337 (`defs_of(&reg, "bounce.basic").iter().filter(|d| d.group == group)`): read it and update the group name
   it passes.

Verify: `cargo test -p bpe-kit -p bpe-core` green. `cargo test -p bpe-kit --test golden` (and `--release`) green
with **no** re-bless.

### Task 2: `Card::Physics` and the merged card

File: `crates/bpe-studio/src/inspector.rs`.

1. Add `Card::Physics` to `enum Card`. `fold_key`: `(id, None, "physics")`.
2. Add `const PHYSICS: [SlotId; 2] = [SlotId::Motion, SlotId::Bounce];` and
   `fn physics_slots(thing: &Thing) -> Vec<SlotId>`: the members of `PHYSICS` the thing has, in that order.
3. Refactor: move the body of the per-slot loop that draws a plain card's settings (from
   `let Some(ckind) = reg.module(&card_now.kind) else { return };` to the three `edits.extend(...)` lines) into
   `fn slot_grid(ui, inspector, scene, reg, &choices, id, slot, time, lw, &thing, &mut edits)` (pass what the
   closure captures today; keep the exact behaviour, including `folded_groups`/`fold_groups` keyed by
   `(id, Some(slot))`). The Look branches stay inline.
4. In the loop over `SlotId::ALL`: when `slot` is in `PHYSICS`:
   - if `slot` is not the first of `physics_slots(&thing)`, `continue` (the Physics card was drawn already);
   - else draw one card: header with `fold_arrow(ui, &mut inspector.collapsed, Card::Physics.fold_key(id),
     "Physics")`, the title "Physics", `folded_summary` with the sum of both cards' `params.iter().count()`, and a
     **Remove** button labelled "Remove Physics" (widget info) that pushes `Edit::RemoveSlot(s)` for every
     `s` in `physics_slots`. `card_header(ui, inspector, Card::Physics, &header.response)`.
   - problems: those with `p.slot` in `physics_slots`, joined as today.
   - if open: for each `s` in `physics_slots`, `slot_grid(...)` with a 4 px gap between them. The groups of the
     two grids give the folds: Motion; Bounce, Growing, Other balls, Against some things.
   - `debug_assert!(reg.kinds_fitting(s, kind).count() <= 1)` for each: a second kind would need a Type menu.
5. `jump_bar`: build the slot entries so that `Motion`/`Bounce` give one `(Card::Physics, "Physics")` entry at the
   first of them present; other slots unchanged.
6. "+ Add slot" menu: build `missing` as today, then replace any `Motion`/`Bounce` entries with one **Physics**
   button (at the first one's place) that pushes `Edit::AddSlot(s)` for every missing `s` in `PHYSICS`.
   (`Edit::AddSlot` already skips slots the thing has.)
7. `marked_card` and anything else that matches on `Card` compile-check: add the `Physics` arm where needed.

Verify: `cargo build -p bpe-studio`, `cargo clippy -p bpe-studio`, `cargo fmt`.

### Task 3: tests

File: `crates/bpe-studio/src/inspector.rs` tests (and any other studio test that names the Motion or Bounce card
header or bar button: `grep -rn '"Motion"\|"Bounce"\|Card::Slot(SlotId::Bounce)\|Card::Slot(SlotId::Motion)'
crates/bpe-studio/src`).

Update:
- `add_slot_lists_only_slots_the_kind_lacks_and_adds_a_starter`: after removing Motion, the menu shows "Physics";
  clicking it brings `motion.basic` back.
- `a_short_last_card_stays_marked_after_jumping_to_it`: the bar button is "Physics" and the card top is
  `Card::Physics`. If Physics is no longer "too short to reach the top" on a new scene, keep the test's point (a
  short last card stays marked) by asserting on whatever card is now last; read the test before editing.
- any test clicking a "Motion" or "Bounce" header or bar button.

Add (kittest, `harness(Some(ThingId(2)))` on the default scene's ball):
1. `a_ball_shows_one_physics_card_with_motion_and_bounce_inside`: exactly one label "Physics" in the card headers
   (`query_all_by_label("Physics")` count as the bar chip plus the header: assert the header via
   `Card::Physics` in `card_tops`), no `Card::Slot(SlotId::Motion)`/`Card::Slot(SlotId::Bounce)` in `card_tops`,
   rows "Gravity ×" and "Bounciness" both present, fold headings "Motion" and "Bounce" present.
2. `removing_physics_takes_both_cards_off`: click "Remove Physics"; the ball has neither slot.
3. `adding_physics_brings_both_back`: after removing, "+ Add slot" ▸ "Physics"; both slots back with
   `motion.basic` and `bounce.basic`.
4. `a_shape_shows_bounce_settings_only`: select a shape (add one with `bpe_kit::add_thing(.., "shape", ..)` if the
   default scene has none); Physics card present, "Bounciness" absent unless Takes over (its `when`), "Takes over"
   present, "Gravity ×" absent.
5. `the_jump_bar_has_one_physics_chip`: bar labels contain "Physics" once and neither "Motion" nor "Bounce".
6. `editing_inside_physics_edits_the_right_card`: drag the "Air drag" slider (as
   `a_new_balls_card_edits_straight_away` does): Motion's params change, Bounce's don't; then drag "Bounciness":
   Bounce's change.

Verify: `cargo test -p bpe-studio` green (run in the background; it takes minutes). Then the full
`cargo test --workspace` in debug and `cargo test --workspace --release` (expect only the known debug-only
`pushing_an_earlier_step_panics_in_debug` failure in release). `git diff main -- crates/bpe-kit/tests/` is empty.

### Task 4: docs and delivery

1. `CLAUDE.md` (bpe-studio paragraph): one sentence: "Motion and Bounce show as one **Physics** card
   (`Card::Physics`, `PHYSICS`); the scene still keeps the two cards."
2. One Opus review of the whole diff (project rule), fix findings, then the PR. Build the Windows exes
   (`cargo build --release --target x86_64-pc-windows-gnu -p bpe-studio -p bpe-cli`) into project files
   `ballphysics/physics-card/` for delivery after merge.

## Done when

- A ball shows one Physics card with folds Motion, Bounce, Growing, Other balls, Against some things; a shape's
  shows its bounce settings; Remove/Add/jump bar treat it as one card.
- All tests green in debug and release; goldens untouched.
