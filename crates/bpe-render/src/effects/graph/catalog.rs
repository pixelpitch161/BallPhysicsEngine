//! The blocks a shader graph is built from.
//!
//! Every block is a few lines of WGSL with `$name` placeholders: `$input` is replaced by the
//! value flowing into that input, `$output` by the variable the block writes, and `$_temp` by a
//! name private to this block. Positions are centred on the middle of the frame, with 1.0 being
//! the frame height, the same as `to_centered(uv)` in hand-written effects.

/// What flows along a wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Ty {
    Number,
    /// A position or direction (`vec2`).
    Vector,
    /// A linear-light colour (`vec3`).
    Color,
    /// Takes the type of whatever is plugged in: a number, a position or a colour. All the `Any`
    /// inputs and outputs of one block share a type.
    Any,
}

impl Ty {
    pub fn wgsl(self) -> &'static str {
        match self {
            Ty::Number | Ty::Any => "f32",
            Ty::Vector => "vec2<f32>",
            Ty::Color => "vec3<f32>",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Ty::Number => "number",
            Ty::Vector => "position",
            Ty::Color => "colour",
            Ty::Any => "any value",
        }
    }
}

/// What an input uses when nothing is plugged into it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Default {
    Number(f32),
    Vector([f32; 2]),
    Color([f32; 3]),
    /// This pixel's position.
    Position,
    /// The picture coming into the effect, at this pixel.
    Picture,
}

/// How an unplugged number input is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Slider,
    Whole,
    Toggle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    pub name: &'static str,
    pub ty: Ty,
    pub default: Default,
    pub min: f32,
    pub max: f32,
    pub control: Control,
    pub help: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    pub name: &'static str,
    pub ty: Ty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Inputs,
    Music,
    Math,
    Patterns,
    Shapes,
    Warps,
    Colour,
    Blend,
    Output,
}

impl Category {
    pub const ALL: [Category; 9] = [
        Category::Inputs,
        Category::Music,
        Category::Math,
        Category::Patterns,
        Category::Shapes,
        Category::Warps,
        Category::Colour,
        Category::Blend,
        Category::Output,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Category::Inputs => "Inputs",
            Category::Music => "Music and hits",
            Category::Math => "Math",
            Category::Patterns => "Patterns",
            Category::Shapes => "Shapes",
            Category::Warps => "Warps",
            Category::Colour => "Colour",
            Category::Blend => "Blend",
            Category::Output => "Output",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub kind: &'static str,
    pub title: &'static str,
    pub category: Category,
    pub help: &'static str,
    pub inputs: Vec<Input>,
    pub outputs: Vec<Output>,
    pub code: &'static str,
    /// Names of [`HELPERS`] the code calls.
    pub helpers: &'static [&'static str],
}

impl Block {
    pub fn input(&self, name: &str) -> Option<&Input> {
        self.inputs.iter().find(|i| i.name == name)
    }
}

fn input(name: &'static str, ty: Ty, default: Default, help: &'static str) -> Input {
    Input { name, ty, default, min: -10.0, max: 10.0, control: Control::Slider, help }
}

/// A number slider.
fn num(name: &'static str, default: f32, min: f32, max: f32, help: &'static str) -> Input {
    Input { min, max, ..input(name, Ty::Number, Default::Number(default), help) }
}

/// A whole-number slider.
fn whole(name: &'static str, default: f32, min: f32, max: f32, help: &'static str) -> Input {
    Input { control: Control::Whole, ..num(name, default, min, max, help) }
}

fn toggle(name: &'static str, on: bool, help: &'static str) -> Input {
    Input { control: Control::Toggle, ..num(name, if on { 1.0 } else { 0.0 }, 0.0, 1.0, help) }
}

/// An input of any type, defaulting to a number.
fn any(name: &'static str, default: f32, help: &'static str) -> Input {
    input(name, Ty::Any, Default::Number(default), help)
}

fn any_range(name: &'static str, default: f32, min: f32, max: f32, help: &'static str) -> Input {
    Input { min, max, ..any(name, default, help) }
}

/// A position input that defaults to this pixel.
fn at() -> Input {
    input("at", Ty::Vector, Default::Position, "Where to look. Leave empty for this pixel")
}

fn point(name: &'static str, default: [f32; 2], help: &'static str) -> Input {
    Input { min: -1.0, max: 1.0, ..input(name, Ty::Vector, Default::Vector(default), help) }
}

fn color(name: &'static str, default: [f32; 3], help: &'static str) -> Input {
    input(name, Ty::Color, Default::Color(default), help)
}

/// A colour input that defaults to the incoming picture.
fn picture(name: &'static str) -> Input {
    input(name, Ty::Color, Default::Picture, "The colour to change. Leave empty for the picture")
}

fn out(name: &'static str, ty: Ty) -> Output {
    Output { name, ty }
}

#[allow(clippy::too_many_arguments)]
fn block(
    kind: &'static str,
    title: &'static str,
    category: Category,
    help: &'static str,
    inputs: Vec<Input>,
    outputs: Vec<Output>,
    code: &'static str,
    helpers: &'static [&'static str],
) -> Block {
    Block { kind, title, category, help, inputs, outputs, code, helpers }
}

