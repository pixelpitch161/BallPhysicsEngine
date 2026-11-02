//! Drawing the canvas: one canvas layer in the top frame, at the canvas's place and turn now. The
//! renderer replays the canvas's marks into its textures and draws them with its style. Skins
//! are drawn the same way, by their arena's or obstacle's own drawing part, through a mask of
//! the thing's shape.

use std::sync::atomic::{AtomicBool, Ordering};

use bpe_core::{Blend, Settings, ThingId};
use bpe_gfx::{
    CanvasLayer, CanvasPlace, DrawCx, Frame, MAX_MASK_SHAPES, MaskShape, PaintLook, PaintStyle, ThingDraw, clamp_texels,
};
use bpe_runtime::{MarkTarget, Solid, World};
use glam::Vec2;

use super::sim::canvas_area;
use super::skin::{SkinSettings, is_solid, line_reach, parts_local, skin_area, wall_local};
use super::{CanvasLayerPos, CanvasSettings, CanvasStyle, StartsAs};

pub struct CanvasDraw;

/// The renderer's place for the canvas's layer.
fn place(layer: CanvasLayerPos) -> CanvasPlace {
    match layer {
        CanvasLayerPos::BehindEverything => CanvasPlace::BehindEverything,
        CanvasLayerPos::BehindBalls => CanvasPlace::BehindBalls,
        CanvasLayerPos::OnTop => CanvasPlace::OnTop,
    }
}

fn style_of(style: CanvasStyle) -> PaintStyle {
    match style {
        CanvasStyle::Flat => PaintStyle::Flat,
        CanvasStyle::WetPaint => PaintStyle::WetPaint,
        CanvasStyle::Watercolour => PaintStyle::Watercolour,
        CanvasStyle::NeonInk => PaintStyle::NeonInk,
    }
}

/// `value` held to `lo..=hi`; a number that is not finite becomes `default`.
fn within(value: f32, lo: f32, hi: f32, default: f32) -> f32 {
    if value.is_finite() { value.clamp(lo, hi) } else { default }
}

#[allow(clippy::too_many_arguments)]
fn look_from(
    style: CanvasStyle,
    dry_time: f32,
    light_angle: f32,
    gloss: f32,
    grain: f32,
    edge_dark: f32,
    glow_size: f32,
    glow_strength: f32,
) -> PaintLook {
    let d = PaintLook::default();
    PaintLook {
        style: style_of(style),
        dry_time: within(dry_time, 0.0, 60.0, d.dry_time),
        light_angle: within(light_angle, 0.0, 360.0, d.light_angle),
        gloss: within(gloss, 0.0, 1.0, d.gloss),
        grain: within(grain, 0.0, 1.0, d.grain),
        edge_dark: within(edge_dark, 0.0, 1.0, d.edge_dark),
        glow_size: within(glow_size, 1.0, 200.0, d.glow_size),
        glow_strength: within(glow_strength, 0.0, 4.0, d.glow_strength),
    }
}

/// How a canvas's paint looks, from its settings.
pub(crate) fn paint_look_of(s: &CanvasSettings) -> PaintLook {
    look_from(s.style, s.dry_time, s.light_angle, s.gloss, s.grain, s.edge_dark, s.glow_size, s.glow_strength)
}

/// How a skin's paint looks, from its settings.
pub(crate) fn skin_look_of(s: &SkinSettings) -> PaintLook {
    look_from(
        s.skin_style,
        s.skin_dry_time,
        s.skin_light_angle,
        s.skin_gloss,
        s.skin_grain,
        s.skin_edge_dark,
        s.skin_glow_size,
        s.skin_glow_strength,
    )
}

