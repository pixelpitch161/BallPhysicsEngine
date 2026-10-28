//! Node effects that come with the program, and node versions of the built-in code effects.
//!
//! [`graphs`] are offered in the effect list like any other effect. [`rebuild`] gives the node
//! version of a built-in code effect, used by "Edit as nodes" and by the tests that prove the
//! block set can make every built-in effect.

use super::graph::{Builder, Control, Graph};

/// The node effects offered in the effect list.
pub fn graphs() -> Vec<Graph> {
    vec![bass_tunnel(), treble_sparkle(), beat_kaleido(), liquid_pulse()]
}

/// Built-in code effects that have a node version.
pub const REBUILT: [&str; 10] =
    ["echo", "kaleidoscope", "shockwave", "color_split", "color_cycle", "liquid", "mirror", "pixels", "film", "glitch"];

/// The node version of a built-in code effect, with sliders of the same names.
pub fn rebuild(id: &str) -> Option<Graph> {
    let title = |t: &str| format!("{t} (nodes)");
    let mut b;
    match id {
        "kaleidoscope" => {
            b = Builder::new(&title("Kaleidoscope"));
            let k = b.add("kaleidoscope");
            for name in ["slices", "spin", "zoom", "angle"] {
                b.expose(k, name, None);
            }
            let pic = b.add("picture");
            let out = b.add("output");
            b.wire(k, "out", pic, "at").wire(pic, "out", out, "color");
        }
        "color_cycle" => {
            b = Builder::new(&title("Colour cycle"));
            let time = b.add("time");
            b.set(time, "speed", 0.05).expose(time, "speed", None).range(time, "speed", -1.0, 1.0);
            let pulse = b.add("pulse");
            let kick = b.add("multiply");
            b.set(kick, "b", 0.0).expose(kick, "b", Some("kick")).range(kick, "b", 0.0, 0.5);
            let turns = b.add("add");
            let hue = b.add("hue_shift");
            let sat = b.add("saturation");
            b.set(sat, "amount", 1.0).expose(sat, "amount", Some("saturation")).range(sat, "amount", 0.0, 2.0);
            let contrast = b.add("brightness_contrast");
            b.expose(contrast, "contrast", None);
            let out = b.add("output");
            b.wire(pulse, "out", kick, "a")
                .wire(time, "out", turns, "a")
                .wire(kick, "out", turns, "b")
                .wire(turns, "out", hue, "turns")
                .wire(hue, "out", sat, "color")
                .wire(sat, "out", contrast, "color")
                .wire(contrast, "out", out, "color");
        }
        "mirror" => {
            b = Builder::new(&title("Mirror"));
            let m = b.add("mirror");
            for name in ["left_right", "top_bottom", "swap"] {
                b.expose(m, name, None);
            }
            let pic = b.add("picture");
            let out = b.add("output");
            b.wire(m, "out", pic, "at").wire(pic, "out", out, "color");
        }
        "liquid" => {
            b = Builder::new(&title("Liquid"));
            let flow = b.add("flow");
            for name in ["amount", "scale", "speed"] {
                b.expose(flow, name, None);
            }
            let pic = b.add("picture");
            let out = b.add("output");
            b.wire(flow, "out", pic, "at").wire(pic, "out", out, "color");
        }
        "echo" => {
            b = Builder::new(&title("Echo"));
            let feedback = b.add("feedback");
            b.set(feedback, "hue", 0.004);
            for name in ["fade", "zoom", "spin", "hue"] {
                b.expose(feedback, name, None);
            }
            let lighten = b.add("blend_lighten");
            let out = b.add("output");
            b.wire(feedback, "out", lighten, "layer").wire(lighten, "out", out, "color");
        }
        "shockwave" => {
            b = Builder::new(&title("Shockwaves"));
            let wave = b.add("shockwave");
            for name in ["strength", "speed", "width", "life"] {
                b.expose(wave, name, None);
            }
            let pic = b.add("picture");
            let glow = b.add("multiply");
            b.set(glow, "a", 0.4).expose(glow, "a", Some("glow")).range(glow, "a", 0.0, 2.0).set(glow, "b", 0.25);
            let add = b.add("blend_add");
            let out = b.add("output");
            b.wire(wave, "out", pic, "at")
                .wire(pic, "out", add, "base")
                .wire(wave, "light", add, "layer")
                .wire(glow, "out", add, "amount")
                .wire(add, "out", out, "color");
        }
        "color_split" => {
            b = Builder::new(&title("Colour split"));
            let pulse = b.add("pulse");
            let kick = b.add("multiply");
            b.set(kick, "b", 2.0).expose(kick, "b", Some("kick")).range(kick, "b", 0.0, 10.0);
            let one_plus = b.add("add");
            b.set(one_plus, "a", 1.0);
            let amount = b.add("multiply");
            b.set(amount, "a", 0.004).expose(amount, "a", Some("amount")).range(amount, "a", 0.0, 0.03);
            let split = b.add("chromatic_split");
            b.expose(split, "radial", None);
            let out = b.add("output");
            b.wire(pulse, "out", kick, "a")
                .wire(kick, "out", one_plus, "b")
                .wire(one_plus, "out", amount, "b")
                .wire(amount, "out", split, "amount")
                .wire(split, "out", out, "color");
        }
        "film" => {
            b = Builder::new(&title("Film"));
            let vignette = b.add("vignette");
            b.expose(vignette, "strength", Some("vignette"));
            let grain = b.add("grain");
            b.expose(grain, "amount", Some("grain"));
            let lines = b.add("scanlines");
            b.set(lines, "strength", 0.0).expose(lines, "strength", Some("scanlines"));
            let out = b.add("output");
            b.wire(vignette, "out", grain, "color").wire(grain, "out", lines, "color").wire(lines, "out", out, "color");
        }
        "glitch" => {
            b = Builder::new(&title("Glitch"));
            let bands = b.add("glitch");
            for name in ["amount", "bands", "react", "rate"] {
                b.expose(bands, name, None);
            }
            // Colour split measures its gap in frame heights across, Glitch bands in screen widths.
            let aspect = b.add("aspect");
            let gap = b.add("multiply");
            let split = b.add("chromatic_split");
            b.set(split, "radial", 0.0);
            let out = b.add("output");
            b.wire(bands, "split", gap, "a")
                .wire(aspect, "out", gap, "b")
                .wire(gap, "out", split, "amount")
                .wire(bands, "out", split, "at")
                .wire(split, "out", out, "color");
        }
        "pixels" => {
            b = Builder::new(&title("Pixels"));
            let cells = b.add("pixelate");
            b.expose(cells, "size", None);
            let pic = b.add("picture");
            // Dot radius 0.5 - gap / 2, faded over its last 0.08; the same edge for squares.
            let gap = b.add("number");
            b.set(gap, "value", 0.15).expose(gap, "value", Some("gap")).range(gap, "value", 0.0, 0.6);
            let edge = b.add("remap");
            b.set(edge, "in_low", 0.0).set(edge, "in_high", 1.0).set(edge, "out_low", 0.46).set(edge, "out_high", -0.04);
            let dot = b.add("circle");
            b.set(dot, "softness", 0.04);
            let square = b.add("box");
            b.set(square, "softness", 0.02);
            let side = b.add("remap");
            b.set(side, "in_low", 0.0).set(side, "in_high", 1.0).set(side, "out_low", 0.96).set(side, "out_high", -0.04);
            let dots = b.add("number");
            b.set(dots, "value", 0.0).expose(dots, "value", Some("dots")).control(dots, "value", Control::Toggle);
            let shape = b.add("mix");
            let lit = b.add("multiply");
            let out = b.add("output");
            b.wire(cells, "out", pic, "at")
                .wire(gap, "out", edge, "x")
                .wire(edge, "out", dot, "radius")
                .wire(cells, "local", dot, "at")
                .wire(gap, "out", side, "x")
                .wire(side, "out", square, "width")
                .wire(side, "out", square, "height")
                .wire(cells, "local", square, "at")
                .wire(square, "fill", shape, "a")
                .wire(dot, "fill", shape, "b")
                .wire(dots, "out", shape, "t")
                .wire(pic, "out", lit, "a")
                .wire(shape, "out", lit, "b")
                .wire(lit, "out", out, "color");
        }
        _ => return None,
    }
    Some(b.finish())
}

