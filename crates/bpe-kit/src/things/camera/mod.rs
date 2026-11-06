//! Camera: what the video shows of the world, where it looks, how close, and how it moves.

pub mod draw;
pub mod sim;

use bpe_core::{Choice, KindId, SettingDef, Settings, SlotId, Target, ThingKind};
use glam::Vec2;

pub struct CameraKind;

/// How a moved Stage camera keeps its picture's edges inside what the front shot drew.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum FillEdges {
    /// Zoom in as little as needed, so the edges never show past the stage.
    #[default]
    ZoomInToFill,
    /// Keep the framing: the stage's edge stretches where the camera looks past it.
    Stretch,
}

impl FillEdges {
    pub fn gpu(self) -> bpe_gfx::FillEdges {
        match self {
            FillEdges::ZoomInToFill => bpe_gfx::FillEdges::ZoomIn,
            FillEdges::Stretch => bpe_gfx::FillEdges::Stretch,
        }
    }
}

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
    /// Swing the camera round a 3D stage: needs the Lighting card's Depth set to Stage.
    #[setting(label = "Stage camera", group = "Stage camera")]
    pub stage_camera: bool,
    /// Swings the camera round the stage: positive goes right.
    #[setting(label = "Orbit", when = "stage_camera", range(-89, 89), span(-45, 45), unit = "°", keyframe, group = "Stage camera")]
    pub orbit: f32,
    /// Raises the camera over the stage to look down on it (negative looks up from below).
    #[setting(label = "Tilt", when = "stage_camera", range(-89, 89), span(-30, 30), unit = "°", keyframe, group = "Stage camera")]
    pub tilt: f32,
    /// Lifts the camera, still looking at the middle.
    #[setting(label = "Height", when = "stage_camera", span(-500, 500), unit = "px", keyframe, group = "Stage camera")]
    pub stage_height: f32,
    /// Short lenses see the stage's depth strongly; long ones flatten it.
    #[setting(
        label = "Lens",
        when = "stage_camera",
        range(1, 1e9),
        span(12, 200),
        unit = "mm",
        keyframe,
        group = "Stage camera"
    )]
    pub lens: f32,
    /// How deep the point the camera swings round sits: 0 the balls' plane, negative toward the wall.
    #[setting(label = "Turns round", when = "stage_camera", span(-500, 500), unit = "px", keyframe, group = "Stage camera")]
    pub pivot: f32,
    /// What happens where a moved camera would see past the stage's edge.
    #[setting(label = "Edges", when = "stage_camera", group = "Stage camera")]
    pub fill_edges: FillEdges,
}

impl CameraSettings {
    /// The Stage camera at this moment, when it is on.
    pub fn stage(&self) -> Option<bpe_gfx::StageCamera> {
        self.stage_camera.then(|| bpe_gfx::StageCamera {
            orbit: self.orbit,
            tilt: self.tilt,
            height: self.stage_height,
            lens: self.lens,
            pivot: self.pivot,
            fill: self.fill_edges.gpu(),
        })
    }
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
            stage_camera: false,
            orbit: 0.0,
            tilt: 0.0,
            stage_height: 0.0,
            lens: 35.0,
            pivot: 0.0,
            fill_edges: FillEdges::ZoomInToFill,
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
