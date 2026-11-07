//! How a ball or a shape takes light (lighting spec part 2a): the **Surface** section at the top of
//! its Look card. A Surface (Matte, Plastic, Chrome, Metal, Glass, Toon) is a preset of the same
//! numbers, all editable: Roughness, Metal, Clear coat, Bends light, See-through, Toon steps,
//! Reflects; and Bulge, how round a ball stands to the light. Nothing here changes the picture
//! until the scene has a Lighting card.

use bpe_core::{Choice, Color, LayerId, Params, ResolvedSlot, SettingDef, Settings, ThingId, Value};
use bpe_gfx::SurfaceGpu;

/// What a surface is like: each a preset of the numbers below it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum SurfaceKind {
    /// Takes light evenly, no shine (as lighting always drew).
    #[default]
    Matte,
    /// A soft highlight and a faint mirror of the room at the edges.
    Plastic,
    /// A perfect mirror tinted by its colour.
    Chrome,
    /// A blurred mirror tinted by its colour.
    Metal,
    /// Clear glass: shows what is behind it, bent, with sharp highlights and lighter, coloured shadows.
    Glass,
    /// Cartoon: light in flat bands and a hard highlight.
    Toon,
    /// Jelly and wax: light soaks in and glows out of the far side; tinted shadows.
    Jelly,
    /// Pearl and soap bubble: swirling thin-film colours (raise See-through for a bubble).
    Pearl,
    /// Car paint: tiny sparkling flakes under a glossy clear coat.
    Flake,
    /// Brushed or spun metal: a long, stretched highlight along fine lines.
    Brushed,
    /// Frosted glass: blurs what is behind it.
    Frosted,
    /// Velvet: no shine, bright rims.
    Velvet,
    /// Lava and heat: glows from inside.
    Lava,
}

/// A Surface's preset numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preset {
    pub roughness: f32,
    pub metal: f32,
    pub clear_coat: f32,
    pub bends_light: f32,
    pub see_through: f32,
    pub toon_steps: u32,
    pub rainbow: f32,
    pub soak: f32,
    pub film: f32,
    pub flakes: f32,
    pub flake_size: f32,
    pub brushing: f32,
    pub frost: f32,
    pub rim: f32,
    pub inner_glow: f32,
}

impl Preset {
    /// Shine and nothing else: every advanced number 0.
    const fn base(roughness: f32, metal: f32, clear_coat: f32, bends_light: f32, see_through: f32) -> Preset {
        Preset {
            roughness,
            metal,
            clear_coat,
            bends_light,
            see_through,
            toon_steps: 0,
            rainbow: 0.0,
            soak: 0.0,
            film: 0.0,
            flakes: 0.0,
            flake_size: 0.12,
            brushing: 0.0,
            frost: 0.0,
            rim: 0.0,
            inner_glow: 0.0,
        }
    }
}

impl SurfaceKind {
    /// Its numbers.
    pub fn preset(self) -> Preset {
        let base = Preset::base;
        match self {
            Self::Matte => base(1.0, 0.0, 0.0, 1.0, 0.0),
            Self::Plastic => base(0.3, 0.0, 0.0, 1.5, 0.0),
            Self::Chrome => base(0.05, 1.0, 0.0, 1.5, 0.0),
            Self::Metal => base(0.35, 1.0, 0.0, 1.5, 0.0),
            Self::Glass => Preset { rainbow: 0.3, ..base(0.02, 0.0, 0.6, 1.5, 0.85) },
            Self::Toon => Preset { toon_steps: 3, ..base(0.35, 0.0, 0.0, 1.5, 0.0) },
            Self::Jelly => Preset { soak: 0.8, ..base(0.35, 0.0, 0.3, 1.4, 0.35) },
            Self::Pearl => Preset { soak: 0.1, film: 0.55, ..base(0.2, 0.0, 0.6, 1.6, 0.0) },
            Self::Flake => Preset { flakes: 0.7, ..base(0.4, 0.3, 1.0, 1.5, 0.0) },
            Self::Brushed => Preset { brushing: 0.8, ..base(0.3, 1.0, 0.0, 1.5, 0.0) },
            Self::Frosted => Preset { frost: 0.5, ..base(0.45, 0.0, 0.0, 1.5, 0.85) },
            Self::Velvet => Preset { rim: 1.2, ..base(1.0, 0.0, 0.0, 1.0, 0.0) },
            Self::Lava => Preset { soak: 0.3, inner_glow: 1.5, ..base(0.6, 0.0, 0.0, 1.5, 0.0) },
        }
    }
}

