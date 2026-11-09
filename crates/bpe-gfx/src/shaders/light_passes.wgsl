// The light passes (lighting, see the renderer's `light_pass` module, which has the same
// numbers). With a Lighting card the renderer draws the things again, once per light pass, each
// time into a picture of its own; `Globals.light.x` says which, read through `light_pass`.
// Pasted before tint.wgsl into every shader that draws things.

// The picture without what is see-through: what glass shows behind it (lighting part 2b). The
// renderer's pass 7 draws with this number.
const PASS_BEHIND: i32 = -1;
// Not a light pass: the picture as always.
const PASS_PICTURE: i32 = 0;
// Each thing's colour times its Lit.
const PASS_LIT: i32 = 1;
// How much each thing blocks light, its height and its cover.
const PASS_SURFACE: i32 = 2;
// The light things give off.
const PASS_EMISSION: i32 = 3;
// The light the background gives off, alone.
const PASS_BACKGROUND: i32 = 4;
// What glows under UV light (lighting part 4).
const PASS_UV: i32 = 5;
// The tags: what catches beams and each thing's link number (lighting part 4).
const PASS_TAGS: i32 = 6;
// The tint picture: how deep each see-through thing's colour is (glass and light part 1).
const PASS_TINT: i32 = 8;
// The glass picture: free glass that bends lamp light (glass and light part 3).
const PASS_GLASS: i32 = 9;

// The light pass `x` (`g.light.x`) names: always a whole number, so rounding only turns it into one.
fn light_pass(x: f32) -> i32 {
    return i32(round(x));
}

