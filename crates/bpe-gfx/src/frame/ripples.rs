//! Ripple sources: the outlines of things that make the water ripple.

use super::*;

/// The most Ripples sources a frame keeps (its layers' together); past it they are left out.
pub const MOST_RIPPLE_SOURCES: usize = 16384;

/// The most segments one outline gives Ripples.
pub const MOST_RIPPLE_SEGMENTS: usize = 256;

/// Something Ripples measure from: a circle (a ball) or a thick segment of a shape's outline, in
/// world pixels. Matches the sources in ripples.wgsl and paint.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct RippleSource {
    /// From xy, to xy (a circle: its centre twice).
    pub a: [f32; 4],
    /// Radius (half the band of a segment), its owner's number, 0 a ball or 1 a shape, spare.
    pub b: [f32; 4],
    /// Its owner's centre xy (what a paint inside it skips), two spare.
    pub c: [f32; 4],
}

impl Frame {
    /// A new owner number for [`Frame::push_ripple_ball`] and [`Frame::push_ripple_outline`]:
    /// the sources of one thing (a ball, a shape) share one, and Ripples measure from the two
    /// nearest owners.
    pub fn ripple_owner(&mut self) -> u32 {
        self.ripple_owners += 1;
        self.ripple_owners
    }

    /// A ball as a Ripples source (only when the frame has Ripples).
    pub fn push_ripple_ball(&mut self, centre: [f32; 2], radius: f32) {
        if !self.ripples || self.ripple_sources.len() >= MOST_RIPPLE_SOURCES {
            return;
        }
        let owner = self.ripple_owner() as f32;
        let r = if radius.is_finite() { radius.max(0.0) } else { 0.0 };
        self.ripple_sources.push(RippleSource {
            a: [centre[0], centre[1], centre[0], centre[1]],
            b: [r, owner, 0.0, 0.0],
            c: [centre[0], centre[1], 0.0, 0.0],
        });
    }

    /// A closed outline through `points` (world pixels), its band `half` wide each side, owned by
    /// a thing centred at `centre`, as Ripples sources: at most [`MOST_RIPPLE_SEGMENTS`]
    /// segments, every few corners when it has more.
    pub fn push_ripple_outline(&mut self, centre: [f32; 2], points: &[[f32; 2]], half: f32) {
        self.push_ripple_shape(centre, std::slice::from_ref(&points), half);
    }

    /// Several closed outlines (a letter's contours) as one Ripples owner centred at `centre`:
    /// at most [`MOST_RIPPLE_SEGMENTS`] segments across them, every few corners when they have more.
    pub fn push_ripple_shape<P: AsRef<[[f32; 2]]>>(&mut self, centre: [f32; 2], contours: &[P], half: f32) {
        let total: usize = contours.iter().map(|c| c.as_ref().len()).sum();
        if !self.ripples || total == 0 {
            return;
        }
        let owner = self.ripple_owner() as f32;
        let half = if half.is_finite() { half.max(0.0) } else { 0.0 };
        let step = total.div_ceil(MOST_RIPPLE_SEGMENTS).max(1);
        for points in contours {
            let points = points.as_ref();
            // A small contour (an i's dot) keeps at least three corners.
            let step = step.min(points.len() / 3).max(1);
            let kept: Vec<[f32; 2]> = points.iter().step_by(step).copied().collect();
            for k in 0..kept.len() {
                if self.ripple_sources.len() >= MOST_RIPPLE_SOURCES {
                    return;
                }
                let (p, q) = (kept[k], kept[(k + 1) % kept.len()]);
                self.ripple_sources.push(RippleSource {
                    a: [p[0], p[1], q[0], q[1]],
                    b: [half, owner, 1.0, 0.0],
                    c: [centre[0], centre[1], 0.0, 0.0],
                });
            }
        }
    }

    /// Every Ripples source of this frame and its layers' frames, each frame's owners numbered
    /// after the last's so no two things share one.
    pub fn all_ripple_sources(&self) -> Vec<RippleSource> {
        let mut all = Vec::new();
        let mut base = 0.0;
        self.gather_ripple_sources(&mut all, &mut base);
        all.truncate(MOST_RIPPLE_SOURCES);
        all
    }

    pub(super) fn gather_ripple_sources(&self, all: &mut Vec<RippleSource>, base: &mut f32) {
        all.extend(
            self.ripple_sources.iter().map(|s| RippleSource { b: [s.b[0], s.b[1] + *base, s.b[2], s.b[3]], ..*s }),
        );
        *base += self.ripple_owners as f32;
        for l in &self.layers {
            l.frame.gather_ripple_sources(all, base);
        }
    }
}
