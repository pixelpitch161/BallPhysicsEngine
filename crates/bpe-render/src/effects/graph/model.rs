//! TEMPORARY: a copy of the `bpe-graph` types from the foundations plan, so the shader graph
//! compiler can be built before that crate lands. Replaced by `pub use bpe_graph::*` on rebase.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Graph {
    pub name: String,
    pub nodes: Vec<Node>,
    pub links: Vec<Link>,
    pub notes: Vec<Note>,
    pub enabled: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Node {
    pub id: u32,
    pub kind: String,
    pub pos: [f32; 2],
    pub params: BTreeMap<String, Param>,
    pub enabled: bool,
    pub exposed: BTreeSet<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub from: (u32, u8),
    pub to: (u32, u8),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Note {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub text: String,
    pub color: [f32; 4],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Param {
    Number(f32),
    Bool(bool),
    Text(String),
    Color([f32; 4]),
    Vector([f32; 2]),
    Choice(String),
}
