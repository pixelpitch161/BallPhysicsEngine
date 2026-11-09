# Liquid and smoke lit by real lights (workstream W15, review finding G11)

Approved by Aedan on 2026-11-08 as part of the code review's work plan ("everything but Size"). The finding:
**Liquid and Soft smoke use their own pretend light.** They have their own light angle (Liquid's Light angle,
Smoke's Light from) and Liquid has its own 6-chip Surface, so in a scene with a Lighting card they ignore the real
lights and the 15-chip Surface system. The fix: when the scene has Lighting, shade them from the real lights and reuse
the Surface chips.

## What changes for the owner

- **Liquid** gets the same **Surface** as balls and shapes: the 15 chips (Matte, Plastic, Chrome, Metal, Glass, Toon,
  Jelly, Pearl, Flake, Brushed, Frosted, Velvet, Lava, Prism, Mirror ball) with their numbers underneath (Roughness,
  Metal, Clear coat, Bends light, See-through, Rim, Soak, Film, ...). Picking a chip sets the numbers, as everywhere
  else. A new Liquid starts as Plastic, as before.
- In a scene **with a Lighting card**, a Liquid is lit by the real lights: its rounded edges turn to the lamps (a lamp
  on the left lights the left edges), it takes their colour, it sits in the shadows of balls and shapes, it shines
  with the real lights' highlights and reflects the room and sky the Lighting card sets, Glass and Frosted show what is
  behind them bent, Prism splits it, and so on: everything a ball's Surface does that makes sense on a flat-lying
  surface. Its **Lighting** fold now does what it says: **Lit** (0 keeps its own look), **Blocks light** (the liquid
  casts shadows; off by default).
- In a scene **without a Lighting card** (or a Liquid at Lit 0), it looks exactly as it does today: the pretend light.
  Those settings move into their own fold, **Without lighting**: Light angle, Shine, Rim light, Bend, Tint, Sky top,
  Sky bottom, with a help line saying they're used only when the scene has no lighting. The newer chips pick the
  nearest old look there (Jelly, Pearl, Flake, Lava as Plastic; Brushed as Metal; Frosted and Prism as Glass; Velvet as
  Matte; Mirror ball as Chrome).
- **Soft smoke** in a scene with lighting is shaded by the real lights: the side of each puff facing a lamp is lit,
  the far side falls into shade, in the lamp's colour, and it sits in the shadows. **Shading** becomes "how much the
  lights' direction shows on it" (0 flat). **Light from** is used only without lighting (help line says so). Dark
  inside, Thickness and the rest are unchanged. Smoke keeps taking the light flat-on as it already did when Shading is
  0, and at Lit 0 it keeps its own look.
- Nothing changes in scenes without a Lighting card: those pictures stay byte-identical.

## How it works

The lighting system (lighting spec part 1a/2a) lights whatever writes three pictures in the light passes: the lit
colour (pass 1: colour × Lit), the surface (pass 2: R how much it blocks, G its height, B its surface's table index,
A its cover), and nothing in the pass for light given off. The mix then works each pixel's normal from the height's
slope, lights it from every lamp with shadows, and shades it with the surface table's entry (GGX, sky, glass through
the behind picture, toon bands ...). Today a Liquid writes covering black in every light pass (so the mix leaves its
pretend shading as drawn) and Soft smoke writes only its blocking (flat height, no surface), with its pretend shading
baked into its picture.

**Liquid, real-lit** (the scene has a Lighting card and the layer's Lit is above 0; `LiquidStyle.d`):

- Picture pass: its plain colour over its cover, no pretend shading.
- Lit colour pass: colour × Lit over its cover.
- Surface pass: R its Blocks light (times the share its Surface keeps, as `Stamps::apply` does; whole on a Stage),
  G its height, B its surface index (glass flag taken off), all times its cover. The height is the same rounded edge
  its pretend normal used: a quarter circle of radius Roundness, rising over the first Roundness pixels in from the
  edge and flat inside (h = R·√(1 − (1 − x)²), x the share of Roundness in). So the real lights see the same shape
  the pretend light did.
- Light-given-off, UV and tag passes: covering black and its whole cover, as now.
- The picture behind glass (pass −1): left out when its Surface is see-through (as glass records are).
- The glass picture (pass 9): nothing. Free glass there would write over its surface index, so the mix would no
  longer know it is glass. Instead, when its Surface bends light (`surface::bends`) and it blocks light, the light
  hook adds a bender round its blobs with its surface index, so lamp light bends through it (caustics) exactly as it
  does through a glass shape that blocks light.
- Its Glass no longer copies the picture under it in the picture pass (the mix shows what is behind).

**Surface table entry**: each Liquid layer of a Particles thing gets an index numbered after every thing's surface and
every Surface sub-layer's (`surface::taken`), in thing order then layer order over every Liquid layer of a Particles
Look (shown or not, so the draw and the light hook agree from the scene alone). The Particles thing's `light` hook puts
`SurfaceSettings::gpu(SurfaceGpu::SHEET)` there and marks the frame as refracting when see-through.

**Soft smoke, real-lit** (the scene has a Lighting card, Lit above 0 and Shading above 0; `FieldLook.shade.w` its
relief): the picture pass drops the pretend facing term (Dark inside stays); the surface pass writes R its blocking,
G a height of relief × cover (relief = Shading × 40 world px), B 0 (a matte sheet), A its cover. The lit colour pass is
unchanged (colour × Lit).

## Edge cases and decisions

- **Lit 0** keeps the pretend look in a lit scene (the meaning of Lit 0 everywhere: keeps its own colour).
- **A Liquid in front of balls** covers their surface where it covers them (it is in front); behind them it is covered.
- **Smoke drawn in front of a ball that blocks light** now covers the ball's blocking where the smoke is thick (one
  blend can't keep R and change G), so that ball's shadow lightens a little under thick front smoke. Smoke behind balls
  (the usual case) is unaffected. Known and accepted.
- **Mirror ball's tiles, Flakes and Brushed** sit in a ball's frame; on a liquid (a sheet) tiles do nothing and flakes
  and brushing read the liquid in screen space. Fine.
- **Stage**: a lit liquid or smoke pixel is a sheet standing at its height in front of the picture plane, not the
  wall behind it.
- **Huge swarm** doesn't draw Liquid or Soft smoke (unchanged).
- **No backward compatibility**: Liquid's old 6-name Surface key now holds a 15-name Surface (the six names are among
  them); its own Bends light and Rim keys are replaced by the Surface's, and its pretend rim is a new key `own_rim`.

## Not in scope

Flame (it gives off its own look), Catches beams / Under UV / Makes rainbows on Liquid (it writes covering black in
those passes as before), coloured shadows from see-through liquid (the tint pass), lit Web lines and Marks.

## Checks

Picture tests (`bpe-kit/tests/lit_liquid_pictures.rs`, `test_device()`): a lamp on the left lights the liquid's left
edge more than its right and moving the lamp changes it; the lamp's colour tints it; Lit 0 keeps the pretend look;
Plastic shines brighter than Matte under the same lamp; lit Glass shows the stripes behind it; Blocks light casts a
shadow; lit smoke is brighter on its lamp side; existing liquid and unlit pictures unchanged. Kit tests: the Liquid's
settings carry the Surface section with Plastic's numbers and no key twice; surface indices come after balls', shapes'
and sub-layer surfaces; the light hook fills the table. Full-size look check: a new `lit-liquid-smoke` scene.