/// What a shiny surface mirrors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Reflects {
    /// The balls round it, the wall and the floor (and past them the sky).
    #[default]
    TheScene,
    /// The sky's two colours (the Lighting card's Room), as bright as the room.
    ASkyGradient,
    /// Nothing: only the lamps' highlights.
    Nothing,
}

impl Reflects {
    /// Its number in lighting.wgsl.
    pub fn code(self) -> f32 {
        match self {
            Self::Nothing => 0.0,
            Self::ASkyGradient => 1.0,
            Self::TheScene => 2.0,
        }
    }
}

/// Which way brushed lines run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum BrushLines {
    /// Round and round the middle, like spun metal.
    #[default]
    Spun,
    /// Straight across, at Brush angle.
    Straight,
}

/// Which side of a shape's outline shows the other side mirrored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum MirrorSide {
    /// Not a mirror.
    #[default]
    Off,
    /// Outside the shape shows the inside, reflected.
    Outside,
    /// Inside the shape shows the outside, reflected.
    Inside,
    /// Each side shows the other.
    BothSides,
}

impl MirrorSide {
    /// Its number in lighting.wgsl (0 off, 1 outside, 2 inside, 3 both).
    pub fn code(self) -> f32 {
        match self {
            Self::Off => 0.0,
            Self::Outside => 1.0,
            Self::Inside => 2.0,
            Self::BothSides => 3.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct SurfaceSettings {
    /// What the surface is like. Picking one sets the numbers below, which stay editable.
    #[setting(label = "Surface", group = "Surface")]
    pub surface: SurfaceKind,
    /// How blurred its highlights and mirror are: 0 a perfect mirror, 1 no shine to speak of.
    #[setting(label = "Roughness", span(0, 1), keyframe, group = "Surface")]
    pub roughness: f32,
    /// 0 plastic-like (white highlights over its colour), 1 metal (its colour is its mirror).
    #[setting(label = "Metal", span(0, 1), keyframe, group = "Surface")]
    pub metal: f32,
    /// A clear, glossy coat over it.
    #[setting(label = "Clear coat", span(0, 1), keyframe, group = "Surface")]
    pub clear_coat: f32,
    /// How much it bends light (its index of refraction): 1 none, so no shine at all; 1.5 glass
    /// and plastic; more shines harder and bends what is seen through it more.
    #[setting(label = "Bends light", range(1, 1e9), span(1, 2.5), keyframe, group = "Surface")]
    pub bends_light: f32,
    /// How much light goes through it: what is behind shows through, bent.
    #[setting(label = "See-through", span(0, 1), keyframe, group = "Surface")]
    pub see_through: f32,
    /// Light in this many flat bands, like a cartoon: 0 smooth.
    #[setting(label = "Toon steps", range(0, 1e9), span(0, 8), group = "Surface")]
    pub toon_steps: u32,
    /// How much what is seen through it splits into rainbow colours at its edges.
    #[setting(label = "Rainbow split", when = "see_through", span(0, 1), keyframe, group = "Surface")]
    pub rainbow: f32,
    /// How much light soaks in and glows out round the far side, like jelly, wax or skin.
    #[setting(label = "Soak", span(0, 1), keyframe, group = "Surface")]
    pub soak: f32,
    /// A thin film's swirling colours over its shine, like a pearl, oil or a soap bubble: more
    /// shifts the colours.
    #[setting(label = "Film", span(0, 1), keyframe, group = "Surface")]
    pub film: f32,
    /// How many tiny sparkling flakes it has.
    #[setting(label = "Flakes", span(0, 1), keyframe, group = "Surface")]
    pub flakes: f32,
    /// How big each flake is (of a ball's radius).
    #[setting(label = "Flake size", when = "flakes", range(0.005, 1e9), span(0.02, 0.5), keyframe, group = "Surface")]
    pub flake_size: f32,
    /// How much its highlight is stretched by fine brushed lines (across them, as on real brushed
    /// metal).
    #[setting(label = "Brushing", span(0, 1), keyframe, group = "Surface")]
    pub brushing: f32,
    /// Which way the brushed lines run.
    #[setting(label = "Brush lines", when = "brushing", group = "Surface")]
    pub brush_lines: BrushLines,
    /// Which way straight brushed lines run.
    #[setting(
        label = "Brush angle",
        when = "brushing and brush_lines=straight",
        span(-180, 180),
        unit = "°",
        keyframe,
        group = "Surface"
    )]
    pub brush_angle: f32,
    /// How much it blurs what is seen through it, like frosted glass.
    #[setting(label = "Frost", when = "see_through", span(0, 1), keyframe, group = "Surface")]
    pub frost: f32,
    /// A soft bright rim from every light, like velvet.
    #[setting(label = "Rim", span(0, 2), keyframe, group = "Surface")]
    pub rim: f32,
    /// How much it glows from inside, like lava (its own glow: add Gives off light for it to light
    /// the rest).
    #[setting(label = "Inner glow", span(0, 4), keyframe, group = "Surface")]
    pub inner_glow: f32,
    /// What a shiny surface mirrors.
    #[setting(label = "Reflects", group = "Surface")]
    pub reflects: Reflects,
    /// How much a ball bulges to the light: 0 a flat disc, 1 a ball, more a taller dome.
    #[setting(label = "Bulge", when = "thing!=shape", span(0, 2), keyframe, group = "Surface")]
    pub bulge: f32,
    /// How much a shape bulges like a lens: 0 flat, 1 a half ball. A see-through lens bends what is
    /// behind it.
    #[setting(label = "Lens", when = "thing=shape", span(0, 2), keyframe, group = "Surface")]
    pub lens: f32,
    /// Makes the shape's outline a mirror: one side shows the other, reflected.
    #[setting(label = "Mirror wall", when = "thing=shape", group = "Surface")]
    pub mirror: MirrorSide,
    /// How much the mirror shows over what is there.
    #[setting(
        label = "Mirror strength",
        when = "thing=shape and mirror!=off",
        span(0, 1),
        keyframe,
        group = "Surface"
    )]
    pub mirror_strength: f32,
    /// How much the mirror squeezes what it shows: 0 a flat mirror, 1 an endless mirror world (on a
    /// circle, exactly a circle inversion).
    #[setting(label = "Squeeze", when = "thing=shape and mirror!=off", span(0, 2), keyframe, group = "Surface")]
    pub squeeze: f32,
    /// The colour the mirror tints what it shows.
    #[setting(label = "Mirror tint", when = "thing=shape and mirror!=off", keyframe, group = "Surface")]
    pub mirror_tint: Color,
}

