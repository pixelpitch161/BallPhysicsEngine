//! The Look's "Trails and particles" list as the drawing reads it: each shown trail or
//! particles layer with its settings and its sub-layers, bottom first.

use bpe_core::{Blend, LayerId, ResolvedSlot, Settings};

use super::arcs::{ARCS, ArcsLayer};
use super::aura::{AURA, AuraLayer};
use super::gpu::SubLayer;
use super::particles::{PARTICLES, ParticlesLayer, SHAPE_PARTICLES, ShapeParticlesLayer};
use super::trail::{AroundPlace, TRAIL, TrailKind, TrailLayer};
use super::wavefront::{WAVEFRONT, WavefrontLayer};

/// What one around layer is.
#[derive(Clone, Debug, PartialEq)]
pub enum AroundKind {
    Trail(Box<TrailLayer>),
    /// A ball's particles, or a shape's read as a ball's.
    Particles(Box<ParticlesLayer>),
    /// A line through each round's balls.
    Wavefront(Box<WavefrontLayer>),
    /// Lightning between near balls.
    Arcs(Box<ArcsLayer>),
    /// One field round all the balls.
    Aura(Box<AuraLayer>),
}

/// One shown trail or particles layer.
#[derive(Clone, Debug, PartialEq)]
pub struct AroundLayer {
    pub id: LayerId,
    pub kind: AroundKind,
    /// Its shown sub-layers, in their order (worked first to last).
    pub subs: Vec<SubLayer>,
    /// The ball numbers that wear it (empty for every ball).
    pub only: Vec<u32>,
    /// How it takes the scene's light ([`super::lighting::of`]), its Lit taken away when it gives
    /// off light.
    pub light: [f32; 4],
    /// The light it gives off (its Gives off light sub-layers), when it gives any.
    pub emit: Option<super::gives_light::Emit>,
    /// Its UV and how it catches beams ([`super::lighting::extra_of`], no link number yet).
    pub more: [f32; 4],
}

/// Where a frame's trail and particle lists ended before an around layer drew, so what it drew
/// can be given its Lighting after ([`Drew::light`]).
#[derive(Clone, Copy, Debug)]
pub struct Drew([usize; 4]);

impl Drew {
    pub fn from(frame: &bpe_gfx::Frame) -> Drew {
        Drew([frame.trails.len(), frame.trails_front.len(), frame.particles.len(), frame.particles_front.len()])
    }

    /// Gives everything drawn since [`Drew::from`] `layer`'s Lighting (the light it gives off
    /// is stamped as it is drawn, on the layer's own pieces only: [`super::around_subs::Pieces::give_light`]),
    /// and in a lit scene its UV, how it catches beams and its thing's link number.
    pub fn light(self, frame: &mut bpe_gfx::Frame, layer: &AroundLayer, cx: &bpe_gfx::DrawCx) {
        let [t, tf, p, pf] = self.0;
        if let Some(more) = layer.more_in(cx, frame.light.as_mut()) {
            frame.trails.iter_mut().skip(t).for_each(|v| v.extra = more);
            frame.trails_front.iter_mut().skip(tf).for_each(|v| v.extra = more);
            frame.particles.iter_mut().skip(p).for_each(|s| s.extra = more);
            frame.particles_front.iter_mut().skip(pf).for_each(|s| s.extra = more);
        }
        let light = layer.light;
        if light != [0.0; 4] {
            let two = [light[0], light[1]];
            frame.trails.iter_mut().skip(t).for_each(|v| v.light = two);
            frame.trails_front.iter_mut().skip(tf).for_each(|v| v.light = two);
            frame.particles.iter_mut().skip(p).for_each(|s| s.light = [light[0], light[1], 0.0, 0.0]);
            frame.particles_front.iter_mut().skip(pf).for_each(|s| s.light = [light[0], light[1], 0.0, 0.0]);
        }
    }
}

impl AroundLayer {
    /// Its UV, catching beams and link number as its pieces carry them, in a lit scene (`light`
    /// the frame's lighting, which learns that something glows under UV or catches beams);
    /// `None` without a Lighting card.
    pub fn more_in(&self, cx: &bpe_gfx::DrawCx, light: Option<&mut bpe_gfx::LightScene>) -> Option<[f32; 4]> {
        let light = light?;
        light.uv |= self.more[0] > 0.0;
        light.smoke |= self.more[2] > 0.0;
        let link = crate::things::light::links::number_of(cx.scene, cx.thing.id);
        Some([self.more[0], self.more[1], self.more[2], link])
    }

    /// Behind the balls or in front of them.
    pub fn place(&self) -> AroundPlace {
        match &self.kind {
            AroundKind::Trail(t) => t.place,
            AroundKind::Particles(p) => p.place,
            AroundKind::Wavefront(w) => w.place,
            AroundKind::Arcs(a) => a.place,
            AroundKind::Aura(a) => a.place,
        }
    }

