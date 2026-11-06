// name: Breathing space
// about: The whole picture swells and twists on every beat or hit, its colours splitting apart at the peak.
// simple swell = 0.08 [0, 0.5] How much it swells on each kick
// simple twist = 0.15 [-1, 1] How far it twists on each kick, in radians
// simple split = 0.012 [0, 0.05] How far the colours split at the peak, as a share of the screen height
// choice on = both [beats, hits, both] What makes it breathe: the tempo's beats, the hits, or both
// param sharp = 6 [1, 30] How quickly each beat's kick fades
// param centre_x = 0 [-0.5, 0.5] Where it swells from, right of the middle, in screen heights
// param centre_y = 0 [-0.5, 0.5] Where it swells from, below the middle, in screen heights

fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let on_beat = select(0.0, exp(-beat_phase() * max(p.sharp, 0.0)), fx.beat.w > 0.5);
    var k = max(on_beat, fx.pulse);
    if p.on < 0.5 {
        k = on_beat;
    } else if p.on < 1.5 {
        k = fx.pulse;
    }
    let centre = vec2<f32>(p.centre_x, p.centre_y);
    let d = to_centered(uv) - centre;
    let fall = max(1.0 - length(d), 0.0);
    let w = rotate(d, k * p.twist * fall) * (1.0 - p.swell * k * fall) + centre;
    let at = from_centered(w);
    let s = vec2<f32>(p.split * k / aspect(), 0.0);
    return vec3<f32>(sample(at + s).r, sample(at).g, sample(at - s).b);
}