impl ThingDraw for CanvasDraw {
    fn kind(&self) -> &'static str {
        "canvas"
    }

    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let Some((pose, size)) = canvas_area(cx.world, cx.thing.id) else { return };
        // The look comes from the live settings: none of it rebuilds the world.
        let settings = CanvasSettings::from_params(&cx.resolved.params);
        let detail = if settings.detail.is_finite() { settings.detail.max(0.0) } else { 1.0 };
        let texels = clamp_texels([(size.x * detail).round() as u32, (size.y * detail).round() as u32]);
        // Always the top frame's list: the renderer draws no other frame's canvases, and mixes
        // each in by its own blend.
        frame.canvases.push(CanvasLayer {
            key: MarkTarget::Canvas(cx.thing.id),
            center: pose.position.to_array(),
            angle: pose.angle,
            size: size.to_array(),
            texels,
            place: place(settings.layer),
            start: (settings.starts_as == StartsAs::Colour).then(|| settings.start_colour.to_linear()),
            fade: settings.fade.max(0.0),
            blend: settings.blend,
            opacity: settings.opacity.clamp(0.0, 1.0),
            look: paint_look_of(&settings),
            time: cx.time as f32,
            mask: Vec::new(),
        });
    }
}

/// Pushes the skin of the thing being drawn, when it has one, into the top frame: its paint
/// over its own line, shown only where the thing is this frame. Arenas and obstacles call it as
/// they draw; the renderer draws it after their shapes (see [`CanvasPlace::OnThing`]).
pub fn draw_skin(cx: &DrawCx, frame: &mut Frame) {
    let thing = cx.thing.id;
    let Some((pose, size)) = skin_area(cx.world, thing) else { return };
    // The look comes from the live settings, so a keyframed opacity follows.
    let skin = SkinSettings::from_params(&cx.resolved.params);
    let texels = clamp_texels([size.x.round() as u32, size.y.round() as u32]);
    frame.canvases.push(CanvasLayer {
        key: MarkTarget::Skin(thing),
        center: pose.position.to_array(),
        angle: pose.angle,
        size: size.to_array(),
        texels,
        place: CanvasPlace::OnThing,
        start: None,
        fade: skin.skin_fade.max(0.0),
        blend: Blend::Normal,
        opacity: skin.skin_opacity.clamp(0.0, 1.0),
        look: skin_look_of(&skin),
        time: cx.time as f32,
        mask: skin_mask(cx.world, thing, &skin),
    });
}

/// Said once: a skin had more pieces than its mask can hold.
static TOO_MANY_PIECES: AtomicBool = AtomicBool::new(false);