impl Default for SurfaceSettings {
    fn default() -> Self {
        let p = SurfaceKind::Matte.preset();
        Self {
            surface: SurfaceKind::Matte,
            roughness: p.roughness,
            metal: p.metal,
            clear_coat: p.clear_coat,
            bends_light: p.bends_light,
            see_through: p.see_through,
            toon_steps: p.toon_steps,
            rainbow: p.rainbow,
            soak: p.soak,
            film: p.film,
            flakes: p.flakes,
            flake_size: p.flake_size,
            brushing: p.brushing,
            brush_lines: BrushLines::Spun,
            brush_angle: 0.0,
            frost: p.frost,
            rim: p.rim,
            inner_glow: p.inner_glow,
            reflects: Reflects::TheScene,
            bulge: 1.0,
            lens: 0.0,
            mirror: MirrorSide::Off,
            mirror_strength: 0.85,
            squeeze: 1.0,
            mirror_tint: Color::WHITE,
        }
    }
}

/// The settings a Surface sets when it is picked, as (key, value).
pub fn preset_values(kind: SurfaceKind) -> Vec<(&'static str, Value)> {
    let p = kind.preset();
    // As typed (0.05, not 0.05000000074505806), so the inspector shows them plainly.
    let number = |v: f32| Value::Number((v as f64 * 1e6).round() / 1e6);
    vec![
        ("roughness", number(p.roughness)),
        ("metal", number(p.metal)),
        ("clear_coat", number(p.clear_coat)),
        ("bends_light", number(p.bends_light)),
        ("see_through", number(p.see_through)),
        ("toon_steps", Value::Int(p.toon_steps as i64)),
        ("rainbow", number(p.rainbow)),
        ("soak", number(p.soak)),
        ("film", number(p.film)),
        ("flakes", number(p.flakes)),
        ("flake_size", number(p.flake_size)),
        ("brushing", number(p.brushing)),
        ("frost", number(p.frost)),
        ("rim", number(p.rim)),
        ("inner_glow", number(p.inner_glow)),
    ]
}