/// Shared WGSL functions, added to a graph's shader only when one of its blocks needs them.
pub const HELPERS: &[(&str, &str)] = &[
    (
        "px",
        "// One video pixel in centred units.
fn g_px() -> f32 {
    return fx.scale / fx.resolution.y;
}
",
    ),
    (
        "simplex",
        "// 2D simplex noise from -1 to 1.
fn g_simplex(v: vec2<f32>) -> f32 {
    let c = vec4<f32>(0.211324865, 0.366025404, -0.577350269, 0.024390244);
    var i = floor(v + dot(v, c.yy));
    let x0 = v - i + dot(i, c.xx);
    let i1 = select(vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), x0.x > x0.y);
    let x1 = x0 + c.xx - i1;
    let x2 = x0 + c.zz;
    var m = max(0.5 - vec3<f32>(dot(x0, x0), dot(x1, x1), dot(x2, x2)), vec3<f32>(0.0));
    m = m * m * m * m;
    let h0 = hash21(i) * TAU;
    let h1 = hash21(i + i1) * TAU;
    let h2 = hash21(i + 1.0) * TAU;
    let g = vec3<f32>(
        dot(vec2<f32>(cos(h0), sin(h0)), x0),
        dot(vec2<f32>(cos(h1), sin(h1)), x1),
        dot(vec2<f32>(cos(h2), sin(h2)), x2),
    );
    return clamp(dot(m, g) * 99.0, -1.0, 1.0);
}
",
    ),
    (
        "voronoi",
        "// Distance to the nearest cell centre, and a random number for that cell.
fn g_voronoi(p: vec2<f32>, jitter: f32, t: f32) -> vec2<f32> {
    let n = floor(p);
    let f = fract(p);
    var best = vec2<f32>(8.0, 0.0);
    for (var j = -1; j <= 1; j++) {
        for (var i = -1; i <= 1; i++) {
            let g = vec2<f32>(f32(i), f32(j));
            let id = n + g;
            let h = vec2<f32>(hash21(id), hash21(id + 17.31));
            let o = 0.5 + 0.5 * sin(t + TAU * h) ;
            let d = length(g + mix(vec2<f32>(0.5), o, jitter) - f);
            if d < best.x {
                best = vec2<f32>(d, h.x);
            }
        }
    }
    return best;
}
",
    ),
    (
        "polygon",
        "// Signed distance to a regular polygon with `n` sides and outer radius `r`.
fn g_polygon(p: vec2<f32>, r: f32, n: f32) -> f32 {
    let sides = max(round(n), 3.0);
    let a = atan2(p.x, -p.y) + PI;
    let slice = TAU / sides;
    let d = cos(floor(0.5 + a / slice) * slice - a) * length(p);
    return d - r * cos(PI / sides);
}
",
    ),
    (
        "star",
        "// Signed distance to a star with `n` points, outer radius `r` and inner radius `r * inner`.
fn g_star(p: vec2<f32>, r: f32, n: f32, inner: f32) -> f32 {
    let points = max(round(n), 2.0);
    let slice = PI / points;
    var a = atan2(p.x, -p.y) + PI;
    a = abs((a % (2.0 * slice)) - slice);
    let q = length(p) * vec2<f32>(cos(a), sin(a));
    let tip = vec2<f32>(r, 0.0);
    let valley = r * clamp(inner, 0.05, 1.0) * vec2<f32>(cos(slice), sin(slice));
    let e = valley - tip;
    let w = q - tip;
    let h = clamp(dot(w, e) / dot(e, e), 0.0, 1.0);
    let d = length(w - e * h);
    return select(d, -d, e.x * w.y - e.y * w.x > 0.0);
}
",
    ),
];

