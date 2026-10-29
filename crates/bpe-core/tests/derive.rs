use bpe_core::{Choice, Color, Depth, Params, Settings, Value};

#[derive(Clone, Copy, Debug, PartialEq, Default, Choice)]
enum Shape {
    #[default]
    Circle,
    RoundBox,
}

#[derive(Debug, PartialEq, Settings)]
struct Look {
    /// How brightly the halo shines.
    #[setting(label = "Glow", range(0.0, 2.0), simple, keyframe, random, pulse)]
    glow: f32,
    #[setting(label = "Colour", simple)]
    color: Color,
    #[setting(label = "Shape")]
    shape: Shape,
}

impl Default for Look {
    fn default() -> Self {
        Self { glow: 0.5, color: Color::WHITE, shape: Shape::Circle }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Choice)]
#[allow(clippy::upper_case_acronyms)] // the point is how such names come out
enum Space {
    HSV,
    GPULiquid,
    Mode2D,
    Plain,
}

#[test]
fn choice_names_keep_acronyms_together() {
    assert_eq!(Space::NAMES, &["hsv", "gpu_liquid", "mode2_d", "plain"]);
    assert_eq!(Space::from_name("gpu_liquid"), Some(Space::GPULiquid));
    assert_eq!(Space::from_name("GPULiquid"), None);
}

#[test]
fn defs_describe_each_field() {
    let d = Look::defs();
    assert_eq!(d.iter().map(|d| d.key).collect::<Vec<_>>(), ["glow", "color", "shape"]);
    assert_eq!(d[0].help, "How brightly the halo shines.");
    assert_eq!(d[0].range, Some((0.0, 2.0)));
    assert_eq!(d[0].default, Value::Number(0.5));
    assert!(d[0].flags.keyframe && d[0].flags.random && d[0].flags.pulse);
    assert_eq!(d[2].depth, Depth::More);
    assert_eq!(Shape::NAMES, &["circle", "round_box"]);
}

#[test]
fn to_params_is_sparse_and_from_params_fills_defaults() {
    assert!(Look::default().to_params().is_empty());
    let mut p = Params::default();
    p.set("glow", Value::Number(1.5));
    p.set("shape", Value::Choice("round_box".into()));
    let look = Look::from_params(&p);
    assert_eq!(look, Look { glow: 1.5, shape: Shape::RoundBox, ..Look::default() });
    assert_eq!(Look::from_params(&look.to_params()), look);
}

#[test]
fn wrong_typed_values_fall_back_to_default() {
    let mut p = Params::default();
    p.set("glow", Value::Text("bright".into()));
    assert_eq!(Look::from_params(&p).glow, 0.5);
}

#[test]
fn clamp_keeps_numbers_in_range() {
    assert_eq!(Look::defs()[0].clamp(Value::Number(9.0)), Value::Number(2.0));
}