/// Keys of the Surface settings that only the whole thing's Surface section has: a Surface
/// sub-layer leaves them out (a layer has no height or outline of its own).
pub const WHOLE_THING_ONLY: [&str; 6] = ["bulge", "lens", "mirror", "mirror_strength", "squeeze", "mirror_tint"];

impl SurfaceSettings {
    pub fn of(params: &Params) -> Self {
        Self::from_params(params)
    }

    /// How much it reflects looked at straight on, from how much it bends light.
    pub fn f0(&self) -> f32 {
        let n = self.bends_light.max(1.0);
        ((n - 1.0) / (n + 1.0)).powi(2)
    }

    /// Whether it lets light through (and so shows what is behind it).
    pub fn see_through(&self) -> bool {
        self.see_through.is_finite() && self.see_through > 0.0
    }

    /// The surface as the renderer reads it; `kind` [`SurfaceGpu::BALL`] or `SHAPE`.
    pub fn gpu(&self, kind: f32) -> SurfaceGpu {
        let f = |v: f32| if v.is_finite() { v } else { 0.0 };
        SurfaceGpu {
            a: [f(self.roughness), f(self.metal), f(self.clear_coat).max(0.0), f(self.see_through)],
            b: [self.f0(), self.toon_steps as f32, self.reflects.code(), kind],
            c: [f(self.rainbow), f(self.soak), f(self.film), f(self.flakes)],
            d: [
                f(self.flake_size).max(0.005),
                f(self.brushing),
                if self.brush_lines == BrushLines::Straight { 1.0 } else { 0.0 },
                f(self.brush_angle).to_radians(),
            ],
            e: [f(self.frost).max(0.0), f(self.rim), f(self.inner_glow), self.bends_light.max(1.0)],
        }
    }

    /// The shape's mirror wall, when it has one, as the renderer reads it (its outline's segments
    /// filled in by the caller).
    pub fn mirror_gpu(&self, size: f32) -> Option<bpe_gfx::MirrorGpu> {
        let strength = if self.mirror_strength.is_finite() { self.mirror_strength } else { 0.0 };
        if self.mirror == MirrorSide::Off || strength == 0.0 {
            return None;
        }
        let tint = self.mirror_tint.to_linear();
        let squeeze = if self.squeeze.is_finite() { self.squeeze.max(0.0) } else { 0.0 };
        Some(bpe_gfx::MirrorGpu {
            a: [0.0, 0.0, self.mirror.code(), strength],
            b: [squeeze, size.max(1e-3), 0.0, 0.0],
            c: [tint[0], tint[1], tint[2], 0.0],
        })
    }
}

