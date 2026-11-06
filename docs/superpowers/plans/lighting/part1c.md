# Lighting part 1c: the Lights tab, light handles, Show modes and past-span values

Spec: `docs/superpowers/specs/2026-11-06-lighting-design.md` (approved), sections "The editor" (B, C's handles) and
"No limits". Part 1a (`docs/superpowers/plans/lighting/part1a.md`, PR #106) is in. Built straight after this plan (no
approval stops), one Opus review at the end, then the PR, merged by this thread once tests and the review pass (the
owner's overnight instruction, 2026-11-06). Parts 1b, 2a, 6 and 8 are built at the same time; 1b lands first for
shared files, so this part rebases on whatever has landed and runs every test again before it merges.

## What 1c delivers

- **The Lights tab** (layout B): the left panel switches Things / **Lights** / Mixer. At its top the scene's Depth and
  Quality chips and "+ Point / Spot / Sun / Area / Strip" buttons. Then a strip per light source: every Light thing
  (grouped under the folder it sits in), every Gives off light sub-layer (once part 1b registers that kind), and
  **Ambient** last. A strip has its track tag, name and kind word ("Spot · on Drifters"), the light's colour dot, a
  **Strength** fader (the real setting, read and written at the playhead, keyed when the setting has keys,
  undoable), **M** (turns the light off, saved) and **S** (solo, editor only). Clicking a strip selects what it
  belongs to (the light; the thing and the layer for a sub-layer; Scene settings for Ambient).
- **Light handles** in the preview (C), whenever a light is selected or the Lights tab is open: the lamp (with the
  placement ring and the light's name when it is selected: "add lights to the placement ring"), the reach ring, a
  spot's two cone edges and its aim, a sun's direction arrow, an area light's corner and aim, a line strip's two
  ends. All drag, keyed at the playhead when the setting has keys.
- **Show: Picture / Light only / Heat map** under the preview whenever the scene has a Lighting card (editor only,
  never saved or exported), with a heat-map legend.
- **The past-span value look across the inspector**: every number setting with a usual span shows a slider over the
  span whose value can go past it; past the span the knob sits at the end with a small arrow and the value box turns
  `TEXT_STRONG` with a `TEXT` border. Dragging past the end keeps going at the same rate.

## Decisions made while planning (each a fill-in of the spec, stated here)

1. **Mute is the thing's own on/off.** A Light thing is nothing but its light, so **M** on its strip flips
   `Thing.enabled` (the tree's eye; saved, undoable). A Gives off light sub-layer's M flips the sub-layer's
   `enabled` (`Scene::set_layer_enabled`). Ambient's M flips a new Lighting card setting **Ambient on**
   (`ambient_on`, bool, default true, group Room; off makes `LightingSettings::gpu` send no ambient). No new
   per-light "muted" key: one switch per thing, one place it lives.
2. **Solo is the editor's alone** (`Studio.light_solo: BTreeSet<LightSource>`, never in the scene). While any strip
   is soloed the preview draws from a copy of the resolved scene (`lights_panel::soloed`): Light things not soloed are
   left out, Gives off light layers not soloed are taken out of their card's layers, and the Lighting card's
   `ambient` is set to 0 unless Ambient is soloed. Export reads the scene, so it never sees solo. A pill in the
   preview's corner says "Solo: Torch, Ambient"; clicking it ends the solo. Sources that no longer exist drop out.
3. **Sources** (`LightSource`: `Light(ThingId)`, `Layer(ThingId, SlotId, LayerId)`, `Ambient`) are listed by
   `lights_panel::sources(scene, reg)` in tree order. A Layer source is any layer whose kind is in
   `GIVES_OFF_LIGHT = ["sub.light", "shape.sub.light"]` (the spec's kind ids for part 1b) and is registered; until 1b
   lands nothing matches. Its strip is named "Thing › Layer it changes" and its fader is that layer's `strength`.
   Groups: a light whose parent is a folder (or, later, part 6's rig) is listed under the parent's name; riders show
   "· on <parent>". Ambient is listed only when the scene has a Lighting card.
4. **The fader** is the setting itself: `Address::own(light, "strength")` (a layer's `"<id>/strength"` on its card;
   Ambient's `ambient` on the Lighting card), read with `Scene::value_at_time` at the playhead and written with
   `inspector::set_or_key` (keys when keyed; a keyed edit pauses as the inspector's do). It is the same span slider
   as the inspector's (decision 9), so Strength 14 reads as past its span. Its right-click menu is the setting's
   value menu (Copy / Paste / Create Keyframe) through `widgets::value_menu`.
5. **The colour dot** shows the light's colour (Own: its colour; Temperature: `kelvin_to_linear`) encoded to sRGB, a
   scene colour (`// scene colour`). A sub-layer's dot is its `colour` when it has one, else the thing's track colour.
6. **No Lighting card**: the tab says the picture isn't lit, with **Add Lighting** (adds the card through
   `Scene::add_slot`), and still offers the "+ light" buttons (which add the card through `add_light`). The "+"
   buttons put the light where the tree's Add thing ▸ Light would (inside a selected folder or ridable thing) and
   select it.
7. **Handles** (`preview/light_handles.rs`): read the light's settings at the playhead (`settings_at`: the resolved
   settings with each keyed setting's `value_at_time`) and its places from the world
   (`bpe_kit::things::light::draw::places_in`, split out of `places` so the editor and the draw agree). Parts
   (`LightPart`): `Lamp(k)` (place k), `Reach`, `Cone(side)`, `Aim`, `Corner`, `End(0|1)`.
   - Lamp: moves Position. A rider's Position is its offset from what it rides, so the drag moves the offset by how
     far the pointer goes from where it was pressed (like the ride drag of other riders). Snaps to the grid.
   - Reach: Reach = distance from the lamp (k = 0 for riders). Not for a sun.
   - Cone: Cone width = 2 × the angle between the pointer and the aim. Aim: Direction = the pointer's angle. With
     Points: Spins the Direction stored is the angle minus Spin × the playhead; with Where it's going the aim follows
     the ball and its handle isn't offered.
   - Sun: an arrow from the lamp along its Direction; dragging its tip sets Direction. Its lamp still moves Position
     (only where the handle sits; a sun lights from everywhere).
   - Area: a corner handle sets Width (along the aim) and Length (across) from the pointer's place in the panel's own
     frame; an aim handle past its far edge turns it.
   - Strip (Line): its two ends; dragging one keeps the other where it is (Position, Length and Direction change
     together). A strip along a shape's outline only has its lamp.
   Each drag is its own undo step and keeps the playhead (`Outcome::keep_playhead`), so keys land where the light is
   seen. Hit order: the selected light's handles first (a lamp riding a ball sits on the ball), other lamps (Lights
   tab open) after the balls and before the shapes. Riding Each ball draws at most `MOST_LAMPS` (400) lamps.
8. **Show** (`bpe_gfx::LightShow`: Picture, LightOnly, HeatMap; `LightScene.show`) is set by the editor on the
   built frame (`Studio.light_show`), never by the kit, so export always draws the picture. The light pass sends it
   in the uniform's spare `ambient.w`; `fs_light` returns, instead of the mix, the light arriving at the pixel
   (Light only: ambient + every light with its shadows, as it would fall on a white surface) or that light's
   luminance through the heat palette (Heat map). While showing light the editor also drops the picture effects,
   bloom, exposure and grade from the frame so the light reads as it is. Heat palette (sRGB, the same stops in
   `style::HEAT` for the legend and in lighting.wgsl): below 0 violet (negative light), 0 black, 0.25 deep blue,
   0.5 cyan, 1 green (as unlit), 1.5 yellow, 2 red, 3 and up white. The buttons sit under the preview and show only
   when the scene has a Lighting card; World and Camera views alike.
9. **Past-span slider** (`widgets::span_slider`): for a Number or Int setting with a `span`. The rail covers the
   span; dragging the knob moves the value by the drag's distance at span ÷ rail width a point (relative, so it
   carries on past either end at the same rate; Shift ten times finer); a click on the rail jumps there. The box
   beside it takes any typed value. The only clamp is the hard `range`, if the setting has one (Reach: 0 up). Past
   the span: rail full to that end, knob at the end, a small `TEXT_STRONG` arrow just past it, and the value box
   drawn `TEXT_STRONG` with a `TEXT` border. A setting with a range and no span keeps today's `number_slider`; one
   with neither keeps its drag box. The rail's rect is stored like `number_slider`'s so a following setting's band
   still draws. The slider is named for kittest (`labelled_by`) and reports its value.
10. **Deferred from 1a's list**: 1a moved "Lights up" (light linking) and "Picked from a thing's layer" colour to 1c's
    list. Neither is in the spec's 1c row, and linking needs a per-pixel thing mask in the surface picture, which part
    2a is reshaping for Stage. Both move to part 4 (with gobos and flicker, the light's own extras); the coordinator
    is told so it can carry them.

## Files and types

### bpe-gfx
- `frame.rs`: `LightShow` (`Picture` default, `LightOnly`, `HeatMap`, `code()`), `LightScene.show: LightShow`.
- `lib.rs`: export `LightShow`.
- `lighting.rs`: `base.ambient[3] = scene.show.code()`.
- `shaders/lighting.wgsl`: `heat(l)` and the two early returns at the end of `fs_light`. Mode 0 output unchanged
  when `show` is Picture (the no-card and lit picture tests prove it).

### bpe-kit
- `things/light/draw.rs`: `places_in(world, parent, s)` (what `places` calls); `aim_angle(s, place, time)` (what
  `push_light` uses) so the editor's handles point where the light does.
- `things/light/mod.rs`: `LightSettings::display_colour()` (sRGB bytes of its colour) for the strip dot.
- `modules/lighting.rs`: `ambient_on`.

### bpe-studio
- `app.rs`: `LeftPanel::Lights`, the three chips, the Lights panel branch (its own panel id like the Mixer's, opening
  at the tree's width), `Studio.light_solo`, `Studio.light_show`.
- `lights_panel/mod.rs` (new): `LightSource`, `sources`, `soloed`, `show` (header, chips, add buttons, strips).
- `lights_panel/strip.rs` (new): one strip.
- `preview.rs`: `Handle::Light(ThingId, LightPart)`, hit order, drag, `keep_playhead`, the Show bar, the solo pill,
  the heat legend, `Drawn` gains the solo and show so the picture redraws when they change, `draw_picture` applies
  solo and show.
- `preview/light_handles.rs` (new): `handles`, `hit`, `draw`, `apply`, `settings_at`.
- `widgets.rs`: `span_slider`, used by `widget` for spanned numbers.
- `style/tokens.rs`: `HEAT` stops, `HEAT_NEGATIVE`.

## Tests (what passing looks like)

- **Show** (bpe-kit `tests/lit_pictures.rs`): the lit room with Show Light only is brighter near the lamp than far
  from it and darker in a ball's shadow than beside it; ambient alone gives the ambient grey; Heat map maps a dark
  pixel to near black and the lamp's pool to a warm (red-heavy) colour; Picture is byte-identical to a frame without
  `show` set (the default).
- **Places/aim** (bpe-kit unit): `places_in` matches `places`; `aim_angle` for Fixed, Spins at a time, Where it's
  going.
- **Ambient on** (bpe-kit unit): off sends zero ambient.
- **Lights tab** (bpe-studio kittest): the Lights chip opens it; a light's strip shows its name; moving the fader sets
  `strength` (and a key when keyed); M flips `enabled`; S solos (the scene doesn't change) and the solo copy leaves out
  the other light and zeroes ambient; clicking a strip selects the light; with no Lighting card "Add Lighting" adds
  it; "+ Spot" adds a spot light and the card.
- **Handles** (bpe-studio unit): dragging the lamp moves Position (a rider's offset), the reach ring sets Reach, a
  cone edge sets Cone width, the aim sets Direction (Spins subtracts Spin × time), a strip end keeps the other end,
  an area corner sets Width and Length, a keyed Position gets a key at the playhead; hit testing finds the selected
  light's lamp before the ball it rides.
- **Span slider** (bpe-studio kittest/unit): a value past the span is shown as typed (not clamped); dragging past the
  end keeps going; a hard range still clamps; the past look is on only past the span.
- Physics and sound goldens unchanged; the unlit picture hashes unchanged; `cargo clippy --workspace`, `cargo fmt`,
  the design-system colour test.

## Things not to do

- Solo and Show never reach the scene file or the export.
- No lighting code runs for a scene without a Lighting card (Show is read only inside the light pass).
- No colours outside `style/` (the light's own colour dot is a scene colour).
- No hard ranges for taste; the span slider must never clamp to its span.
