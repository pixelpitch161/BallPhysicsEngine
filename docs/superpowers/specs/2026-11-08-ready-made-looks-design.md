# Ready-made looks for Balls and Shapes: design

Date: 2026-11-08. Finding E12's optional extra in the improvement review
(https://claude.ai/artifact/RpJEYqdjL2NWTPtUzCArYe). Aedan picked "everything but Size, Memory and Build", which puts
this in scope and, for this feature only, overrides the older "no presets" stance. Mockups:
https://claude.ai/artifact/2hT4UEbPJuETxecKtAcbYK. Aedan chose **C (the Look library page) + D (favourite looks in
the Add thing menu)** on 2026-11-08 21:34Z.

## What it is for

A new ball is a plain white dot and a new shape a plain white outline. Getting a good-looking thing today means
building a stack of layers by hand. Ready-made looks give a one-click start: pick "Chrome" or "Neon tube" and the
thing wears a finished stack that can then be changed like any other.

Success: a person who has never opened the Look editor can make a ball look like chrome, glass, neon, fire or
something trippy in two clicks, and a shape look like a neon tube or a gradient, and then keep editing it.

## The rule that keeps "own settings" true

A ready-made look is **pasted** onto the thing as ordinary layers (the same path as "Paste whole look",
`Scene::paste_look`). After that the thing owns every layer and setting; nothing stays linked to the ready-made
look, there is no "reset to look" and no library entry in the scene file. Ctrl+Z takes the paste back. Ready-made
looks are not built-in scenes: nothing is added to a scene unless the person picks a look.

## Where you pick one

### C: the Looks tab (library page)

Balls, Emitters and Shapes get a **Looks** chip next to Settings at the top of the inspector (a Shape's chips
become Settings | Looks | Sound). The Looks tab widens the inspector like the Sound tab (it shares the Sound tab's
wide width) and shows three columns:

1. **Families**: All, then each family with its count. Balls: Clean, Shiny, Neon, Fire, Trippy. Shapes: Clean, Glow,
   Colour.
2. **The grid**: a "Find a look" box, then a tile for every look in the family (filtered by the words typed, which
   must all be in the name or the description). Each tile is a picture of the look on a ball (or on a sample rounded
   rectangle for shapes) on a dark ground, with its name. Hovering a tile shows it in the preview; clicking picks it.
