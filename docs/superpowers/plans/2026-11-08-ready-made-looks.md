# Ready-made looks: implementation plan

Spec: `docs/superpowers/specs/2026-11-08-ready-made-looks-design.md`. Mockups:
https://claude.ai/artifact/2hT4UEbPJuETxecKtAcbYK (C library page + D add menu chosen). Branch
`claude/project-thread-texii5`. No physics changes: goldens must not move.

## 0. Ground rules

- Every look is built from existing layer kinds through their typed `Settings` structs; only the Shader layer (whose
  settings are dynamic) is written with raw keys, and a test checks each of those keys is a real setting.
- Stored values are only those that differ from the kind's defaults (`own()` below), as `copy_layer` stores them.
- The editor never makes up a colour: tile and swatch colours come from the catalogue (bpe-kit data, scene colours),
  painted with a `// scene colour` comment where a literal is unavoidable in bpe-studio.
- Nothing is added to a scene unless a person picks a look. No look adds lights, Lighting or things.

## 1. bpe-kit: `crates/bpe-kit/src/ready_looks/`

New module `pub mod ready_looks;` in `lib.rs`.

### 1.1 `mod.rs`: the catalogue types

```rust
pub enum Wears { Balls, Shape }               // ball looks fit Balls and Emitters
pub struct ReadyLook {
    pub id: &'static str,                     // "chrome", "shape.neon_tube"; never saved
    pub name: &'static str,                   // "Chrome"
    pub family: &'static str,                 // "Shiny"
    pub help: &'static str,                   // one sentence
    pub wears: Wears,
    pub takes_colour: bool,                   // Keep mine can apply
    pub main: Color,                          // the look's own main colour
    pub swatch: [Color; 2],                   // menu icon and the tile fallback
    pub build: fn(&Gradient) -> Recipe,       // given the main colours (the look's or the thing's)
}
pub struct Recipe { pub layers: Vec<RLayer>, pub card: Params, pub pulses: Vec<RPulse> }
pub struct RLayer { pub kind: &'static str, pub name: Option<&'static str>, pub params: Params, pub subs: Vec<RLayer> }
pub struct RPulse { pub layer: usize /* index into Recipe.layers */, pub key: &'static str, pub pulse: Pulse }
pub const BALL_FAMILIES: [&str; 5] = ["Clean", "Shiny", "Neon", "Fire", "Trippy"];
pub const SHAPE_FAMILIES: [&str; 3] = ["Clean", "Glow", "Colour"];
pub const BALL_FAVOURITES: [&str; 7] = ["candy", "chrome", "glass", "neon_ring", "comet", "lava", "rainbow"];
pub const SHAPE_FAVOURITES: [&str; 7] = [solid, neon_tube, glass, glow_ring, sunset, rainbow_flow, hit_ripple];
pub fn all() -> &'static [ReadyLook];         // balls then shapes, family order, as listed in the spec
pub fn find(id: &str) -> Option<&'static ReadyLook>;
pub fn wears(kind: &str) -> Option<Wears>;    // has_balls -> Balls, shape::KIND -> Shape, else None
pub fn for_kind(kind: &str) -> impl Iterator<Item = &'static ReadyLook>;
pub fn families(w: Wears) -> &'static [&'static str];
pub fn own<T: Settings + Default>(s: &T) -> Params;  // to_params minus keys equal to T::default().to_params()
```

