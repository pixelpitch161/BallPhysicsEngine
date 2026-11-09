//! Building a frame: each thing's and card's drawing part ([`ThingDraw`], [`ModuleDraw`]) draws into
//! it, routed into its blend layer.

use super::*;

/// The z-order `thing` draws at: its own, within [`Thing::Z_ORDER_RANGE`]; things stuck to the
/// screen draw over the world anyway and keep 0.
pub(super) fn z_order(thing: &Thing, r: &ResolvedThing) -> i32 {
    if r.screen { 0 } else { thing.z_order.clamp(*Thing::Z_ORDER_RANGE.start(), *Thing::Z_ORDER_RANGE.end()) }
}

/// What a thing's drawing part can read.
pub struct DrawCx<'a> {
    pub thing: &'a Thing,
    pub resolved: &'a ResolvedThing,
    /// The whole resolved scene as it is at this moment, keyframes and pulses applied.
    pub scene: &'a Resolved,
    pub world: &'a World,
    pub log: &'a Log,
    /// The moment being drawn, in seconds: the clock of the world, the log and the keyframes.
    pub time: f64,
    /// The scene as saved: its tree (a thing's parent, siblings and order) and its keyframes.
    pub tree: &'a Scene,
    /// The scene's light probe, when some look follows light (lighting part 7: a ball's place in
    /// a Light or Shadow spread). Never read by anything that makes light.
    pub light: Option<&'a dyn LightAt>,
    /// How far each sound is up at a moment (Follow sound), when some setting follows sound: a
    /// Text's Follow sound move reads each letter's level at its own moment through it.
    pub sound: Option<&'a dyn SoundAt>,
}

/// How a thing kind draws itself.
pub trait ThingDraw: Send + Sync + 'static {
    /// The thing kind's id, as registered in `bpe_core`.
    fn kind(&self) -> &'static str;
    fn draw(&self, cx: &DrawCx, frame: &mut Frame);
    /// How the thing mixes with what is under it (blend, amount, blend with each other), when it
    /// says so itself rather than through its Look card's `blend` settings.
    fn blend(&self, _resolved: &ResolvedThing) -> Option<(Blend, f32, bool)> {
        None
    }
    /// Whether the thing sits behind the balls (a shape): when it blends, its layer is mixed in
    /// before the balls are drawn, so it never tints them.
    fn behind(&self) -> bool {
        false
    }
    /// What the thing draws over its blended part, painted normally whatever its blend: the
    /// layers of a ball above its Blend layer. Drawn right after [`ThingDraw::draw`].
    fn draw_over(&self, _cx: &DrawCx, _frame: &mut Frame) {}
    /// What the thing draws round itself, each with a blend of its own (its trails and
    /// particles), bottom first: each one's blend, amount, and whether it sits behind the balls.
    /// Normal (at any amount) and Add draw straight into the frame, mixing themselves in; any
    /// other blend draws into a layer of its own. Drawn after every thing has drawn itself.
    fn around(&self, _resolved: &ResolvedThing) -> Vec<Around> {
        Vec::new()
    }
    /// Draws the `index`th of what [`ThingDraw::around`] lists.
    fn draw_around(&self, _cx: &DrawCx, _index: usize, _frame: &mut Frame) {}
    /// Adds the thing's lights to a lit frame's `scene`. Called only when the scene has a
    /// Lighting card (its draw set [`Frame::light`]), after every thing has drawn itself.
    fn light(&self, _cx: &DrawCx, _scene: &mut LightScene) {}
}

/// One of the things a thing draws round itself (a trail, particles), with its own blend.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Around {
    pub blend: Blend,
    pub amount: f32,
    /// Behind the balls: a layer of its own is mixed in before they are drawn.
    pub behind: bool,
}

/// How a module kind draws for every thing whose slot it fills: a trail behind the balls, a
/// burst where they hit, a shader over them. Runs after every thing has drawn itself.
pub trait ModuleDraw: Send + Sync + 'static {
    /// The module kind's id, as registered in `bpe_core`.
    fn kind(&self) -> &'static str;
    /// `slot` is the slot this module fills on `cx.thing`; its settings are
    /// `cx.resolved.slots[&slot].params`.
    fn draw(&self, cx: &DrawCx, slot: SlotId, frame: &mut Frame);
    /// Whether the module mixes Normal (at any amount) and Add in by itself, drawing straight
    /// into the frame; any other blend still goes into a layer.
    fn draws_add_itself(&self) -> bool {
        false
    }
}

