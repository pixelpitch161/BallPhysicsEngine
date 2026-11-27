//! Where the course sits on the Water stage: the whole course fitted into the room, or the
//! camera's shot fitted into it (the stage's Shot view), and the mapping between world points and
//! screen points.

use eframe::egui::{Pos2, Rect, vec2};
use glam::Vec2;

/// The room left around the course, on every side.
const MARGIN: f32 = 16.0;

/// The course on screen. The course is centred on the world's middle, so `rect`'s centre is the
/// world point (0, 0).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CourseView {
    /// The course's outline on screen (in the Shot view, larger than the room).
    pub rect: Rect,
    /// The course's size in world pixels.
    pub size: Vec2,
    /// The part of the world shown (its lowest corner and size, world pixels): `None` the whole
    /// course, else the camera's shot.
    pub shot: Option<(Vec2, Vec2)>,
}

impl CourseView {
    /// The whole course of `course` world pixels, centred in `room` with a margin around it.
    pub fn fit(room: Rect, course: Vec2) -> CourseView {
        let course = course.max(Vec2::ONE);
        let inside = room.shrink(MARGIN);
        let scale = (inside.width() / course.x).min(inside.height() / course.y).max(1e-4);
        let rect = Rect::from_center_size(room.center(), vec2(course.x * scale, course.y * scale));
        CourseView { rect, size: course, shot: None }
    }

    /// The shot of `shot` world pixels round world point `centre`, centred in `room` with a
    /// margin around it, over the course of `course` world pixels.
    pub fn fit_shot(room: Rect, course: Vec2, centre: Vec2, shot: Vec2) -> CourseView {
        let (course, shot) = (course.max(Vec2::ONE), shot.max(Vec2::ONE));
        let inside = room.shrink(MARGIN);
        let scale = (inside.width() / shot.x).min(inside.height() / shot.y).max(1e-4);
        let middle = room.center() - vec2(centre.x, centre.y) * scale;
        let rect = Rect::from_center_size(middle, vec2(course.x * scale, course.y * scale));
        CourseView { rect, size: course, shot: Some((centre - shot / 2.0, shot)) }
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

    /// The part of the world shown: its lowest corner and size, in world pixels.
    pub fn seen(self) -> (Vec2, Vec2) {
        self.shot.unwrap_or((-self.size / 2.0, self.size))
    }

    /// Where the part of the world shown is on screen.
    pub fn seen_rect(self) -> Rect {
        match self.shot {
            None => self.rect,
            Some((lo, size)) => Rect::from_min_max(self.to_screen(lo), self.to_screen(lo + size)),
        }
    }

    /// Where drawing over the course may go, `pad` points past the part shown (only the shot in
    /// the Shot view, where the course reaches past the room).
    pub fn clip(self, pad: f32) -> Rect {
        self.rect.expand(pad).intersect(self.seen_rect().expand(pad))
    }

    /// Whether the screen point is on the course, where it is shown.
    pub fn holds(self, screen: Pos2) -> bool {
        self.rect.contains(screen) && self.seen_rect().contains(screen)
    }
}
