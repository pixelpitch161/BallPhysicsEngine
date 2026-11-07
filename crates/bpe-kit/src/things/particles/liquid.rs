//! The **Liquid** Look layer (particles part 6): the particles run together into one smooth
//! surface. Each particle is a blob of the field the graphics card cuts into a surface and shades
//! ([`bpe_gfx::Liquid`]); this side picks each blob's size, weight and colour and the surface's
//! look.

use bpe_core::{Blend, Choice, Color, Settings};
use bpe_gfx::{LiquidBlob, LiquidStyle};

use super::look::{ColourFrom, Common, FineDetail, MOST_COPIES, Only, look_settings};
use crate::modules::look::layers::particles::{ParticleFade, SizeOverLife};
use crate::modules::look::layers::trail::AroundPlace;
use crate::paint::{Follow, Paint};

/// The layer kind.
pub const LIQUID: &str = "particles.liquid";

/// How the surface is shaded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Surface {
    /// Flat colour, lit softly from one side.
    Matte,
    /// Smooth, with a bright highlight: water, goo, syrup.
    #[default]
    Plastic,
    /// A mirror of the sky: liquid metal.
    Chrome,
    /// Chrome tinted by its colours: gold, copper.
    Metal,
    /// See-through, bending the picture behind it.
    Glass,
    /// Flat bands of light, as drawn by hand.
    Toon,
}

impl Surface {
    /// Its code in [`LiquidStyle::a`].
    pub fn code(self) -> f32 {
        match self {
            Surface::Matte => 0.0,
            Surface::Plastic => 1.0,
            Surface::Chrome => 2.0,
            Surface::Metal => 3.0,
            Surface::Glass => LiquidStyle::GLASS,
            Surface::Toon => 5.0,
        }
    }
}

look_settings! {
    /// Liquid: the particles as one smooth surface.
    LiquidLayer {
        /// How far each particle's blob reaches, in its sizes: bigger blobs run together sooner.
        #[setting(label = "Blob size", group = "Liquid", span(0, 12), simple, keyframe, pulse)]
        blob: f32 = 4.0,
        /// Where the surface is cut: higher, thinner and more broken up; lower, fuller.
        #[setting(label = "Level", group = "Liquid", span(0.05, 2), simple, keyframe)]
        level: f32 = 0.5,
        /// How soft its edge is.
        #[setting(label = "Edge softness", group = "Liquid", span(0, 10), unit = "px", keyframe)]
        softness: f32 = 1.5,
        /// How far in from its edge the surface rounds over.
        #[setting(label = "Roundness", group = "Liquid", span(0, 60), unit = "px", keyframe)]
        roundness: f32 = 14.0,
        /// How it is shaded.
        #[setting(label = "Surface", group = "Liquid", simple)]
        surface: Surface = Surface::Plastic,
        /// The way the light comes from, on the screen (0 from the right, -90 from above).
        #[setting(label = "Light angle", group = "Liquid", span(-180, 180), unit = "°", keyframe)]
        light_angle: f32 = -120.0,
        /// How sharp and bright its highlight is.
        #[setting(label = "Shine", group = "Liquid", range(0, 1), keyframe, pulse)]
        shine: f32 = 0.6,
        /// How far Glass bends what is behind it.
        #[setting(label = "Bend", group = "Liquid", when = "surface=glass", span(0, 60), unit = "px", keyframe)]
        bend: f32 = 12.0,
        /// How much Glass takes on its colours.
        #[setting(label = "Tint", group = "Liquid", when = "surface=glass", range(0, 1), keyframe)]
        tint: f32 = 0.3,
        /// The sky Chrome and Metal reflect, above.
        #[setting(label = "Sky top", group = "Liquid", when = "surface=chrome|metal|glass|plastic")]
        sky_top: Color = Color::rgb(225, 235, 255),
        /// The sky they reflect, below.
        #[setting(label = "Sky bottom", group = "Liquid", when = "surface=chrome|metal|glass|plastic")]
        sky_bottom: Color = Color::rgb(40, 40, 50),
        /// Light round its edge.
        #[setting(label = "Rim", group = "Liquid", range(0, 1), keyframe)]
        rim: f32 = 0.3,
    }
}

impl LiquidLayer {
    /// The surface pass's numbers.
    pub fn style(&self, opacity: f32) -> LiquidStyle {
        let sky = |c: Color| {
            let l = c.to_linear();
            [l[0], l[1], l[2], 0.0]
        };
        LiquidStyle {
            a: [self.level.max(1e-3), self.softness.max(0.0), self.roundness.max(0.0), self.surface.code()],
            b: [self.light_angle.to_radians(), self.shine.clamp(0.0, 1.0), self.bend, self.tint.clamp(0.0, 1.0)],
            c: [self.rim.clamp(0.0, 1.0), opacity.clamp(0.0, 1.0), 0.0, 0.0],
            sky_top: sky(self.sky_top),
            sky_bottom: sky(self.sky_bottom),
        }
    }

    /// The blob of a particle at `at`, half size `size`, showing `alpha` of itself, coloured
    /// `colour` (linear).
    pub fn blob(&self, at: glam::Vec2, size: f32, alpha: f32, colour: [f32; 4]) -> LiquidBlob {
        LiquidBlob {
            place: [at.x, at.y, (size * self.blob).max(0.0), alpha.clamp(0.0, 1.0)],
            colour: [colour[0], colour[1], colour[2], 1.0],
        }
    }
}

impl LiquidLayer {
    fn starting(s: Self) -> Self {
        s
    }
}
