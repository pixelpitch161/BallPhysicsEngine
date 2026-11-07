//! A Particles thing's Look (`look.particles`): a stack of draw layers, each drawing every
//! particle of the thing (or only some: by age or a share), bottom first. Part 1 has **Sprites**
//! (the sprite shapes `sprites.wgsl` draws) and **Streaks** (stretched along the way a particle
//! goes, by its speed); part 5 adds **Soft smoke** and **Flame** (soft fields the GPU splats and
//! shades), **More shapes**, **Glitter** and **Web lines**, and every layer's **Fine detail**.
//! Each takes the around sub-layers: Glow, Outline, Shadow, Recolour, Flash on hit and Colour on
//! hit, with sizes in pixels.

use bpe_core::{
    Blend, Choice, Color, Gradient, LayerSection, Mix, ModuleKind, ResolvedSlot, SettingDef, Settings, SlotId,
};

use crate::modules::look::layers::gives_light::{Emit, GivesLight, unlit_if_lamp};
use crate::modules::look::layers::gpu::SubLayer;
use crate::modules::look::layers::lighting;
use crate::modules::look::layers::particles::{ParticleFade, ShapeMadeOf, SizeOverLife};
use crate::modules::look::layers::subs::{SubFlash, SubHue, SubOutline, SubRecolour};
use crate::modules::look::layers::trail::AroundPlace;
use crate::modules::look::layers::{Kind, sub_kind};
use crate::modules::look::shape::{ShapeSubGlow, ShapeSubShadow};
use crate::paint::{Follow, Paint};

/// The card kind.
pub const CARD: &str = "look.particles";
pub const SPRITES: &str = "particles.sprites";
pub const STREAKS: &str = "particles.streaks";
pub const SMOKE: &str = "particles.smoke";
pub const FLAME: &str = "particles.flame";
pub const SHAPES: &str = "particles.shapes";
pub const GLITTER: &str = "particles.glitter";
pub const WEB: &str = "particles.web";
pub const PILES: &str = "particles.piles";
pub const SUB_GLOW: &str = "particles.sub.glow";
pub const SUB_OUTLINE: &str = "particles.sub.outline";
pub const SUB_SHADOW: &str = "particles.sub.shadow";
pub const SUB_RECOLOUR: &str = "particles.sub.recolour";
pub const SUB_FLASH: &str = "particles.sub.flash";
pub const SUB_HUE: &str = "particles.sub.hue";
/// Gives off light (particles part 7): the layer's particles light the scene round them.
pub const SUB_LIGHT: &str = "particles.sub.light";

/// Which of the thing's particles a layer draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Only {
    #[default]
    All,
    /// Those younger than Age.
    YoungerThan,
    /// Those older than Age.
    OlderThan,
    /// A share of them, picked at random once for each particle.
    AShare,
    /// The rest: those A share leaves out, so two layers can split the particles between them.
    TheRest,
}

/// Where a layer's colours come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ColourFrom {
    /// Its own Colours.
    #[default]
    Own,
    /// The colour of the ball each particle came from (riding balls, or born where one hit).
    ParentBall,
    /// The colour of the particle each was born from (inside another Particles thing): the
    /// parent's first layer with colours of its own, as that particle was then.
    ParentParticle,
}

/// What More shapes draws each particle as.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum MoreShape {
    #[default]
    Leaves,
    Petals,
    /// Six-armed snowflakes.
    Flakes,
    Needles,
    /// Birds seen from the front, flapping.
    Birds,
    /// Fish seen from the side, swimming.
    Fish,
    /// Squares on a grid, as on an old screen.
    Pixels,
}

impl MoreShape {
    /// Its sprite code in sprites.wgsl (Pixels are crisp squares) and how long it is for its
    /// width.
    pub fn code_and_length(self) -> (u32, f32) {
        match self {
            Self::Leaves => (10, 1.8),
            Self::Petals => (11, 1.4),
            Self::Flakes => (12, 1.0),
            Self::Needles => (13, 4.0),
            Self::Birds => (14, 2.2),
            Self::Fish => (15, 1.9),
            Self::Pixels => (2, 1.0),
        }
    }
}

