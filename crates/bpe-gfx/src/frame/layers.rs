//! Blend layers: frames inside the frame that paint with a blend mode, behind or above, and the
//! overlay.

use super::*;

/// Whether things drawn with `blend` at `amount` need a layer of their own.
pub(super) fn layered(blend: Blend, amount: f32) -> bool {
    !(blend == Blend::Normal && amount >= 1.0)
}

/// Runs `draw` on what a thing of z-order `z` drawn with `blend` at `amount` (blending with
/// each other when `each`, behind the balls of its z-order when `behind`) draws into: `target`
/// itself, or its layer (see [`Frame::target_mut`]). A layer borrows `target`'s gradient atlas,
/// hits and effects while it is drawn into, so its paint rows are the rows the renderer uploads
/// and every module reads every hit. A layer nothing was drawn into is not kept.
pub(super) fn draw_routed_at(
    target: &mut Frame,
    z: i32,
    blend: Blend,
    amount: f32,
    each: bool,
    behind: bool,
    draw: impl FnOnce(&mut Frame),
) {
    if z == 0 && !layered(blend, amount) {
        draw(target);
        return;
    }
    draw_into_layer(target, |t| t.target_mut(z, blend, amount, each, behind), draw);
}

/// Runs `draw` on the over layer of z-order `z` (see [`Frame::over_mut`]), borrowing `target`'s
/// atlas, hits and effects as [`draw_routed_at`] does.
pub(super) fn draw_over_routed(target: &mut Frame, z: i32, behind: bool, draw: impl FnOnce(&mut Frame)) {
    draw_into_layer(target, |t| t.over_mut(z, behind), draw);
}

pub(super) fn draw_into_layer(
    target: &mut Frame,
    layer: impl FnOnce(&mut Frame) -> &mut Frame,
    draw: impl FnOnce(&mut Frame),
) {
    let before = target.layers.len();
    let take =
        |f: &mut Frame| (std::mem::take(&mut f.gradients), std::mem::take(&mut f.hits), std::mem::take(&mut f.effects));
    let shared = take(target);
    let layer = layer(target);
    (layer.gradients, layer.hits, layer.effects) = shared;
    draw(layer);
    let shared = take(layer);
    (target.gradients, target.hits, target.effects) = shared;
    if target.layers.len() > before && target.layers.last().is_some_and(|l| l.frame.draws_nothing()) {
        target.layers.pop();
    }
}

impl Frame {
    /// The frame things drawn with `blend` at `amount` go into: this one for Normal at full
    /// amount, else the layer for exactly that blend, amount and `each` (whether its things
    /// blend with each other, kept only where the blend [combines with
    /// itself](Blend::combines_with_itself)), made on first use. The layer takes this frame's
    /// size, moment, camera and picture settings.
    pub fn layer_mut(&mut self, blend: Blend, amount: f32, each: bool) -> &mut Frame {
        self.layer_behind_mut(blend, amount, each, false)
    }

    /// [`Frame::layer_mut`] for things that sit behind the balls when `behind` (see
    /// [`Place::Behind`]): a layer of their own, mixed in before the balls are drawn.
    pub fn layer_behind_mut(&mut self, blend: Blend, amount: f32, each: bool, behind: bool) -> &mut Frame {
        self.target_mut(0, blend, amount, each, behind)
    }

    /// The frame a thing of z-order `z` drawn with `blend` at `amount` goes into: at z-order 0
    /// as [`Frame::layer_behind_mut`]; at another, a plain layer of that z-order (`Back` for
    /// things `behind` the balls, `Front` for the rest) when it doesn't blend, else its blend
    /// layer of that z-order.
    pub fn target_mut(&mut self, z: i32, blend: Blend, amount: f32, each: bool, behind: bool) -> &mut Frame {
        if !layered(blend, amount) {
            if z == 0 {
                return self;
            }
            let place = if behind { Place::Back } else { Place::Front };
            return self.layer_at(z, place, Blend::Normal, 1.0, false);
        }
        // -0 and 0 are one amount (their bits differ).
        let amount = amount + 0.0;
        let each = each && blend.combines_with_itself();
        self.layer_at(z, if behind { Place::Behind } else { Place::Blend }, blend, amount, each)
    }

