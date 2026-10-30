//! Camera: what the video shows of the world, where it looks, how close, and how it moves.



use bpe_core::{KindId, SettingDef, Settings, SlotId, Target, ThingKind};
use glam::Vec2;

pub struct CameraKind;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct CameraSettings {
    /// The world point at the middle of the shot.
    #[setting(label = "Position", unit = "px", group = "Shot", keyframe)]
    pub position: Vec2,
    /// Above 1 shows less of the world, bigger. At 1 the shot is 1080 world units across.
    #[setting(label = "Zoom", range(0.05, 20), unit = "×", group = "Shot", keyframe)]
    pub zoom: f32,
    /// How far the camera is turned, clockwise.
    #[setting(label = "Turn", range(-360, 360), unit = "°", group = "Shot", keyframe)]
    pub turn: f32,
    /// Follow something as it moves.
    #[setting(label = "Follow", group = "Follow")]
    pub follow: bool,
    /// What to follow: a thing, a tag, or every ball.
    #[setting(label = "Follow what", when = "follow", group = "Follow")]
    pub follow_target: Target,
    /// 0 keeps the shot you set, 1 follows fully; in between mixes the two.
    #[setting(label = "Follow amount", when = "follow", range(0, 1), group = "Follow", keyframe)]
    pub follow_amount: f32,
    /// How lazily the camera catches up: 0 snaps straight there.
    #[setting(label = "Smoothness", when = "follow", range(0, 1), group = "Follow")]
    pub smoothness: f32,
    /// Leads a moving target by this much of its travel, so fast balls don't leave the shot.
    #[setting(label = "Look ahead", when = "follow", range(0, 1), unit = "s", group = "Follow")]
    pub look_ahead: f32,
    /// A middle part of the shot the target can move in without the camera moving.
    #[setting(label = "Slack", when = "follow", range(0, 0.8), group = "Follow")]
    pub slack: f32,
    /// Zoom out so everything followed stays in the shot.
    #[setting(label = "Fit all", when = "follow", group = "Follow")]
    pub fit_all: bool,
    /// Room kept around everything followed when fitting.
    #[setting(label = "Fit margin", when = "follow and fit_all", range(0, 500), unit = "px", group = "Follow")]
    pub fit_margin: f32,
    /// How hard hits shake the camera.
    #[setting(label = "Shake", range(0, 100), unit = "px", group = "On hits")]
    pub shake: f32,
    /// How long a shake takes to die away.
    #[setting(label = "Shake fade", when = "shake", range(0.05, 2), unit = "s", group = "On hits")]
    pub shake_fade: f32,
    /// How much a hit zooms the camera in for a moment.
    #[setting(label = "Zoom punch", range(0, 0.5), group = "On hits")]
    pub punch: f32,
    /// How long the zoom punch takes to ease back.
    #[setting(label = "Punch recovery", when = "punch", range(0.05, 2), unit = "s", group = "On hits")]
    pub punch_recovery: f32,
    /// Whose hits shake and punch.
    #[setting(label = "Hits from", when = "shake or punch", group = "On hits")]
    pub hits_from: Target,
    /// Never show anything past the edge of a box.
    #[setting(label = "Stay inside", group = "World box")]
    pub stay_inside: bool,
    /// The box's width and height.
    #[setting(label = "Box size", when = "stay_inside", unit = "px", group = "World box")]
    pub box_size: Vec2,
    /// The box's middle.
    #[setting(label = "Box centre", when = "stay_inside", unit = "px", group = "World box")]
    pub box_centre: Vec2,
}

fn balls() -> Target {
    Target::Kind(KindId::from("balls"))
}

impl Default for CameraSettings {
    fn default() -> Self {
        Self {
            position: Vec2::ZERO,
            zoom: 1.0,
            turn: 0.0,
            follow: false,
            follow_target: balls(),
            follow_amount: 1.0,
            smoothness: 0.6,
            look_ahead: 0.15,
            slack: 0.1,
            fit_all: false,
            fit_margin: 120.0,
            shake: 0.0,
            shake_fade: 0.4,
            punch: 0.0,
            punch_recovery: 0.3,
            hits_from: balls(),
            stay_inside: false,
            box_size: Vec2::new(3240.0, 5760.0),
            box_centre: Vec2::ZERO,
        }
    }
}

impl ThingKind for CameraKind {
    fn id(&self) -> &'static str {
        "camera"
    }
    fn name(&self) -> &'static str {
        "Camera"
    }
    fn icon(&self) -> &'static str {
        "camera"
    }
    fn help(&self) -> &'static str {
        "What the video shows: where it looks, how close, and how it follows, shakes and punches."
    }
    fn settings(&self) -> &'static [SettingDef] {
        CameraSettings::defs()
    }
    fn slots(&self) -> &'static [SlotId] {
        &[]
    }
    fn default_slots(&self) -> &'static [SlotId] {
        &[]
    }
    fn single(&self) -> bool {
        true
    }
}
