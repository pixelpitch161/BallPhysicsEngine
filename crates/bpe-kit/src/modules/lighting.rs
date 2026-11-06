//! The scene's Lighting card (`scene.lighting`, lighting spec parts 1a and 2a): with it the scene
//! is lit by its Light things, with soft shadows, flat or on a 3D stage; without it the picture is
//! exactly as it always was.

use bpe_core::{Choice, Color, ModuleKind, SettingDef, Settings, SlotId};
use bpe_gfx::LightingGpu;

/// The card kind's id.
pub const LIGHTING: &str = "scene.lighting";

pub struct LightingKind;

/// How the scene is laid out for light.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Depth {
    /// The flat picture, lit from lamps hanging above it.
    #[default]
    Flat,
    /// A 3D stage: balls are spheres before a back wall, with a floor under them.
    Stage,
}

/// Where the stage's floor is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Floor {
    /// No floor: the back wall goes all the way down.
    None,
    /// Along the bottom of the picture.
    AtTheBottom,
    /// Under the lowest shape (the bottom of the picture when there is no shape).
    #[default]
    UnderTheLowestShape,
    /// At a height of its own.
    AtAHeight,
}

/// How the floor takes light.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum FloorSurface {
    Matte,
    /// Polished: it mirrors what stands on it, blurred further away.
    #[default]
    Glossy,
    Mirror,
}

/// How the back wall takes light.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum WallSurface {
    #[default]
    Matte,
    /// With highlights where the lamps catch it.
    Glossy,
}

/// How shadows look.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ShadowStyle {
    /// Soft edges that grow softer away from what casts them.
    #[default]
    Soft,
    /// Crisp edges.
    Hard,
    /// Crisp edges and the light in flat bands, like a cartoon.
    Toon,
    /// No shadows at all.
    Off,
}

impl ShadowStyle {
    pub fn code(self) -> f32 {
        match self {
            Self::Soft => 0.0,
            Self::Hard => 1.0,
            Self::Toon => 2.0,
            Self::Off => 3.0,
        }
    }
}

/// How hard the lighting works: higher looks better and costs more.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Quality {
    Draft,
    #[default]
    Good,
    High,
    Ultra,
}

impl Quality {
    pub fn code(self) -> f32 {
        match self {
            Self::Draft => 0.0,
            Self::Good => 1.0,
            Self::High => 2.0,
            Self::Ultra => 3.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct LightingSettings {
    /// How the scene is laid out for light.
    #[setting(label = "Depth", group = "Room")]
    pub depth: Depth,
    /// The light everywhere, even where no lamp reaches.
    #[setting(label = "Ambient colour", keyframe, group = "Room")]
    pub ambient_colour: Color,
    /// How bright the light everywhere is: 0 is darkness but for the lamps, 1 is as unlit.
    #[setting(label = "Ambient", span(0, 1), simple, keyframe, group = "Room")]
    pub ambient: f32,
    /// How much the background takes the light: 0 leaves it as it is.
    #[setting(label = "Background takes light", span(0, 1), keyframe, group = "Room")]
    pub background_lit: f32,
    /// Light from all round, brighter from above: the sky's two colours. Shiny surfaces mirror
    /// the sky whatever this is.
    #[setting(label = "Sky light", span(0, 2), keyframe, group = "Room")]
    pub sky_light: f32,
    /// The sky's colour above (what chrome and glass mirror at the top).
    #[setting(label = "Sky top", keyframe, group = "Room")]
    pub sky_top: Color,
    /// The sky's colour below.
    #[setting(label = "Sky bottom", keyframe, group = "Room")]
    pub sky_bottom: Color,
    /// How far behind the balls the back wall stands.
    #[setting(
        label = "Back wall",
        when = "depth=stage",
        range(0, 1e9),
        span(0, 1000),
        unit = "px",
        keyframe,
        group = "Stage"
    )]
    pub wall_depth: f32,
    /// How far shapes stand out from the wall (as far as the wall is deep: their fronts level
    /// with the balls' middles).
    #[setting(
        label = "Shape depth",
        when = "depth=stage",
        range(0, 1e9),
        span(0, 1000),
        unit = "px",
        keyframe,
        group = "Stage"
    )]
    pub shape_depth: f32,
    /// Where the floor is.
    #[setting(label = "Floor", when = "depth=stage", group = "Stage")]
    pub floor: Floor,
    /// The floor's height (y in the world), when it is at a height of its own.
    #[setting(label = "Floor at", when = "floor=at_a_height", span(-1000, 1000), unit = "px", keyframe, group = "Stage")]
    pub floor_at: f32,
    /// How far above the floor the stage is seen from: more shows more floor.
    #[setting(
        label = "Looking down",
        when = "floor!=none",
        range(0, 89),
        span(0, 45),
        unit = "°",
        keyframe,
        group = "Stage"
    )]
    pub looking_down: f32,
    /// How the floor takes light.
    #[setting(label = "Floor surface", when = "floor!=none", group = "Stage")]
    pub floor_surface: FloorSurface,
    /// How much a glossy floor blurs what it mirrors, more the further from the floor.
    #[setting(label = "Floor blur", when = "floor_surface=glossy", span(0, 1), keyframe, group = "Stage")]
    pub floor_blur: f32,
    /// How strongly the floor mirrors: 1 as the surface does, more for a brighter mirror.
    #[setting(label = "Reflection", when = "floor_surface!=matte", span(0, 1), keyframe, group = "Stage")]
    pub floor_reflection: f32,
    /// How the back wall takes light.
    #[setting(label = "Wall surface", when = "depth=stage", group = "Stage")]
    pub wall_surface: WallSurface,
    /// How shadows look.
    #[setting(label = "Style", group = "Shadows")]
    pub shadow_style: ShadowStyle,
    /// How soft every shadow is, on top of each lamp's size.
    #[setting(label = "Softness", when = "shadow_style=soft", span(0, 4), unit = "×", keyframe, group = "Shadows")]
    pub shadow_softness: f32,
    /// How far behind what casts it a shadow reaches.
    #[setting(label = "Length", when = "shadow_style!=off", span(0, 4000), unit = "px", keyframe, group = "Shadows")]
    pub shadow_length: f32,
    /// How much light dims close to things that block it.
    #[setting(label = "Contact shading", span(0, 1), keyframe, group = "Shadows")]
    pub contact: f32,
    /// How far from a blocker contact shading reaches.
    #[setting(
        label = "Contact reach",
        when = "contact",
        range(0, 1e9),
        span(0, 200),
        unit = "px",
        keyframe,
        group = "Shadows"
    )]
    pub contact_reach: f32,
    /// How many bands of light a toon picture has.
    #[setting(label = "Toon steps", when = "shadow_style=toon", range(1, 1e9), span(1, 8), group = "Shadows")]
    pub toon_steps: u32,
    /// How hard the lighting works, while editing and in the video alike.
    #[setting(label = "Quality", group = "Quality")]
    pub quality: Quality,
}

