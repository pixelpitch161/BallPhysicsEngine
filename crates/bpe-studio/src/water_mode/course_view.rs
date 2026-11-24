//! Where the course sits on the Water stage: the whole course fitted into the room, and the
//! mapping between world points and screen points.

use eframe::egui::{Pos2, Rect, vec2};
use glam::Vec2;

/// The room left around the course, on every side.
const MARGIN: f32 = 16.0;

/// The course on screen. The course is centred on the world's middle, so `rect`'s centre is the
/// world point (0, 0).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CourseView {
    /// The course's outline on screen.
    pub rect: Rect,
    /// The course's size in world pixels.
    pub size: Vec2,
}

impl CourseView {
    /// The whole course of `course` world pixels, centred in `room` with a margin around it.
    pub fn fit(room: Rect, course: Vec2) -> CourseView {
        let course = course.max(Vec2::ONE);
        let inside = room.shrink(MARGIN);
        let scale = (inside.width() / course.x).min(inside.height() / course.y).max(1e-4);
        let rect = Rect::from_center_size(room.center(), vec2(course.x * scale, course.y * scale));
        CourseView { rect, size: course }
    }

    /// Screen points per world pixel.
    pub fn scale(self) -> f32 {
        self.rect.width() / self.size.x
    }

    pub fn to_screen(self, world: Vec2) -> Pos2 {
        self.rect.center() + vec2(world.x, world.y) * self.scale()
    }

    pub fn to_world(self, screen: Pos2) -> Vec2 {
        let d = (screen - self.rect.center()) / self.scale();
        Vec2::new(d.x, d.y)
    }

    /// Whether the screen point is on the course.
    pub fn holds(self, screen: Pos2) -> bool {
        self.rect.contains(screen)
    }
}