/// Which way a shape points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum PointAlong {
    /// Its own turn, spinning as it was set to.
    #[default]
    OwnTurn,
    /// The way it is going.
    WayItGoes,
}

/// Every layer's Fine detail: small copies of each particle round it, for nothing in the sim.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FineDetail {
    /// How many copies each particle gets (0: none).
    pub copies: u32,
    /// How far round it they sit, times its size.
    pub spread: f32,
    /// Their size, times its.
    pub size: f32,
    /// How much of them shows, times its.
    pub opacity: f32,
    /// How fast they go round it, in turns a second.
    pub shimmer: f32,
}

/// The most copies Fine detail gives a particle.
pub const MOST_COPIES: u32 = 64;

/// Every Look layer's settings, the same on every kind.
macro_rules! look_settings {
    ($(#[$meta:meta])* $name:ident { $($(#[$fm:meta])* $field:ident: $ty:ty = $default:expr,)* }) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Settings)]
        pub struct $name {
            $($(#[$fm])* pub $field: $ty,)*
            /// Under the balls or over them.
            #[setting(label = "Where", group = "Which")]
            pub place: AroundPlace,
            /// Which of the particles it draws.
            #[setting(label = "Only", group = "Which")]
            pub only: Only,
            /// The age Younger than and Older than go by.
            #[setting(label = "Age", group = "Which", when = "only=younger_than|older_than", span(0, 5), unit = "s", keyframe)]
            pub age: f32,
            /// The share A share draws (and The rest leaves out).
            #[setting(label = "Share", group = "Which", when = "only=a_share|the_rest", range(0, 1), keyframe)]
            pub share: f32,
            /// Its own colours, or each particle's ball's.
            #[setting(label = "Colour from", group = "Colours", simple)]
            pub colour_from: ColourFrom,
            /// Its own colours: over each particle's life, a stop picked for each, or by where,
            /// how fast or when.
            #[setting(
                paint,
                label = "Colours",
                group = "Colours",
                prefix = "",
                when = "colour_from=own",
                follows = "life|order|place|speed|time"
            )]
            pub colour: Paint,
            /// How much of it shows.
            #[setting(label = "Opacity", group = "Colours", range(0, 1), simple, keyframe, pulse)]
            pub opacity: f32,
            /// How much it twinkles.
            #[setting(label = "Twinkle", group = "Colours", range(0, 1), keyframe)]
            pub twinkle: f32,
            /// How fast it twinkles.
            #[setting(label = "Twinkle speed", group = "Colours", when = "twinkle", span(0, 30), unit = "per s", keyframe)]
            pub twinkle_speed: f32,
            /// Its size, times each particle's own.
            #[setting(label = "Size ×", group = "Size", span(0, 5), simple, keyframe, pulse)]
            pub size: f32,
            /// How each particle's size changes over its life.
            #[setting(label = "Size over life", group = "Size")]
            pub size_curve: SizeOverLife,
            /// How each particle fades over its life.
            #[setting(label = "Fade", group = "Size")]
            pub fade: ParticleFade,
            /// How it mixes with what is under it. Add glows; the others mix with it as one
            /// picture.
            #[setting(label = "Blend", group = "Blending")]
            pub blend: Blend,
            /// How much of the blend shows.
            #[setting(label = "Blend amount", group = "Blending", range(0, 1), keyframe)]
            pub blend_amount: f32,
            /// How many small copies of each particle are drawn round it (no cost to the sim).
            #[setting(label = "Copies", group = "Fine detail", range(0, 64), span(0, 8), keyframe)]
            pub copies: u32,
            /// How far round each particle its copies sit, times its size.
            #[setting(label = "Spread", group = "Fine detail", when = "copies", span(0, 10), keyframe)]
            pub copy_spread: f32,
            /// The copies' size, times the particle's.
            #[setting(label = "Copy size", group = "Fine detail", when = "copies", span(0, 1), keyframe)]
            pub copy_size: f32,
            /// How much of the copies shows, times the particle's.
            #[setting(label = "Copy opacity", group = "Fine detail", when = "copies", range(0, 1), keyframe)]
            pub copy_opacity: f32,
            /// How fast the copies go round each particle, in turns a second (0: they stay put).
            #[setting(label = "Shimmer", group = "Fine detail", when = "copies", span(0, 2), unit = "per s", keyframe)]
            pub copy_shimmer: f32,
        }

        impl Default for $name {
            fn default() -> Self {
                $name::starting(Self {
                    $($field: $default,)*
                    place: AroundPlace::Behind,
                    only: Only::All,
                    age: 1.0,
                    share: 0.5,
                    colour_from: ColourFrom::Own,
                    colour: Paint { follow: Follow::Life, ..Paint::solid(Color::WHITE) },
                    opacity: 1.0,
                    twinkle: 0.0,
                    twinkle_speed: 8.0,
                    size: 1.0,
                    size_curve: SizeOverLife::Even,
                    fade: ParticleFade::Out,
                    blend: Blend::Normal,
                    blend_amount: 1.0,
                    copies: 0,
                    copy_spread: 3.0,
                    copy_size: 0.4,
                    copy_opacity: 0.7,
                    copy_shimmer: 0.0,
                })
            }
        }

        impl $name {
            /// The settings every layer has.
            pub fn common(&self) -> Common {
                Common {
                    place: self.place,
                    only: self.only,
                    age: self.age,
                    share: self.share,
                    colour_from: self.colour_from,
                    colour: self.colour.clone(),
                    opacity: self.opacity,
                    twinkle: self.twinkle,
                    twinkle_speed: self.twinkle_speed,
                    size: self.size,
                    size_curve: self.size_curve,
                    fade: self.fade,
                    blend: self.blend,
                    blend_amount: self.blend_amount,
                    detail: FineDetail {
                        copies: self.copies.min(MOST_COPIES),
                        spread: self.copy_spread,
                        size: self.copy_size,
                        opacity: self.copy_opacity,
                        shimmer: self.copy_shimmer,
                    },
                }
            }
        }
    };
}

look_settings! {
    /// Sprites: each particle drawn as one of the sprite shapes.
    SpritesLayer {
        /// What each particle is drawn as.
        #[setting(label = "Made of", group = "Sprites", simple)]
        made_of: ShapeMadeOf = ShapeMadeOf::Dots,
        /// How soft its edge is.
        #[setting(label = "Softness", group = "Sprites", range(0, 1), keyframe)]
        softness: f32 = 0.3,
    }
}

impl SpritesLayer {
    fn starting(s: Self) -> Self {
        s
    }
}

impl StreaksLayer {
    fn starting(s: Self) -> Self {
        s
    }
}

look_settings! {
    /// Streaks: each particle drawn as a line along the way it goes, longer the faster it goes.
    StreaksLayer {
        /// How long a streak is for its speed: the way it goes in this many seconds.
        #[setting(label = "Stretch", group = "Streaks", span(0, 0.2), unit = "s", simple, keyframe)]
        stretch: f32 = 0.03,
        /// The shortest a streak gets, however slowly it goes.
        #[setting(label = "Least length", group = "Streaks", span(0, 40), unit = "px", keyframe)]
        least: f32 = 4.0,
        /// How wide it is, as a share of the particle's size.
        #[setting(label = "Width", group = "Streaks", span(0, 3), keyframe)]
        width: f32 = 1.0,
    }
}

look_settings! {
    /// Soft smoke: the particles splatted into one soft cloud, thick where many meet, broken up by
    /// drifting detail and lit from one side.
    SmokeLayer {
        /// How thick the smoke gets where particles meet.
        #[setting(label = "Thickness", group = "Smoke", span(0, 6), simple, keyframe, pulse)]
        thickness: f32 = 1.5,
        /// Each puff's size, times its particle's.
        #[setting(label = "Puff size", group = "Smoke", span(0, 6), simple, keyframe)]
        puff: f32 = 2.0,
        /// How much drifting detail breaks it up.
        #[setting(label = "Detail", group = "Smoke", range(0, 1), keyframe)]
        detail: f32 = 0.5,
        /// The detail's size.
        #[setting(label = "Detail size", group = "Smoke", span(5, 300), unit = "px", keyframe)]
        detail_size: f32 = 70.0,
        /// How fast the detail drifts.
        #[setting(label = "Drift", group = "Smoke", span(0, 200), unit = "px/s", keyframe)]
        drift: f32 = 20.0,
        /// Which way it drifts.
        #[setting(label = "Drift way", group = "Smoke", when = "drift", span(-180, 180), unit = "°", keyframe)]
        drift_way: f32 = -90.0,
        /// How much the side facing the light is lit and the far side shaded.
        #[setting(label = "Shading", group = "Smoke", range(0, 1), keyframe)]
        shading: f32 = 0.6,
        /// The way the light comes from.
        #[setting(label = "Light from", group = "Smoke", when = "shading", span(-180, 180), unit = "°", keyframe)]
        light_from: f32 = -135.0,
        /// How much darker it is where it is thickest.
        #[setting(label = "Dark inside", group = "Smoke", range(0, 1), keyframe)]
        dark_inside: f32 = 0.4,
    }
}

impl SmokeLayer {
    fn starting(s: Self) -> Self {
        Self { opacity: 0.6, size_curve: SizeOverLife::Grow, ..s }
    }
}

/// The colours a new Flame layer burns through, cold to hot.
pub fn flame_colours() -> Gradient {
    Gradient::spread(
        &[
            Color::rgb(0, 0, 0),
            Color::rgb(120, 12, 4),
            Color::rgb(235, 80, 10),
            Color::rgb(255, 190, 40),
            Color::rgb(255, 245, 210),
        ],
        Mix::Light,
        false,
    )
}

look_settings! {
    /// Flame: the particles' heat splatted into one field and coloured by how hot it is there,
    /// flickering, with noise that rises and bends it.
    FlameLayer {
        /// Its colours from cold to hot. Its Colours tint them (white leaves them as they are).
        #[setting(label = "Heat colours", group = "Flame", simple, keyframe)]
        heat_colours: Gradient = flame_colours(),
        /// How hot it gets where particles meet.
        #[setting(label = "Heat", group = "Flame", span(0, 6), simple, keyframe, pulse)]
        heat: f32 = 1.4,
        /// Each tongue's size, times its particle's.
        #[setting(label = "Puff size", group = "Flame", span(0, 6), simple, keyframe)]
        puff: f32 = 1.8,
        /// How much a particle cools over its life (1: cold when it dies).
        #[setting(label = "Cools", group = "Flame", range(0, 1), keyframe)]
        cools: f32 = 0.7,
        /// How much rising detail breaks it into tongues.
        #[setting(label = "Detail", group = "Flame", range(0, 1), keyframe)]
        detail: f32 = 0.6,
        /// The detail's size.
        #[setting(label = "Detail size", group = "Flame", span(5, 300), unit = "px", keyframe)]
        detail_size: f32 = 45.0,
        /// How fast the detail rises.
        #[setting(label = "Rise", group = "Flame", span(0, 500), unit = "px/s", keyframe)]
        rise: f32 = 140.0,
        /// How far the detail bends it.
        #[setting(label = "Bend", group = "Flame", span(0, 60), unit = "px", keyframe)]
        bend: f32 = 10.0,
        /// How much it flickers.
        #[setting(label = "Flicker", group = "Flame", range(0, 1), keyframe)]
        flicker: f32 = 0.4,
        /// How fast it flickers.
        #[setting(label = "Flicker speed", group = "Flame", when = "flicker", span(0, 30), unit = "per s", keyframe)]
        flicker_speed: f32 = 7.0,
        /// How bright it burns.
        #[setting(label = "Brightness", group = "Flame", span(0, 6), keyframe, pulse)]
        brightness: f32 = 1.6,
    }
}

impl FlameLayer {
    fn starting(s: Self) -> Self {
        Self { blend: Blend::Add, size_curve: SizeOverLife::Shrink, ..s }
    }
}

look_settings! {
    /// More shapes: leaves, petals, snowflakes, needles, flapping birds, swimming fish or pixels.
    ShapesLayer {
        /// What each particle is drawn as.
        #[setting(label = "Shape", group = "Shapes", simple)]
        shape: MoreShape = MoreShape::Leaves,
        /// Which way each one points.
        #[setting(label = "Point along", group = "Shapes", when = "shape!=pixels")]
        point_along: PointAlong = PointAlong::OwnTurn,
        /// Turns each one by this much more.
        #[setting(label = "Turn by", group = "Shapes", when = "shape!=pixels", span(-180, 180), unit = "°", keyframe)]
        turn_by: f32 = 0.0,
        /// How long each one is, times the shape's own length.
        #[setting(label = "Length", group = "Shapes", when = "shape!=pixels", span(0, 4), keyframe)]
        length: f32 = 1.0,
        /// How soft its edge is.
        #[setting(label = "Softness", group = "Shapes", range(0, 1), keyframe)]
        softness: f32 = 0.15,
        /// How fast birds flap and fish swim.
        #[setting(label = "Flap speed", group = "Shapes", when = "shape=birds|fish", span(0, 20), unit = "per s", keyframe)]
        flap_speed: f32 = 4.0,
        /// The grid pixels sit on (0: they don't snap).
        #[setting(label = "Pixel grid", group = "Shapes", when = "shape=pixels", span(0, 40), unit = "px", keyframe)]
        grid: f32 = 6.0,
    }
}

impl ShapesLayer {
    fn starting(s: Self) -> Self {
        s
    }
}

look_settings! {
    /// Glitter: diamond facets that flash with a star when they turn to face the light.
    GlitterLayer {
        /// The way the light comes from.
        #[setting(label = "Light from", group = "Glitter", span(-180, 180), unit = "°", keyframe)]
        light_from: f32 = -60.0,
        /// How briefly each facet flashes as it turns (higher: shorter, rarer flashes).
        #[setting(label = "Sharpness", group = "Glitter", range(1, 1e6), span(1, 128), keyframe)]
        sharpness: f32 = 24.0,
        /// How bright a facet is when it isn't flashing.
        #[setting(label = "Base", group = "Glitter", range(0, 1), simple, keyframe)]
        base: f32 = 0.35,
        /// The star a flash makes, times the particle's size (0: none).
        #[setting(label = "Star size", group = "Glitter", span(0, 10), simple, keyframe, pulse)]
        star: f32 = 3.0,
        /// The flash's colour.
        #[setting(label = "Flash colour", group = "Glitter", keyframe)]
        flash_colour: Color = Color::WHITE,
        /// How fast each facet turns on its own, in turns a second, as well as its spin.
        #[setting(label = "Shimmer", group = "Glitter", span(0, 4), unit = "per s", keyframe)]
        shimmer: f32 = 0.5,
    }
}

impl GlitterLayer {
    fn starting(s: Self) -> Self {
        Self { fade: ParticleFade::Both, ..s }
    }
}

look_settings! {
    /// Web lines: lines between particles near each other (and to balls), fainter the further
    /// apart, with a dot at each particle.
    WebLayer {
        /// How far apart two particles can be and still be joined.
        #[setting(label = "Reach", group = "Web", range(1, 1e6), span(5, 600), unit = "px", simple, keyframe, pulse)]
        reach: f32 = 140.0,
        /// The most lines each particle starts, to its nearest.
        #[setting(label = "Links", group = "Web", range(1, 32), simple)]
        links: u32 = 6,
        /// How wide the lines are.
        #[setting(label = "Line width", group = "Web", span(0, 6), unit = "px", keyframe)]
        line_width: f32 = 0.8,
        /// How fast lines fade as their ends draw apart (0: they don't).
        #[setting(label = "Fade", group = "Web", span(0, 5), keyframe)]
        fade_power: f32 = 1.5,
        /// How much of the lines shows.
        #[setting(label = "Line opacity", group = "Web", range(0, 1), keyframe)]
        line_opacity: f32 = 0.8,
        /// Lines from particles to the balls near them too.
        #[setting(label = "To balls", group = "Web")]
        to_balls: bool = false,
        /// How far from a ball's edge a particle is joined to it.
        #[setting(label = "Ball reach", group = "Web", when = "to_balls", span(0, 600), unit = "px", keyframe)]
        ball_reach: f32 = 120.0,
        /// The dot at each particle, times its size (0: no dots).
        #[setting(label = "Dots", group = "Web", span(0, 4), keyframe)]
        dots: f32 = 0.8,
    }
}

impl WebLayer {
    fn starting(s: Self) -> Self {
        Self { blend: Blend::Add, fade: ParticleFade::None, ..s }
    }
}

/// Piles: the heaps the particles make (Meets things: Pile up), each drawn as one band.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct PilesLayer {
    /// Under the balls or over them.
    #[setting(label = "Where", group = "Which")]
    pub place: AroundPlace,
    /// Its colours, by where along the pile, a stop for each column, or when.
    #[setting(paint, label = "Colours", group = "Colours", prefix = "", follows = "place|order|time")]
    pub colour: Paint,
    #[setting(label = "Opacity", group = "Colours", range(0, 1), simple, keyframe, pulse)]
    pub opacity: f32,
    /// How soft the sand's top edge is.
    #[setting(label = "Softness", group = "Piles", range(0, 1), keyframe)]
    pub softness: f32,
    #[setting(label = "Blend", group = "Blending")]
    pub blend: Blend,
    #[setting(label = "Blend amount", group = "Blending", range(0, 1), keyframe)]
    pub blend_amount: f32,
}

