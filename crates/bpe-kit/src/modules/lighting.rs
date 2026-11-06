//! The scene's Lighting card (`scene.lighting`, lighting spec parts 1a and 2a): with it the scene
//! is lit by its Light things, with soft shadows, flat or on a 3D stage; without it the picture is
//! exactly as it always was.

use bpe_core::{Choice, Color, KindId, LightingScene, ModuleKind, SettingDef, Settings, SlotId, Target};

use super::light_scenes::{SceneOrder, ScenesPlay};
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

/// How the video's lighting is worked out (lighting part 3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum OnExport {
    /// As the editor draws it, at the Quality above.
    #[default]
    SameAsLive,
    /// The cinema render: every frame drawn many times and averaged, with more light bounces,
    /// deeper reflections and motion blur. Slower.
    Bake,
}

/// How the haze moves (lighting part 4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum HazeMoves {
    /// Even and still, the same everywhere.
    #[default]
    Still,
    /// Uneven clouds drifting one way.
    Drifting,
    /// Uneven clouds turning and curling.
    Swirling,
}

impl HazeMoves {
    pub fn code(self) -> f32 {
        match self {
            Self::Still => 0.0,
            Self::Drifting => 1.0,
            Self::Swirling => 2.0,
        }
    }
}

/// What glows in the dark after light has fallen on it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Remembers {
    /// Only the background, where no thing covers it.
    #[default]
    Background,
    /// Everything the light falls on.
    Everything,
}

