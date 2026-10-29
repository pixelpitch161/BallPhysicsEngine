//! A folder: groups things, and whatever it links in a slot is used by everything inside it.

pub mod sim;

pub use sim::FolderSim;

use bpe_core::{SettingDef, Settings, SlotId, ThingKind};

pub struct FolderKind;

/// A folder has no settings of its own.
#[derive(Clone, Debug, Default, PartialEq, Settings)]
pub struct FolderSettings {}

impl ThingKind for FolderKind {
    fn id(&self) -> &'static str {
        "folder"
    }
    fn name(&self) -> &'static str {
        "Folder"
    }
    fn icon(&self) -> &'static str {
        "folder"
    }
    fn help(&self) -> &'static str {
        "Groups things. Anything it links in a slot is used by everything inside it."
    }
    fn settings(&self) -> &'static [SettingDef] {
        FolderSettings::defs()
    }
    fn slots(&self) -> &'static [SlotId] {
        &SlotId::ALL
    }
    fn default_slots(&self) -> &'static [SlotId] {
        &[]
    }
}
