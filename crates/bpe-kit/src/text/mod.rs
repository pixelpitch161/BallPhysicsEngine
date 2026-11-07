//! The text engine (Text part 1, spec `docs/superpowers/specs/2026-11-07-text-thing-design.md`):
//! fonts ([`fonts`]: the 52 built in, the scene's loaded ones and the fallback), laying words out
//! into lines of letters ([`layout`]: shaping with rustybuzz, outlines with skrifa, overlaps merged
//! with i_overlay) and putting those letters along a path at a moment ([`arrange`]). The Text
//! thing's drawing and the editor's handles both read it.

pub mod arrange;
pub mod fonts;
pub mod layout;
pub mod live;
pub mod tiles;

pub use arrange::{PathAt, Placed, arrange, placed};
pub use fonts::{BUILT_IN, BuiltIn, DEFAULT_FONT, FontData, FontGroup, Found, find, find_in, fonts_dir};
pub use layout::{Align, Case, LayoutSpec, Letter, MOST_LETTERS, MOST_POINTS, TextLayout, layout};