/// The colour of a glow in the dark.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum GlowFrom {
    /// Its own Glow colour, as bright as the light was.
    #[default]
    GlowColour,
    /// The colour of the light that fell there.
    TheLight,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct LightingSettings {
    /// How the scene is laid out for light.
    #[setting(label = "Depth", group = "Room")]
    pub depth: Depth,
    /// The light everywhere, even where no lamp reaches.
    #[setting(label = "Ambient colour", keyframe, group = "Room")]
    pub ambient_colour: Color,
    /// Whether the light everywhere is on (the Lights tab's M on Ambient).
    #[setting(label = "Ambient on", group = "Room")]
    pub ambient_on: bool,
    /// How bright the light everywhere is: 0 is darkness but for the lamps, 1 is as unlit.
    #[setting(label = "Ambient", when = "ambient_on", span(0, 1), simple, keyframe, group = "Room")]
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
    #[setting(label = "Floor at", when = "depth=stage and floor=at_a_height", span(-1000, 1000), unit = "px", keyframe, group = "Stage")]
    pub floor_at: f32,
    /// How far above the floor the stage is seen from: more shows more floor.
    #[setting(
        label = "Looking down",
        when = "depth=stage and floor!=none",
        range(0, 89),
        span(0, 45),
        unit = "°",
        keyframe,
        group = "Stage"
    )]
    pub looking_down: f32,
    /// How the floor takes light.
    #[setting(label = "Floor surface", when = "depth=stage and floor!=none", group = "Stage")]
    pub floor_surface: FloorSurface,
    /// How much a glossy floor blurs what it mirrors, more the further from the floor.
    #[setting(
        label = "Floor blur",
        when = "depth=stage and floor!=none and floor_surface=glossy",
        span(0, 1),
        keyframe,
        group = "Stage"
    )]
    pub floor_blur: f32,
    /// How strongly the floor mirrors: 1 as the surface does, more for a brighter mirror.
    #[setting(
        label = "Reflection",
        when = "depth=stage and floor!=none and floor_surface!=matte",
        span(0, 1),
        keyframe,
        group = "Stage"
    )]
    pub floor_reflection: f32,
    /// How the back wall takes light.
    #[setting(label = "Wall surface", when = "depth=stage", group = "Stage")]
    pub wall_surface: WallSurface,
    /// Blurs what is nearer or farther than the focus, like a camera lens (Stage).
    #[setting(label = "Depth of field", when = "depth=stage", group = "Stage")]
    pub depth_of_field: bool,
    /// How far toward the camera the sharp depth is: 0 the balls' middles, below 0 toward the wall.
    #[setting(
        label = "Focus at",
        when = "depth=stage and depth_of_field",
        span(-480, 480),
        unit = "px",
        keyframe,
        group = "Stage"
    )]
    pub focus_at: f32,
    /// How quickly things blur away from the focus.
    #[setting(label = "Blur", when = "depth=stage and depth_of_field", span(0, 0.2), keyframe, group = "Stage")]
    pub dof_blur: f32,
    /// How much light lit things that block light bounce onto what is round them.
    #[setting(label = "Bounce light", span(0, 2), keyframe, group = "Room")]
    pub bounce: f32,
    /// How far the light of glowing things (and bounced light) reaches.
    #[setting(label = "Glow reach", range(0, 1e9), span(0, 4000), unit = "px", keyframe, group = "Room")]
    pub glow_reach: f32,
    /// How much the lamps and glowing things spill into a soft bloom (the lamps show as bulbs).
    #[setting(label = "Light blooms", span(0, 2), keyframe, group = "Room")]
    pub light_blooms: f32,
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
    /// How much the air shows the light: beams from the lamps, halos round glowing things.
    #[setting(label = "Haze", span(0, 2), keyframe, group = "Air")]
    pub haze: f32,
    /// The colour the haze takes the light in.
    #[setting(label = "Haze colour", when = "haze", keyframe, group = "Air")]
    pub haze_colour: Color,
    /// How much the haze veils what is behind it.
    #[setting(label = "Haze dims", when = "haze", span(0, 1), keyframe, group = "Air")]
    pub haze_dims: f32,
    /// How the haze moves: still and even, or in clouds drifting or swirling.
    #[setting(label = "Haze moves", when = "haze", group = "Air")]
    pub haze_moves: HazeMoves,
    /// How fast the haze's clouds move.
    #[setting(label = "Speed", when = "haze_moves!=still", span(0, 4), unit = "×", keyframe, group = "Air")]
    pub haze_speed: f32,
    /// How big the haze's clouds are.
    #[setting(
        label = "Cloud size",
        when = "haze_moves!=still",
        range(0.001, 1e9),
        span(50, 2000),
        unit = "px",
        keyframe,
        group = "Air"
    )]
    pub haze_size: f32,
    /// How uneven the haze is: 0 even, 1 thick clouds and clear gaps.
    #[setting(label = "Patchiness", when = "haze_moves!=still", span(0, 1), keyframe, group = "Air")]
    pub patchiness: f32,
    /// Which way the haze drifts.
    #[setting(label = "Drift direction", when = "haze_moves=drifting", span(-180, 180), unit = "°", keyframe, group = "Air")]
    pub drift_direction: f32,
    /// How much moving balls stir the haze as they pass through it.
    #[setting(label = "Balls stir it", when = "haze_moves!=still", span(0, 4), unit = "×", keyframe, group = "Air")]
    pub haze_stir: f32,
    /// How much the scene is under water: rippling light from above, the water's colour deeper down.
    #[setting(label = "Underwater", span(0, 1), keyframe, group = "Underwater")]
    pub underwater: f32,
    /// The water's colour, taking over deeper down.
    #[setting(label = "Water colour", when = "underwater", keyframe, group = "Underwater")]
    pub water_colour: Color,
    /// Where the water's top is (down is deeper).
    #[setting(label = "Water top", when = "underwater", span(-2000, 2000), unit = "px", keyframe, group = "Underwater")]
    pub water_top: f32,
    /// How deep the water's colour takes over.
    #[setting(
        label = "Depth fade",
        when = "underwater",
        range(0, 1e9),
        span(0, 6000),
        unit = "px",
        keyframe,
        group = "Underwater"
    )]
    pub depth_fade: f32,
    /// How much the light ripples in bright lines (caustics).
    #[setting(label = "Caustics", when = "underwater", span(0, 2), keyframe, group = "Underwater")]
    pub caustics: f32,
    /// How big the caustics' cells are.
    #[setting(
        label = "Caustic size",
        when = "underwater",
        range(0.001, 1e9),
        span(20, 600),
        unit = "px",
        keyframe,
        group = "Underwater"
    )]
    pub caustic_size: f32,
    /// How fast the caustics ripple.
    #[setting(label = "Caustic speed", when = "underwater", span(0, 4), unit = "×", keyframe, group = "Underwater")]
    pub caustic_speed: f32,
    /// How strong the slanting rays of sunlight from the top are.
    #[setting(label = "Sun rays", when = "underwater", span(0, 2), keyframe, group = "Underwater")]
    pub sun_rays: f32,
    /// How brightly things glow in the dark after light has fallen on them: 0 never.
    #[setting(label = "Glow in the dark", span(0, 4), keyframe, group = "Glow in the dark")]
    pub glow_dark: f32,
    /// What glows: only the background, or everything.
    #[setting(label = "What glows", when = "glow_dark", group = "Glow in the dark")]
    pub remembers: Remembers,
    /// How fast light charges the glow up.
    #[setting(
        label = "Charge speed",
        when = "glow_dark",
        span(0, 20),
        unit = "×",
        keyframe,
        group = "Glow in the dark"
    )]
    pub charge: f32,
    /// How long the glow takes to fade to about a third.
    #[setting(
        label = "Fade time",
        when = "glow_dark",
        range(0.001, 1e9),
        span(0.1, 20),
        unit = "s",
        keyframe,
        group = "Glow in the dark"
    )]
    pub glow_fade: f32,
    /// The colour the glow takes.
    #[setting(label = "Colour from", when = "glow_dark", group = "Glow in the dark")]
    pub glow_from: GlowFrom,
    /// The glow's own colour.
    #[setting(label = "Glow colour", when = "glow_from=glow_colour", keyframe, group = "Glow in the dark")]
    pub glow_colour: Color,
    /// How hard the lighting works, while editing and in the video alike.
    #[setting(label = "Quality", group = "Quality")]
    pub quality: Quality,
    /// How many times a reflection bounces between mirror-like balls before it settles for the sky.
    #[setting(label = "Reflection depth", range(1, 1e9), span(1, 6), group = "Quality")]
    pub reflection_depth: u32,
    /// How much light glass balls focus into bright spots (caustics) on the wall, the floor or the
    /// picture behind them: 0 none. Rainbow split splits them into colours.
    #[setting(label = "Glass caustics", span(0, 2), keyframe, group = "Quality")]
    pub glass_caustics: f32,
    /// How the video's lighting is worked out: as live, or baked (the cinema render).
    #[setting(label = "On export", group = "Quality")]
    pub on_export: OnExport,
    /// How many times each frame is drawn and averaged: more is smoother and slower.
    #[setting(label = "Samples", when = "on_export=bake", range(1, 1e9), span(1, 64), group = "Quality")]
    pub bake_samples: u32,
    /// How many times light bounces from lit things onto others (with Bounce light above 0).
    #[setting(label = "Light bounces", when = "on_export=bake", range(0, 1e9), span(0, 6), group = "Quality")]
    pub light_bounces: u32,
    /// How many times a reflection bounces between mirror-like things in the bake.
    #[setting(label = "Bake reflections", when = "on_export=bake", range(1, 1e9), span(1, 16), group = "Quality")]
    pub bake_reflections: u32,
    /// Blurs what moves, as a film camera does.
    #[setting(label = "Motion blur", when = "on_export=bake", group = "Quality")]
    pub motion_blur: bool,
    /// How much of each frame's time the shutter is open: more is a longer blur. At most a whole
    /// frame (the blur can't reach before the frame before).
    #[setting(
        label = "Shutter",
        when = "on_export=bake and motion_blur",
        range(0, 1),
        span(0, 1),
        unit = "of a frame",
        group = "Quality"
    )]
    pub shutter: f32,
    /// Smooths the grain of the bake's bounced light, keeping edges sharp.
    #[setting(label = "Noise clean-up", when = "on_export=bake", group = "Quality")]
    pub clean_up: bool,
    /// What shows the lighting scenes: Off leaves every light as it is.
    #[setting(label = "Scenes play", group = "Scenes")]
    pub scenes_play: ScenesPlay,
    /// The scene showing: 0 is the lights' own settings. Its keys on the timeline switch scenes.
    #[setting(
        label = "Show scene",
        when = "scenes_play=on_the_timeline",
        range(0, 1e9),
        span(0, 8),
        keyframe,
        hold,
        group = "Scenes"
    )]
    pub show_scene: i32,
    /// How many bars each scene shows for.
    #[setting(
        label = "Every",
        when = "scenes_play=in_turn",
        range(0.001, 1e9),
        span(0.25, 16),
        unit = "bars",
        keyframe,
        group = "Scenes"
    )]
    pub scene_every: f32,
    /// Whose hits move to the next scene.
    #[setting(label = "Heard from", when = "scenes_play=on_hits", group = "Scenes")]
    pub scene_heard: Target,
    /// Hits closer together than this count once.
    #[setting(
        label = "Least gap",
        when = "scenes_play=on_hits",
        range(0, 1e9),
        span(0, 2),
        unit = "s",
        keyframe,
        group = "Scenes"
    )]
    pub scene_gap: f32,
    /// The order scenes come in.
    #[setting(label = "Order", when = "scenes_play=in_turn or scenes_play=on_hits", group = "Scenes")]
    pub scene_order: SceneOrder,
    /// Picks another random run of scenes.
    #[setting(label = "Seed", when = "scene_order=random", span(0, 100), group = "Scenes")]
    pub scene_seed: u32,
    /// How long one scene takes to fade into the next: 0 switches at once.
    #[setting(
        label = "Crossfade",
        when = "scenes_play!=off",
        range(0, 1e9),
        span(0, 4),
        unit = "s",
        keyframe,
        group = "Scenes"
    )]
    pub scene_fade: f32,
    /// The lighting scenes: each the settings every light had when it was saved.
    #[setting(label = "Scenes", group = "Scenes")]
    pub scenes: Vec<LightingScene>,
}