/// A light tunnel that flies forward harder on every bass hit.
fn bass_tunnel() -> Graph {
    let mut b = Builder::new("Bass tunnel");
    let music = b.add("music");
    let punch = b.add("multiply");
    b.set(punch, "b", 0.04).expose(punch, "b", Some("punch")).range(punch, "b", 0.0, 0.2);
    let zoom = b.add("add");
    b.set(zoom, "a", 1.005);
    let feedback = b.add("feedback");
    b.set(feedback, "fade", 0.9).set(feedback, "hue", 0.003).set(feedback, "spin", 0.3);
    b.expose(feedback, "fade", None).expose(feedback, "spin", None);
    let lighten = b.add("blend_lighten");
    let rings = b.add("rings");
    b.set(rings, "scale", 5.0).set(rings, "speed", -1.0);
    let sharp = b.add("power");
    b.set(sharp, "exponent", 12.0);
    let ramp = b.add("ramp");
    let glow = b.add("multiply");
    b.set(glow, "b", 0.6).expose(glow, "b", Some("glow")).range(glow, "b", 0.0, 3.0);
    let add = b.add("blend_add");
    let out = b.add("output");
    b.wire(music, "bass", punch, "a")
        .wire(punch, "out", zoom, "b")
        .wire(zoom, "out", feedback, "zoom")
        .wire(feedback, "out", lighten, "layer")
        .wire(rings, "out", sharp, "x")
        .wire(sharp, "out", ramp, "t")
        .wire(music, "bass", glow, "a")
        .wire(lighten, "out", add, "base")
        .wire(ramp, "out", add, "layer")
        .wire(glow, "out", add, "amount")
        .wire(add, "out", out, "color");
    b.finish()
}