/// The shape `thing`'s skin shows through, in its own frame as it is now: a wall's line and an
/// obstacle's thin pieces as far as the paint reaches from them, its solid pieces all over. Past
/// [`MAX_MASK_SHAPES`] pieces (a big peg grid), the box around them all.
pub fn skin_mask(world: &World, thing: ThingId, skin: &SkinSettings) -> Vec<MaskShape> {
    if let Some(outline) = wall_local(world, thing) {
        return vec![MaskShape::line(&outline, line_reach(outline.thickness(), skin))];
    }
    let parts = parts_local(world, thing);
    if parts.len() > MAX_MASK_SHAPES {
        if !TOO_MANY_PIECES.swap(true, Ordering::Relaxed) {
            eprintln!(
                "warning: a skin has {} pieces, more than the {MAX_MASK_SHAPES} its mask holds; its paint shows over the box around them",
                parts.len()
            );
        }
        let (lo, hi) = parts.iter().fold((Vec2::INFINITY, Vec2::NEG_INFINITY), |(lo, hi), part| {
            let (centre, reach) = part.bounds();
            (lo.min(centre - Vec2::splat(reach)), hi.max(centre + Vec2::splat(reach)))
        });
        return vec![MaskShape::filled_box((lo + hi) * 0.5, (hi - lo) * 0.5, 0.0)];
    }
    parts
        .iter()
        .map(|part| match *part {
            Solid::Disc { center, radius } => MaskShape::disc(center, radius),
            // A polygon's side is a thin line: paint reaches past it by the skin width.
            Solid::Capsule { from, to, radius } if !is_solid(world, thing, part) => {
                MaskShape::capsule(from, to, line_reach(radius * 2.0, skin))
            }
            Solid::Capsule { from, to, radius } => MaskShape::capsule(from, to, radius),
            Solid::Ring { center, radius, thickness } => {
                let ring = bpe_runtime::Outline::Ring { center, radius, sides: 0, angle: 0.0, thickness };
                MaskShape::line(&ring, line_reach(thickness, skin))
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use bpe_core::{Params, Scene, SlotId, Value, resolve};
    use bpe_gfx::{Frame, MaskShape, build_frame};
    use bpe_runtime::{Log, MarkTarget, World};

    use crate::testing::{ARENA, BALLS, OBSTACLE, module_of, scene_with_obstacle};
    use crate::{Kit, fill_slot};

    /// The frame of `scene` at its start, its balls splatting what they hit when `splat`.
    fn frame(scene: &mut Scene, kit: &Kit, splat: bool) -> Frame {
        if splat {
            fill_slot(scene, &kit.registry, BALLS, SlotId::Marks).unwrap();
            let m = module_of(scene, &kit.registry, BALLS, SlotId::Marks);
            scene.library.modules.get_mut(&m).unwrap().params.set("splat_on", Value::Bool(true));
        }
        let resolved = resolve(scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let world = World::build(scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        frame
    }

    fn obstacle(f: impl FnOnce(&mut Params)) -> (Scene, Kit) {
        let (scene, _, kit) = scene_with_obstacle(f);
        (scene, kit)
    }

    #[test]
    fn skins_are_drawn_with_their_things_shape() {
        // No brush splats things: no skins, so nothing for the graphics card to keep.
        let (mut scene, kit) = obstacle(|_| {});
        assert!(frame(&mut scene, &kit, false).canvases.is_empty());

        // The arena's line reaches its half width times (1 + skin width) either side; a peg is
        // covered all over. Both are drawn on their things, at their poses.
        let (mut scene, kit) = obstacle(|_| {});
        scene.things.get_mut(&ARENA).unwrap().params.set("thickness", Value::Number(10.0));
        scene.things.get_mut(&ARENA).unwrap().params.set("skin_width", Value::Number(2.0));
        scene.things.get_mut(&ARENA).unwrap().params.set("skin_opacity", Value::Number(0.25));
        let f = frame(&mut scene, &kit, true);
        let keys: Vec<_> = f.canvases.iter().map(|c| c.key).collect();
        assert_eq!(keys, [MarkTarget::Skin(ARENA), MarkTarget::Skin(OBSTACLE)]);
        let arena = &f.canvases[0];
        assert_eq!((arena.place, arena.start, arena.opacity), (bpe_gfx::CanvasPlace::OnThing, None, 0.25));
        assert_eq!(arena.mask.len(), 1);
        assert_eq!((arena.mask[0].kind, arena.mask[0].a[3], arena.mask[0].b[1]), (MaskShape::RING, 508.0, 15.0));
        let peg = &f.canvases[1].mask;
        assert_eq!(peg.len(), 1);
        assert_eq!((peg[0].kind, peg[0].a[2]), (MaskShape::DISC, 30.0));

        // A polygon's sides are thin lines, reaching 4 x (1 + 2) by the default Skin width; a
        // bar is solid.
        let (mut scene, kit) = obstacle(|p| {
            p.set("shape", Value::Choice("polygon".into()));
            p.set("sides", Value::Int(5));
            p.set("thickness", Value::Number(8.0));
        });
        let sides = frame(&mut scene, &kit, true).canvases[1].mask.clone();
        assert_eq!(sides.len(), 5);
        assert!(sides.iter().all(|s| s.kind == MaskShape::CAPSULE && s.b[0] == 12.0), "{sides:?}");
        let (mut scene, kit) = obstacle(|p| p.set("shape", Value::Choice("bar".into())));
        let bar = frame(&mut scene, &kit, true).canvases[1].mask.clone();
        assert_eq!((bar.len(), bar[0].kind, bar[0].b[0]), (1, MaskShape::CAPSULE, 4.0));

        // A grid of more pegs than a mask holds shows through the box around them.
        let (mut scene, kit) = obstacle(|p| {
            p.set("shape", Value::Choice("peg_grid".into()));
            p.set("columns", Value::Int(20));
            p.set("rows", Value::Int(15));
            p.set("radius", Value::Number(5.0));
            p.set("spacing", Value::Number(20.0));
            p.set("stagger", Value::Bool(false));
        });
        let f = frame(&mut scene, &kit, true);
        let grid = &f.canvases[1].mask;
        assert_eq!(grid.len(), 1);
        assert_eq!((grid[0].kind, grid[0].b[3]), (MaskShape::BOX, 1.0), "a filled box");
        // 20 columns 20 px apart with pegs of 5 px: 390 px across.
        assert!((grid[0].b[0] - 195.0).abs() < 0.5 && (grid[0].b[1] - 145.0).abs() < 0.5, "{:?}", grid[0]);
    }

    #[test]
    fn skins_show_whatever_their_things_look_blend() {
        // A blended, see-through or stuck-to-screen Look draws the arena into a layer or the
        // overlay; its skin still lands in the top frame, the only one whose canvases are drawn.
        let cases: [(&str, f64, bool); 3] = [("add", 1.0, false), ("normal", 0.5, false), ("normal", 1.0, true)];
        for (blend, amount, screen) in cases {
            let (mut scene, kit) = obstacle(|_| {});
            fill_slot(&mut scene, &kit.registry, ARENA, SlotId::Look).unwrap();
            let look = module_of(&scene, &kit.registry, ARENA, SlotId::Look);
            let params = &mut scene.library.modules.get_mut(&look).unwrap().params;
            params.set("blend", Value::Choice(blend.into()));
            params.set("blend_amount", Value::Number(amount));
            if screen {
                scene.things.get_mut(&ARENA).unwrap().screen = Some(true);
            }
            let f = frame(&mut scene, &kit, true);
            let keys: Vec<_> = f.canvases.iter().map(|c| c.key).collect();
            assert!(keys.contains(&MarkTarget::Skin(ARENA)), "{blend} {amount} {screen}: {keys:?}");
            assert!(f.layers.iter().all(|l| l.frame.canvases.is_empty()), "{blend} {amount}");
            assert!(f.overlay.as_ref().is_none_or(|o| o.canvases.is_empty()), "{screen}");
        }
    }

    #[test]
    fn canvas_style_reaches_the_frame() {
        use crate::testing::{CANVAS, painted};
        use bpe_gfx::{PaintLook, PaintStyle};
        let (mut scene, _, kit) = painted(false, |_| {});
        let params = &mut scene.things.get_mut(&CANVAS).unwrap().params;
        params.set("style", Value::Choice("wet_paint".into()));
        params.set("gloss", Value::Number(0.3));
        params.set("dry_time", Value::Number(2.0));
        let f = frame(&mut scene, &kit, false);
        let look = f.canvases.iter().find(|c| c.key == MarkTarget::Canvas(CANVAS)).unwrap().look;
        assert_eq!(look, PaintLook { style: PaintStyle::WetPaint, gloss: 0.3, dry_time: 2.0, ..PaintLook::default() });
    }

    #[test]
    fn skin_style_reaches_the_frame() {
        use bpe_gfx::PaintStyle;
        let (mut scene, kit) = obstacle(|_| {});
        let params = &mut scene.things.get_mut(&ARENA).unwrap().params;
        params.set("skin_style", Value::Choice("neon_ink".into()));
        params.set("skin_glow_size", Value::Number(30.0));
        let f = frame(&mut scene, &kit, true);
        let look = f.canvases.iter().find(|c| c.key == MarkTarget::Skin(ARENA)).unwrap().look;
        assert_eq!((look.style, look.glow_size), (PaintStyle::NeonInk, 30.0));
    }

    #[test]
    fn looks_are_held_to_their_ranges() {
        use super::{paint_look_of, skin_look_of};
        use crate::things::canvas::{CanvasSettings, SkinSettings};
        use bpe_core::Settings;
        let mut p = Params::default();
        p.set("gloss", Value::Number(9.0));
        p.set("glow_size", Value::Number(f64::NAN));
        p.set("dry_time", Value::Number(-3.0));
        let look = paint_look_of(&CanvasSettings::from_params(&p));
        assert_eq!((look.gloss, look.dry_time), (1.0, 0.0));
        assert_eq!(look.glow_size, bpe_gfx::PaintLook::default().glow_size);
        let _ = skin_look_of(&SkinSettings::from_params(&Params::default()));
    }
}