3. **The preview**: the hovered look, else the picked one, worn by this thing's own ball (its own size and shape),
   Still / Moving / Hit for balls, Still / Hit for shapes (the shape's own outline). Under it the look's name, one
   sentence on how it looks, and "Comes as: Body, Shine, Outline, Flash on hit" (its layers' kinds). Then:
   - **Colours**: "The look's" | "Keep mine". Keep mine puts this thing's own colours where the look's main colour
     goes (a ball: its Body's Colours; a shape: its lowest Colour and gradient layer's colours). It is greyed, with a
     hover note, when the look has fixed colours (Chrome, Rainbow) or the thing has no colours of its own yet
     (a shape with no Colour and gradient layer).
   - **Use this look** (the primary button): replaces this thing's look with the picked one.
   - **Add its layers on top**: adds the look's layers above the thing's own, keeping its Body and its own
     settings.
   - A small line: "Replaces Ball 1's look. Ctrl+Z undoes." (or "Adds 3 layers to Ball 1." for the other button).

With several things selected, both buttons apply to every selected thing that can wear the look (balls looks on
Balls and Emitters, shape looks on Shapes), and the line says how many.

### D: the Add thing menu

In "+ Add thing", **Balls**, **Emitter** and **Shape** each open a short submenu instead of adding at once:

- "Plain" first (what a click on the name gave before: a white ball or a white shape),
- a line, then seven favourite looks, each with a small painted swatch of its colours,
- a line, then "More looks…", which adds the plain thing and opens its Looks tab.

Favourites for balls and emitters: Candy, Chrome, Glass, Neon ring, Comet, Lava, Rainbow. For shapes: Solid,
Neon tube, Glass, Glow ring, Sunset, Rainbow flow, Hit ripple. They are fixed in the program (not a setting).

## The looks

30 ball looks (used by Balls and Emitters) and 18 shape looks, each a short recipe of existing layer kinds. Nothing
new is drawn: every look is made of layers the Look editor already offers.

| Family | Ball looks |
| --- | --- |
| Clean | Plain white, Pastel, Candy, Tennis, Ink, Billiard |
| Shiny | Chrome, Gold, Steel, Glass, Marble, Pearl |
| Neon | Neon ring, Glow orb, Electric, Laser, Pulse, Ghost |
| Fire | Comet, Lava, Ember, Sparks, Heat, Fireball |
| Trippy | Rainbow, Hypno, Liquid, Glitch, Bubble, Kaleido |

| Family | Shape looks |
| --- | --- |
| Clean | Solid, Outline, Pastel, Chalk, Glass, Frosted |
| Glow | Neon tube, Glow ring, Laser, Hit ripple, Chrome edge, Gold edge |
| Colour | Sunset, Ocean, Rainbow flow, Aurora, Plasma, Sparks |

Each look has an id (never shown, never saved), a name, a family, a one-sentence description, whether it takes the
thing's colours, its main colour, two swatch colours for the menu, and a recipe. The exact layers of each recipe
are in the plan; they may be tuned while building by looking at the pictures, as long as each keeps its
description true.

Rules every recipe follows:

- **Balls keep their shape.** A ball look sets the Body's colours and opacity but keeps the thing's Body Shape and
  Roundness, so a square ball stays square.
- **Shapes keep their outline.** A shape's outline settings are the thing's own, not the Look's, so a shape look
  never changes the shape.
- **The group stays.** Group layers (the group gradient and group shaders) stay, as with Paste whole look.
- **No scene changes.** A look never adds a Lighting card, a light or another thing. Layers that give off light
  (Fireball, Glow ring) only light the scene when it already has Lighting; they still glow without it.
- **Pulse** follows the music: its glow's Strength follows Bus 1's level (a Follow sound pulse on that layer
  setting, added with the look and taken away with it).
- **Replace vs on top.** Replace swaps the whole stack and the card's own settings (Surface). On top adds the look's
  layers (not its Body) above the thing's own and leaves the card's own settings alone; a reaction the thing already
  has takes the look's settings (a card holds each reaction once). Pulses come with the layers either way.

## How it is built (summary; details in the plan)

- **bpe-kit `ready_looks`**: the catalogue (`balls.rs`, `shapes.rs`) written with the layers' own typed settings
  structs (`BodyLayer`, `ShineLayer`, `PaintLayer`…), so a misspelt setting does not compile; each layer's values
  are stored as only those that differ from its kind's defaults. `apply.rs` turns a recipe into a `CopiedLook` and
  pastes it (Replace) or pastes its layers (On top), maps its pulses onto the pasted layers and keeps the Body's
  shape. `add_with_look` adds a thing wearing a look for the Add thing menu.
- **bpe-studio `looks_page/`**: the Looks tab (families, grid, preview, buttons), the tile pictures (rendered on the
  graphics card a few per frame into small textures and kept until the look, the colours or the ball's shape
  change; without a graphics device tiles are painted swatches), and the preview (the ball view of the Look editor
  showing a small scene that wears the look, and a shape view for shapes).
- **Edits**: one new inspector edit, `Edit::ReadyLook`, one undo step, applied to every selected thing that can
  wear it. The tree gets `Action::AddWithLook` and `Action::AddForLooks`.

## Testing

- bpe-kit: every look builds, pastes onto a new Balls, Emitter and Shape (as fits) with no problems from resolve;
  every stored key is a real setting of its layer kind; names and ids are unique; every family has six looks; the
  favourites exist; Keep mine puts the colours in; the Body keeps its Shape and Roundness; group layers stay; On top
  keeps the thing's layers and Body; Pulse's pulse lands on its glow and goes with a later Replace.
- Pictures (on the graphics device): every look draws something on a dark ground, and no two looks of a family draw
  the same picture.
- Editor (kittest): the Looks chip shows for Balls, Emitters and Shapes and not for Text; picking a tile and Use this
  look changes the stack in one undo step; Add on top keeps the old layers; Keep mine is greyed for Chrome; the Add
  thing menu's Balls ▸ Chrome adds a chrome ball and More looks… opens the Looks tab.
- Full-size look check: a "ready_looks" scene with every ball look in a grid and every shape look, looked at in full
  size before the pull request.

## Not doing

- No saving your own looks, no favourites you pick, no look library in the scene file, no links back to a look.
- No looks for Text or Particles here (they already start from their own looks in the Add thing menu).
- No moving tiles: tiles are still pictures; the preview moves.
- No look that needs the scene to change (lights, lighting, other things).
