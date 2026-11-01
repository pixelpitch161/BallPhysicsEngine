# Editor layout: type switch, Across and Inside colours, one section order

Date: 2026-11-01. Requested by Aedan. Findings: "Editor layout review"
(https://claude.ai/artifact/QKQFq9xvHxsggJf7ToAiMg). Design he approved on 2026-11-01:
"Editor layout design" (https://claude.ai/artifact/U8PuhUxdN2oKfsestu484Q).

## Why

The editor has the settings Aedan wants, but the same idea shows in different places, under
different names and in a different order from one module to the next. He named three problems:

- There is no "self" gradient for a group of balls. Colours follow Order, Place and so on, and a
  Place gradient is laid over the whole world, so a ball can't fade top to bottom inside itself.
- A module's type (Classic or Styled look, which shader effect, Notes or Drum machine) is fixed
  when the module is made. Going from Classic to Shiny needs a new module each time.
- The layout as a whole can be confusing.

Going through every thing, module and setting, and the built editor on the colour-paints and
neon-ring scenes, found seven problems. They are listed in the findings doc.

## What we agreed

- Do all of it, as one design built in three parts, each its own pull request.
- Merge `look.classic` and `look.styled` into one Look type.
- Keep the inspector as one scroll of cards, with a jump bar pinned at the top.
- Inside colours offer both "Shade of own colour" (the default) and "Own gradient".
- Old scenes need not load (Aedan, 2026-10-30). Built-in scenes are rebuilt to look the same.

## Part 1: Type switch, one Look, Across and Inside colours

### 1.1 Card header

The slot card header today is: fold arrow, slot label, one menu button reading the module's name,
Nodes, dice. It becomes:

`▸ Look   [Type ▾]   [Module ▾]   Nodes   🎲`

- **Type ▾** reads the module kind's `name()`. It lists `reg.modules_for(slot)` filtered by
  `ThingKind::accepts(slot, kind)` and by a new check that the kind fits the thing (see 1.3), in
  registry order. It is not drawn when only one kind fits. For an inherited card (Follow folder)
  it is greyed out, the same as other inherited edits.
- **Module ▾** reads `"<module name>"` or `"<module name> · shared ×N"`, as the state label does
  today. Its menu keeps every entry the picker has today: the library modules of this slot, New
  module (a plain button when one kind fits, a submenu of kinds otherwise), Make unique, Push all
  to module, Reset all, Open in Library, Follow folder, Remove.
- Picking a type on a shared module first shows, in the Type menu itself, a line "Changes all N
  things using <name>". There is no extra confirmation, because Undo covers it.

### 1.2 Switching a module's type (core)

There is a new edit in bpe-core: `Scene::set_module_kind(&mut self, reg, m: ModuleId, kind: &str)`.

- `Module` gets `stash: BTreeMap<KindId, Stash>` (serde default, skipped when empty), where
  `Stash { params: Params, tracks: BTreeMap<String, Track>, pulses: BTreeMap<String, Pulse>,
  locks: BTreeSet<String>, overrides: BTreeMap<ThingId, Params> }` is keyed by setting key.
- Switching from kind A to kind B:
  1. A key of A's params that B also has, with an accepted type (`Registry::carry_over`), stays.
     Every other key moves into `stash[A]`.
  2. Keyframe tracks, pulses and locks whose `Address` is `Owner::Module(m)` follow the same rule.
     So do those on `Owner::Thing(t)` with `slot = Some(m.slot)` for each thing `t` linked to `m`.
  3. Each linked thing's overrides (`SlotLink::Linked { overrides }`) follow the same rule, into
     `stash[A].overrides[t]`.
  4. If `stash[B]` exists, its entries are put back where nothing carried over already holds the
     key, so values changed since the last switch win. Then `stash[B]` is removed.
  5. `m.kind = B`. A `graph` with `custom: true` is dropped, because it was built from A's template.
     A non-custom graph is replaced by B's template, or by `None`.
  6. `channel` and `macros` stay. They only mean something to sound kinds and are harmless otherwise.
- It errors with `EditError::MissingModule`, or a new `EditError::WrongSlot` when B fills another
  slot. It is one undo step through the editor's normal history.
- The editor dispatches a new `Edit::SetKind(slot, kind)` to `set_module_kind` on the shown
  module. For multi-selection it applies once per distinct module among the selected things.

### 1.3 Which types fit which thing

A new `ModuleKind::fits(&self, thing_kind: &str) -> bool` defaults to true. It is used by the Type
menu, the New module menu and the Library's "+ New" lists:

- `look.liquid` fits only `liquid`, `look.swarm` only `swarm`, and the merged `look.basic` every
  other kind.
- `motion.liquid` fits only `liquid`, `motion.flock` only `swarm`, and `motion.basic` every other.
- Sound and shader kinds fit every kind. The Scene's existing `accepts` still limits its Sound
  slot to drums and melody.

So a Balls Look card shows no Type menu (one kind fits), and a Balls Effects card lists all 13
effects.

### 1.4 One Look

`look.classic` and `look.styled` are replaced by one kind, `look.basic`, named "Look", in
`bpe-kit/src/modules/look/`. Its settings are the union of both, re-sectioned as in Part 2. Rows
that only matter for balls keep `when = "thing=balls"`.

- **Shape** (balls): `ball_shape` Circle, Square, Triangle, Diamond, Hexagon, Star, Heart, Donut,
  plus Roundness and Outline (px) with Outline colours.
- **Style** (balls): a new `style` choice, Shiny (the default, today's Classic ball, lit like a
  sphere) or Flat (today's Solid fill). A separate Glow style was dropped while building: the
  Classic ball and the Styled Shiny fill shade the same way, so two names would have meant one
  look. A Shiny circle whose Outline, Glow size, Opacity, Rainbow and Stretch are at their
  defaults draws through today's classic ball path; anything else draws through the styled path.
- **Face motion** (balls, not circles) stays a Shape row and turns the shape; Turns with the ball
  (Inside) turns only the inside pattern.
- The old `finish` patterns (Radial, Linear, Stripes, Rings, Dots, Checker, Halves, Slices,
  Swirl, Face) move into Inside (1.6).
- Glow, Flash, Flash size, Flash time, Hue shift per hit, Glow size, Opacity, Rainbow, Stretch,
  Turn, Spin and Blending keep their meaning.

Drawing: see Style above for which path a ball takes (`balls/draw.rs`, `styled.wgsl`).
Things other than balls read the look as `look_of` does now, keeping only Colours, Inside and Glow.

Defaults: a new ball's starter look is Shiny, Circle, one colour, Inside None, which is today's
classic default.

### 1.5 Across the group

The `Paint` setting group keeps its gradient and follow rows (colours, follow, shape, angle,
centre, space, span, repeat, scroll, take hit colour, step time, and the variation rows). It loses
`inside`, `inside_angle`, `inside_spin`, `inside_scale` and `inside_scroll`. On a thing with many
instances (balls, particles, swarm, text per letter) its heading reads **Across the group**. On a
single thing it reads **Colours**. The derive's `label` and `group` options already allow this.

### 1.6 Inside each one

A new setting group, `InsidePaint` (in `bpe-kit/src/paint/inside.rs`), is embedded with
`#[setting(inside, prefix = "in_", group = "Inside each one")]` wherever `inside` was set on a
Paint today: the Look's fill (balls, text letters, soft bodies). Its rows:

| Key | Label | Type | Shows when |
| --- | --- | --- | --- |
| `in_pattern` | Pattern | None, Linear, Radial, Sweep, Stripes, Rings, Dots, Checker, Halves, Slices, Swirl, Face | always |
| `in_from` | Inside colours | Shade of own colour, Own gradient | pattern ≠ none, face |
| `in_shade` | Shade | -1 (darker) to 1 (lighter), default -0.5 | from = shade |
| `in_colours` | Colours | Gradient (stops) | from = own |
| `in_repeat` | Beyond the last colour | Clamp, Repeat, Mirror | from = own |
| `in_angle` | Angle | -180 to 180 deg, keyframe | pattern ≠ none |
| `in_spin` | Spin | -720 to 720 deg/s, keyframe | pattern ≠ none |
| `in_size` | Size | 0.1 to 24, keyframe | pattern ≠ none |
| `in_scroll` | Scroll | -4 to 4 cycles/s, keyframe | pattern ≠ none |
| `in_turns` | Turns with the ball | bool (today's Face motion) | pattern ≠ none |

- With Shade, the inside runs from the instance's own colour (whatever Across the group gave it,
  after variation) to that colour shaded. Shade -1 is black, 0 is the same colour and 1 is white,
  mixed in linear light.
- With Own gradient, it samples `in_colours` and ignores the instance's colour.
- Linear, Radial and Sweep cover the shape once at Size 1 and follow `in_repeat` past the ends.
  Stripes, Rings, Dots, Checker, Halves, Slices and Swirl repeat Size times across the shape.
  Face draws today's face in the shaded colour.
- Linear, Radial and Sweep follow `in_repeat` (Clamp, Repeat or Mirror). Stripes and Rings are
  repeating patterns, so they always mirror their gradient and `in_repeat` is hidden for them and
  for the mask patterns (Dots to Face), where it would do nothing. Face hides Inside colours too.

GPU: the pattern maths in `paint.wgsl` (`paint_colour`, `paint_pattern`) and the finish patterns
in `styled.wgsl` become one function, `inside_colour(inside: InsideGpu, base: vec4, local: vec2,
time: f32) -> vec4`, in a new `inside.wgsl` that both pipelines and the glyph pipeline include.
`InsideGpu` carries the pattern code, from, shade, gradient row, repeat, angle, spin, size and
scroll. Own-gradient rows use the existing gradient atlas.

### 1.7 Other type switches this gives

- **Effects:** the 13 `shader.*` kinds become a Type list on the Effects card. Settings keep where
  names match: every effect has Blend first.
- **Sound:** Notes, Melody and Drum machine on a thing's Sound card. Instrument, volume and pan
  carry over.
- **Motion and Look** on liquid and swarm show no Type menu (one kind fits).

## Part 2: One section order, jump bar, header

### 2.1 Sections

`widgets::rows_for` today puts ungrouped rows first, then groups in the order they first appear.
It changes to:

- Every `SettingDef` of a module kind names a `group`. Ungrouped rows are allowed only in a thing
  kind's own card, and show first there.
- Groups are ordered by a fixed list, `widgets::SECTION_ORDER`: Basics, Shape, Style, Burst, Each
  particle, Across the group, Colours, Inside each one, Light, Motion, Growing, Instrument, Notes,
  Which hits, Level, Chords, Other balls, Against some things, Variation, Channel, Blending. A
  prefixed paint's variation (for example "Outline colours variation") sorts with Variation.
  Unknown groups go last in appearance order, and a debug assert names them.
- A new `SettingDef` flag, `folded: bool` (derive: `folded`), marks a group as folded when nobody
  has opened it yet: Variation, Blending, Other balls, Against some things, Channel.
  `Inspector.collapsed` keeps working, now seeded from the defaults with an explicit-open set.
- A folded group with any row not at its default (a value, a keyframe track or a pulse) shows a
  small accent dot after its heading.

Section contents (the label changes are part of this):

| Card | Sections |
| --- | --- |
| Look | Shape · Style · Across the group · Inside each one · Light (glow, glow size, opacity, flash, flash size, flash time, hue shift per hit, rainbow) · Motion (stretch, turn, spin) · Variation · Blending |
| Trail | Shape (length, width, taper) · Colours (colour from, gradient rows) · Light (brightness) · Variation · Blending |
| Particles | Burst (preset, count, speed, spread, least speed, follow impact) · Each particle (size, life, gravity, drag) · Colours · Variation · Blending |
| Motion | Basics (Gravity ×, air drag, wind) |
| Bounce | Basics (bounciness, speed gain, top speed, friction) · Growing (grow per hit, biggest size, pops after) · Other balls · Against some things |
| Sound | Part 3 |

The Motion setting `gravity` is relabelled "Gravity ×", with help "How strongly the scene's
gravity pulls these, as a multiple."

### 2.2 Jump bar

A new `inspector::jump_bar` row sits above the inspector's `ScrollArea`, outside it, so it never
scrolls. Each button is a slot the selected thing has (in `SlotId::ALL` order, using
`slot_label`), plus **Thing** first. For Scene settings it lists Part 3's cards. Clicking one:

- unfolds that card,
- stores a pending target, so that next frame the card's header calls `scroll_to_me(Some(Align::TOP))`.

The button for the topmost card whose header is at or above the top of the visible area is drawn
selected. The row wraps when the panel is narrow.

### 2.3 Header

The header grid (nine rows today) becomes:

- Row 1: the name field, then a muted badge with the kind name (its hover is the kind's help),
  then an eye toggle (Shown), on things other than the root.
- Row 2: Tags.
- Then the thing kind's own card, titled with the kind name alone ("Balls", "Arena"; the root
  keeps "Scene settings").
- Then a **Placement** card (fold arrow, open by default) with Position, Rotation, Attach
  (Grouped / Rides on it) and Stick to screen. The camera has no Placement card, as today it has
  no tree rows.
- The Parent row leaves the inspector. The tree's right-click menu gets **Move into ▸**, listing
  the root and every folder. It is the same `set_parent` edit the Parent picker makes today.

### 2.4 Smaller fixes

- **Inspector width:** the right panel keeps the width the user drags it to. Its contents are
  laid out with `ui.set_max_width(panel width)`. Label columns wrap at a fixed width, so a long
  label never widens the panel. The width is saved in the editor settings file beside the recent
  scenes.
- **One label column:** every settings grid in the inspector uses one `LABEL_WIDTH` (the larger
  of 120 px or 38% of the panel), so sliders line up across cards.
- **Top bar:** the save status text is clipped (elided with "…") in its own fixed-width slot, so
  it can't draw under Library.
- **Add thing:** the menu is grouped under headings, with a new `ThingKind::family() ->
  &'static str` (default "Other"). Bouncers: Balls, Emitter. Walls: Arena, Obstacle. Flowing:
  Liquid, Swarm, Soft body. Forces: Field. On screen: Text. Then Folder. Camera stays out of the
  menu while one exists.
- **Labels:** "Place from: Direction/Centre" becomes "Gradient: Straight/Round". "Past the end"
  becomes "Beyond the last colour". "Colours end at" gets the unit "px" when following Place or
  Start.

## Part 3: Scene settings and a shorter Sound card

### 3.1 Scene settings cards

The root's own settings are drawn as several cards instead of one. A card is a group: the
inspector draws each top-level group of the root kind as its own card, with a fold arrow and
dice, in this order:

| Card | Settings |
| --- | --- |
| Video | width, height, frame rate, length |
| World | gravity, physics rate |
| Background | background paint rows (Across the group shown as "Colours"), background stays on screen |
| Picture | bloom, exposure, brightness, contrast, saturation, warm/cool, tint, vignette, vignette size, gradient map, gradient map amount |
| Music | tempo, beat sync, beats split into, stretch, song, song starts at, song volume, ducks under hits, sound seed |
| Mix | loudness, clipper, clipper drive, ceiling, auto release, release, off-shot fade, then today's Buses and Master effects sections |

The Effects, Sound and Rules slot cards follow as today. The jump bar for the root lists Video,
World, Background, Picture, Music, Mix, Effects, Sound, Rules.

### 3.2 A thing's Sound card

For `sound.notes` the sections are:

1. **Instrument**: today's instrument picker and its macro knobs. Below them a folded "Try it"
   sub-section with today's octave buttons and keyboard.
2. **Notes**: lowest note, highest note, picks from, order, plays.
3. **Which hits**: plays when, only hits against, shortest gap, quietest hit, loudest hit.
4. **Level**: volume, pan, stereo width.
5. **Chords**: the progression picker (today's Chords row) and an **Edit** button. Key, mode,
   meter, the chord blocks, the in-key and borrowed chord buttons, Play and Presets no longer draw
   in the card.
6. **Channel** (folded): today's inserts and sends.

Melody and Drum machine use the same section names where they apply.

**Chord editor in the bottom panel:** Edit opens today's chord editor (`chords::editor`) in the
bottom panel that Nodes uses, in place of the node canvas. The panel title reads "Chords:
<progression name>" and has a Close button. Only one of Nodes or Chords is open at a time.
Clicking the timeline's chord lane label also opens it.

## Built-in scenes

`scenes.rs` and `scenes_stage5.rs` build looks with `look.basic`. A Classic look becomes Style
Shiny. A Styled look's `finish` maps to Style plus Inside:

- Solid becomes Style Flat, Inside None.
- Shiny becomes Style Shiny, Inside None.
- Any pattern (Radial … Face) becomes Style Flat, Inside <the same pattern>, with Own gradient
  set to the old fill colour then the old pattern colours, so it draws the same two colours.

The colour-paints scene's classic Inside Rings becomes Inside Rings with Own gradient (its old
colours) and Mirror. It looks almost the same: the rings no longer shift colour with each ball's
place in the group, because Inside now has its own gradient (an expected picture change). Shader kinds generated from effect headers put every
parameter in a "Basics" group, so the section rule (2.1) holds for them too.

## Testing

- **Core:** `set_module_kind` round trip. Classic-only, styled-only and shared values survive A →
  B → A. Keyframes, pulses, locks and a thing's overrides are stashed and restored. A changed
  shared value wins over the stash. A custom graph is dropped. WrongSlot errors.
- **Kit:** `look.basic` defs pass `check_registry`. `fits` limits Type lists. `InsidePaint`
  round-trips params. Every `when` names a real key. `SECTION_ORDER` covers every group of every
  module kind (test).
- **Pictures:** every built-in scene renders frame 0 and a mid frame within the existing picture
  tolerance of main. The scenes using styled looks (blend-modes, kaleido-hexagon, lava-ball,
  neon-ring) and colour-paints are checked by eye from screenshots. New picture tests cover Inside
  Linear with Shade, with Own gradient, and on text letters.
- **Editor (kittest):** the Type menu shows for Effects and not for a ball's Look; switching type
  and back restores a setting; the jump bar unfolds and scrolls to Sound; section order on Look,
  Trail and Bounce cards; the folded-with-changes dot; the inspector width doesn't change when
  selecting Scene settings; grouped Add thing; Move into from the tree; Scene settings shows six
  cards; Sound card Edit opens the chord editor in the bottom panel.
- **On screen:** each part ends with Xvfb screenshots and a Windows exe for Aedan before merging.

## Out of scope

- New effects, sounds or physics. Chains of several effects in one slot.
- A visual redesign of the tree, timeline or node editor beyond the items above.
- Loading scenes saved before this change.
