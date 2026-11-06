//! A Particles thing's Look (`look.particles`): a stack of draw layers, each drawing every
//! particle of the thing (or only some: by age or a share), bottom first. Part 1 has **Sprites**
//! (the sprite shapes `sprites.wgsl` draws) and **Streaks** (stretched along the way a particle
//! goes, by its speed). Each takes the around sub-layers: Glow, Outline, Shadow, Recolour, Flash on
//! hit and Colour on hit, with sizes in pixels.

use bpe_core::{Blend, Choice, Color, LayerSection, ModuleKind, ResolvedSlot, SettingDef, Settings, SlotId};

use crate::modules::look::layers::gpu::SubLayer;
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
pub const SUB_GLOW: &str = "particles.sub.glow";
pub const SUB_OUTLINE: &str = "particles.sub.outline";
pub const SUB_SHADOW: &str = "particles.sub.shadow";
pub const SUB_RECOLOUR: &str = "particles.sub.recolour";
pub const SUB_FLASH: &str = "particles.sub.flash";
pub const SUB_HUE: &str = "particles.sub.hue";

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
}

/// Every Look layer's settings, the same on Sprites and Streaks.
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
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
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
                }
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

/// The settings every Look layer has, read from either kind.
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
}

/// What a layer draws each particle as.
#[derive(Clone, Debug, PartialEq)]
pub enum Drawn {
    Sprites { made_of: ShapeMadeOf, softness: f32 },
    Streaks { stretch: f32, least: f32, width: f32 },
}

/// One shown Look layer as the drawing reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct LookLayer {
    pub id: bpe_core::LayerId,
    pub drawn: Drawn,
    pub common: Common,
    /// Its shown sub-layers, worked first to last.
    pub subs: Vec<SubLayer>,
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
            kind => {
                if let Some(parent) = layer.parent
                    && let Some(l) = out.iter_mut().find(|l| l.id == parent)
                {
                    l.subs.extend(sub_of(kind, &p));
                }
                continue;
            }
        };
        out.push(LookLayer { id: layer.id, drawn, common, subs: Vec::new() });
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
        drawn(SPRITES, "Sprites", "Each particle drawn as a dot, a spark, a star, confetti and more.", SpritesLayer::defs),
        drawn(STREAKS, "Streaks", "Each particle drawn as a line along the way it goes: longer the faster it goes.", StreaksLayer::defs),
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
    ]
}