/// The Surface settings of a Look card (defaults for any setting it doesn't hold).
pub fn of_card(look: &ResolvedSlot) -> SurfaceSettings {
    SurfaceSettings::from_params(&look.params)
}

/// The Look card kinds a Surface section belongs on.
pub fn has_surface(card_kind: &str) -> bool {
    card_kind == super::layers::CARD || card_kind == super::shape::CARD
}

/// The surface index of thing `id` in a lit frame (see `LightScene::surfaces`): its place among
/// the scene's things with a Look card that has a Surface, from 1; 0 for any other thing. The
/// draw and the light hook both work it out from the scene alone, so they agree.
pub fn surface_index(scene: &bpe_core::Resolved, id: ThingId) -> u32 {
    let mut n = 0;
    for (tid, t) in &scene.things {
        let surfaced = t.slots.get(&bpe_core::SlotId::Look).is_some_and(|l| has_surface(&l.kind));
        if surfaced {
            n += 1;
            if *tid == id {
                return n;
            }
        }
    }
    0
}

/// The Surface section's settings (for the inspector's Look card).
pub fn defs() -> &'static [SettingDef] {
    SurfaceSettings::defs()
}

/// The Surface sub-layer kind on ball layers (part 2b): gives its layer a surface of its own.
pub const SUB_SURFACE: &str = "sub.surface";
/// The Surface sub-layer kind on shape layers.
pub const SHAPE_SUB_SURFACE: &str = "shape.sub.surface";
/// What the Surface sub-layer kinds say about themselves.
pub const SUB_HELP: &str =
    "Gives its layer a surface of its own (with a Lighting card): shiny, glass, velvet and more.";

/// Whether layer kind `kind` is a Surface sub-layer.
pub fn is_sub_surface(kind: &str) -> bool {
    kind == SUB_SURFACE || kind == SHAPE_SUB_SURFACE
}

/// The Surface sub-layer's settings: the Surface section's, but those only a whole thing has.
pub fn sub_defs() -> &'static [SettingDef] {
    static DEFS: std::sync::LazyLock<Vec<SettingDef>> = std::sync::LazyLock::new(|| {
        SurfaceSettings::defs().iter().filter(|d| !WHOLE_THING_ONLY.contains(&d.key)).cloned().collect()
    });
    &DEFS
}

/// The surfaces a Look card's shown Surface sub-layers give their layers: (the layer, its
/// surface), in the card's order. A layer with several takes the last.
pub fn layer_surfaces(look: &ResolvedSlot) -> Vec<(LayerId, SurfaceSettings)> {
    let mut out: Vec<(LayerId, SurfaceSettings)> = Vec::new();
    for l in look.layers.iter().filter(|l| is_sub_surface(&l.kind)) {
        let Some(parent) = l.parent else { continue };
        let s = SurfaceSettings::from_params(&look.layer_params(l.id));
        match out.iter_mut().find(|(p, _)| *p == parent) {
            Some(e) => e.1 = s,
            None => out.push((parent, s)),
        }
    }
    out
}

/// The surface index a record carries ([`bpe_gfx::LayerGpu::light`]`[3]`): `index`, flagged
/// when the surface is see-through (the behind picture leaves it out).
pub fn stamp(index: u32, surface: &SurfaceSettings) -> f32 {
    index as f32 + if surface.see_through() { SurfaceGpu::GLASS_FLAG } else { 0.0 }
}

/// What thing `id` (whose Look card is `look`) stamps on its records: the whole thing's surface
/// index, flagged, and each layer given a surface of its own by a Surface sub-layer, with that
/// surface's index and its settings. Sub-layer surfaces are numbered after every thing's, in
/// thing order then layer order, so the draw and the light hook agree without shared state.
pub struct Stamps {
    pub thing: f32,
    /// How much of the light the whole thing's surface lets through (its See-through).
    pub thing_passes: f32,
    pub layers: Vec<(LayerId, u32, SurfaceSettings)>,
}

