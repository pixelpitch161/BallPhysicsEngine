# Plan: Liquid and smoke lit by real lights (W15, G11)

Spec: `docs/superpowers/specs/2026-11-08-liquid-smoke-real-light-design.md`. Read `docs/notes/lighting.md` (parts 1a,
2a, 2b) and `docs/notes/particles.md` (parts 5, 6, 7) first.

## Step 1: shared Surface on the Liquid layer (`bpe-kit`)

`crates/bpe-kit/src/things/particles/liquid.rs`:

- Remove the liquid's own `Surface` enum. `LiquidLayer.surface` becomes `SurfaceKind` (default Plastic), group
  "Surface". Remove `bends_light` (the Surface's is used). Rename the pretend `rim` to key `own_rim` (label "Rim
  light"). Light angle, Shine, Rim light, Bend, Tint, Sky top, Sky bottom move to group "Without lighting", each help
  line ending "(only without a Lighting card, or at Lit 0)". Bend and Tint: `when = "surface=glass|frosted|prism"`.
- `pub fn pretend_code(kind: SurfaceKind) -> f32`: the 6 old codes (Matte 0, Plastic 1, Chrome 2, Metal 3, Glass 4,
  Toon 5) for the 15 kinds as the spec maps them.
- `pub fn defs() -> &'static [SettingDef]` (LazyLock): `LiquidLayer::defs()` with the Surface numbers
  (`surface::sub_defs()` minus `surface`) inserted right after the `surface` def, their defaults set to
  `preset_values(SurfaceKind::Plastic)`. The layer kind in `look.rs::kinds()` uses this instead of
  `LiquidLayer::defs`.
- `pub fn surface_of(p: &Params) -> SurfaceSettings` (`SurfaceSettings::from_params`).
- `pub fn params_of(l: &LiquidLayer) -> Params`: `to_params()` plus `preset_values(l.surface)` (for looks and tests,
  so a Liquid made as Glass has Glass's numbers, as the editor's pick writes them).
- `pub struct RealLight { pub lit: f32, pub stamp: f32, pub blocks: f32 }` and
  `style(&self, opacity, surface: &SurfaceSettings, real: Option<RealLight>) -> LiquidStyle`: `a.w` the pretend code,
  `c.x` own_rim, `c.z` bends: real → 0 (a bender does it); pretend → code Glass and the Surface's
  Bends light above `BENDS_FROM`; else 0. `d = [lit, stamp, blocks, 0]` when real, all 0 otherwise.
- Update `looks.rs` (Liquid metal, Liquid splashes, Paint drips) to `params_of`, and every test that builds a
  `LiquidLayer` (`tests_part6.rs`, `lighting.rs` test, `tests/liquid_pictures.rs`, `tests/bent_light_pictures.rs`).

`crates/bpe-kit/src/modules/look/surface.rs`:

- `pub fn taken(scene: &Resolved) -> u32`: how many table indices things and Surface sub-layers use (the count of
  things with a surfaced Look plus every such Look's `layer_surfaces`).

`crates/bpe-kit/src/things/particles/liquid.rs` (index):

- `pub fn surface_index(scene: &Resolved, id: ThingId, layer: LayerId) -> Option<u32>`: `taken(scene)` + its place
  (from 1) among every Liquid layer (`kind == LIQUID`, top level) of every Particles Look card, in thing then layer
  order.

## Step 2: drawing it (`bpe-kit/src/things/particles/draw.rs`)

- `fn lit_scene(cx) -> bool`: the root has a `SlotId::Lighting` card.
- `liquid_of`: real when `lit_scene && layer.light[0] > 0`: `RealLight { lit: layer.light[0], stamp:
  surface::stamp(index, &s), blocks: if layer.light[1] > 0 { keeps_of(passes(&s)) } else { 0 } }`.
- `impl ThingDraw for ParticlesDraw { fn light(..) }`: for each shown Liquid layer (`layers(cx.resolved)`) with an
  index: `scene.set_surface(index, s.gpu(SurfaceGpu::SHEET))`, `scene.refracts |= s.see_through()`. When it is real, blocks light and `surface::bends(s)`: `scene.benders.extend(Bender::round(blob boxes, 2, index))`.
- Smoke: in `draw_around`, after `f.look.light = ..`: when the field is smoke and `lit_scene && layer.light[0] > 0`,
  `f.look.shade[3] = SMOKE_RELIEF * shading` (`SMOKE_RELIEF` 40.0 world px, in draw.rs). Smoke's `light_from` and
  `shading` help lines updated in `look.rs`.

## Step 3: the renderer (`bpe-gfx`)

`liquid.rs`: `LiquidStyle.d: [f32; 4]` (lit, surface stamp, blocks, spare), `LiquidStyle::real()`; `glass()` false
when real. `shaders/liquid.wgsl`: `Style.d`; in `fs_surface`, when `st.d.x > 0`: the passes as the spec says (picture:
plain colour; 1: colour × lit; 2: (blocks, h, index) × shown, shown, blocks whole on a Stage (`g.light.z`); 3/5/6: as
now; 8: nothing; 9: nothing (the light hook's bender bends lamp light); −1: nothing when the stamp has the glass flag). The height
`h = roundness · √(1 − (1 − x)²)`, the same `x` the pretend normal uses. Pass 9 drops its `code == GLASS` test
(the kit only sets `c.z` when it should bend).

`shaders/fields.wgsl`: smoke reads `look.shade.w` (relief). Relief above 0: `facing` is 0 in the picture; in
`fs_field`, pass 2 (`1.5 < g.light.x < 2.5`, not adding) returns `(blocks·a, relief·a·a, 0, a)` with
`a = clamp(c.a, 0, 1)` (blocks whole on a Stage); other passes go through `light_flat` as now.
`frame.rs`: `FieldLook.shade` doc gains the relief.

## Step 4: tests

- `bpe-kit/src/things/particles/tests_part6.rs` (or a new `tests_w15.rs` beside it, `#[cfg(test)] mod`): the Liquid
  kind's defs have every Surface number with Plastic's default and no key twice (against the layer's dynamic and
  light defs too); `pretend_code` maps each kind; a scene with a ball, a shape with a Surface sub-layer and two
  Particles things with Liquids numbers them `taken + 1`, `taken + 2`; the light hook puts a SHEET surface at that
  index; `style` is all pretend (d zero) without a Lighting card and real with one.
- `bpe-kit/tests/lit_liquid_pictures.rs` (GPU, `test_device()`, 270 × 480): the spec's checks.
- Existing: `liquid_pictures.rs`, `bent_light_pictures.rs`, `particle_pictures.rs`, `particle_light_pictures.rs`,
  `unlit_pictures.rs`, `restarts.rs` must pass unchanged in meaning.
- `full_size_look.rs`: a `lit-liquid-smoke` shot in `demo_shots` (a Plastic and a Glass puddle, smoke, a coloured lamp
  to one side, a ball casting a shadow over the liquid). Run it, look at the full pictures and crops.

## Not to do

No golden changes (drawing only). Don't touch Flame. Don't make Liquid's light passes write tags/UV. Don't share any
ProgramStore across devices. Don't change unlit pictures.

## As built

Built as planned, with one change: a lit glass liquid is **not** free glass in pass 9. Free glass writes its own code
into the surface picture's blue channel, over the liquid's surface index, so the mix saw a matte surface and never
showed the picture behind it. Instead `LiquidStyle.c.z` is 0 when real, the shader's real pass 9 writes nothing, and
the Particles `light` hook adds `Bender::round(blob boxes, 2, index)` for a real liquid that blocks light and whose
Surface bends light (as a glass shape needs Blocks light to bend lamp light). `Frame::free_glass_box` now picks
liquids by `c.z` alone. The tests went into `tests_real_light.rs` (6) and `tests/lit_liquid_pictures.rs` (7);
`bent_light_pictures.rs`'s glass liquid now blocks light. `full_size_look.rs` gained `lit-liquid-smoke`. Notes:
`docs/notes/lighting.md` ("Liquid and Soft smoke lit by the real lights") and `docs/notes/particles.md` part 6.

After the review: the real pass 8 writes `tint_out` (a liquid's shadow vanished on a Stage when anything else turned
the tint picture on) and `see_through_blockers` counts liquids; pass 6 writes the link number (`d.w`); smoke stands
up only Behind and not Add (front smoke turned the surfaces under it matte; adding smoke went flat); Throws light,
Facets and Facet gaps are left out of the liquid's numbers.