    /// Its own blend and amount. Ghosts have none: they are copies of the ball, blending as it does.
    pub fn blend(&self) -> (Blend, f32) {
        match &self.kind {
            AroundKind::Trail(t) => (t.blend, t.blend_amount),
            AroundKind::Particles(p) => (p.blend, p.blend_amount),
            AroundKind::Wavefront(w) => (w.blend, w.blend_amount),
            AroundKind::Arcs(a) => (a.blend, a.blend_amount),
            AroundKind::Aura(a) => (a.blend, a.blend_amount),
        }
    }

    /// Whether it is a Ghosts trail, which the ball draws with itself.
    pub fn is_ghosts(&self) -> bool {
        matches!(&self.kind, AroundKind::Trail(t) if t.kind == TrailKind::Ghosts)
    }

    /// Whether it is particles made of mini balls, which draw as balls.
    pub fn is_mini_balls(&self) -> bool {
        matches!(&self.kind, AroundKind::Particles(p) if p.made_of == super::particles::MadeOf::MiniBalls)
    }

    /// Whether ball number `number` wears it.
    pub fn applies(&self, number: u32) -> bool {
        self.only.is_empty() || self.only.contains(&number)
    }
}

/// Every shown trail and particles layer of the Look card `slot`, bottom first, each with its
/// shown sub-layers; `shape` reads a shape's (sizes in pixels).
pub fn around_layers(slot: &ResolvedSlot, shape: bool) -> Vec<AroundLayer> {
    let mut out: Vec<AroundLayer> = Vec::new();
    for layer in &slot.layers {
        let p = slot.layer_params(layer.id);
        let kind = match layer.kind.as_str() {
            TRAIL if !shape => AroundKind::Trail(Box::new(TrailLayer::from_params(&p))),
            PARTICLES if !shape => AroundKind::Particles(Box::new(ParticlesLayer::from_params(&p))),
            WAVEFRONT if !shape => AroundKind::Wavefront(Box::new(WavefrontLayer::from_params(&p))),
            ARCS if !shape => AroundKind::Arcs(Box::new(ArcsLayer::from_params(&p))),
            AURA if !shape => AroundKind::Aura(Box::new(AuraLayer::from_params(&p))),
            SHAPE_PARTICLES if shape => AroundKind::Particles(Box::new(ShapeParticlesLayer::from_params(&p).as_ball())),
            kind => {
                // A sub-layer of one of them.
                if let Some(parent) = layer.parent
                    && let Some(a) = out.iter_mut().find(|a| a.id == parent)
                {
                    let sub = if shape {
                        crate::modules::look::shape::gpu::shape_sub(kind, &p)
                    } else {
                        SubLayer::of_ball(kind, &p)
                    };
                    a.subs.extend(sub);
                }
                continue;
            }
        };
        let light = super::lighting::of(&p);
        let more = super::lighting::extra_of(&p, 0.0, true);
        out.push(AroundLayer {
            id: layer.id,
            kind,
            subs: Vec::new(),
            only: layer.only.clone(),
            light,
            emit: None,
            more,
        });
    }
    // Gives off light makes its layer a lamp: it shows its own colours, unlit.
    for a in &mut out {
        a.emit = super::gives_light::Emit::of_subs(&a.subs);
        a.light = super::gives_light::unlit_if_lamp(a.light, a.emit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::look::layers::{SUB_GLOW, SUB_RECOLOUR};
    use crate::registry;
    use bpe_core::{SlotId, ThingId};

    #[test]
    fn the_list_reads_each_layer_and_its_sub_layers_in_order() {
        let reg = registry();
        let mut scene = crate::new_studio_scene(&reg);
        let ball = crate::add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
        let trail = scene.add_layer(&reg, ball, SlotId::Look, TRAIL).unwrap();
        let burst = scene.add_layer(&reg, ball, SlotId::Look, PARTICLES).unwrap();
        scene.add_sub_layer(&reg, ball, SlotId::Look, trail, SUB_GLOW).unwrap();
        scene.add_sub_layer(&reg, ball, SlotId::Look, trail, SUB_RECOLOUR).unwrap();
        let resolved = bpe_core::resolve(&scene, &reg);
        let look = &resolved.things[&ball].slots[&SlotId::Look];
        let list = around_layers(look, false);
        assert_eq!(list.iter().map(|a| a.id).collect::<Vec<_>>(), [trail, burst]);
        assert_eq!(list[0].subs.len(), 2);
        assert!(matches!(list[0].subs[0], SubLayer::Glow(_)) && matches!(list[0].subs[1], SubLayer::Recolour(_)));
        assert!(list[1].subs.is_empty());
        assert_eq!(list[0].place(), AroundPlace::Behind);
        assert!(!list[0].is_ghosts() && list[0].applies(7));
        assert!(around_layers(look, true).is_empty(), "a ball's layers aren't a shape's");
    }
}
