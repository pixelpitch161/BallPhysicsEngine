// The editor's clipping views (colour grading part 6, grade/scopes.rs::Clipping): J's marks and
// the Alt views of Whites and Blacks, worked out from the finished 8-bit picture into a picture
// of their own that the editor draws over the preview. Never part of an export.

// 1 J's marks, 2 the Whites view, 3 the Blacks view.
struct ClipMode {
    mode: u32,
    _p0: u32,
    _p1: u32,
    _p2: u32,
};

@group(0) @binding(0) var shown: texture_2d<f32>;
@group(0) @binding(1) var clipped: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> clip: ClipMode;

// A channel at white or at black in the 8-bit picture (the composite keeps both exact).
const AT_WHITE: f32 = 254.5 / 255.0;
const AT_BLACK: f32 = 0.5 / 255.0;

@compute @workgroup_size(16, 16)
fn cs_clipping(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(shown);
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    let c = textureLoad(shown, vec2<i32>(id.xy), 0).rgb;
    let white = c >= vec3<f32>(AT_WHITE);
    let black = c <= vec3<f32>(AT_BLACK);
    var out = vec4<f32>(0.0);
    if clip.mode == 1u {
        if all(white) {
            out = vec4<f32>(1.0, 0.0, 0.0, 1.0);
        } else if all(black) {
            out = vec4<f32>(0.0, 0.25, 1.0, 1.0);
        }
    } else if clip.mode == 2u {
        out = vec4<f32>(select(vec3<f32>(0.0), vec3<f32>(1.0), white), 1.0);
    } else if clip.mode == 3u {
        out = vec4<f32>(select(vec3<f32>(1.0), vec3<f32>(0.0), black), 1.0);
    }
    textureStore(clipped, vec2<i32>(id.xy), out);
}