/// Glittering specks that appear with the hi-hats and cymbals.
fn treble_sparkle() -> Graph {
    let mut b = Builder::new("Treble sparkle");
    let music = b.add("music");
    let specks = b.add("random");
    b.set(specks, "rate", 20.0);
    let sharp = b.add("power");
    b.set(sharp, "exponent", 60.0);
    let amount = b.add("multiply");
    b.set(amount, "b", 6.0).expose(amount, "b", Some("sparkle")).range(amount, "b", 0.0, 20.0);
    let colour = b.add("rainbow");
    b.set(colour, "saturation", 0.35).set(colour, "brightness", 2.0);
    let time = b.add("time");
    b.set(time, "speed", 0.1);
    let add = b.add("blend_add");
    let hue = b.add("hue_shift");
    b.set(hue, "turns", 0.0);
    let shimmer = b.add("multiply");
    b.set(shimmer, "b", 0.15).expose(shimmer, "b", Some("shimmer")).range(shimmer, "b", 0.0, 1.0);
    let out = b.add("output");
    let scaled = b.add("multiply");
    b.wire(specks, "out", sharp, "x")
        .wire(music, "treble", amount, "a")
        .wire(sharp, "out", scaled, "a")
        .wire(amount, "out", scaled, "b")
        .wire(time, "out", colour, "hue")
        .wire(scaled, "out", add, "amount")
        .wire(colour, "out", add, "layer")
        .wire(music, "treble", shimmer, "a")
        .wire(shimmer, "out", hue, "turns")
        .wire(hue, "out", add, "base")
        .wire(add, "out", out, "color");
    b.finish()
}

/// A kaleidoscope that kicks in on every beat.
fn beat_kaleido() -> Graph {
    let mut b = Builder::new("Beat kaleido");
    let beat = b.add("beat");
    let kick = b.add("multiply");
    b.set(kick, "b", 0.35).expose(kick, "b", Some("kick")).range(kick, "b", 0.0, 1.0);
    let zoom = b.add("add");
    b.set(zoom, "a", 1.0);
    let k = b.add("kaleidoscope");
    b.set(k, "slices", 8.0).set(k, "spin", 0.05);
    b.expose(k, "slices", None).expose(k, "spin", None);
    let pic = b.add("picture");
    let sat = b.add("saturation");
    let vivid = b.add("add");
    b.set(vivid, "a", 1.0);
    let out = b.add("output");
    b.wire(beat, "hit", kick, "a")
        .wire(kick, "out", zoom, "b")
        .wire(zoom, "out", k, "zoom")
        .wire(k, "out", pic, "at")
        .wire(pic, "out", sat, "color")
        .wire(beat, "hit", vivid, "b")
        .wire(vivid, "out", sat, "amount")
        .wire(sat, "out", out, "color");
    b.finish()
}

/// Water that churns with the bass and splits colour with the treble.
fn liquid_pulse() -> Graph {
    let mut b = Builder::new("Liquid pulse");
    let music = b.add("music");
    let depth = b.add("multiply");
    b.set(depth, "b", 0.08).expose(depth, "b", Some("depth")).range(depth, "b", 0.0, 0.3);
    let bend = b.add("add");
    b.set(bend, "a", 0.01);
    let flow = b.add("flow");
    b.expose(flow, "scale", None).expose(flow, "speed", None);
    let fringe = b.add("multiply");
    b.set(fringe, "b", 0.012).expose(fringe, "b", Some("fringe")).range(fringe, "b", 0.0, 0.05);
    let split = b.add("chromatic_split");
    let out = b.add("output");
    b.wire(music, "bass", depth, "a")
        .wire(depth, "out", bend, "b")
        .wire(bend, "out", flow, "amount")
        .wire(flow, "out", split, "at")
        .wire(music, "treble", fringe, "a")
        .wire(fringe, "out", split, "amount")
        .wire(split, "out", out, "color");
    b.finish()
}