/// Every registered drawing part, by kind id.
#[derive(Default)]
pub struct Draws {
    pub(super) things: BTreeMap<&'static str, Box<dyn ThingDraw>>,
    pub(super) modules: BTreeMap<&'static str, Box<dyn ModuleDraw>>,
}

impl Draws {
    pub fn add(&mut self, d: impl ThingDraw) {
        let id = d.kind();
        assert!(!self.things.contains_key(id), "draw registered twice: {id}");
        self.things.insert(id, Box::new(d));
    }

    pub fn add_module(&mut self, d: impl ModuleDraw) {
        let id = d.kind();
        assert!(!self.modules.contains_key(id), "module draw registered twice: {id}");
        self.modules.insert(id, Box::new(d));
    }

    pub fn thing(&self, kind: &str) -> Option<&dyn ThingDraw> {
        self.things.get(kind).map(|d| d.as_ref())
    }

    pub fn module(&self, kind: &str) -> Option<&dyn ModuleDraw> {
        self.modules.get(kind).map(|d| d.as_ref())
    }
}

/// Fills `frame` for `time`: the lists are emptied, the scene's keyframes and pulses are
/// applied to a copy of the resolved scene for this moment, then every resolved thing in id
/// order whose kind has a drawing part draws, then every module with a drawing part draws for
/// each thing (in id order) and slot (in slot order) it fills. Things stuck to the screen draw
/// into the overlay. A thing follows its look's blend and a module its own: anything but Normal
/// at full amount goes into a layer (see [`Frame::layer_mut`]), except Normal and Add on a module
/// that [draws them itself](ModuleDraw::draws_add_itself). The root, the scene, never blends.
pub fn build_frame(
    scene: &Scene,
    resolved: &Resolved,
    world: &World,
    log: &Log,
    time: f64,
    draws: &Draws,
    frame: &mut Frame,
) {
    build_frame_with(scene, resolved, world, log, time, draws, None, frame);
}

/// [`build_frame`] with the scene's sound, for settings that follow it.
#[allow(clippy::too_many_arguments)]
pub fn build_frame_with(
    scene: &Scene,
    resolved: &Resolved,
    world: &World,
    log: &Log,
    time: f64,
    draws: &Draws,
    sound: Option<&dyn SoundAt>,
    frame: &mut Frame,
) {
    build_frame_sensing(scene, resolved, world, log, time, draws, sound, None, frame);
}