/// How much of the light surface `s` lets through: its See-through, 0 to 1.
pub fn passes(s: &SurfaceSettings) -> f32 {
    if s.see_through.is_finite() { s.see_through.clamp(0.0, 1.0) } else { 0.0 }
}

impl Stamps {
    pub fn of(scene: &bpe_core::Resolved, id: ThingId, look: &ResolvedSlot) -> Stamps {
        let card = of_card(look);
        let thing = stamp(surface_index(scene, id), &card);
        let thing_passes = if has_surface(&look.kind) { passes(&card) } else { 0.0 };
        let own = layer_surfaces(look);
        if own.is_empty() {
            return Stamps { thing, thing_passes, layers: Vec::new() };
        }
        let mut n = scene
            .things
            .values()
            .filter(|t| t.slots.get(&bpe_core::SlotId::Look).is_some_and(|l| has_surface(&l.kind)))
            .count() as u32;
        for (tid, t) in &scene.things {
            if *tid == id {
                break;
            }
            if let Some(l) = t.slots.get(&bpe_core::SlotId::Look).filter(|l| has_surface(&l.kind)) {
                n += layer_surfaces(l).len() as u32;
            }
        }
        let layers = own.into_iter().enumerate().map(|(k, (layer, s))| (layer, n + 1 + k as u32, s)).collect();
        Stamps { thing, thing_passes, layers }
    }

    /// Whether anything stamped is see-through.
    pub fn see_through(&self) -> bool {
        self.thing >= SurfaceGpu::GLASS_FLAG || self.layers.iter().any(|(_, _, s)| s.see_through())
    }

    /// Stamps `records`: every record the thing's index, then each layer with a surface of its own
    /// (found by `record_of`: its record's place) and its sub-layer records that layer's. How much
    /// a record blocks light (`light[1]`) loses the share its surface lets through, so a
    /// see-through thing casts a lighter shadow in Flat (lighting's see-through blockers plan; a
    /// Stage takes it as blocking whole).
    pub fn apply(&self, records: &mut [bpe_gfx::LayerGpu], record_of: impl Fn(LayerId) -> Option<usize>) {
        let keep = 1.0 - self.thing_passes;
        for r in records.iter_mut() {
            r.light[3] = self.thing;
        }
        let mut own = vec![false; records.len()];
        for (layer, index, s) in &self.layers {
            let Some(at) = record_of(*layer).filter(|at| *at < records.len()) else { continue };
            let n = 1 + records[at].head[3] as usize;
            let value = stamp(*index, s) + SurfaceGpu::OWN_FLAG;
            let end = (at + n).min(records.len());
            let layer_keep = 1.0 - passes(s);
            for (r, own) in records[at..end].iter_mut().zip(&mut own[at..end]) {
                r.light[3] = value;
                r.light[1] *= layer_keep;
                *own = true;
            }
        }
        for (r, own) in records.iter_mut().zip(own) {
            if !own {
                r.light[1] *= keep;
            }
        }
    }

    /// Puts the layers' own surfaces in a lit frame's table, `kind` [`SurfaceGpu::BALL`] or
    /// `SHAPE`.
    pub fn fill(&self, scene: &mut bpe_gfx::LightScene, kind: f32) {
        for (_, index, s) in &self.layers {
            scene.set_surface(*index, s.gpu(kind));
            scene.refracts |= s.see_through();
        }
    }
}

