# Lighting and picture speed (review W8: S18 to S24)

Aedan, 2026-11-08: picked everything in the code review but the Size section. Workstream W8 of the review's work
plan is about how long lit frames take to draw and how much an export costs. No settings, no scene format, no
physics or sound changes. Every picture stays byte-identical except an export's video colours (S23), which match the
old ones to within ffmpeg's own rounding.

## First, measure

`crates/bpe-kit/tests/light_speed.rs` (ignored) draws a set of lit scenes at 540 x 960 again and again and prints the
mean time a frame (`SPEED_SIZE`, `SPEED_FRAMES`, `SPEED_ONLY`; `SPEED_DUMP` writes each scene's picture as raw RGBA
so two builds compare byte for byte). On the cloud's software GPU (lavapipe) the base draw is 21 to 32 ms, the
lighting's extra redraws of the scene (S18) about 27 ms in the room and 41 ms with 120 lights, and `Lighting::run`
73 ms in the room, about 515 ms on a Stage and 1.5 s with 120 lights. So the shadow work in the light passes is what
to cut, and S18 is a small share.

## S18: draw the scene's light pictures in one pass — skipped

Aedan chose to skip it for now (2026-11-09). The repeated `draw_scene` calls per mode (`Globals.light.x`) could
become one pass writing several targets; the measurements say it would save under a tenth of a lit frame.

## S19: each screen tile lists the lights that reach it

`LightTiles::build` (CPU, `lighting.rs`) bins the lights into 32 x 32-pixel tiles from 8 lights on (`TILED_FROM`):
a sun goes in every tile, a light with no reach in none, a point or spot by (place, Reach), an area by Reach plus its
half diagonal, a strip or laser by a circle round each segment. Radii use the larger scale and a 2-pixel margin; the
camera's turn and zoom are applied; anything not finite goes in every tile. The lists (heads, then each tile's
ascending light numbers) are packed as numbers after the light links in the `links` storage buffer, because the main
pass already uses all eight storage buffers. `lights_near` / `light_in` in `lighting.wgsl` replace the loop over
every light in `lamps`, `shade`, `hit_light` and `lamps_on`; with no lists (`tiles.z == 0`) they walk every light, as
before. Since a light adds nothing beyond its Reach (`falloff`), the picture is the same.

The Lights tab shows a warning above 64 lights (`lights_panel::many_lights_warning`, `MANY_LIGHTS` in `app.rs`).

## S20: Stage skips the Flat shadows it never shows

`lamps` (the direct pass) works a light's shadow on the picture's surface only when something reads it: Flat, or a
Stage with Glow in the dark, light memory or rainbows (`surface_wanted`); its UV share only for UV pictures or memory
(`uv_wanted`); its air only with haze. A Stage shades its own surfaces in the mix (`shade`).

## S21: gradient rows kept between frames

`GradientAtlas` keeps a key per row; the renderer's `PaintTexture` uploads only runs of rows whose key changed. Baked
rows sit in a thread-local cache (`BAKED`, at most 512).

## S22: copy only what animates

`Resolved.things` holds `Arc<ResolvedThing>`; applying keyframes, pulses, gradient moves, rules or soloed lights
copies a thing (`Arc::make_mut`) only when something on it changes.

## S23: video colours on the graphics card

`bpe_gfx::yuv`: `PixelLayout` (`Rgba`, `Yuv420p` for x264 and NVENC, `Nv12` for AMF and Quick Sync) and
`VideoPlanes`, two passes (`shaders/yuv.wgsl`) that turn the output picture into BT.709 TV-range planes, the chroma
sited left as H.264 expects by default (its pair's two rows, across columns weighted 1/4, 1/2, 1/4). `Readback::with_layout` reads the planes back (falls back to RGBA for odd
sizes). The export pipes them as `-pix_fmt yuv420p`/`nv12` with BT.709 TV-range tags: 1.5 bytes a pixel instead of
4, and no conversion on ffmpeg's CPU. `yuv_of_rgba` is the CPU reference the tests compare with.

## S24: smaller costs

- Bloom's down and up chain is skipped when the frame's bloom is 0 (its first picture cleared once).
- `Lighting` keeps its bind groups between frames (`BindCache`, keyed by the layout and what each entry binds; each
  frame drops the groups the last one didn't use, so a buffer that grew isn't kept alive; at most 256, cleared when
  its pictures are remade).
- The UV and tag pictures exist only while the scene uses them (`keep_uv_and_tags`).

## Checks

- `lit_pictures.rs::tiled_lights_draw_the_same_picture_as_every_light_everywhere`: 24 ball lights and one of each
  other kind, Flat and Stage, tiles on and off byte-identical; a second frame makes no new bind groups. Halving the
  tiles' radius makes it fail.
- `lighting::tile_tests`: the lists' order, the sun, no reach, off picture, a turned and zoomed camera.
- `output_tests::video_colours_from_the_card_match_the_cpus` (both layouts, two sizes, within 1) and
  `odd_sizes_come_back_as_rgba`; `bpe-cli/tests/export.rs::exported_colours_are_the_frames_own` (a decoded export
  against the CPU's planes, and its colour tags).
- `paint_tests::rows_kept_between_frames_draw_as_a_fresh_renderer_does`;
  `lights_panel::tests::many_lights_show_a_warning_on_the_lights_tab`.
- Goldens unchanged (nothing moves balls).

## As built (2026-11-09)

Before and after pictures of all 12 speed scenes are byte-identical. Frame times at 540 x 960 on lavapipe, before
and after: Stage 545 to 292 ms, shiny surfaces 715 to 432 ms, 120 lights on a Stage 1505 to 1058 ms, 120 lights of
short reach 472 to 359 ms, 120 lights that each reach most of the picture 1615 to 1515 ms (every light really
reaches most pixels there). The review's three small points were taken: a strip whose segments aren't all in the
buffer goes in every tile, unused bind groups are dropped each frame, and the chroma is sited left. Left: Flat shiny pixels still work a light's shadow out twice (the direct pass has no
surface table bound), and S18.