impl Default for LightingSettings {
    fn default() -> Self {
        Self {
            depth: Depth::Flat,
            ambient_colour: Color::WHITE,
            ambient: 0.25,
            background_lit: 1.0,
            sky_light: 0.0,
            sky_top: Color::rgb(0xcf, 0xd8, 0xe8),
            sky_bottom: Color::rgb(0x2a, 0x27, 0x24),
            wall_depth: 240.0,
            shape_depth: 240.0,
            floor: Floor::UnderTheLowestShape,
            floor_at: 600.0,
            looking_down: 14.0,
            floor_surface: FloorSurface::Glossy,
            floor_blur: 0.25,
            floor_reflection: 1.0,
            wall_surface: WallSurface::Matte,
            shadow_style: ShadowStyle::Soft,
            shadow_softness: 1.0,
            shadow_length: 2400.0,
            contact: 0.0,
            contact_reach: 40.0,
            toon_steps: 3,
            quality: Quality::Good,
        }
    }
}

impl LightingSettings {
    /// The card as the renderer reads it; `lowest_shape` is the bottom of the lowest shape's
    /// wall (world y), for a floor under it.
    pub fn gpu(&self, lowest_shape: Option<f32>) -> LightingGpu {
        let a = self.ambient_colour.to_linear();
        let (top, bottom) = (self.sky_top.to_linear(), self.sky_bottom.to_linear());
        let stage = self.depth == Depth::Stage;
        let (floor_kind, floor_y) = match self.floor {
            _ if !stage => (0.0, 0.0),
            Floor::None => (0.0, 0.0),
            Floor::AtTheBottom => (2.0, 0.0),
            Floor::UnderTheLowestShape => lowest_shape.map_or((2.0, 0.0), |y| (1.0, y)),
            Floor::AtAHeight => (1.0, self.floor_at),
        };
        let k = if floor_kind > 0.0 { self.looking_down.clamp(0.0, 89.0).to_radians().tan() } else { 0.0 };
        let back = self.wall_depth.max(0.0);
        LightingGpu {
            ambient: [a[0] * self.ambient, a[1] * self.ambient, a[2] * self.ambient, self.background_lit],
            shadows: [self.shadow_style.code(), self.shadow_softness, self.shadow_length, self.contact],
            more: [self.contact_reach, self.toon_steps.max(1) as f32, self.quality.code(), 0.0],
            stage: [f32::from(u8::from(stage)), back, self.shape_depth.max(0.0) - back, k],
            floor: [
                floor_kind,
                floor_y,
                match self.floor_surface {
                    FloorSurface::Matte => 0.0,
                    FloorSurface::Glossy => 1.0,
                    FloorSurface::Mirror => 2.0,
                },
                match self.wall_surface {
                    WallSurface::Matte => 0.0,
                    WallSurface::Glossy => 1.0,
                },
            ],
            floor_more: [self.floor_blur, self.floor_reflection, 0.0, 0.0],
            sky_top: [top[0], top[1], top[2], self.sky_light],
            sky_bottom: [bottom[0], bottom[1], bottom[2], 0.0],
        }
    }
}

impl ModuleKind for LightingKind {
    fn id(&self) -> &'static str {
        LIGHTING
    }
    fn name(&self) -> &'static str {
        "Lighting"
    }
    fn help(&self) -> &'static str {
        "Lights the scene with its Light things: flat or on a 3D stage, ambient and sky light, shadows and how hard the lighting works."
    }
    fn slot(&self) -> SlotId {
        SlotId::Lighting
    }
    fn settings(&self) -> &'static [SettingDef] {
        LightingSettings::defs()
    }
    fn fits(&self, thing_kind: &str) -> bool {
        thing_kind == "scene"
    }
}