/// Whether a Look card's Surface is glass a laser bends through (part 4's tracer answers
/// `Meets::Bend { index }`): its index of refraction when See-through is at least a half and it
/// bends light at all.
pub fn glass_of(look: &ResolvedSlot) -> Option<f32> {
    if !has_surface(&look.kind) {
        return None;
    }
    let s = of_card(look);
    (s.see_through >= 0.5 && s.bends_light > 1.0 && s.bends_light.is_finite()).then_some(s.bends_light)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matte_takes_no_shine_and_the_others_do() {
        let matte = SurfaceSettings::default();
        assert_eq!(matte.f0(), 0.0, "Matte bends no light, so it shines not at all");
        assert_eq!(matte.gpu(SurfaceGpu::BALL).b[0], 0.0);
        for kind in [SurfaceKind::Plastic, SurfaceKind::Chrome, SurfaceKind::Glass, SurfaceKind::Toon] {
            let mut p = Params::default();
            p.set("surface", Value::Choice(kind.name().into()));
            for (k, v) in preset_values(kind) {
                p.set(k, v);
            }
            let s = SurfaceSettings::of(&p);
            assert_eq!(s.surface, kind);
            assert!((s.f0() - 0.04).abs() < 1e-6, "{kind:?}");
        }
        assert_eq!(SurfaceKind::Velvet.preset().bends_light, 1.0, "velvet doesn't shine either");
    }

    #[test]
    fn presets_set_every_number_but_the_ways_and_the_whole_things_own() {
        let keys: Vec<&str> = preset_values(SurfaceKind::Glass).into_iter().map(|(k, _)| k).collect();
        for d in SurfaceSettings::defs() {
            let left_alone = ["surface", "reflects", "brush_lines", "brush_angle"].contains(&d.key)
                || WHOLE_THING_ONLY.contains(&d.key);
            assert_eq!(keys.contains(&d.key), !left_alone, "{}", d.key);
        }
    }

    #[test]
    fn each_advanced_surface_raises_its_own_number() {
        let p = |k: SurfaceKind| k.preset();
        assert!(p(SurfaceKind::Jelly).soak > 0.5);
        assert!(p(SurfaceKind::Pearl).film > 0.0);
        assert!(p(SurfaceKind::Flake).flakes > 0.0 && p(SurfaceKind::Flake).clear_coat > 0.0);
        assert!(p(SurfaceKind::Brushed).brushing > 0.0 && p(SurfaceKind::Brushed).metal == 1.0);
        assert!(p(SurfaceKind::Frosted).frost > 0.0 && p(SurfaceKind::Frosted).see_through > 0.5);
        assert!(p(SurfaceKind::Velvet).rim > 0.0);
        assert!(p(SurfaceKind::Lava).inner_glow > 0.0);
        assert_eq!(SurfaceKind::NAMES.len(), 13);
    }

    #[test]
    fn glass_is_named_for_lasers_and_flagged_for_the_behind_picture() {
        let reg = crate::kit().registry;
        let mut scene = crate::scenes::lit_room(&reg);
        let ball = scene.things.values().find(|t| t.kind == "balls").map(|t| t.id).unwrap();
        let look = |scene: &bpe_core::Scene| {
            bpe_core::resolve(scene, &reg).things[&ball].slots[&bpe_core::SlotId::Look].clone()
        };
        assert_eq!(glass_of(&look(&scene)), None);
        let card = scene.card_mut(ball, bpe_core::SlotId::Look).unwrap();
        for (k, v) in preset_values(SurfaceKind::Glass) {
            card.params.set(k, v);
        }
        assert_eq!(glass_of(&look(&scene)), Some(1.5));
        let s = of_card(&look(&scene));
        assert_eq!(stamp(3, &s), 3.0 + SurfaceGpu::GLASS_FLAG);
        assert_eq!(stamp(3, &SurfaceSettings::default()), 3.0);
    }

    #[test]
    fn a_surface_sub_layer_gives_its_layer_an_index_after_every_things() {
        let reg = crate::kit().registry;
        let mut scene = crate::scenes::lit_room(&reg);
        let ball = scene.things.values().find(|t| t.kind == "balls").map(|t| t.id).unwrap();
        let outline =
            scene.add_layer(&reg, ball, bpe_core::SlotId::Look, crate::modules::look::layers::OUTLINE).unwrap();
        let sub = scene.add_sub_layer(&reg, ball, bpe_core::SlotId::Look, outline, SUB_SURFACE).unwrap();
        let card = scene.card_mut(ball, bpe_core::SlotId::Look).unwrap();
        card.params.set(&bpe_core::layered_key(sub, "see_through"), Value::Number(0.9));
        let resolved = bpe_core::resolve(&scene, &reg);
        let things = resolved
            .things
            .values()
            .filter(|t| t.slots.get(&bpe_core::SlotId::Look).is_some_and(|l| has_surface(&l.kind)))
            .count() as u32;
        let look = &resolved.things[&ball].slots[&bpe_core::SlotId::Look];
        let stamps = Stamps::of(&resolved, ball, look);
        assert_eq!(stamps.thing, surface_index(&resolved, ball) as f32);
        assert_eq!(stamps.layers.len(), 1);
        assert_eq!((stamps.layers[0].0, stamps.layers[0].1), (outline, things + 1));
        assert!(stamps.see_through());
        let stack = crate::modules::look::layers::gpu::Stack::of(look).unwrap();
        let mut records = stack.records(&mut bpe_gfx::GradientAtlas::default(), &mut Vec::new(), 0.0);
        let at = stack.record_of(outline).unwrap();
        let blocks = records[at].light[1];
        let body_blocks = records[0].light[1];
        stamps.apply(&mut records, |id| stack.record_of(id));
        // How much a record blocks loses its surface's See-through (the whole thing's is 0 here).
        assert!((records[at].light[1] - blocks * 0.1).abs() < 1e-6, "{}", records[at].light[1]);
        assert_eq!(records[0].light[1], body_blocks);
        assert_eq!(records[0].light[3], stamps.thing);
        assert_eq!(records[at].light[3], (things + 1) as f32 + SurfaceGpu::GLASS_FLAG + SurfaceGpu::OWN_FLAG);
        let mut table = bpe_gfx::LightScene::default();
        stamps.fill(&mut table, SurfaceGpu::BALL);
        assert!(table.refracts);
        assert_eq!(table.surfaces.len() as u32, things + 2);
        assert!(sub_defs().iter().all(|d| !WHOLE_THING_ONLY.contains(&d.key)));
    }

    #[test]
    fn a_copied_look_carries_its_surface() {
        let reg = crate::kit().registry;
        let mut scene = crate::scenes::lit_room(&reg);
        let from = scene.things.values().find(|t| t.kind == "balls").map(|t| t.id).unwrap();
        let to = crate::add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
        let card = scene.card_mut(from, bpe_core::SlotId::Look).unwrap();
        card.params.set("surface", Value::Choice(SurfaceKind::Chrome.name().into()));
        for (k, v) in preset_values(SurfaceKind::Chrome) {
            card.params.set(k, v);
        }
        scene.card_mut(to, bpe_core::SlotId::Look).unwrap().params.set("bulge", Value::Number(0.3));
        let copied = scene.copy_look(&reg, from, bpe_core::SlotId::Look).unwrap();
        scene.paste_look(&reg, to, bpe_core::SlotId::Look, &copied).unwrap();
        let s = SurfaceSettings::of(&scene.things[&to].slots[&bpe_core::SlotId::Look].params);
        assert_eq!((s.surface, s.metal, s.bulge), (SurfaceKind::Chrome, 1.0, 1.0), "the target's own Bulge is gone");
    }

    #[test]
    fn surface_indices_count_looked_things_from_one() {
        let reg = crate::kit().registry;
        let scene = crate::scenes::lit_room(&reg);
        let resolved = bpe_core::resolve(&scene, &reg);
        let mut seen = Vec::new();
        for (id, t) in &resolved.things {
            let i = surface_index(&resolved, *id);
            if t.slots.get(&bpe_core::SlotId::Look).is_some_and(|l| has_surface(&l.kind)) {
                seen.push(i);
            } else {
                assert_eq!(i, 0, "{}", t.kind);
            }
        }
        assert_eq!(seen, (1..=seen.len() as u32).collect::<Vec<_>>());
        assert!(seen.len() >= 2, "the room has a ball and a shape");
    }
}