/// Every block, in palette order.
pub fn blocks() -> Vec<Block> {
    use Category::*;
    use Ty::*;
    vec![
        // ---- Inputs ----
        block(
            "position",
            "Position",
            Inputs,
            "Where this pixel is, measured from the middle of the frame. 1 is the frame height.",
            vec![],
            vec![out("out", Vector)],
            "let $out = q0;",
            &[],
        ),
        block(
            "screen_uv",
            "Screen UV",
            Inputs,
            "Where this pixel is on screen: 0,0 is the top left corner and 1,1 the bottom right.",
            vec![],
            vec![out("out", Vector)],
            "let $out = uv;",
            &[],
        ),
        block(
            "polar",
            "Polar",
            Inputs,
            "Distance from the middle and the angle around it, in turns from 0 to 1.",
            vec![at()],
            vec![out("radius", Number), out("angle", Number)],
            "let $radius = length($at);
let $angle = atan2($at.y, $at.x) / TAU + 0.5;",
            &[],
        ),
        block(
            "time",
            "Time",
            Inputs,
            "Seconds since the video started, times Speed.",
            vec![num("speed", 1.0, -10.0, 10.0, "How fast the time runs")],
            vec![out("out", Number)],
            "let $out = fx.time * $speed;",
            &[],
        ),
        block(
            "aspect",
            "Aspect",
            Inputs,
            "Width of the frame divided by its height (0.5625 for 9:16).",
            vec![],
            vec![out("out", Number)],
            "let $out = aspect();",
            &[],
        ),
        block(
            "picture",
            "Picture",
            Inputs,
            "The picture coming into this effect: the balls, walls and any earlier effects.",
            vec![at()],
            vec![out("out", Color)],
            "let $out = sample(from_centered($at));",
            &[],
        ),
        block(
            "previous_frame",
            "Previous frame",
            Inputs,
            "What this effect showed one frame ago, for trails and feedback loops.",
            vec![at()],
            vec![out("out", Color)],
            "let $out = prev(from_centered($at));",
            &[],
        ),
        block(
            "feedback",
            "Feedback",
            Inputs,
            "The previous frame, zoomed, turned, recoloured and faded a little every frame, the same at any frame rate. Combine it with the picture using Lighten for light tunnels.",
            vec![
                num("fade", 0.9, 0.0, 0.99, "How long the ghosts last"),
                num("zoom", 1.0, 0.9, 1.1, "Size change each frame. Above 1 flies into the screen"),
                num("spin", 0.0, -5.0, 5.0, "Turn each frame, in degrees"),
                num("hue", 0.0, -0.05, 0.05, "Colour change each frame, in turns of the colour wheel"),
                at(),
            ],
            vec![out("out", Color)],
            "let $_steps = fx.dt * 60.0;
let $_q = rotate($at, -radians($spin) * $_steps) / pow(max($zoom, 0.01), $_steps);
let $out = hue_shift(prev(from_centered($_q)), $hue * $_steps) * pow($fade, $_steps);",
            &[],
        ),
        block(
            "number",
            "Number",
            Inputs,
            "A fixed number. Expose it to get a slider on the effect card.",
            vec![num("value", 1.0, -10.0, 10.0, "The number")],
            vec![out("out", Number)],
            "let $out = $value;",
            &[],
        ),
        block(
            "colour",
            "Colour",
            Inputs,
            "A fixed colour.",
            vec![color("color", [1.0, 0.4, 0.1], "The colour")],
            vec![out("out", Color)],
            "let $out = $color;",
            &[],
        ),
        block(
            "point",
            "Point",
            Inputs,
            "A fixed position, from the middle of the frame.",
            vec![
                num("x", 0.0, -1.0, 1.0, "Left (negative) to right"),
                num("y", 0.0, -1.0, 1.0, "Up (negative) to down"),
            ],
            vec![out("out", Vector)],
            "let $out = vec2<f32>($x, $y);",
            &[],
        ),
        // ---- Music and hits ----
        block(
            "music",
            "Music",
            Music,
            "How loud the music is right now in the bass, middle and treble, from 0 to about 1. Follows the song and the bounce sounds together.",
            vec![],
            vec![out("bass", Number), out("mid", Number), out("treble", Number), out("level", Number)],
            "let $bass = fx.audio.x;
let $mid = fx.audio.y;
let $treble = fx.audio.z;
let $level = fx.level;",
            &[],
        ),
        block(
            "beat",
            "Beat",
            Music,
            "Phase goes from 0 to 1 between beats. Hit is 1 on each beat and fades quickly, by Sharpness.",
            vec![num("sharpness", 4.0, 1.0, 16.0, "Higher makes each beat's flash shorter")],
            vec![out("phase", Number), out("hit", Number)],
            "let $phase = fx.audio.w;
let $hit = pow(1.0 - fx.audio.w, $sharpness);",
            &[],
        ),
        block(
            "pulse",
            "Bounce pulse",
            Music,
            "Jumps to 1 when balls bounce and fades back to 0.",
            vec![],
            vec![out("out", Number)],
            "let $out = fx.pulse;",
            &[],
        ),
        block(
            "nearest_hit",
            "Nearest bounce",
            Music,
            "The recent bounce closest to a point: how far away it is, how long ago it was, how hard, and the ball's colour.",
            vec![at()],
            vec![out("distance", Number), out("age", Number), out("strength", Number), out("color", Color)],
            "var $_d = 1000.0;
var $_age = 1000.0;
var $_strength = 0.0;
var $_color = vec3<f32>(0.0);
for (var i = 0u; i < fx.hit_count; i++) {
    let d = length($at - to_centered(hit_uv(i)));
    if d < $_d {
        $_d = d;
        $_age = hit_age(i);
        $_strength = hit_strength(i);
        $_color = hit_color(i);
    }
}
let $distance = $_d;
let $age = $_age;
let $strength = $_strength;
let $color = $_color;",
            &[],
        ),
        block(
            "hit_glow",
            "Bounce glow",
            Music,
            "A soft glow in each ball's colour around every recent bounce, fading with age.",
            vec![
                num("size", 0.05, 0.005, 0.5, "Glow radius, in frame heights"),
                num("life", 0.5, 0.05, 3.0, "Seconds a glow lasts"),
                at(),
            ],
            vec![out("out", Color)],
            "var $_light = vec3<f32>(0.0);
for (var i = 0u; i < fx.hit_count; i++) {
    let d = length($at - to_centered(hit_uv(i))) / max($size, 1e-4);
    let fade = hit_strength(i) * max(1.0 - hit_age(i) / max($life, 1e-3), 0.0);
    $_light += hit_color(i) * exp(-d * d) * fade;
}
let $out = $_light;",
            &[],
        ),
        // ---- Math ----
        block(
            "add",
            "Add",
            Math,
            "A + B.",
            vec![any("a", 0.0, "First value"), any("b", 0.0, "Second value")],
            vec![out("out", Any)],
            "let $out = $a + $b;",
            &[],
        ),
        block(
            "subtract",
            "Subtract",
            Math,
            "A - B.",
            vec![any("a", 0.0, "First value"), any("b", 0.0, "Taken away")],
            vec![out("out", Any)],
            "let $out = $a - $b;",
            &[],
        ),
        block(
            "multiply",
            "Multiply",
            Math,
            "A × B. Multiply a colour by a number to brighten or darken it.",
            vec![any("a", 1.0, "First value"), any("b", 1.0, "Second value")],
            vec![out("out", Any)],
            "let $out = $a * $b;",
            &[],
        ),
        block(
            "divide",
            "Divide",
            Math,
            "A ÷ B. Dividing by zero gives 0.",
            vec![any("a", 1.0, "Value"), any("b", 1.0, "Divided by")],
            vec![out("out", Any)],
            "let $out = select($a / $b, $a * 0.0, $b == $b * 0.0);",
            &[],
        ),
        block(
            "mix",
            "Mix",
            Math,
            "Blends from A (at 0) to B (at 1).",
            vec![any("a", 0.0, "Value at 0"), any("b", 1.0, "Value at 1"), num("t", 0.5, 0.0, 1.0, "How far towards B")],
            vec![out("out", Any)],
            "let $out = mix($a, $b, $t);",
            &[],
        ),
        block(
            "smoothstep",
            "Smooth step",
            Math,
            "0 below Edge 1, 1 above Edge 2, a smooth curve between.",
            vec![any("edge1", 0.0, "Start of the curve"), any("edge2", 1.0, "End of the curve"), any("x", 0.5, "Value")],
            vec![out("out", Any)],
            "let $out = smoothstep($edge1, $edge2, $x);",
            &[],
        ),
        block(
            "step",
            "Step",
            Math,
            "0 below Edge, 1 at or above it.",
            vec![any("edge", 0.5, "Where it switches"), any("x", 0.0, "Value")],
            vec![out("out", Any)],
            "let $out = step($edge, $x);",
            &[],
        ),
        block(
            "sine",
            "Sine wave",
            Math,
            "A wave from 0 to 1: Frequency waves per unit of X, shifted by Phase (in turns).",
            vec![any("x", 0.0, "Input, often Time"), num("frequency", 1.0, 0.0, 20.0, "Waves per unit"), num("phase", 0.0, 0.0, 1.0, "Shift, in turns")],
            vec![out("out", Any)],
            "let $out = 0.5 + 0.5 * sin(($x * $frequency + $phase) * TAU);",
            &[],
        ),
        block(
            "sin",
            "Sin",
            Math,
            "The sine of X, from -1 to 1.",
            vec![any("x", 0.0, "Angle in radians")],
            vec![out("out", Any)],
            "let $out = sin($x);",
            &[],
        ),
        block(
            "cos",
            "Cos",
            Math,
            "The cosine of X, from -1 to 1.",
            vec![any("x", 0.0, "Angle in radians")],
            vec![out("out", Any)],
            "let $out = cos($x);",
            &[],
        ),
        block(
            "fract",
            "Fraction",
            Math,
            "The part after the decimal point, so values repeat from 0 to 1.",
            vec![any("x", 0.0, "Value")],
            vec![out("out", Any)],
            "let $out = fract($x);",
            &[],
        ),
        block(
            "floor",
            "Round down",
            Math,
            "Rounds down to a whole number.",
            vec![any("x", 0.0, "Value")],
            vec![out("out", Any)],
            "let $out = floor($x);",
            &[],
        ),
        block(
            "abs",
            "Absolute",
            Math,
            "Makes negative values positive.",
            vec![any("x", 0.0, "Value")],
            vec![out("out", Any)],
            "let $out = abs($x);",
            &[],
        ),
        block(
            "power",
            "Power",
            Math,
            "X to the power of Exponent. Above 1 sharpens a 0 to 1 value, below 1 softens it.",
            vec![any("x", 0.5, "Value"), any_range("exponent", 2.0, 0.0, 64.0, "Exponent")],
            vec![out("out", Any)],
            "let $out = pow(abs($x), $exponent);",
            &[],
        ),
        block(
            "min",
            "Minimum",
            Math,
            "The smaller of A and B.",
            vec![any("a", 0.0, "First value"), any("b", 1.0, "Second value")],
            vec![out("out", Any)],
            "let $out = min($a, $b);",
            &[],
        ),
        block(
            "max",
            "Maximum",
            Math,
            "The larger of A and B.",
            vec![any("a", 0.0, "First value"), any("b", 1.0, "Second value")],
            vec![out("out", Any)],
            "let $out = max($a, $b);",
            &[],
        ),
        block(
            "clamp",
            "Clamp",
            Math,
            "Keeps X between Low and High.",
            vec![any("x", 0.5, "Value"), any("low", 0.0, "Lowest allowed"), any("high", 1.0, "Highest allowed")],
            vec![out("out", Any)],
            "let $out = clamp($x, min($low, $high), max($low, $high));",
            &[],
        ),
        block(
            "remap",
            "Remap",
            Math,
            "Maps X from the range In low to In high onto Out low to Out high.",
            vec![
                any("x", 0.5, "Value"),
                any("in_low", 0.0, "Input range start"),
                any("in_high", 1.0, "Input range end"),
                any("out_low", 0.0, "Output range start"),
                any("out_high", 1.0, "Output range end"),
            ],
            vec![out("out", Any)],
            "let $_span = $in_high - $in_low;
let $_t = select(($x - $in_low) / $_span, $_span * 0.0, $_span == $_span * 0.0);
let $out = $out_low + ($out_high - $out_low) * $_t;",
            &[],
        ),
        block(
            "length",
            "Length",
            Math,
            "How long a position is: its distance from the middle.",
            vec![point("v", [0.0, 0.0], "Position")],
            vec![out("out", Number)],
            "let $out = length($v);",
            &[],
        ),
        block(
            "distance",
            "Distance",
            Math,
            "Distance between two positions.",
            vec![at(), point("to", [0.0, 0.0], "Other position")],
            vec![out("out", Number)],
            "let $out = distance($at, $to);",
            &[],
        ),
        block(
            "dot",
            "Dot",
            Math,
            "The dot product of two positions.",
            vec![point("a", [0.0, 0.0], "First"), point("b", [1.0, 0.0], "Second")],
            vec![out("out", Number)],
            "let $out = dot($a, $b);",
            &[],
        ),
        block(
            "combine",
            "Make position",
            Math,
            "Builds a position from X and Y.",
            vec![num("x", 0.0, -1.0, 1.0, "X"), num("y", 0.0, -1.0, 1.0, "Y")],
            vec![out("out", Vector)],
            "let $out = vec2<f32>($x, $y);",
            &[],
        ),
        block(
            "split",
            "Split position",
            Math,
            "Takes X and Y out of a position.",
            vec![at()],
            vec![out("x", Number), out("y", Number)],
            "let $x = $at.x;
let $y = $at.y;",
            &[],
        ),
        block(
            "split_colour",
            "Split colour",
            Math,
            "Takes the red, green and blue out of a colour, and its brightness.",
            vec![picture("color")],
            vec![out("r", Number), out("g", Number), out("b", Number), out("brightness", Number)],
            "let $r = $color.r;
let $g = $color.g;
let $b = $color.b;
let $brightness = luma($color);",
            &[],
        ),
        block(
            "make_colour",
            "Make colour",
            Math,
            "Builds a colour from red, green and blue (0 to 1, higher glows).",
            vec![num("r", 1.0, 0.0, 4.0, "Red"), num("g", 1.0, 0.0, 4.0, "Green"), num("b", 1.0, 0.0, 4.0, "Blue")],
            vec![out("out", Color)],
            "let $out = vec3<f32>($r, $g, $b);",
            &[],
        ),
        // ---- Patterns ----
        block(
            "noise",
            "Value noise",
            Patterns,
            "Soft random blobs from 0 to 1.",
            vec![num("scale", 4.0, 0.1, 50.0, "Blobs per frame height"), num("speed", 0.0, -5.0, 5.0, "How fast it drifts"), at()],
            vec![out("out", Number)],
            "let $out = noise($at * $scale + vec2<f32>(fx.time * $speed, 0.0));",
            &[],
        ),
        block(
            "simplex",
            "Simplex noise",
            Patterns,
            "Smooth natural noise from 0 to 1, with fewer grid lines than value noise.",
            vec![num("scale", 4.0, 0.1, 50.0, "Detail per frame height"), num("speed", 0.0, -5.0, 5.0, "How fast it drifts"), at()],
            vec![out("out", Number)],
            "let $out = 0.5 + 0.5 * g_simplex($at * $scale + vec2<f32>(0.0, fx.time * $speed));",
            &["simplex"],
        ),
        block(
            "fbm",
            "Clouds",
            Patterns,
            "Layered noise (fbm) from 0 to 1, for clouds, smoke and flowing shapes.",
            vec![num("scale", 3.0, 0.1, 30.0, "Size of the clouds; higher makes them smaller"), num("speed", 0.2, -5.0, 5.0, "How fast they drift"), at()],
            vec![out("out", Number)],
            "let $out = fbm($at * $scale + vec2<f32>(fx.time * $speed, -fx.time * $speed * 0.7));",
            &[],
        ),
        block(
            "voronoi",
            "Cells",
            Patterns,
            "Voronoi cells: distance to the nearest cell centre (0 in the middle) and a random value per cell.",
            vec![
                num("scale", 6.0, 0.5, 60.0, "Cells per frame height"),
                num("jitter", 1.0, 0.0, 1.0, "0 is a neat grid, 1 is random"),
                num("speed", 0.5, 0.0, 5.0, "How fast the cells wobble"),
                at(),
            ],
            vec![out("distance", Number), out("cell", Number)],
            "let $_v = g_voronoi($at * $scale, $jitter, fx.time * $speed);
let $distance = $_v.x;
let $cell = $_v.y;",
            &["voronoi"],
        ),
        block(
            "checker",
            "Checker",
            Patterns,
            "A chessboard of 0 and 1 squares.",
            vec![num("scale", 8.0, 0.5, 100.0, "Squares per frame height"), at()],
            vec![out("out", Number)],
            "let $_c = floor($at * $scale);
let $out = abs(($_c.x + $_c.y) % 2.0);",
            &[],
        ),
        block(
            "stripes",
            "Stripes",
            Patterns,
            "Soft stripes from 0 to 1 at any angle, optionally moving.",
            vec![
                num("scale", 10.0, 0.5, 100.0, "Stripes per frame height"),
                num("angle", 0.0, 0.0, 1.0, "Direction, in turns"),
                num("speed", 0.0, -10.0, 10.0, "Stripes per second"),
                num("sharpness", 0.0, 0.0, 1.0, "0 is a soft wave, 1 is hard edges"),
                at(),
            ],
            vec![out("out", Number)],
            "let $_w = 0.5 + 0.5 * sin((rotate($at, -$angle * TAU).x * $scale - fx.time * $speed) * TAU);
let $_k = 0.5 * (1.0 - clamp($sharpness, 0.0, 0.999));
let $out = smoothstep(0.5 - $_k, 0.5 + $_k, $_w);",
            &[],
        ),
        block(
            "rings",
            "Rings",
            Patterns,
            "Circles spreading out from a point, from 0 to 1.",
            vec![
                num("scale", 10.0, 0.5, 100.0, "Rings per frame height"),
                num("speed", 1.0, -10.0, 10.0, "Rings per second; negative moves inward"),
                point("center", [0.0, 0.0], "Middle of the rings"),
                at(),
            ],
            vec![out("out", Number)],
            "let $out = 0.5 + 0.5 * cos((length($at - $center) * $scale - fx.time * $speed) * TAU);",
            &[],
        ),
        block(
            "grid",
            "Grid",
            Patterns,
            "Thin lines in a square grid: 1 on a line, 0 between.",
            vec![num("scale", 8.0, 0.5, 100.0, "Cells per frame height"), num("width", 2.0, 0.5, 20.0, "Line width, in video pixels"), at()],
            vec![out("out", Number)],
            "let $_g = abs(fract($at * $scale - 0.5) - 0.5) / $scale;
let $out = 1.0 - smoothstep($width * 0.5 * g_px(), ($width * 0.5 + 1.0) * g_px(), min($_g.x, $_g.y));",
            &["px"],
        ),
        block(
            "random",
            "Random",
            Patterns,
            "A random number from 0 to 1 for each point, changing Rate times per second (0 never).",
            vec![num("rate", 0.0, 0.0, 60.0, "Changes per second"), at()],
            vec![out("out", Number)],
            "let $out = hash21($at * 1000.0 + floor(fx.time * $rate) * vec2<f32>(37.0, 113.0));",
            &[],
        ),
        // ---- Shapes ----
        block(
            "circle",
            "Circle",
            Shapes,
            "A filled circle: Fill is 1 inside and 0 outside, Distance is how far from the edge (negative inside).",
            vec![
                num("radius", 0.2, 0.0, 1.0, "Size, in frame heights"),
                num("softness", 0.003, 0.0, 0.5, "Blur at the edge"),
                point("center", [0.0, 0.0], "Middle"),
                at(),
            ],
            vec![out("fill", Number), out("distance", Number)],
            "let $distance = length($at - $center) - $radius;
let $fill = 1.0 - smoothstep(-$softness - 1e-4, $softness + 1e-4, $distance);",
            &[],
        ),
        block(
            "box",
            "Box",
            Shapes,
            "A filled rectangle with optional round corners.",
            vec![
                num("width", 0.3, 0.0, 2.0, "Width, in frame heights"),
                num("height", 0.3, 0.0, 2.0, "Height, in frame heights"),
                num("round", 0.0, 0.0, 0.5, "Corner radius"),
                num("angle", 0.0, 0.0, 1.0, "Turn, in turns"),
                num("softness", 0.003, 0.0, 0.5, "Blur at the edge"),
                point("center", [0.0, 0.0], "Middle"),
                at(),
            ],
            vec![out("fill", Number), out("distance", Number)],
            "let $_p = rotate($at - $center, -$angle * TAU);
let $_r = min($round, 0.5 * min($width, $height));
let $_d = abs($_p) - vec2<f32>($width, $height) * 0.5 + $_r;
let $distance = length(max($_d, vec2<f32>(0.0))) + min(max($_d.x, $_d.y), 0.0) - $_r;
let $fill = 1.0 - smoothstep(-$softness - 1e-4, $softness + 1e-4, $distance);",
            &[],
        ),
        block(
            "polygon",
            "Polygon",
            Shapes,
            "A filled triangle, square, hexagon or any regular shape.",
            vec![
                num("radius", 0.2, 0.0, 1.0, "Size, in frame heights"),
                whole("sides", 6.0, 3.0, 16.0, "Number of sides"),
                num("angle", 0.0, 0.0, 1.0, "Turn, in turns"),
                num("softness", 0.003, 0.0, 0.5, "Blur at the edge"),
                point("center", [0.0, 0.0], "Middle"),
                at(),
            ],
            vec![out("fill", Number), out("distance", Number)],
            "let $distance = g_polygon(rotate($at - $center, -$angle * TAU), $radius, $sides);
let $fill = 1.0 - smoothstep(-$softness - 1e-4, $softness + 1e-4, $distance);",
            &["polygon"],
        ),
        block(
            "star",
            "Star",
            Shapes,
            "A filled star.",
            vec![
                num("radius", 0.2, 0.0, 1.0, "Size, in frame heights"),
                whole("points", 5.0, 2.0, 16.0, "Number of points"),
                num("inner", 0.45, 0.05, 1.0, "Inner corners, as a share of the radius"),
                num("angle", 0.0, 0.0, 1.0, "Turn, in turns"),
                num("softness", 0.003, 0.0, 0.5, "Blur at the edge"),
                point("center", [0.0, 0.0], "Middle"),
                at(),
            ],
            vec![out("fill", Number), out("distance", Number)],
            "let $distance = g_star(rotate($at - $center, -$angle * TAU), $radius, $points, $inner);
let $fill = 1.0 - smoothstep(-$softness - 1e-4, $softness + 1e-4, $distance);",
            &["star"],
        ),
        block(
            "outline",
            "Outline",
            Shapes,
            "Turns a shape's Distance into a glowing line along its edge.",
            vec![
                num("distance", 0.0, -1.0, 1.0, "Plug in a shape's Distance"),
                num("width", 4.0, 0.5, 100.0, "Line width, in video pixels"),
                num("glow", 0.0, 0.0, 1.0, "Soft glow around the line"),
            ],
            vec![out("out", Number)],
            "let $_w = $width * 0.5 * g_px();
let $_d = abs($distance);
let $out = 1.0 - smoothstep($_w, $_w + g_px(), $_d) + $glow * exp(-$_d / max($_w * 4.0, 1e-4));",
            &["px"],
        ),
        // ---- Warps ----
        block(
            "offset",
            "Move",
            Warps,
            "Shifts positions by X and Y, plus a steady drift per second.",
            vec![
                num("x", 0.0, -1.0, 1.0, "Shift right"),
                num("y", 0.0, -1.0, 1.0, "Shift down"),
                num("speed_x", 0.0, -2.0, 2.0, "Drift right per second"),
                num("speed_y", 0.0, -2.0, 2.0, "Drift down per second"),
                at(),
            ],
            vec![out("out", Vector)],
            "let $out = $at - vec2<f32>($x, $y) - fx.time * vec2<f32>($speed_x, $speed_y);",
            &[],
        ),
        block(
            "zoom",
            "Zoom",
            Warps,
            "Zooms in (above 1) or out (below 1) around a point.",
            vec![num("amount", 1.5, 0.05, 10.0, "Zoom; above 1 zooms in"), point("center", [0.0, 0.0], "Zoom around this point"), at()],
            vec![out("out", Vector)],
            "let $out = ($at - $center) / max($amount, 1e-3) + $center;",
            &[],
        ),
        block(
            "spin",
            "Turn",
            Warps,
            "Turns positions around the middle by Angle, plus Speed turns per second.",
            vec![num("angle", 0.0, -1.0, 1.0, "Turn, in turns"), num("speed", 0.0, -2.0, 2.0, "Turns per second"), at()],
            vec![out("out", Vector)],
            "let $out = rotate($at, -($angle + fx.time * $speed) * TAU);",
            &[],
        ),
        block(
            "twirl",
            "Twirl",
            Warps,
            "Twists the middle round like a whirlpool.",
            vec![
                num("strength", 0.5, -4.0, 4.0, "Turns at the very middle"),
                num("radius", 0.5, 0.01, 2.0, "How far the twist reaches"),
                point("center", [0.0, 0.0], "Middle of the twist"),
                at(),
            ],
            vec![out("out", Vector)],
            "let $_d = $at - $center;
let $_k = max(1.0 - length($_d) / $radius, 0.0);
let $out = rotate($_d, $strength * TAU * $_k * $_k) + $center;",
            &[],
        ),
        block(
            "mirror",
            "Mirror",
            Warps,
            "Copies one half of the picture onto the other for perfect symmetry.",
            vec![
                toggle("left_right", true, "Mirror the left half onto the right"),
                toggle("top_bottom", false, "Mirror the top half onto the bottom"),
                toggle("swap", false, "Use the right or bottom half as the source instead"),
                at(),
            ],
            vec![out("out", Vector)],
            "let $_s = select(-1.0, 1.0, $swap > 0.5);
let $out = vec2<f32>(
    select($at.x, $_s * abs($at.x), $left_right > 0.5),
    select($at.y, $_s * abs($at.y), $top_bottom > 0.5),
);",
            &[],
        ),
        block(
            "kaleidoscope",
            "Kaleidoscope",
            Warps,
            "Folds positions into mirrored slices around the middle.",
            vec![
                whole("slices", 6.0, 2.0, 24.0, "Number of mirrored slices"),
                num("spin", 0.03, -1.0, 1.0, "Turns per second"),
                num("zoom", 1.0, 0.25, 4.0, "Above 1 zooms in"),
                num("angle", 0.0, 0.0, 360.0, "Which part the slices show, in degrees"),
                at(),
            ],
            vec![out("out", Vector)],
            "let $_r = length($at) / $zoom;
let $_slice = TAU / max(round($slices), 1.0);
var $_a = atan2($at.y, $at.x) + fx.time * $spin * TAU;
$_a = $_a - $_slice * floor($_a / $_slice);
$_a = min($_a, $_slice - $_a) + radians($angle);
let $out = vec2<f32>(cos($_a), sin($_a)) * $_r;",
            &[],
        ),
        block(
            "repeat",
            "Repeat",
            Warps,
            "Tiles space so whatever comes after repeats in a grid.",
            vec![num("count", 4.0, 0.5, 50.0, "Tiles per frame height"), at()],
            vec![out("out", Vector), out("tile", Vector)],
            "let $tile = floor($at * $count + 0.5);
let $out = ($at * $count - $tile) / $count;",
            &[],
        ),
        block(
            "ripple",
            "Ripple",
            Warps,
            "Waves spreading from a point that push the picture in and out.",
            vec![
                num("amount", 0.01, 0.0, 0.1, "How far the picture moves"),
                num("frequency", 20.0, 1.0, 100.0, "Waves per frame height"),
                num("speed", 1.0, -10.0, 10.0, "Waves per second"),
                point("center", [0.0, 0.0], "Middle of the waves"),
                at(),
            ],
            vec![out("out", Vector)],
            "let $_d = $at - $center;
let $_r = length($_d);
let $out = $at + $_d / max($_r, 1e-4) * sin(($_r * $frequency - fx.time * $speed) * TAU) * $amount;",
            &[],
        ),
        block(
            "flow",
            "Flow",
            Warps,
            "Bends positions with slowly flowing swirls, as if seen through moving water.",
            vec![
                num("amount", 0.03, 0.0, 0.15, "How far it bends"),
                num("scale", 3.0, 0.5, 12.0, "Swirl count; higher makes smaller swirls"),
                num("speed", 0.3, 0.0, 3.0, "How fast the swirls flow"),
                at(),
            ],
            vec![out("out", Vector)],
            "let $_t = fx.time * $speed;
let $_n = vec2<f32>(fbm($at * $scale + vec2<f32>($_t, 0.0)), fbm($at * $scale + vec2<f32>(5.2, 1.3 - $_t))) - 0.5;
let $out = $at + $_n * $amount * 2.0;",
            &[],
        ),
        block(
            "shockwave",
            "Shockwaves",
            Warps,
            "Every bounce sends a ripple out from where the ball hit. Light is the rings' glow in the ball's colour.",
            vec![
                num("strength", 0.6, 0.0, 2.0, "How much the ripples bend the picture"),
                num("speed", 0.5, 0.05, 2.0, "How fast rings spread, in frame heights per second"),
                num("width", 0.02, 0.005, 0.2, "Thickness of each ring"),
                num("life", 0.6, 0.1, 3.0, "Seconds before a ring fades out"),
                at(),
            ],
            vec![out("out", Vector), out("light", Color)],
            "var $_offset = vec2<f32>(0.0);
var $_light = vec3<f32>(0.0);
for (var i = 0u; i < fx.hit_count; i++) {
    let age = hit_age(i);
    let d = $at - to_centered(hit_uv(i));
    let dist = length(d);
    let x = (dist - age * $speed) / $width;
    let fade = hit_strength(i) * max(1.0 - age / $life, 0.0);
    let ring = exp(-x * x) * fade;
    $_offset += d / max(dist, 1e-4) * x * ring * $width * $strength;
    $_light += hit_color(i) * ring;
}
let $out = $at - $_offset;
let $light = $_light;",
            &[],
        ),
        block(
            "pixelate",
            "Pixelate",
            Warps,
            "Snaps positions to the middle of big square cells. Local is where in its cell a pixel is, from -0.5 to 0.5.",
            vec![whole("size", 12.0, 2.0, 64.0, "Cell size, in video pixels"), at()],
            vec![out("out", Vector), out("local", Vector)],
            "let $_cell = max(round($size), 1.0) * fx.scale;
let $_pixel = from_centered($at) * fx.resolution;
let $_center = (floor($_pixel / $_cell) + 0.5) * $_cell;
let $out = to_centered($_center / fx.resolution);
let $local = ($_pixel - $_center) / $_cell;",
            &[],
        ),
        block(
            "glitch",
            "Glitch bands",
            Warps,
            "Tears positions into shifted horizontal bands. Split is how far to pull colours apart there.",
            vec![
                num("amount", 0.5, 0.0, 1.0, "How often and how far bands jump"),
                whole("bands", 24.0, 4.0, 120.0, "Number of horizontal bands"),
                num("react", 1.0, 0.0, 1.0, "1 only glitches on bounces, 0 glitches all the time"),
                num("rate", 12.0, 1.0, 60.0, "Times per second the pattern changes"),
                at(),
            ],
            vec![out("out", Vector), out("split", Number)],
            "let $_uv = from_centered($at);
let $_band = floor($_uv.y * round($bands));
let $_tick = floor(fx.time * $rate);
let $_level = $amount * mix(1.0, fx.pulse, $react);
let $_on = step(1.0 - $_level * 0.7, hash21(vec2<f32>($_band * 0.37, $_tick)));
let $_shift = (hash21(vec2<f32>($_band, $_tick + 7.0)) - 0.5) * 0.2 * $_level * $_on;
let $out = to_centered(vec2<f32>($_uv.x + $_shift, $_uv.y));
let $split = 0.01 * $_level * $_on;",
            &[],
        ),
        // ---- Colour ----
        block(
            "ramp",
            "Colour ramp",
            Colour,
            "Turns a number from 0 to 1 into a colour: Dark at 0, Middle at 0.5, Bright at 1.",
            vec![
                num("t", 0.5, 0.0, 1.0, "Where on the ramp"),
                color("dark", [0.02, 0.0, 0.1], "Colour at 0"),
                color("middle", [0.9, 0.1, 0.5], "Colour at 0.5"),
                color("bright", [1.0, 0.85, 0.3], "Colour at 1"),
            ],
            vec![out("out", Color)],
            "let $_t = clamp($t, 0.0, 1.0) * 2.0;
let $out = mix(mix($dark, $middle, min($_t, 1.0)), $bright, max($_t - 1.0, 0.0));",
            &[],
        ),
        block(
            "rainbow",
            "Rainbow",
            Colour,
            "A colour from the colour wheel: Hue in turns, plus saturation and brightness.",
            vec![
                num("hue", 0.0, 0.0, 1.0, "Position on the colour wheel"),
                num("saturation", 1.0, 0.0, 1.0, "0 is white"),
                num("brightness", 1.0, 0.0, 4.0, "Above 1 glows"),
            ],
            vec![out("out", Color)],
            "let $out = hsv_to_rgb(fract($hue), clamp($saturation, 0.0, 1.0), max($brightness, 0.0));",
            &[],
        ),
        block(
            "hue_shift",
            "Hue shift",
            Colour,
            "Turns a colour round the colour wheel.",
            vec![picture("color"), num("turns", 0.25, -1.0, 1.0, "How far round; 1 is all the way")],
            vec![out("out", Color)],
            "let $out = hue_shift($color, $turns);",
            &[],
        ),
        block(
            "saturation",
            "Saturation",
            Colour,
            "0 is black and white, 1 unchanged, 2 extra vivid.",
            vec![picture("color"), num("amount", 1.5, 0.0, 3.0, "Colourfulness")],
            vec![out("out", Color)],
            "let $out = max(mix(vec3<f32>(luma($color)), $color, $amount), vec3<f32>(0.0));",
            &[],
        ),
        block(
            "brightness_contrast",
            "Brightness and contrast",
            Colour,
            "Brightness multiplies the colour; contrast above 1 makes darks darker and brights brighter.",
            vec![picture("color"), num("brightness", 1.0, 0.0, 4.0, "Multiplies the colour"), num("contrast", 1.0, 0.5, 2.0, "Spread between dark and bright")],
            vec![out("out", Color)],
            "let $out = 0.18 * pow(max($color * $brightness, vec3<f32>(0.0)) / 0.18, vec3<f32>($contrast));",
            &[],
        ),
        block(
            "posterise",
            "Posterise",
            Colour,
            "Cuts colours down to a few flat steps, like a poster.",
            vec![picture("color"), whole("levels", 4.0, 2.0, 16.0, "Steps per colour channel")],
            vec![out("out", Color)],
            "let $_n = max(round($levels) - 1.0, 1.0);
let $out = floor(clamp($color, vec3<f32>(0.0), vec3<f32>(1.0)) * $_n + 0.5) / $_n;",
            &[],
        ),
        block(
            "invert",
            "Invert",
            Colour,
            "Turns dark to bright and every colour to its opposite.",
            vec![picture("color")],
            vec![out("out", Color)],
            "let $out = max(vec3<f32>(1.0) - $color, vec3<f32>(0.0));",
            &[],
        ),
        block(
            "chromatic_split",
            "Colour split",
            Colour,
            "Samples red and blue a little apart, like a cheap lens.",
            vec![
                num("amount", 0.004, 0.0, 0.1, "Gap between red and blue, as a share of the screen"),
                num("radial", 1.0, 0.0, 1.0, "0 splits sideways, 1 splits outward from the middle"),
                at(),
            ],
            vec![out("out", Color)],
            "let $_dir = mix(vec2<f32>(1.0, 0.0), $at * 2.0, $radial);
let $_off = $_dir * $amount / vec2<f32>(aspect(), 1.0);
let $_uv = from_centered($at);
let $out = vec3<f32>(sample($_uv + $_off).r, sample($_uv).g, sample($_uv - $_off).b);",
            &[],
        ),
        block(
            "vignette",
            "Vignette",
            Colour,
            "Darkens the corners.",
            vec![picture("color"), num("strength", 0.5, 0.0, 1.0, "How dark the corners get"), at()],
            vec![out("out", Color)],
            "let $_q = (from_centered($at) - 0.5) * 2.0;
let $out = $color * (1.0 - $strength * smoothstep(0.4, 1.5, dot($_q, $_q)));",
            &[],
        ),
        block(
            "grain",
            "Film grain",
            Colour,
            "Moving grain, one speck per video pixel.",
            vec![picture("color"), num("amount", 0.06, 0.0, 0.4, "How strong the grain is"), at()],
            vec![out("out", Color)],
            "let $_pixel = floor(from_centered($at) * fx.resolution / fx.scale);
let $_n = hash21($_pixel + floor(fx.time * 60.0) * vec2<f32>(37.0, 113.0)) - 0.5;
let $out = max($color * (1.0 + $_n * $amount * 2.0) + $_n * $amount * 0.02, vec3<f32>(0.0));",
            &[],
        ),
        block(
            "scanlines",
            "Scanlines",
            Colour,
            "Horizontal TV lines.",
            vec![picture("color"), num("strength", 0.5, 0.0, 1.0, "How dark the lines are"), at()],
            vec![out("out", Color)],
            "let $_line = 0.5 + 0.5 * cos(from_centered($at).y * fx.resolution.y / fx.scale * PI * 0.5);
let $out = $color * (1.0 - $strength * 0.6 * $_line);",
            &[],
        ),
        // ---- Blend ----
        block(
            "blend_add",
            "Add light",
            Blend,
            "Adds Layer on top of Base like light, by Amount.",
            vec![picture("base"), color("layer", [0.0, 0.0, 0.0], "Light to add"), num("amount", 1.0, 0.0, 4.0, "How much")],
            vec![out("out", Color)],
            "let $out = $base + $layer * $amount;",
            &[],
        ),
        block(
            "blend_screen",
            "Screen",
            Blend,
            "Brightens Base with Layer without blowing out, by Amount.",
            vec![picture("base"), color("layer", [0.0, 0.0, 0.0], "Brightening layer"), num("amount", 1.0, 0.0, 1.0, "How much")],
            vec![out("out", Color)],
            "let $_a = clamp($base, vec3<f32>(0.0), vec3<f32>(1.0));
let $_b = clamp($layer, vec3<f32>(0.0), vec3<f32>(1.0));
let $out = mix($base, $base + $_b - $_a * $_b, $amount);",
            &[],
        ),
        block(
            "blend_multiply",
            "Multiply",
            Blend,
            "Darkens Base by Layer, like tinted glass, by Amount.",
            vec![picture("base"), color("layer", [1.0, 1.0, 1.0], "Tint"), num("amount", 1.0, 0.0, 1.0, "How much")],
            vec![out("out", Color)],
            "let $out = mix($base, $base * $layer, $amount);",
            &[],
        ),
        block(
            "blend_overlay",
            "Overlay",
            Blend,
            "Adds contrast and colour from Layer: darks get darker, brights brighter.",
            vec![picture("base"), color("layer", [0.5, 0.5, 0.5], "Overlay layer"), num("amount", 1.0, 0.0, 1.0, "How much")],
            vec![out("out", Color)],
            "let $_a = clamp($base, vec3<f32>(0.0), vec3<f32>(1.0));
let $_low = 2.0 * $_a * $layer;
let $_high = 1.0 - 2.0 * (1.0 - $_a) * (1.0 - $layer);
let $out = mix($base, select($_high, $_low, $_a < vec3<f32>(0.5)) + max($base - 1.0, vec3<f32>(0.0)), $amount);",
            &[],
        ),
        block(
            "blend_lighten",
            "Lighten",
            Blend,
            "Keeps the brighter of Base and Layer in each channel.",
            vec![picture("base"), color("layer", [0.0, 0.0, 0.0], "Other picture"), num("amount", 1.0, 0.0, 1.0, "How much")],
            vec![out("out", Color)],
            "let $out = mix($base, max($base, $layer), $amount);",
            &[],
        ),
        block(
            "blend_difference",
            "Difference",
            Blend,
            "The difference between Base and Layer, for psychedelic inverted overlaps.",
            vec![picture("base"), color("layer", [1.0, 1.0, 1.0], "Other picture"), num("amount", 1.0, 0.0, 1.0, "How much")],
            vec![out("out", Color)],
            "let $out = mix($base, abs($base - $layer), $amount);",
            &[],
        ),
        block(
            "mask",
            "Mask",
            Blend,
            "Shows A where Mask is 0 and B where it is 1, such as a shape's Fill.",
            vec![picture("a"), color("b", [1.0, 1.0, 1.0], "Shown where the mask is 1"), num("mask", 0.5, 0.0, 1.0, "Which one shows")],
            vec![out("out", Color)],
            "let $out = mix($a, $b, clamp($mask, 0.0, 1.0));",
            &[],
        ),
        // ---- Output ----
        block(
            "output",
            "Output",
            Output,
            "The colour this effect shows. Every node effect needs exactly one.",
            vec![picture("color")],
            vec![],
            "return $color;",
            &[],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn blocks_are_well_formed() {
        let blocks = blocks();
        assert!(blocks.len() >= 60, "only {} blocks", blocks.len());
        let mut kinds = BTreeSet::new();
        for block in &blocks {
            assert!(kinds.insert(block.kind), "{} twice", block.kind);
            assert!(!block.help.is_empty(), "{}", block.kind);
            let mut names = BTreeSet::new();
            for input in &block.inputs {
                assert!(names.insert(input.name), "{}.{} twice", block.kind, input.name);
                assert!(!input.help.is_empty(), "{}.{}", block.kind, input.name);
                assert!(input.min < input.max, "{}.{}", block.kind, input.name);
                if let Default::Number(v) = input.default {
                    assert!((input.min..=input.max).contains(&v), "{}.{} default", block.kind, input.name);
                }
                assert!(block.code.contains(&format!("${}", input.name)), "{}.{} unused", block.kind, input.name);
            }
            for output in &block.outputs {
                assert!(names.insert(output.name), "{}.{} twice", block.kind, output.name);
                assert!(block.code.contains(&format!("let ${} ", output.name)), "{}.{} never set", block.kind, output.name);
            }
            for helper in block.helpers {
                assert!(HELPERS.iter().any(|(name, _)| name == helper), "{} needs unknown helper {helper}", block.kind);
            }
            // Any outputs need an Any input to take their type from.
            if block.outputs.iter().any(|o| o.ty == Ty::Any) {
                assert!(block.inputs.iter().any(|i| i.ty == Ty::Any), "{}", block.kind);
            }
        }
        for category in Category::ALL {
            assert!(blocks.iter().any(|b| b.category == category), "{category:?} is empty");
        }
    }
}