impl Default for PilesLayer {
    fn default() -> Self {
        Self {
            place: AroundPlace::Behind,
            colour: Paint { follow: Follow::Place, ..Paint::solid(Color::rgb(214, 180, 120)) },
            opacity: 1.0,
            softness: 0.3,
            blend: Blend::Normal,
            blend_amount: 1.0,
        }
    }
}

impl PilesLayer {
    /// The settings every layer has (those it doesn't have at their defaults).
    pub fn common(&self) -> Common {
        Common {
            place: self.place,
            only: Only::All,
            age: 1.0,
            share: 0.5,
            colour_from: ColourFrom::Own,
            colour: self.colour.clone(),
            opacity: self.opacity,
            twinkle: 0.0,
            twinkle_speed: 8.0,
            size: 1.0,
            size_curve: SizeOverLife::Even,
            fade: ParticleFade::None,
            blend: self.blend,
            blend_amount: self.blend_amount,
            detail: FineDetail::default(),
        }
    }
}

/// The settings every Look layer has, read from any kind.
#[derive(Clone, Debug, PartialEq)]
pub struct Common {
    pub place: AroundPlace,
    pub only: Only,
    pub age: f32,
    pub share: f32,
    pub colour_from: ColourFrom,
    pub colour: Paint,
    pub opacity: f32,
    pub twinkle: f32,
    pub twinkle_speed: f32,
    pub size: f32,
    pub size_curve: SizeOverLife,
    pub fade: ParticleFade,
    pub blend: Blend,
    pub blend_amount: f32,
    pub detail: FineDetail,
}