impl Default for LightingSettings {
    fn default() -> Self {
        Self {
            depth: Depth::Flat,
            ambient_colour: Color::WHITE,
            ambient_on: true,
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
            depth_of_field: false,
            focus_at: 0.0,
            dof_blur: 0.04,
            bounce: 0.0,
            glow_reach: 2000.0,
            light_blooms: 0.0,
            haze: 0.0,
            haze_colour: Color::WHITE,
            haze_dims: 0.0,
            shadow_style: ShadowStyle::Soft,
            shadow_softness: 1.0,
            shadow_length: 2400.0,
            contact: 0.0,
            contact_reach: 40.0,
            toon_steps: 3,
            haze_moves: HazeMoves::Still,
            haze_speed: 1.0,
            haze_size: 400.0,
            patchiness: 0.6,
            drift_direction: 0.0,
            haze_stir: 1.0,
            underwater: 0.0,
            water_colour: Color::rgb(0x0E, 0x5A, 0x7A),
            water_top: -960.0,
            depth_fade: 2400.0,
            caustics: 1.0,
            caustic_size: 160.0,
            caustic_speed: 1.0,
            sun_rays: 0.6,
            glow_dark: 0.0,
            remembers: Remembers::Background,
            charge: 4.0,
            glow_fade: 3.0,
            glow_from: GlowFrom::GlowColour,
            glow_colour: Color::rgb(0x7D, 0xFF, 0x9A),
            quality: Quality::Good,
            reflection_depth: 3,
            glass_caustics: 1.0,
            on_export: OnExport::SameAsLive,
            bake_samples: 16,
            light_bounces: 2,
            bake_reflections: 8,
            motion_blur: true,
            shutter: 0.5,
            clean_up: true,
            scenes_play: ScenesPlay::Off,
            show_scene: 1,
            scene_every: 4.0,
            scene_heard: Target::Kind(KindId::from("balls")),
            scene_gap: 0.25,
            scene_order: SceneOrder::InOrder,
            scene_seed: 1,
            scene_fade: 0.5,
            scenes: Vec::new(),
        }
    }
}

