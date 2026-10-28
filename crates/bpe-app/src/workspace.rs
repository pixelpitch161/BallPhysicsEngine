//! The editor's four workspaces. Scene is the classic layout; the others put a big canvas in
//! the middle and float the preview in a corner.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Workspace {
    #[default]
    Scene,
    Rules,
    Shaders,
    Batch,
}

impl Workspace {
    pub const ALL: [Workspace; 4] = [Workspace::Scene, Workspace::Rules, Workspace::Shaders, Workspace::Batch];

    pub fn name(self) -> &'static str {
        match self {
            Workspace::Scene => "Scene",
            Workspace::Rules => "Rules",
            Workspace::Shaders => "Shaders",
            Workspace::Batch => "Batch",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            Workspace::Scene => "Design the scene and watch it big",
            Workspace::Rules => "Wire up \"when this happens, do that\" rules with blocks",
            Workspace::Shaders => "Build picture effects from blocks",
            Workspace::Batch => "Render many variations at once and pick the best",
        }
    }
}
