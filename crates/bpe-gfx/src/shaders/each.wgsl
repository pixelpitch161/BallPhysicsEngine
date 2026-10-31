// Things that blend with each other (a blend layer's `each`): each fragment, premultiplied colour
// `c` and coverage `a`, becomes what the layer's fixed-function blend for the mode combines with
// what is there, so that overlapping things meet by the mode. The layer starts as the mode's
// identity; alpha keeps the largest coverage (blend op Max), which tells the composite where
// anything drew. `mode` is `Blend::shader_code`, read from the layer's own globals (`post.w`).
//
//   Add, Subtract, Lighten (cleared to 0; One+One, One+One, Max): c, which is the straight
//     colour s mixed from 0 by the coverage.
//   Screen (cleared to 0; One, OneMinusSrc): c clamped to 0..1, so 1 - (1 - dst)(1 - c).
//   Multiply (cleared to 1; Dst, Zero): mix(1, s, a) = 1 - a + c.
//   Darken (cleared to EACH_FAR; Min): s + (1 - a), nothing where nothing covers. Mixing from
//     the far identity would hide every soft edge, so an edge fades from one above s instead.
const EACH_FAR: f32 = 60000.0;

fn each_out(o: vec4<f32>, mode: f32) -> vec4<f32> {
    let code = u32(mode + 0.5);
    let c = max(o.rgb, vec3<f32>(0.0));
    var a = clamp(o.a, 0.0, 1.0);
    // Light added with no coverage covers as much as its brightest channel, as in blend.wgsl.
    let light = max(c.r, max(c.g, c.b));
    if a < 1e-4 && light > 0.0 {
        a = clamp(light, 0.0, 1.0);
    }
    switch code {
        case 2u: {
            return vec4<f32>(min(c, vec3<f32>(1.0)), a);
        }
        case 3u: {
            return vec4<f32>(1.0 - a + c, a);
        }
        case 5u: {
            if a <= 0.0 {
                return vec4<f32>(vec3<f32>(EACH_FAR), 0.0);
            }
            return vec4<f32>(c / a + (1.0 - a), a);
        }
        default: {
            return vec4<f32>(c, a);
        }
    }
}
