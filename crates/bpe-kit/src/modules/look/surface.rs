//! How a ball or a shape takes light (lighting spec part 2a): the **Surface** section at the top of
//! its Look card. A Surface (Matte, Plastic, Chrome, Metal, Glass, Toon) is a preset of the same
//! numbers, all editable: Roughness, Metal, Clear coat, Bends light, See-through, Toon steps,
//! Reflects; and Bulge, how round a ball stands to the light. Nothing here changes the picture
//! until the scene has a Lighting card.

use bpe_core::{Choice, Params, ResolvedSlot, SettingDef, Settings, ThingId, Value};
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
    /// Clear: dark inside, sharp highlights, lets light through (lighter, coloured shadows).
    Glass,
    /// Cartoon: light in flat bands and a hard highlight.
    Toon,
}

impl SurfaceKind {
    /// Its numbers: roughness, metal, clear coat, bends light, see-through, toon steps.
    pub fn preset(self) -> [f32; 6] {
        match self {
            Self::Matte => [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            Self::Plastic => [0.3, 0.0, 0.0, 1.5, 0.0, 0.0],
            Self::Chrome => [0.05, 1.0, 0.0, 1.5, 0.0, 0.0],
            Self::Metal => [0.35, 1.0, 0.0, 1.5, 0.0, 0.0],
            Self::Glass => [0.02, 0.0, 0.6, 1.5, 0.85, 0.0],
            Self::Toon => [0.35, 0.0, 0.0, 1.5, 0.0, 3.0],
        }
    }
}

/// What a shiny surface mirrors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Reflects {
    /// The sky's two colours (the Lighting card's Room), as bright as the room.
    #[default]
    ASkyGradient,
    /// Nothing: only the lamps' highlights.
    Nothing,
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
    /// and plastic; more shines harder.
    #[setting(label = "Bends light", range(1, 1e9), span(1, 2.5), keyframe, group = "Surface")]
    pub bends_light: f32,
    /// How much light goes through it.
    #[setting(label = "See-through", span(0, 1), keyframe, group = "Surface")]
    pub see_through: f32,
    /// Light in this many flat bands, like a cartoon: 0 smooth.
    #[setting(label = "Toon steps", range(0, 1e9), span(0, 8), group = "Surface")]
    pub toon_steps: u32,
    /// What a shiny surface mirrors.
    #[setting(label = "Reflects", group = "Surface")]
    pub reflects: Reflects,
    /// How much a ball bulges to the light: 0 a flat disc, 1 a ball, more a taller dome.
    #[setting(label = "Bulge", span(0, 2), keyframe, group = "Surface")]
    pub bulge: f32,
}

impl Default for SurfaceSettings {
    fn default() -> Self {
        let [roughness, metal, clear_coat, bends_light, see_through, toon] = SurfaceKind::Matte.preset();
        Self {
            surface: SurfaceKind::Matte,
            roughness,
            metal,
            clear_coat,
            bends_light,
            see_through,
            toon_steps: toon as u32,
            reflects: Reflects::ASkyGradient,
            bulge: 1.0,
        }
    }
}

/// The settings a Surface sets when it is picked, as (key, value).
pub fn preset_values(kind: SurfaceKind) -> Vec<(&'static str, Value)> {
    let [roughness, metal, clear_coat, bends_light, see_through, toon] = kind.preset();
    // As typed (0.05, not 0.05000000074505806), so the inspector shows them plainly.
    let number = |v: f32| Value::Number((v as f64 * 1e6).round() / 1e6);
    vec![
        ("roughness", number(roughness)),
        ("metal", number(metal)),
        ("clear_coat", number(clear_coat)),
        ("bends_light", number(bends_light)),
        ("see_through", number(see_through)),
        ("toon_steps", Value::Int(toon as i64)),
    ]
}

impl SurfaceSettings {
    pub fn of(params: &Params) -> Self {
        Self::from_params(params)
    }

    /// How much it reflects looked at straight on, from how much it bends light.
    pub fn f0(&self) -> f32 {
        let n = self.bends_light.max(1.0);
        ((n - 1.0) / (n + 1.0)).powi(2)
    }

    /// The surface as the renderer reads it; `kind` [`SurfaceGpu::BALL`] or `SHAPE`.
    pub fn gpu(&self, kind: f32) -> SurfaceGpu {
        SurfaceGpu {
            a: [self.roughness, self.metal, self.clear_coat.max(0.0), self.see_through],
            b: [self.f0(), self.toon_steps as f32, if self.reflects == Reflects::Nothing { 0.0 } else { 1.0 }, kind],
        }
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
    }

    #[test]
    fn presets_set_every_number_but_reflects_and_bulge() {
        let keys: Vec<&str> = preset_values(SurfaceKind::Glass).into_iter().map(|(k, _)| k).collect();
        for d in SurfaceSettings::defs() {
            let left_alone = ["surface", "reflects", "bulge"].contains(&d.key);
            assert_eq!(keys.contains(&d.key), !left_alone, "{}", d.key);
        }
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