/// [`build_frame_with`] with the scene's light probe too, for looks that follow light (Follow
/// light, Light and Shadow spreads). The lights themselves never follow light: the editor
/// doesn't offer it on anything that makes light.
#[allow(clippy::too_many_arguments)]
pub fn build_frame_sensing(
    scene: &Scene,
    resolved: &Resolved,
    world: &World,
    log: &Log,
    time: f64,
    draws: &Draws,
    sound: Option<&dyn SoundAt>,
    light: Option<&dyn LightAt>,
    frame: &mut Frame,
) {
    frame.clear();
    let animation = Animation::new(scene, resolved);
    let animated;
    // What the scene's rules changed (rules part 1): settings, then broken things are gone.
    let ruled = world.rules().filter(|r| r.changes_picture());
    // Gradients' companions (Bands, Grain) are applied once a frame, after the animation.
    let resolved = if animation.is_empty() && !bpe_core::gradient_extras::any_in(resolved) && ruled.is_none() {
        resolved
    } else {
        let mut copy = resolved.clone();
        let levels = sound.map(|sound| LevelsAt { sound, world, log });
        let lit = light.map(|light| LightLevelsAt { light, world, log });
        let inputs = Inputs {
            hits: hit_strength(log, time, world.steps_per_second),
            sound: levels.as_ref().map(|l| l as &dyn bpe_core::SoundLevels),
            light: lit.as_ref().map(|l| l as &dyn bpe_core::LightLevels),
            health: world.health_levels(),
        };
        animation.apply(&mut copy, time, &inputs);
        if let Some(rules) = ruled {
            rules.apply_whole(&mut copy, time, |_| true);
            // A broken thing whose Break look still plays out stays, for its look to draw.
            for id in rules.broken() {
                if !rules.showing(id, time) {
                    copy.things.remove(&id);
                }
            }
        }
        bpe_core::gradient_extras::apply_moves(&mut copy, time);
        animated = copy;
        &animated
    };
    // Things push what Ripples measure from only when some gradient is laid out as Ripples.
    frame.ripples = bpe_core::gradient_extras::any_ripples(resolved);
    for (id, r) in &resolved.things {
        let Some(thing) = scene.things.get(id) else { continue };
        let Some(d) = draws.thing(&r.kind) else { continue };
        let ((blend, amount), each) = match (r.slots.get(&SlotId::Look), d.blend(r)) {
            _ if *id == ThingId::ROOT => ((Blend::Normal, 1.0), false),
            (_, Some((blend, amount, each))) => ((blend, amount), each),
            (Some(look), None) => (Blend::from_params(&look.params), Blend::each_from_params(&look.params)),
            (None, None) => ((Blend::Normal, 1.0), false),
        };
        let target = if r.screen { frame.overlay_mut() } else { &mut *frame };
        let cx = DrawCx { thing, resolved: r, scene: resolved, world, log, time, tree: scene, light, sound };
        let z = z_order(thing, r);
        draw_routed_at(target, z, blend, amount, each, d.behind(), |into| d.draw(&cx, into));
        // Only a thing that blends has parts to paint over the blend: into the over layer of its
        // z-order, which is mixed in after its blend layers (a shape's before the balls).
        if layered(blend, amount) {
            let target = if r.screen { frame.overlay_mut() } else { &mut *frame };
            draw_over_routed(target, z, d.behind(), |into| d.draw_over(&cx, into));
        }
    }
    for (id, r) in &resolved.things {
        let Some(thing) = scene.things.get(id) else { continue };
        let Some(d) = draws.thing(&r.kind) else { continue };
        for (index, around) in d.around(r).into_iter().enumerate() {
            let (blend, amount) = match around.blend {
                Blend::Normal | Blend::Add => (Blend::Normal, 1.0),
                other => (other, around.amount),
            };
            let target = if r.screen { frame.overlay_mut() } else { &mut *frame };
            let cx = DrawCx { thing, resolved: r, scene: resolved, world, log, time, tree: scene, light, sound };
            let z = z_order(thing, r);
            draw_routed_at(target, z, blend, amount, false, around.behind, |into| d.draw_around(&cx, index, into));
        }
    }
    for (id, r) in &resolved.things {
        let Some(thing) = scene.things.get(id) else { continue };
        for slot in SlotId::ALL {
            let Some(s) = r.slots.get(&slot) else { continue };
            let Some(d) = draws.module(&s.kind) else { continue };
            let (mut blend, mut amount) = Blend::from_params(&s.params);
            if d.draws_add_itself() && matches!(blend, Blend::Normal | Blend::Add) {
                (blend, amount) = (Blend::Normal, 1.0);
            }
            let each = Blend::each_from_params(&s.params);
            let target = if r.screen { frame.overlay_mut() } else { &mut *frame };
            let cx = DrawCx { thing, resolved: r, scene: resolved, world, log, time, tree: scene, light, sound };
            let z = z_order(thing, r);
            draw_routed_at(target, z, blend, amount, each, false, |into| d.draw(&cx, slot, into));
        }
    }
    // A lit scene's lights, only when its Lighting card set `frame.light`.
    if let Some(mut lit) = frame.light.take() {
        for (id, r) in &resolved.things {
            let Some(thing) = scene.things.get(id) else { continue };
            let Some(d) = draws.thing(&r.kind) else { continue };
            // What makes light never reads the light probe (one-way sensing).
            let cx = DrawCx { thing, resolved: r, scene: resolved, world, log, time, tree: scene, light: None, sound };
            d.light(&cx, &mut lit);
        }
        // Light pipes (glass and light part 4a) carry the light every thing has made.
        lit.light_pipes();
        lit.glows = frame.gives_light();
        frame.light = Some(lit);
    }
    frame.sort_layers();
    if let Some(o) = frame.overlay.as_deref_mut() {
        o.sort_layers();
    }
    frame.camera = world.camera;
    for l in &mut frame.layers {
        l.frame.camera = world.camera;
    }
    // Masks stuck to things go where the moved Stage camera shows those things (colour grading
    // part 5): their places were worked out on the flat picture.
    if let Some(view) = frame.stage_view().filter(|v| v.motion > 0.0) {
        frame.grade.see_through(&view);
    }
    frame.lift_overlay_extras();
    frame.fit_lists();
}