    /// The layer of z-order `z` at `place` with exactly `blend`, `amount` and `each`, made on
    /// first use, with this frame's size, moment, camera and picture settings.
    pub(super) fn layer_at(&mut self, z: i32, place: Place, blend: Blend, amount: f32, each: bool) -> &mut Frame {
        let same = |l: &BlendLayer| {
            l.z == z && l.place == place && l.blend == blend && l.amount.to_bits() == amount.to_bits() && l.each == each
        };
        let i = match self.layers.iter().position(same) {
            Some(i) => i,
            None => {
                self.layers.push(BlendLayer { blend, amount, each, z, place, frame: Frame::default() });
                self.layers.len() - 1
            }
        };
        let (width, height, time, video, camera) = (self.width, self.height, self.time, self.video, self.camera);
        let (bloom, exposure, on_screen) = (self.bloom, self.exposure, self.background_on_screen);
        let f = &mut self.layers[i].frame;
        (f.width, f.height, f.time, f.video, f.camera) = (width, height, time, video, camera);
        (f.bloom, f.exposure, f.background_on_screen) = (bloom, exposure, on_screen);
        f.ripples = self.ripples;
        f
    }

    /// The layer of what things paint normally over their blended parts (the layers of a ball
    /// above its Blend layer): a Normal layer at full amount, which [`Frame::layer_mut`] never
    /// makes. [`build_frame`] sorts it after every other layer of z-order 0, so it is mixed in
    /// last.
    pub fn over_layer_mut(&mut self) -> &mut Frame {
        self.over_mut(0, false)
    }

    /// The layer of what things of z-order `z` paint normally over their blended parts: `Over`,
    /// or `BehindOver` for things `behind` the balls (a shape's layers above its Blend layer).
    pub fn over_mut(&mut self, z: i32, behind: bool) -> &mut Frame {
        let place = if behind { Place::BehindOver } else { Place::Over };
        self.layer_at(z, place, Blend::Normal, 1.0, false)
    }

    /// Puts the layers in drawing order: by z-order, then by [`Place`], keeping the order they
    /// were first drawn into otherwise.
    pub(super) fn sort_layers(&mut self) {
        self.layers.sort_by_key(|l| (l.z, l.place));
    }

    /// The frame stuck-to-screen things draw into, made on first use with this frame's size and
    /// moment.
    pub fn overlay_mut(&mut self) -> &mut Frame {
        let (width, height, time, video) = (self.width, self.height, self.time, self.video);
        let ripples = self.ripples;
        let overlay = self.overlay.get_or_insert_with(Box::default);
        (overlay.width, overlay.height, overlay.time, overlay.video) = (width, height, time, video);
        overlay.ripples = ripples;
        overlay
    }

    /// Moves the overlay's shader effects and hit marks into this frame: those of stuck things
    /// work on the whole picture like everyone else's. So do its Ripples sources and the marks
    /// in the effects' ball list (its blend layers' too), taken from the screen to the world
    /// where they show through this frame's camera. A layer left with nothing to draw is dropped.
    pub fn lift_overlay_extras(&mut self) {
        let camera = self.camera;
        let Some(o) = self.overlay.as_deref_mut() else { return };
        let (effects, hits) = (std::mem::take(&mut o.effects), std::mem::take(&mut o.hits));
        let mut sources = Vec::new();
        let mut owners = self.ripple_owners as f32;
        o.gather_ripple_sources(&mut sources, &mut owners);
        let mut marks = std::mem::take(&mut o.ball_marks);
        for l in &mut o.layers {
            marks.append(&mut l.frame.ball_marks);
            l.frame.ripple_sources.clear();
            l.frame.ripple_owners = 0;
        }
        (o.ripple_sources, o.ripple_owners) = (Vec::new(), 0);
        o.layers.retain(|l| !l.frame.draws_nothing());
        self.effects.extend(effects);
        self.hits.extend(hits);
        let world = |x: f32, y: f32| camera.to_world(Vec2::new(x, y)).to_array();
        let zoom = if camera.zoom.is_finite() && camera.zoom > 0.0 { camera.zoom } else { 1.0 };
        for s in &mut sources {
            let (p, q, c) = (world(s.a[0], s.a[1]), world(s.a[2], s.a[3]), world(s.c[0], s.c[1]));
            (s.a, s.b[0], s.c) = ([p[0], p[1], q[0], q[1]], s.b[0] / zoom, [c[0], c[1], s.c[2], s.c[3]]);
        }
        self.ripple_sources.extend(sources);
        self.ripple_owners = owners as u32;
        for mut m in marks {
            (m.center, m.radius) = (world(m.center[0], m.center[1]), m.radius / zoom);
            self.ball_marks.push(m);
        }
    }

    /// Whether the overlay has anything to draw.
    pub fn has_overlay(&self) -> bool {
        self.overlay.as_ref().is_some_and(|o| {
            !(o.paths.is_empty()
                && o.flashes.is_empty()
                && o.layered.is_empty()
                && o.trails.is_empty()
                && o.particles.is_empty()
                && o.trails_front.is_empty()
                && o.particles_front.is_empty()
                && o.fields.is_empty()
                && o.fields_front.is_empty()
                && o.swarms.is_empty()
                && o.swarms_front.is_empty()
                && o.layers.is_empty()
                && o.liquids.is_empty())
        })
    }
}