/// What a layer draws each particle as.
#[derive(Clone, Debug, PartialEq)]
pub enum Drawn {
    Sprites {
        made_of: ShapeMadeOf,
        softness: f32,
    },
    Streaks {
        stretch: f32,
        least: f32,
        width: f32,
    },
    Smoke(SmokeLayer),
    Flame(FlameLayer),
    Shapes(ShapesLayer),
    Glitter(GlitterLayer),
    Web(WebLayer),
    /// The thing's piles, not its particles.
    Piles {
        softness: f32,
    },
}

/// One shown Look layer as the drawing reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct LookLayer {
    pub id: bpe_core::LayerId,
    pub drawn: Drawn,
    pub common: Common,
    /// Its shown sub-layers, worked first to last.
    pub subs: Vec<SubLayer>,
    /// How it takes the scene's light ([`lighting::of`]), its Lit taken away when it gives off
    /// light (part 7).
    pub light: [f32; 4],
    /// Its UV and how it catches beams ([`lighting::extra_of`], no link number yet).
    pub more: [f32; 4],
    /// The light it gives off (its Gives off light sub-layers), when it gives any.
    pub emit: Option<Emit>,
}

/// A sub-layer of `kind` with its settings `p`, as a ball's (sizes in pixels).
fn sub_of(kind: &str, p: &bpe_core::Params) -> Option<SubLayer> {
    Some(match kind {
        SUB_GLOW => SubLayer::Glow(ShapeSubGlow::from_params(p).into()),
        SUB_OUTLINE => SubLayer::Outline(SubOutline::from_params(p)),
        SUB_SHADOW => SubLayer::Shadow(ShapeSubShadow::from_params(p).into()),
        SUB_RECOLOUR => SubLayer::Recolour(SubRecolour::from_params(p)),
        SUB_FLASH => SubLayer::Flash(SubFlash::from_params(p)),
        SUB_HUE => SubLayer::Hue(SubHue::from_params(p)),
        SUB_LIGHT => SubLayer::Light(GivesLight::from_params(p)),
        _ => return None,
    })
}

