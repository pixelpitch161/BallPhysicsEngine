//! Locks on single settings, such as one ball group's size: dice rolls change everything around
//! them but leave them as they were.
//!
//! A setting is named by its path through the saved scene (`spawners/0/Grid/radius`). Each frame
//! the inspector walks the scene once and notes where each setting lives in memory, so a row
//! drawn with `&mut value` can find its own path without every row being told it. After a dice
//! roll, every locked path is copied back from the scene as it was before the roll.

use bpe_scene::Scene;
use eframe::egui;
use serde::ser::{self, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// One step of a path: a field, enum variant or map key, or a place in a list.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Seg {
    Key(String),
    Index(usize),
}

pub type Path = Vec<Seg>;

/// Parts of the scene no dice changes, left out of the walk to keep it quick.
const NOT_ROLLED: [&str; 7] = ["shapes", "shader_graphs", "logic", "animation", "modulators", "obstacles", "view"];

/// Where each setting of a scene lives in memory: its address and size, to its path.
#[derive(Default)]
pub struct Index {
    paths: HashMap<(usize, usize), Path>,
}

impl Index {
    pub fn of(scene: &Scene) -> Self {
        let mut recorder = Recorder::default();
        let _ = scene.serialize(&mut recorder);
        Self { paths: recorder.paths }
    }

    /// The path of `value`, if it is a setting inside the scene this index was made from.
    pub fn path<T>(&self, value: &T) -> Option<&Path> {
        self.paths.get(&(value as *const T as usize, std::mem::size_of::<T>()))
    }
}

// ---------- shared state in the egui context ----------

fn index_id() -> egui::Id {
    egui::Id::unique("setting locks index")
}

fn locks_id() -> egui::Id {
    egui::Id::unique("setting locks")
}

fn rolled_id() -> egui::Id {
    egui::Id::unique("setting locks rolled")
}

/// Walks the scene the panels are about to edit, so rows can find their paths.
pub fn publish(ctx: &egui::Context, scene: &Scene) {
    let index = Arc::new(Index::of(scene));
    ctx.data_mut(|d| d.insert_temp(index_id(), index));
}

/// The path of a setting being drawn, if it is one dice can change.
pub fn path_of<T>(ctx: &egui::Context, value: &T) -> Option<Path> {
    let index = ctx.data(|d| d.get_temp::<Arc<Index>>(index_id()))?;
    index.path(value).cloned()
}

pub fn locked(ctx: &egui::Context) -> HashSet<Path> {
    ctx.data(|d| d.get_temp::<HashSet<Path>>(locks_id())).unwrap_or_default()
}

pub fn is_locked(ctx: &egui::Context, path: &Path) -> bool {
    ctx.data(|d| d.get_temp::<HashSet<Path>>(locks_id()).is_some_and(|l| l.contains(path)))
}

pub fn set_locked(ctx: &egui::Context, path: &Path, lock: bool) {
    ctx.data_mut(|d| {
        let locks = d.get_temp_mut_or_default::<HashSet<Path>>(locks_id());
        if lock {
            locks.insert(path.clone());
        } else {
            locks.remove(path);
        }
    });
}

pub fn unlock_all(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove_temp::<HashSet<Path>>(locks_id()));
}

/// Notes that a dice rolled this frame, so the app puts locked settings back afterwards.
pub fn note_roll(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(rolled_id(), true));
}

/// Whether a dice rolled this frame, once.
pub fn take_roll(ctx: &egui::Context) -> bool {
    ctx.data_mut(|d| d.remove_temp::<bool>(rolled_id())).unwrap_or(false)
}

/// The "Lock when randomizing" item of a setting's right-click menu.
pub fn menu_item(ui: &mut egui::Ui, path: &Path) {
    let lock = !is_locked(ui.ctx(), path);
    let text = if lock { "Lock when randomizing" } else { "Unlock when randomizing" };
    let help = "A locked setting keeps its value when any dice rolls, including Randomize";
    if ui.button(text).on_hover_text(help).clicked() {
        set_locked(ui.ctx(), path, lock);
        ui.close();
    }
}

/// A small closed padlock at `center`, marking a locked setting.
pub fn paint_marker(painter: &egui::Painter, center: egui::Pos2) {
    let color = crate::theme::WARNING;
    let body = egui::Rect::from_center_size(center + egui::vec2(0.0, 2.0), egui::vec2(8.0, 6.0));
    painter.rect_filled(body, 1.5, color);
    let shackle = (0..=6).map(|k| {
        let a = std::f32::consts::PI * k as f32 / 6.0;
        egui::pos2(center.x + 2.5 * a.cos(), body.top() - 2.5 * a.sin())
    });
    painter.add(egui::Shape::line(shackle.collect(), egui::Stroke::new(1.3, color)));
}

// ---------- putting locked settings back ----------

fn get<'v>(value: &'v serde_json::Value, path: &[Seg]) -> Option<&'v serde_json::Value> {
    path.iter().try_fold(value, |v, seg| match seg {
        Seg::Key(key) => v.get(key.as_str()),
        Seg::Index(i) => v.get(*i),
    })
}

