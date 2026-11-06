//! The cinema render (lighting part 3): a baked frame is the scene drawn several times and
//! averaged, each sample a little moved within the pixel (smooth edges), a little earlier within
//! the shutter (motion blur from the physics' own steps) and with its own random rays (see
//! [`crate::Renderer::render_samples`]). This works out when and where each sample is.

/// Which sample of a baked frame a [`crate::Frame`] is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BakeSample {
    pub index: u32,
    pub count: u32,
}

/// How a scene bakes: the Lighting card's bake settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bake {
    /// Samples a pixel (at least 1).
    pub samples: u32,
    /// How much of a frame's time the shutter is open (0 to 1; 0 without motion blur).
    pub shutter: f32,
}

impl Default for Bake {
    fn default() -> Self {
        Bake { samples: 16, shutter: 0.5 }
    }
}

/// One sample's moment: its time, the physics step to draw and how far back toward the step
/// before it the balls are moved (see `World::set_between`; `None` draws the step as it is).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moment {
    pub time: f64,
    pub step: u64,
    pub between: Option<f32>,
}

/// The moments of the samples of the frame at `time` (`fps` frames a second, the physics at
/// `rate` steps a second), oldest first: the last is the frame itself, the first `shutter / fps ×
/// (n − 1) / n` earlier. Never earlier than the frame before's own moment, so the world only
/// runs forward.
pub fn moments(time: f64, fps: u32, rate: u32, bake: &Bake) -> Vec<Moment> {
    let n = bake.samples.max(1);
    let shutter = if bake.shutter.is_finite() { bake.shutter.clamp(0.0, 1.0) as f64 } else { 0.0 };
    let frame = 1.0 / fps.max(1) as f64;
    (0..n)
        .map(|i| {
            let back = shutter * frame * (n - 1 - i) as f64 / n as f64;
            let t = (time - back).max(0.0);
            let exact = t * rate as f64;
            let step = crate::step_at(t, rate);
            // On a step (within the rounding step_at allows) the step itself; else the next step
            // with the balls moved back toward the one before.
            let past = exact - step as f64;
            if back == 0.0 || past < 1e-6 {
                Moment { time: t, step, between: None }
            } else {
                Moment { time: t, step: step + 1, between: Some(past as f32) }
            }
        })
        .collect()
}

/// The `i`th term of the Halton sequence in `base`, 0 to 1.
fn halton(mut i: u32, base: u32) -> f32 {
    let mut f = 1.0;
    let mut r = 0.0;
    while i > 0 {
        f /= base as f32;
        r += f * (i % base) as f32;
        i /= base;
    }
    r
}

/// Sample `i`'s offset within the pixel, in output pixels (−0.5 to 0.5): 0 for the first, then
/// the Halton (2, 3) sequence, which covers the pixel evenly however many samples there are.
pub fn jitter(i: u32) -> [f32; 2] {
    if i == 0 {
        return [0.0, 0.0];
    }
    [halton(i, 2) - 0.5, halton(i, 3) - 0.5]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moments_run_forward_and_end_on_the_frame() {
        let bake = Bake { samples: 8, shutter: 0.5 };
        let m = moments(1.0, 60, 240, &bake);
        assert_eq!(m.len(), 8);
        assert!(m.windows(2).all(|w| w[0].time < w[1].time));
        let last = m.last().unwrap();
        assert_eq!((last.time, last.step, last.between), (1.0, 240, None));
        let first = 1.0 - 0.5 / 60.0 * 7.0 / 8.0;
        assert!((m[0].time - first).abs() < 1e-12);
        // Never before the frame before.
        assert!(m[0].time > 1.0 - 1.0 / 60.0);
        // Two steps of shutter in eighths: a quarter step apart, every fourth on a step.
        for (i, mo) in m[..7].iter().enumerate() {
            match mo.between {
                Some(a) => {
                    assert!(a > 0.0 && a < 1.0);
                    assert!(((mo.step as f64 - 1.0 + a as f64) / 240.0 - mo.time).abs() < 1e-6);
                }
                None => {
                    assert_eq!(i, 3, "on step {}", mo.step);
                    assert!((mo.step as f64 / 240.0 - mo.time).abs() < 1e-9);
                }
            }
        }
    }

    #[test]
    fn no_shutter_is_every_sample_at_the_frame() {
        let m = moments(0.5, 30, 240, &Bake { samples: 4, shutter: 0.0 });
        assert!(m.iter().all(|mo| mo.time == 0.5 && mo.step == 120 && mo.between.is_none()));
        assert_eq!(moments(0.0, 30, 240, &Bake { samples: 4, shutter: 1.0 })[0].time, 0.0, "not before the start");
        assert_eq!(moments(0.5, 30, 240, &Bake { samples: 0, shutter: 0.5 }).len(), 1);
    }

    #[test]
    fn jitter_covers_the_pixel() {
        assert_eq!(jitter(0), [0.0, 0.0]);
        let all: Vec<[f32; 2]> = (0..16).map(jitter).collect();
        assert!(all.iter().all(|[x, y]| (-0.5..0.5).contains(x) && (-0.5..0.5).contains(y)));
        // Every quarter of the pixel gets some.
        for q in 0..4 {
            let (sx, sy) = (q & 1 == 1, q & 2 == 2);
            assert!(all.iter().any(|[x, y]| (*x >= 0.0) == sx && (*y >= 0.0) == sy));
        }
    }
}
