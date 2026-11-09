//! The light passes: with a Lighting card the renderer draws the things again, once per light
//! pass, each time into a picture of its own (`Lighting::picture`), with the pass's number in
//! `Globals.light.x`. The shaders name the same numbers in `shaders/light_passes.wgsl`
//! (`PASS_LIT`, …); [`BEHIND`] draws with -1 there (`PASS_BEHIND`).

/// Not a light pass: the picture as always.
pub(crate) const PICTURE: u32 = 0;
/// Each thing's colour times its Lit.
pub(crate) const LIT: u32 = 1;
/// How much each thing blocks light, its height and its cover.
pub(crate) const SURFACE: u32 = 2;
/// The light things give off.
pub(crate) const EMISSION: u32 = 3;
/// The light the background gives off, alone.
pub(crate) const BACKGROUND: u32 = 4;
/// What glows under UV light (lighting part 4).
pub(crate) const UV: u32 = 5;
/// The tags: what catches beams and each thing's link number (lighting part 4).
pub(crate) const TAGS: u32 = 6;
/// What glass shows behind it (part 2b): the plain picture without what is see-through, drawn
/// with `Globals.light.x` -1; no effects, plain blending.
pub(crate) const BEHIND: u32 = 7;
/// The tint picture (glass and light part 1, tint.wgsl): how deep each see-through thing's colour
/// is, read into the blockers' field.
pub(crate) const TINT: u32 = 8;
/// Glass and light part 3: free glass (particles and glass liquid that bend lamp light) into the
/// glass picture: index of refraction × cover, cover.
pub(crate) const GLASS: u32 = 9;

/// The shader side: the passes' names and `light_pass`, pasted before tint.wgsl.
pub(crate) const WGSL: &str = include_str!("shaders/light_passes.wgsl");

#[cfg(test)]
mod tests {
    #[test]
    fn the_shaders_name_the_same_passes() {
        let named = |name: &str| -> i32 {
            let start = format!("const PASS_{name}: i32 = ");
            let line = super::WGSL.lines().find(|l| l.starts_with(&start)).expect(name);
            line[start.len()..].trim_end_matches(';').parse().unwrap()
        };
        let passes = [
            ("PICTURE", super::PICTURE),
            ("LIT", super::LIT),
            ("SURFACE", super::SURFACE),
            ("EMISSION", super::EMISSION),
            ("BACKGROUND", super::BACKGROUND),
            ("UV", super::UV),
            ("TAGS", super::TAGS),
            ("TINT", super::TINT),
            ("GLASS", super::GLASS),
        ];
        for (name, number) in passes {
            assert_eq!(named(name), number as i32, "PASS_{name}");
        }
        assert_eq!(named("BEHIND"), -1);
    }
}