impl LightingSettings {
    /// The card as the renderer reads it (the background's own light is the scene settings',
    /// filled in by the scene's draw); `lowest_shape` is the bottom of the lowest shape's wall
    /// (world y), for a floor under it.
    pub fn gpu(&self, lowest_shape: Option<f32>) -> LightingGpu {
        let a = self.ambient_colour.to_linear();
        let ambient = if self.ambient_on { self.ambient } else { 0.0 };
        let h = self.haze_colour.to_linear();
        let finite = |v: f32| if v.is_finite() { v } else { 0.0 };
        let haze = finite(self.haze);
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
            ambient: [a[0] * ambient, a[1] * ambient, a[2] * ambient, self.background_lit],
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
            glow: [finite(self.bounce), finite(self.glow_reach).max(0.0), finite(self.light_blooms), 0.0],
            air: [h[0] * haze, h[1] * haze, h[2] * haze, if haze != 0.0 { finite(self.haze_dims) } else { 0.0 }],
            background: [[0.0; 4]; 2],
            haze: [
                if haze != 0.0 { self.haze_moves.code() } else { 0.0 },
                finite(self.haze_speed),
                finite(self.haze_size).max(1e-3),
                finite(self.patchiness),
            ],
            haze_more: {
                let d = finite(self.drift_direction).to_radians();
                [d.cos(), d.sin(), finite(self.haze_stir), 0.0]
            },
            water: [
                finite(self.underwater),
                finite(self.caustics),
                finite(self.caustic_size).max(1e-3),
                finite(self.caustic_speed),
            ],
            water_colour: {
                let w = self.water_colour.to_linear();
                [w[0], w[1], w[2], finite(self.sun_rays)]
            },
            water_depth: [finite(self.water_top), finite(self.depth_fade).max(0.0), 0.0, 0.0],
            memory: [
                finite(self.glow_dark),
                finite(self.glow_fade).max(1e-3),
                finite(self.charge).max(0.0),
                if self.remembers == Remembers::Everything { 1.0 } else { 0.0 },
            ],
            memory_colour: {
                let g = self.glow_colour.to_linear();
                [g[0], g[1], g[2], if self.glow_from == GlowFrom::TheLight { 1.0 } else { 0.0 }]
            },
            dof: [
                f32::from(u8::from(stage && self.depth_of_field && self.dof_blur > 0.0)),
                finite(self.focus_at),
                finite(self.dof_blur).max(0.0),
                self.reflection_depth.max(1) as f32,
            ],
            bake: [
                self.bake_samples.max(1) as f32,
                self.light_bounces as f32,
                self.bake_reflections.max(1) as f32,
                if self.motion_blur { finite(self.shutter).clamp(0.0, 1.0) } else { 0.0 },
            ],
            bake_more: [
                f32::from(u8::from(self.on_export == OnExport::Bake)),
                f32::from(u8::from(self.clean_up)),
                finite(self.glass_caustics),
                0.0,
            ],
        }
    }

    /// How the video bakes, when the card asks for it.
    pub fn bake(&self) -> Option<bpe_gfx::bake::Bake> {
        (self.on_export == OnExport::Bake).then(|| bpe_gfx::bake::Bake {
            samples: self.bake_samples.max(1),
            shutter: if self.motion_blur && self.shutter.is_finite() { self.shutter.clamp(0.0, 1.0) } else { 0.0 },
        })
    }
}