fn get_mut<'v>(value: &'v mut serde_json::Value, path: &[Seg]) -> Option<&'v mut serde_json::Value> {
    path.iter().try_fold(value, |v, seg| match seg {
        Seg::Key(key) => v.get_mut(key.as_str()),
        Seg::Index(i) => v.get_mut(*i),
    })
}

/// Copies every `locked` setting from `before` into `after`. Settings that no longer exist on
/// either side (a removed ball group, a different arena shape) are left as they are. Returns
/// how many were put back.
pub fn restore(locked: &HashSet<Path>, before: &Scene, after: &mut Scene) -> usize {
    if locked.is_empty() {
        return 0;
    }
    let (Ok(old), Ok(mut new)) = (serde_json::to_value(before), serde_json::to_value(&*after)) else { return 0 };
    let mut count = 0;
    for path in locked {
        if let (Some(from), Some(to)) = (get(&old, path), get_mut(&mut new, path))
            && from != to
        {
            *to = from.clone();
            count += 1;
        }
    }
    if count == 0 {
        return 0;
    }
    match serde_json::from_value::<Scene>(new) {
        Ok(mut patched) => {
            // Rolls keep balls inside the arena; a locked size or place may need moving back in.
            if locked.iter().any(|p| matches!(p.first(), Some(Seg::Key(k)) if k == "spawners" || k == "container")) {
                patched.keep_balls_inside();
            }
            *after = patched;
            count
        }
        Err(_) => 0,
    }
}

// ---------- the walk ----------

/// A serde serializer that writes nothing and notes the path of every value it passes.
#[derive(Default)]
struct Recorder {
    paths: HashMap<(usize, usize), Path>,
    path: Path,
}

#[derive(Debug)]
struct Stop;

impl std::fmt::Display for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("stop")
    }
}

impl std::error::Error for Stop {}

impl ser::Error for Stop {
    fn custom<T: std::fmt::Display>(_: T) -> Self {
        Stop
    }
}

impl Recorder {
    fn child<T: ?Sized + Serialize>(&mut self, seg: Seg, value: &T) -> Result<(), Stop> {
        self.path.push(seg);
        let key = (value as *const T as *const () as usize, std::mem::size_of_val(value));
        if !self.paths.contains_key(&key) {
            self.paths.insert(key, self.path.clone());
        }
        let result = value.serialize(&mut *self);
        self.path.pop();
        result
    }
}

/// A list, struct or map part way through being walked.
struct Compound<'a> {
    recorder: &'a mut Recorder,
    next: usize,
    /// A variant name pushed onto the path, taken off at the end.
    variant: bool,
    key: Option<String>,
}

impl<'a> Compound<'a> {
    fn new(recorder: &'a mut Recorder, variant: Option<&'static str>) -> Self {
        if let Some(name) = variant {
            recorder.path.push(Seg::Key(name.into()));
        }
        Self { recorder, next: 0, variant: variant.is_some(), key: None }
    }

    fn element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Stop> {
        let i = self.next;
        self.next += 1;
        self.recorder.child(Seg::Index(i), value)
    }

    fn field<T: ?Sized + Serialize>(&mut self, key: &'static str, value: &T) -> Result<(), Stop> {
        if self.recorder.path.is_empty() && NOT_ROLLED.contains(&key) {
            return Ok(());
        }
        self.recorder.child(Seg::Key(key.into()), value)
    }

    fn finish(self) -> Result<(), Stop> {
        if self.variant {
            self.recorder.path.pop();
        }
        Ok(())
    }
}

macro_rules! leaves {
    ($($name:ident: $ty:ty),*) => {
        $(fn $name(self, _: $ty) -> Result<(), Stop> { Ok(()) })*
    };
}

impl<'a> ser::Serializer for &'a mut Recorder {
    type Ok = ();
    type Error = Stop;
    type SerializeSeq = Compound<'a>;
    type SerializeTuple = Compound<'a>;
    type SerializeTupleStruct = Compound<'a>;
    type SerializeTupleVariant = Compound<'a>;
    type SerializeMap = Compound<'a>;
    type SerializeStruct = Compound<'a>;
    type SerializeStructVariant = Compound<'a>;