/// Every shown Look layer of the card `slot`, bottom first, with its shown sub-layers.
pub fn layers_of(slot: &ResolvedSlot) -> Vec<LookLayer> {
    let mut out: Vec<LookLayer> = Vec::new();
    for layer in &slot.layers {
        let p = slot.layer_params(layer.id);
        let (drawn, common) = match layer.kind.as_str() {
            SPRITES => {
                let s = SpritesLayer::from_params(&p);
                (Drawn::Sprites { made_of: s.made_of, softness: s.softness }, s.common())
            }
            STREAKS => {
                let s = StreaksLayer::from_params(&p);
                (Drawn::Streaks { stretch: s.stretch, least: s.least, width: s.width }, s.common())
            }
            SMOKE => {
                let s = SmokeLayer::from_params(&p);
                (Drawn::Smoke(s.clone()), s.common())
            }
            FLAME => {
                let s = FlameLayer::from_params(&p);
                (Drawn::Flame(s.clone()), s.common())
            }
            SHAPES => {
                let s = ShapesLayer::from_params(&p);
                (Drawn::Shapes(s.clone()), s.common())
            }
            GLITTER => {
                let s = GlitterLayer::from_params(&p);
                (Drawn::Glitter(s.clone()), s.common())
            }
            WEB => {
                let s = WebLayer::from_params(&p);
                (Drawn::Web(s.clone()), s.common())
            }
            PILES => {
                let s = PilesLayer::from_params(&p);
                (Drawn::Piles { softness: s.softness }, s.common())
            }
            kind => {
                if let Some(parent) = layer.parent
                    && let Some(l) = out.iter_mut().find(|l| l.id == parent)
                {
                    l.subs.extend(sub_of(kind, &p));
                }
                continue;
            }
        };
        out.push(LookLayer {
            id: layer.id,
            drawn,
            common,
            subs: Vec::new(),
            light: lighting::of(&p),
            more: lighting::extra_of(&p, 0.0, true),
            emit: None,
        });
    }
    // Gives off light makes its layer a lamp: it shows its own colours, unlit.
    for l in &mut out {
        l.emit = Emit::of_subs(&l.subs);
        l.light = unlit_if_lamp(l.light, l.emit);
    }
    out
}