/// The scene's Lighting card's settings (as set, not keyed), when it has one.
pub fn lighting_of(scene: &bpe_core::Scene, registry: &bpe_core::Registry) -> Option<LightingSettings> {
    let resolved = bpe_core::resolve(scene, registry);
    let root = resolved.things.get(&bpe_core::ThingId::ROOT)?;
    let card = root.slots.get(&SlotId::Lighting)?;
    (card.kind == LIGHTING).then(|| LightingSettings::from_params(&card.params))
}

/// How `scene`'s video bakes: `None` with no Lighting card or On export Same as live.
pub fn bake_of(scene: &bpe_core::Scene, registry: &bpe_core::Registry) -> Option<bpe_gfx::bake::Bake> {
    lighting_of(scene, registry)?.bake()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_leaves_the_stage_off_and_stage_fills_it_in() {
        let flat = LightingSettings::default();
        assert_eq!(flat.depth, Depth::Flat);
        let g = flat.gpu(Some(500.0));
        assert_eq!(g.stage[0], 0.0);
        assert_eq!(g.floor[0], 0.0, "no floor in Flat");
        let stage = LightingSettings { depth: Depth::Stage, ..LightingSettings::default() };
        let g = stage.gpu(Some(500.0));
        assert_eq!(g.stage[0], 1.0);
        assert_eq!(g.stage[1], stage.wall_depth);
        assert_eq!(g.stage[2], stage.shape_depth - stage.wall_depth, "the shapes' front, 0 the picture plane");
        assert!((g.stage[3] - stage.looking_down.to_radians().tan()).abs() < 1e-6);
        assert_eq!((g.floor[0], g.floor[1]), (1.0, 500.0), "under the lowest shape");
        assert_eq!(g.floor[2], 1.0, "glossy");
        // No shape: the floor falls back to the picture's bottom.
        assert_eq!(stage.gpu(None).floor[0], 2.0);
        let none = LightingSettings { floor: Floor::None, ..stage };
        assert_eq!((none.gpu(Some(500.0)).floor[0], none.gpu(Some(500.0)).stage[3]), (0.0, 0.0));
    }

    #[test]
    fn ambient_off_sends_no_ambient_and_keeps_its_strength() {
        let on = LightingSettings { ambient: 0.4, ..LightingSettings::default() };
        assert!((on.gpu(None).ambient[1] - 0.4).abs() < 1e-6);
        let off = LightingSettings { ambient_on: false, ..on.clone() };
        assert_eq!(&off.gpu(None).ambient[..3], &[0.0; 3]);
        assert_eq!(off.ambient, 0.4, "the strength is kept for when it is on again");
    }
}