`impl ReadyLook { pub fn recipe(&self, colours: Option<&Gradient>) -> Recipe }` passes `colours` or
`Gradient::solid(self.main)`. `pub fn comes_as(&self, reg) -> String` ("Body, Shine, Outline, Flash on hit": kind
names of the recipe's top-level layers in order, deduplicated).

### 1.2 `balls.rs` and `shapes.rs`: the recipes

Helpers: `layer(kind, params)`, `.with(sub)`, `body(colours, opacity)`, `solid(c)`, `even(&[c...])`. Every ball recipe
starts with a Body layer (`layers::BODY`); every shape recipe starts with the shape Body (`shape::BODY`, no params).
`k` below is the main colours passed in. Starting values (tuned by looking at the pictures, step 5):

Balls (Clean): Plain white = Body white. Pastel = Body k (main pastel pink), Shine Matte strength low. Candy = Body k
(pink), Shine Glossy. Tennis = Body k (yellow-green), Pattern Swirl shade of own colour light, Shine Matte. Ink = Body
near-black (fixed), Outline Own k (white main) width 0.12. Billiard = Body k (red), Pattern Dots one big white spot
(Pattern Dots size large) or Halves white band, Shine Glossy.

Balls (Shiny): Chrome = Body grey gradient, Shine Metal strong, Outline thin dark, Flash on hit white (fixed colours).
Gold = Body gold k, Shine Metal light Own warm, Glow small warm. Steel = Body blue-grey k, Shine Metal, Outline thin.
Glass = Body k low opacity (0.25), Shine Glass, Outline thin light. Marble = Glass + Pattern Swirl own gradient k.
Pearl = Body pale k, Shine Glossy soft wide, Pattern Gradient faint pastel.

Balls (Neon): Neon ring = Body near-black, Outline Own k centre width ~0.18, Glow Own k. Glow orb = Body k with
Glow sub-layer on Body is not allowed (Body takes only light/heat), so Body white→k radial (Body Colours gradient
across each ball), Glow Own k big. Electric = Body violet k, Glow Own k, Electric arcs layer colour k. Laser = Body k,
Glow small, Trail Streak thin long colour k. Pulse = Body k, Glow Own k with an `RPulse` on Glow `strength`
(Follow sound, `PulseSource::Sound { shape: ROOT, source: SoundSource::Bus(1) }`, amount +1.5, range (0, 2)). Ghost =
Body pale (opacity 0.85), Trail Ghosts.

Balls (Fire): Comet = Body hot white→orange, Glow orange, Trail Ribbon/Streak orange fading. Lava = Body orange→red,
Pattern Swirl dark, Glow orange. Ember = Body dark red, Pattern Dots orange, Trail Smoke grey. Sparks = Body yellow,
Shine Glossy, Particles On hit Sparks yellow. Heat = Body orange, Heat glow sub-layer on Body (glows with hits/speed).
Fireball = Body white→orange→red, Glow orange big, Trail Streak orange, Gives off light sub on Body (only lights a
lit scene).

Balls (Trippy): Rainbow = Body k, Colour over time. Hypno = Body white, Pattern Stripes rings (Gradient form Rings
black/white, repeat), Turning. Liquid = Body, Shader `liquid`. Glitch = Body light, Shader `color_split`. Bubble =
Body clear (opacity 0.15), Shine Glass, Pattern Gradient rainbow faint (Overlay), Outline thin white. Kaleido = Body
gradient k, Shader `kaleidoscope`, Turning.

Shapes (Clean): Solid = Colour and gradient k. Outline = Colour and gradient opacity 0, Outline Own k. Pastel =
Colour and gradient pastel k, Shading sub soft. Chalk = Colour opacity 0, Outline Own k width 8 opacity 0.8. Glass =
Colour and gradient pale k opacity 0.25, Outline thin light, card Surface Glass (`surface::preset_values`). Frosted
= Colour pale opacity 0.4, Outline thin white.

Shapes (Glow): Neon tube = Colour dark, Outline Own k centre 6 px, Glow Own k. Glow ring = Colour opacity 0, Outline
Own k, Glow Own k big, Gives off light sub (shape). Laser = Colour opacity 0, Outline white thin with Glow sub violet.
Hit ripple = Colour k, Outline white with a Flash sub (flashes when hit). Chrome edge = Colour near-black, Outline
grey gradient wide with Shine sub. Gold edge = same in gold.

Shapes (Colour): Sunset = Colour gradient pink→orange→yellow (fixed). Ocean = teal→blue (fixed). Rainbow flow =
rainbow gradient with Flow 0.3 (fixed). Aurora = Colour gradient with Shader `liquid_light` (fixed). Plasma = Shader
`hue_storm`/`liquid` (fixed). Sparks = Colour k, Particles (shape) On hit Sparks.

Colour rules: `takes_colour` is true where `k` is used; fixed-colour looks ignore it (Chrome, Rainbow, Sunset…).

### 1.3 `apply.rs`

```rust
pub enum Colours { Look, Mine }
pub enum How { Replace, OnTop }
pub fn own_colours(scene: &Scene, reg: &Registry, thing: ThingId) -> Option<Gradient>;
pub fn copied(recipe: &Recipe) -> CopiedLook;           // ids 0.. in flat order, subs nested
pub fn apply(scene, reg, thing, look: &ReadyLook, colours: Colours, how: How) -> Result<usize, EditError>;
pub fn add_with_look(scene, reg, kind, parent, look) -> Result<ThingId, EditError>;
```

- `own_colours`: balls: the Body layer's `colours` value from the Look card's params (`layered_key(body.id,
  "colours")`), else the Body default (white). Shapes: the lowest top-level `shape::PAINT` layer's `colours`; `None`
  when there is none. Read with `Scene::value_at_time`? No: the stored value (keys are not followed; a keyframed colour
  gives its current base value from params). Use `card.params.get(key)` falling back to the kind default.
- `apply` with `Colours::Mine` and `own_colours == None` or `!look.takes_colour` uses the look's own colours.
- Replace: `recipe` → `copied()`; for balls, overwrite the Body copy's `ball_shape` and `roundness` with the thing's
  current values (from the card's params; removed when they are the defaults); `scene.paste_look(reg, thing,
  SlotId::Look, &copied)`; then the pulses: the i-th top-level recipe layer is the i-th top-level non-group layer of
  the card after the paste (in card order); insert `Address { thing, slot: Some(Look), key: layered_key(id, key) }`
  → pulse. Returns the number of layers.
- OnTop: for each top-level recipe layer but the base: `scene.paste_layer` (subs come with it); map pulses by the
  returned ids. The card's own settings are left. Returns the number of layers added.
- Refusals: the thing has no Look card or its card kind isn't the look's (`EditError::NotAccepted`).
- `add_with_look`: `starters::add_thing` then `apply(Replace, Colours::Look)`.

### 1.4 Tests (`ready_looks/tests.rs`)

1. `catalogue_is_whole`: 30 ball + 18 shape looks; ids and names unique per wears; six per family; favourites found
   and of the right wears; every family listed.
2. `every_look_pastes_cleanly`: for Balls, Emitter (ball looks) and Shape (shape looks): add the thing, apply
   Replace (Look and Mine), resolve, no problems for the thing; the card's layer kinds equal the recipe's.
3. `every_stored_key_is_a_setting`: for each recipe layer and sub, every key in params passes
   `bpe_core::layer_def(kind, get, key)` (the dynamic defs include the Shader's effect settings).
4. `keep_mine_puts_the_colours_in`: a ball with a pink Body, Neon ring Mine → its Outline colour is pink; Chrome Mine
   = Chrome Look.
5. `ball_keeps_its_shape`: Body Square, roundness 0.4 → after Chrome the Body is still Square 0.4.
6. `group_layers_stay`: a Balls with a group gradient keeps it after Replace.
7. `on_top_keeps_the_stack`: Outline then Neon ring On top → Body untouched, old Outline there, new layers above.
8. `pulse_follows_its_layer`: Pulse Replace → a pulse on the Glow's `strength`; Replace with Chrome → gone.
9. `add_with_look_adds_a_dressed_thing`.

Picture test (`crates/bpe-kit/tests/ready_looks_pictures.rs`, `bpe_gfx::test_device()`): each look on a 96 px mini
scene (a ball or the sample rounded rectangle on dark), checks it has pixels brighter than the ground and that no two
looks in a family give the same picture hash.

## 2. bpe-studio

### 2.1 Inspector tab

- `InspectorTab::Looks` (after Settings). `inspector.rs`: chips for Balls / Emitter: Settings | Looks; Shape:
  Settings | Looks | Sound. Text and Particles unchanged. `Inspector::sound_open` becomes `wide_open` (Sound or
  Looks), and `app.rs::inspector_panel` uses it (same `sound_width`, panel id "inspector wide").
- Looks shows `crate::looks_page::show(ui, inspector, scene, kit, &selected, id, time) -> bool`.

### 2.2 `crates/bpe-studio/src/looks_page/`

- `mod.rs`: `pub struct LooksPage { family: Option<&'static str>, find: String, picked: BTreeMap<Wears, &'static str>,
  colours: Colours, view: BallView, shape_view: ShapeView, thumbs: Thumbs }` held in `Inspector.looks`. `show` lays
  out the three columns with `ui.columns`-like manual widths (families 120 px, preview 260 px, the grid the rest; below
  640 px wide the preview goes under the grid). Returns whether the scene changed (it pushes `Edit::ReadyLook` through
  the inspector's edit path so undo and multi-selection work).
- `grid.rs`: the Find box (labelled), tiles 96 px (picture 88×64 + name), `Sense::click()`, hovered → `hover`,
  clicked → picked. kittest names each tile button by the look's name.
- `preview.rs`: balls: `ball_view::show_mini` (new: `show` split so it takes a prepared mini scene) with the mini
  scene from `ball_view::mini_scene` and the look applied with `ready_looks::apply` on the mini. Shapes: `ShapeView`
  — a mini scene with a copy of the shape at the origin, camera fitted to its outline's bounds (from the shape's
  points; `things::shape` has the outline), Still / Hit (Hit pushes a Hit event with `b = shape`). Name, help, Comes
  as, Colours chips (Keep mine greyed with hover note), "Use this look", "Add its layers on top", the line under them.
- `thumbs.rs`: `Thumbs { renderer: Option<Renderer>, tiles: HashMap<Key, Tile> }`, `Key = (look id, colours hash,
  ball shape hash, pixels)`; `Tile { texture: wgpu::Texture, id: egui::TextureId }`. Each frame renders at most
  `PER_FRAME = 4` missing tiles: builds the mini scene (a white ball radius 24, or the sample shape: a 300×180 Box with
  roundness), applies the look, builds the frame Still, renders with the shared renderer, copies the output texture
  into the tile's texture (`copy_texture_to_texture`), and asks for a repaint while tiles are missing. No GPU: tiles
  paint the look's two swatch colours as a disc (or a rounded rectangle) with `// scene colour`.

### 2.3 Edits

`Edit::ReadyLook(&'static str, Colours, How)`: applied per selected thing that can wear the look (same filter as
`PasteLook`: things whose Look card kind matches), clears `selected_layer` (Replace), status line says "Chrome on
3 things" when more than one. One undo step.

### 2.4 Add thing menu (`tree.rs`)

Balls, Emitter and Shape become `ui.menu_button(kind.name(), ..)`: "Plain" (Action::Add), separator, favourites
(`Action::AddWithLook(kind, id)`, each with a painted swatch), separator, "More looks…" (`Action::AddForLooks(kind)`:
adds the plain thing, selects it and sets its inspector tab to Looks; `TreeOutcome.open_looks = Some(id)` read by the
app). Update existing tree tests that clicked "Balls" to click Balls ▸ Plain.

### 2.5 Studio tests (kittest)

1. Looks chip shows for Balls, Emitter, Shape; not for Text.
2. Pick Chrome, Use this look → the card's top-level kinds are Chrome's; one undo puts the old stack back.
3. Add an Outline, then Neon ring "Add its layers on top" → Outline still there.
4. Keep mine is disabled for Chrome, enabled for Neon ring.
5. Add thing ▸ Balls ▸ Chrome adds a ball wearing Chrome; ▸ More looks… opens the Looks tab.
6. Find "neon" leaves only neon-named or described tiles.

## 3. Full-size look check

Add a `ready_looks` scene to `crates/bpe-kit/tests/full_size_look.rs` (balls of every ball look in a 6×5 grid on dark,
then a column of shapes), run `LOOK_ONLY=ready_looks`, look at the full picture and 1:1 crops for jagged edges,
banding, leaks, and that every look matches its description; tune the recipes until they do.

## 4. Docs

- `docs/notes/editor.md`: "Ready-made looks" as-built section (Looks tab, thumbs, add menu).
- `docs/notes/engine.md`: `ready_looks` under bpe-kit.
- Spec and this plan get an "As built" section at the end.

## 5. Verification

- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`.
- `cargo test -p bpe-kit ready_looks`, the picture test, `cargo test -p bpe-studio` (kittest), then the whole workspace
  in the background with `--test-threads=2`. Goldens must be unchanged (no physics touched).
- One Opus review of the whole branch before the PR; fix what it finds.
- CI green, then merge (project rule: threads merge their own PRs after checks and review; no exe).

## Things not to do

- Don't store a link to the look in the scene, don't add a look library to `save.rs`.
- Don't add built-in scenes. The `ready_looks` scene lives only in the full-size look test.
- Don't change any layer kind's defaults or drawing.
- Don't add lights or a Lighting card from a look.

## As built (2026-11-08)

- bpe-kit `ready_looks/` as planned (`mod.rs`, `balls.rs`, `shapes.rs`, `apply.rs`, `tests.rs`, 13 tests), plus
  `tests/ready_look_pictures.rs` (every look draws on the dark ground; no two of a family draw the same picture, about 40 s;
  the Liquid shader was dropped partly because its in-ball program took about 9 minutes to build on the software GPU)
  and a
  "ready" group in `tests/full_size_look.rs` (30 balls still and moving, 18 shapes).
- bpe-studio: `looks_page/{mod,grid,preview,shape_view,thumbs,tests}.rs`; `inspector.rs::tabs` builds the chips
  (Settings | Looks for Balls and Emitters, Settings | Looks | Sound for Shapes); `Inspector::wide_open` replaces
  `sound_open`; `ball_view::show_dressed`; `tree.rs::with_looks` with `Action::AddWithLook` / `AddForLooks` and
  `TreeOutcome.looks`. Tree tests that added Balls now click the kind and then Plain.
- Tuned after the full-size check: Hypno's ring size, Glitch gets moving bars under its colour split, Liquid / Aurora /
  Plasma lose their bending shaders, dark fills for the line looks, a stronger Glass tint, a round Frosted fill.
- Notes in `docs/notes/editor.md` and `engine.md` ("Ready-made looks").