/// The Look card of a Particles thing.
pub struct ParticlesLookKind;

impl ModuleKind for ParticlesLookKind {
    fn id(&self) -> &'static str {
        CARD
    }
    fn name(&self) -> &'static str {
        "Look"
    }
    fn help(&self) -> &'static str {
        "How the particles look: a stack of layers, each drawing the particles (or some of them), bottom first."
    }
    fn slot(&self) -> SlotId {
        SlotId::Look
    }
    fn settings(&self) -> &'static [SettingDef] {
        &[]
    }
    fn fits(&self, thing_kind: &str) -> bool {
        thing_kind == super::KIND
    }
}

/// The Look's layer kinds, in the add menu's order, then its sub-layer kinds.
pub fn kinds() -> Vec<Kind> {
    let drawn = |id, name, help, settings| Kind {
        id,
        name,
        help,
        section: LayerSection::Drawn,
        repeats: true,
        base: false,
        card: CARD,
        takes_subs: true,
        settings,
        dynamic: crate::modules::look::layers::no_more,
    };
    let sub = |id, name, help, settings| sub_kind(CARD, id, name, help, settings);
    vec![
        drawn(
            SPRITES,
            "Sprites",
            "Each particle drawn as a dot, a spark, a star, confetti and more.",
            SpritesLayer::defs,
        ),
        drawn(
            STREAKS,
            "Streaks",
            "Each particle drawn as a line along the way it goes: longer the faster it goes.",
            StreaksLayer::defs,
        ),
        drawn(
            SMOKE,
            "Soft smoke",
            "The particles as one soft cloud: thick where many meet, drifting, lit from one side.",
            SmokeLayer::defs,
        ),
        drawn(FLAME, "Flame", "The particles' heat as flickering flames, coloured from cold to hot.", FlameLayer::defs),
        drawn(
            SHAPES,
            "More shapes",
            "Each particle drawn as a leaf, a petal, a snowflake, a needle, a bird, a fish or a pixel.",
            ShapesLayer::defs,
        ),
        drawn(GLITTER, "Glitter", "Facets that flash with a star when they turn to the light.", GlitterLayer::defs),
        drawn(
            WEB,
            "Web lines",
            "Lines between particles near each other (and to balls), fading as they draw apart.",
            WebLayer::defs,
        ),
        drawn(
            PILES,
            "Piles",
            "The heaps the particles make where they pile up (Meets things: Pile up), drawn as sand.",
            PilesLayer::defs,
        ),
        sub(SUB_GLOW, "Glow", "Soft light round its layer's particles.", ShapeSubGlow::defs),
        sub(SUB_OUTLINE, "Outline", "A line round its layer's particles, painted over them.", SubOutline::defs),
        sub(SUB_SHADOW, "Shadow", "A soft offset copy of its layer's particles, under them.", ShapeSubShadow::defs),
        sub(SUB_RECOLOUR, "Recolour", "Changes its layer's colours only.", SubRecolour::defs),
        sub(SUB_FLASH, "Flash on hit", "Its layer lights up when the thing it rides is hit.", SubFlash::defs),
        sub(
            SUB_HUE,
            "Colour on hit",
            "Its layer's colours turn round the wheel every time the thing it rides is hit.",
            SubHue::defs,
        ),
        sub(
            SUB_LIGHT,
            "Gives off light",
            "Makes its layer's particles give off light that lights everything round them (with a Lighting card).",
            GivesLight::defs,
        ),
    ]
}
