
// What effects call for a Shader layer inside a shape (see in_ball.wgsl, which `sample` and
// `object_at` come from): the effect's picture is the stack under the shader layer, walked again
// at the world point `uv` maps to. `uv` covers the shape's square (twice its reach across, band
// included, its middle at 0.5), scaled by the layer's Scale about the middle; it turns with the shape or stays
// upright on the screen.

// How far the shape reaches from its middle, in shape units, a hollow shape's band included.
fn in_shape_reach() -> f32 {
    return max(layered_shape.c.y + layered_shape.b.x / max(layered_shape.a.w, 1e-6), 1e-3);
}

// Sets up shader record `l`, the `k`th record of a shape whose records start at `first`, and
// gives the uv of world point `world` (`s` where it is against the outline).
fn shape_begin(l: LayerGpu, first: u32, k: u32, s: ShapeAt, world: vec2<f32>) -> vec2<f32> {
    in_ball = InBall(first, k, max(l.p0.y, 0.1), l.p0.z, 0.0, 1.0);
    fx = fx_frame;
    fx.time = fx_frame.time * l.p0.x;
    let reach = in_shape_reach();
    let side = max(2.0 * reach * layered_shape.a.w / max(g.view.z, 1e-6), 1.0);
    fx.resolution = vec2<f32>(side, side);
    // Output pixels per world unit (a video pixel at zoom 1), so pixel-sized effects (Pixels,
    // Film grain) look the same in the preview and the export.
    fx.scale = 1.0 / max(g.view.z, 1e-6);
    fx.blend = 1.0;
    fx.apply_to = 0.0;
    fx.in_layer = 0.0;
    var pt = s.q / reach;
    if in_ball.screen > 0.5 {
        pt = (world - layered_shape.a.xy) / (reach * max(layered_shape.a.w, 1e-6));
    }
    return pt * 0.5 / in_ball.scale + vec2<f32>(0.5);
}

// The world point a shader uv inside the shape stands for.
fn in_shape_world(uv: vec2<f32>) -> vec2<f32> {
    let reach = in_shape_reach();
    let offset = (uv - vec2<f32>(0.5)) * 2.0 * in_ball.scale * reach * layered_shape.a.w;
    if in_ball.screen > 0.5 {
        return layered_shape.a.xy + offset;
    }
    return layered_shape.a.xy + rotate(offset, layered_shape.a.z);
}

// The layers under the shader layer at `uv`.
fn shape_sample(uv: vec2<f32>) -> vec3<f32> {
    let world = in_shape_world(uv);
    let s = shape_at(world);
    let w = walk_shape_plain(world, s, in_ball.first, in_ball.below, 0u);
    let cover = shape_cover_at(s);
    return (cover * w.inner + (1.0 - cover) * w.outer).rgb;
}

// The shape itself at `uv`: its own position there (-1 to 1 across its reach, turning with it),
// its thing code, and how much it covers.
fn shape_object_at(uv: vec2<f32>) -> vec4<f32> {
    let s = shape_at(in_shape_world(uv));
    return vec4<f32>(s.q / in_shape_reach(), layered_shape.c.x, shape_cover_at(s));
}