    leaves!(serialize_bool: bool, serialize_i8: i8, serialize_i16: i16, serialize_i32: i32, serialize_i64: i64,
        serialize_u8: u8, serialize_u16: u16, serialize_u32: u32, serialize_u64: u64, serialize_f32: f32,
        serialize_f64: f64, serialize_char: char, serialize_str: &str, serialize_bytes: &[u8],
        serialize_unit_struct: &'static str);

    fn serialize_none(self) -> Result<(), Stop> {
        Ok(())
    }
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<(), Stop> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<(), Stop> {
        Ok(())
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str) -> Result<(), Stop> {
        Ok(())
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(self, _: &'static str, value: &T) -> Result<(), Stop> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<(), Stop> {
        self.child(Seg::Key(variant.into()), value)
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Compound<'a>, Stop> {
        Ok(Compound::new(self, None))
    }
    fn serialize_tuple(self, _: usize) -> Result<Compound<'a>, Stop> {
        Ok(Compound::new(self, None))
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Compound<'a>, Stop> {
        Ok(Compound::new(self, None))
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Compound<'a>, Stop> {
        Ok(Compound::new(self, Some(variant)))
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Compound<'a>, Stop> {
        Ok(Compound::new(self, None))
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Compound<'a>, Stop> {
        Ok(Compound::new(self, None))
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Compound<'a>, Stop> {
        Ok(Compound::new(self, Some(variant)))
    }
}

macro_rules! sequence {
    ($($trait:ident :: $method:ident),*) => {
        $(impl ser::$trait for Compound<'_> {
            type Ok = ();
            type Error = Stop;
            fn $method<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Stop> {
                self.element(value)
            }
            fn end(self) -> Result<(), Stop> {
                self.finish()
            }
        })*
    };
}

sequence!(
    SerializeSeq::serialize_element,
    SerializeTuple::serialize_element,
    SerializeTupleStruct::serialize_field,
    SerializeTupleVariant::serialize_field
);

macro_rules! structure {
    ($($trait:ident),*) => {
        $(impl ser::$trait for Compound<'_> {
            type Ok = ();
            type Error = Stop;
            fn serialize_field<T: ?Sized + Serialize>(&mut self, key: &'static str, value: &T) -> Result<(), Stop> {
                self.field(key, value)
            }
            fn end(self) -> Result<(), Stop> {
                self.finish()
            }
        })*
    };
}

structure!(SerializeStruct, SerializeStructVariant);

impl ser::SerializeMap for Compound<'_> {
    type Ok = ();
    type Error = Stop;
    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Stop> {
        // Map keys become JSON object keys: strings as they are, numbers as text.
        self.key = match serde_json::to_value(key) {
            Ok(serde_json::Value::String(s)) => Some(s),
            Ok(other) => Some(other.to_string()),
            Err(_) => None,
        };
        Ok(())
    }
    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Stop> {
        match self.key.take() {
            Some(key) => self.recorder.child(Seg::Key(key), value),
            None => Ok(()),
        }
    }
    fn end(self) -> Result<(), Stop> {
        self.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_scene::{Dice, Group, Spawner};

    fn key(k: &str) -> Seg {
        Seg::Key(k.into())
    }

    #[test]
    fn rows_find_their_paths() {
        let scene = Scene::default();
        let index = Index::of(&scene);
        let Spawner::Grid { radius, gradient, velocity, .. } = &scene.spawners[0] else { panic!() };
        assert_eq!(index.path(radius), Some(&vec![key("spawners"), Seg::Index(0), key("Grid"), key("radius")]));
        assert_eq!(index.path(gradient), Some(&vec![key("spawners"), Seg::Index(0), key("Grid"), key("gradient")]));
        assert_eq!(index.path(velocity), Some(&vec![key("spawners"), Seg::Index(0), key("Grid"), key("velocity")]));
        assert_eq!(index.path(&scene.look.trail_width), Some(&vec![key("look"), key("trail_width")]));
        assert_eq!(index.path(&scene.look.background_top), Some(&vec![key("look"), key("background_top")]));
        let elsewhere = 1.0f32;
        assert_eq!(index.path(&elsewhere), None);
    }

    #[test]
    fn every_preset_survives_the_round_trip() {
        for preset in Scene::presets() {
            let mut scene = preset.scene.clone();
            let before = scene.clone();
            // Locking a path the roll didn't change puts nothing back and changes nothing.
            let locked = [vec![key("look"), key("trail_width")]].into();
            restore(&locked, &before, &mut scene);
            assert_eq!(scene, before, "{}", preset.name);
            let json = serde_json::to_value(&scene).unwrap();
            assert_eq!(serde_json::from_value::<Scene>(json).unwrap(), scene, "{}", preset.name);
        }
    }

    #[test]
    fn a_locked_ball_size_survives_a_roll() {
        let before = Scene::default();
        let Spawner::Grid { radius, gradient, .. } = &before.spawners[0] else { panic!() };
        let path = Index::of(&before).path(radius).cloned().unwrap();
        let (old_radius, old_colors) = (*radius, gradient.clone());
        let mut rolled = before.clone();
        rolled.roll(Group::Balls(0), &mut Dice::new(9), 1.0);
        let put_back = restore(&[path].into(), &before, &mut rolled);
        assert_eq!(put_back, 1);
        let Spawner::Grid { radius, gradient, .. } = &rolled.spawners[0] else { panic!() };
        assert_eq!(*radius, old_radius, "size kept");
        assert_ne!(*gradient, old_colors, "the rest rolled");
    }

    #[test]
    fn paths_that_no_longer_exist_are_skipped() {
        let before = Scene::default();
        let mut after = before.clone();
        after.spawners.clear();
        let gone = [vec![key("spawners"), Seg::Index(0), key("Grid"), key("radius")]].into();
        assert_eq!(restore(&gone, &before, &mut after), 0);
        assert!(after.spawners.is_empty());
    }
}
