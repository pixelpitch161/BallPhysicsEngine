//! Shader layers inside balls: a stack with Shader layers draws with a pipeline built from the
//! scene shader plus each distinct effect it runs (see `effects::in_ball_effects`), one program
//! per distinct list of effect sources. Programs are built the first time a frame names them and
//! kept; one that doesn't build draws its balls with the plain pipeline (which leaves shader
//! layers out) and keeps its error for the editor. Building one takes about a second, so the
//! editor builds them on a background thread (its balls draw without their shader layers until
//! it is ready) and shares one store between its renderers ([`ProgramStore`]). The store keeps the
//! lighting's pipelines ([`crate::lighting::LightKit`]) and the picture effects' too, built the
//! same way: at once for exports and tests, on a background thread for the editor, where a lit
//! frame draws unlit and an effect is left out until it is ready.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use crate::{Frame, HDR_FORMAT, LAYERED_ATTRIBUTES, LayeredInstance, PATH_ATTRIBUTES, PathInstance};

/// The most programs kept; past it, the ones used longest ago go.
const KEEP_PROGRAMS: usize = 64;

/// The most picture effects kept built; past it, the ones built longest ago go (a renderer keeps
/// its own copy of the ones it draws with).
const KEEP_EFFECTS: usize = 64;

/// A picture effect as built: its pipelines, or why it can't be.
pub(crate) type EffectBuilt = Result<Arc<crate::effects::chain::Compiled>, String>;

/// The lighting's pipelines in a store.
#[derive(Default)]
enum LightSlot {
    #[default]
    None,
    Building(Receiver<crate::lighting::LightKit>),
    Ready(Box<crate::lighting::LightKit>),
}

/// How many frames back (calls of `ensure`, by any of the renderers sharing a store) a program
/// counts as in use and is never let go: about five frames of the editor's three views.
const RECENT_FRAMES: u64 = 16;

/// The effects one in-ball program runs, by slot: `sources[i]` is effect slot `i` of its shader
/// records. `key` tells programs apart: the same sources always give the same key.
#[derive(Clone, Debug, PartialEq)]
pub struct LookProgram {
    pub key: u64,
    pub sources: Vec<Cow<'static, str>>,
}

impl LookProgram {
    /// A program running `sources`, keyed by them.
    pub fn new(sources: Vec<Cow<'static, str>>) -> Self {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for s in &sources {
            s.as_ref().hash(&mut h);
        }
        Self { key: h.finish(), sources }
    }
}

/// Layered instances `start..end` of a frame, drawn with program `program` (an index into the
/// frame's `programs`) or, for `None`, the plain pipeline.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LayeredRun {
    pub program: Option<u32>,
    pub start: u32,
    pub end: u32,
}

/// One built program: its module and pipelines, normal and for each blend things blend with
/// each other by, for layered balls and for shapes with layers.
struct Made {
    module: wgpu::ShaderModule,
    normal: wgpu::RenderPipeline,
    each: BTreeMap<u32, wgpu::RenderPipeline>,
    shape: wgpu::RenderPipeline,
    shape_each: BTreeMap<u32, wgpu::RenderPipeline>,
}

/// What a pipeline draws: layered balls or shapes.
#[derive(Clone, Copy)]
enum Draws {
    Balls,
    Shapes,
}

#[derive(Default)]
pub(crate) struct Programs {
    made: HashMap<u64, Result<Made, String>>,
    /// Programs being built on a background thread, by key.
    pending: HashMap<u64, Receiver<Result<Made, String>>>,
    /// The keys the last frame named.
    used: Vec<u64>,
    /// Frames drawn so far, by every renderer sharing the store.
    frames: u64,
    /// The frame each program was last named in, by key.
    last_used: HashMap<u64, u64>,
    /// Programs are built on a background thread rather than while drawing.
    background: bool,
    /// The lighting's pipelines, once asked for.
    light: LightSlot,
    /// The picture effects built, by the key of their text, oldest first in `effects_order`.
    effects: HashMap<u64, EffectBuilt>,
    effects_order: std::collections::VecDeque<u64>,
    /// Picture effects being built on a background thread, by key.
    effects_pending: HashMap<u64, Receiver<Result<crate::effects::chain::Compiled, String>>>,
}

/// The built in-ball programs, which renderers on one device can share (see
/// [`crate::Renderer::share_programs`]), so a program is built once for all of them.
#[derive(Clone, Default)]
pub struct ProgramStore(pub(crate) Arc<Mutex<Programs>>);

impl ProgramStore {
    /// A store whose programs build on a background thread: until one is ready, its balls draw
    /// without their shader layers. For the editor; exports build while drawing, so no frame
    /// misses its effects.
    pub fn in_background() -> Self {
        let store = Self::default();
        store.lock().background = true;
        store
    }

    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, Programs> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Whether a program, the lighting or a picture effect is still being built.
    pub fn pending(&self) -> bool {
        let mut p = self.lock();
        p.collect();
        !p.pending.is_empty() || !p.effects_pending.is_empty() || matches!(p.light, LightSlot::Building(_))
    }

    /// Waits for every program being built.
    pub fn wait(&self) {
        let pending: Vec<(u64, Receiver<Result<Made, String>>)> = self.lock().pending.drain().collect();
        for (key, rx) in pending {
            let made = rx.recv().unwrap_or_else(|_| Err("the shader build stopped".to_owned()));
            self.lock().made.insert(key, made);
        }
        let effects: Vec<_> = self.lock().effects_pending.drain().collect();
        for (key, rx) in effects {
            let built = rx.recv().unwrap_or_else(|_| Err("the shader build stopped".to_owned()));
            self.lock().keep_effect(key, built.map(Arc::new));
        }
        let light = std::mem::take(&mut self.lock().light);
        self.lock().light = match light {
            LightSlot::Building(rx) => rx.recv().map_or(LightSlot::None, |kit| LightSlot::Ready(Box::new(kit))),
            other => other,
        };
    }

    /// Whether two stores hand out the same lighting pipelines (tests).
    #[cfg(test)]
    pub(crate) fn same_light(&self, other: &ProgramStore) -> bool {
        if Arc::ptr_eq(&self.0, &other.0) {
            return matches!(self.lock().light, LightSlot::Ready(_));
        }
        let (a, b) = (self.lock(), other.lock());
        match (&a.light, &b.light) {
            (LightSlot::Ready(x), LightSlot::Ready(y)) => x.same_as(y),
            _ => false,
        }
    }
}

fn pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    draws: Draws,
    each: bool,
    blend: wgpu::BlendState,
) -> wgpu::RenderPipeline {
    let (label, vs, fs, stride, attributes): (_, _, _, _, &[wgpu::VertexAttribute]) = match (draws, each) {
        (Draws::Balls, each) => (
            "layered balls with shader layers",
            "vs_layers",
            if each { "fs_layers_each" } else { "fs_layers" },
            std::mem::size_of::<LayeredInstance>(),
            &LAYERED_ATTRIBUTES,
        ),
        (Draws::Shapes, each) => (
            "shapes with shader layers",
            "vs_path",
            if each { "fs_path_each" } else { "fs_path" },
            std::mem::size_of::<PathInstance>(),
            &PATH_ATTRIBUTES,
        ),
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(vs),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: stride as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes,
            })],
        },
        primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fs),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: HDR_FORMAT,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// Builds a program, or says why it can't be.
fn build(device: &wgpu::Device, layout: &wgpu::PipelineLayout, program: &LookProgram) -> Result<Made, String> {
    let sources: Vec<&str> = program.sources.iter().map(|s| s.as_ref()).collect();
    // Checked first, so a mistake in own code reads as a line about the code.
    let full = checked_source(&sources)?;
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("layered balls with shader layers"),
        source: wgpu::ShaderSource::Wgsl(full.into()),
    });
    let over = wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING;
    let normal = pipeline(device, layout, &module, Draws::Balls, false, over);
    let shape = pipeline(device, layout, &module, Draws::Shapes, false, over);
    match pollster::block_on(scope.pop()) {
        None => Ok(Made { module, normal, each: BTreeMap::new(), shape, shape_each: BTreeMap::new() }),
        Some(error) => Err(first_line(&error.to_string())),
    }
}

/// The whole layered shader with `sources` as its in-ball effects, checked as the GPU would check
/// it, or the first line of why it can't be built.
fn checked_source(sources: &[&str]) -> Result<String, String> {
    let effects = crate::effects::in_ball_effects(sources)?;
    let full = crate::scene_source(Some(&effects));
    let module = naga::front::wgsl::parse_str(&full).map_err(|e| first_line(&e.emit_to_string(&full)))?;
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
        .validate(&module)
        .map_err(|e| first_line(&format!("{}", e.as_inner())))?;
    Ok(full)
}

/// Why an effect's `code` can't run inside a ball, in one line: its header, then its WGSL as
/// part of the ball shader. `None` when it can. No GPU is needed.
pub fn in_ball_problem(code: &str) -> Option<String> {
    if let Err(e) = crate::effects::parse(code) {
        return Some(e);
    }
    checked_source(&[code]).err()
}

/// Why an effect's `code` can't run over the picture (the Effects card), in one line: its
/// header, then its WGSL assembled with the prelude. `None` when it can. No GPU is needed.
pub fn picture_problem(code: &str) -> Option<String> {
    let full = match crate::effects::assemble(code) {
        Ok(full) => full,
        Err(e) => return Some(e),
    };
    let module = match naga::front::wgsl::parse_str(&full) {
        Ok(m) => m,
        Err(e) => return Some(first_line(&e.emit_to_string(&full))),
    };
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
        .validate(&module)
        .err()
        .map(|e| first_line(&format!("{}", e.as_inner())))
}

fn first_line(text: &str) -> String {
    text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("the GPU refused the shader").to_owned()
}

impl Programs {
    /// The lighting's pipelines: built now, or in a background store started on a thread and
    /// `None` until they are ready (the frame then draws unlit).
    pub(crate) fn light_kit(&mut self, device: &wgpu::Device) -> Option<crate::lighting::LightKit> {
        let mut stopped = false;
        if let LightSlot::Building(rx) = &self.light {
            match rx.try_recv() {
                Ok(kit) => self.light = LightSlot::Ready(Box::new(kit)),
                Err(std::sync::mpsc::TryRecvError::Empty) => return None,
                // The thread stopped: build it here instead, once (not a new thread every frame).
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.light = LightSlot::None;
                    stopped = true;
                }
            }
        }
        if let LightSlot::Ready(kit) = &self.light {
            return Some((**kit).clone());
        }
        if self.background && !stopped {
            let (tx, rx) = channel();
            let device = device.clone();
            let spawned = std::thread::Builder::new().name("lighting shader build".into()).spawn(move || {
                let _ = tx.send(crate::lighting::LightKit::build(&device));
            });
            if spawned.is_ok() {
                self.light = LightSlot::Building(rx);
                return None;
            }
        }
        let kit = crate::lighting::LightKit::build(device);
        self.light = LightSlot::Ready(Box::new(kit.clone()));
        Some(kit)
    }

    /// The picture effect `wgsl` (keyed `key`) built with `layout`: built now, or in a background
    /// store started on a thread and `None` until it is ready (the frame then leaves it out).
    pub(crate) fn effect(
        &mut self,
        device: &wgpu::Device,
        layout: &wgpu::PipelineLayout,
        key: u64,
        wgsl: &str,
    ) -> Option<EffectBuilt> {
        // Takes in every build that finished, also those of text no longer asked for (an effect's
        // code typed on), so none waits in its channel for ever.
        self.collect_effects();
        if self.effects_pending.contains_key(&key) {
            return None;
        }
        if let Some(built) = self.effects.get(&key) {
            return Some(built.clone());
        }
        if self.background {
            let (tx, rx) = channel();
            let (device, layout, text) = (device.clone(), layout.clone(), wgsl.to_owned());
            let spawned = std::thread::Builder::new().name("effect shader build".into()).spawn(move || {
                let _ = tx.send(crate::effects::chain::build(&device, &layout, &text));
            });
            if spawned.is_ok() {
                self.effects_pending.insert(key, rx);
                return None;
            }
        }
        let built = crate::effects::chain::build(device, layout, wgsl).map(Arc::new);
        self.keep_effect(key, built.clone());
        Some(built)
    }

    /// Takes in the picture effects finished on their threads.
    fn collect_effects(&mut self) {
        let done: Vec<(u64, Result<crate::effects::chain::Compiled, String>)> = self
            .effects_pending
            .iter()
            .filter_map(|(k, rx)| match rx.try_recv() {
                Ok(built) => Some((*k, built)),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Some((*k, Err("the shader build stopped".into()))),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
            })
            .collect();
        for (k, built) in done {
            self.effects_pending.remove(&k);
            self.keep_effect(k, built.map(Arc::new));
        }
    }

    fn keep_effect(&mut self, key: u64, built: EffectBuilt) {
        if self.effects.insert(key, built).is_none() {
            self.effects_order.push_back(key);
        }
        while self.effects_order.len() > KEEP_EFFECTS {
            if let Some(old) = self.effects_order.pop_front() {
                self.effects.remove(&old);
            }
        }
    }

    /// Takes in the programs finished on the background thread.
    fn collect(&mut self) {
        let done: Vec<u64> = self
            .pending
            .iter()
            .filter_map(|(k, rx)| match rx.try_recv() {
                Ok(made) => {
                    self.made.insert(*k, made);
                    Some(*k)
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.made.insert(*k, Err("the shader build stopped".to_owned()));
                    Some(*k)
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
            })
            .collect();
        for k in done {
            self.pending.remove(&k);
        }
    }

    /// Builds every program `frames` name that isn't built yet (or starts building it, in the
    /// background), with the pipelines for blending with each other by `each` where a frame
    /// needs them. True when one of them is still being built, so the frame draws without it.
    pub(crate) fn ensure<'a>(
        &mut self,
        device: &wgpu::Device,
        layout: &wgpu::PipelineLayout,
        frames: impl IntoIterator<Item = (&'a Frame, Option<bpe_core::Blend>)>,
    ) -> bool {
        self.collect();
        let mut used = Vec::new();
        for (frame, each) in frames {
            for program in &frame.programs {
                used.push(program.key);
                if self.pending.contains_key(&program.key) {
                    continue;
                }
                if self.background && !self.made.contains_key(&program.key) {
                    let (tx, rx) = channel();
                    let (device, layout, copy) = (device.clone(), layout.clone(), program.clone());
                    let spawned = std::thread::Builder::new().name("ball shader build".into()).spawn(move || {
                        let _ = tx.send(build(&device, &layout, &copy));
                    });
                    if spawned.is_ok() {
                        self.pending.insert(program.key, rx);
                        continue;
                    }
                }
                let made = self.made.entry(program.key).or_insert_with(|| build(device, layout, program));
                if let (Ok(made), Some(blend)) = (made, each) {
                    let code = blend.shader_code();
                    let state = crate::each::blend_state(blend);
                    if !made.each.contains_key(&code) {
                        made.each.insert(code, pipeline(device, layout, &made.module, Draws::Balls, true, state));
                    }
                    if !made.shape_each.contains_key(&code) {
                        let p = pipeline(device, layout, &made.module, Draws::Shapes, true, state);
                        made.shape_each.insert(code, p);
                    }
                }
            }
        }
        self.forget_unused(&used);
        let waiting = used.iter().any(|k| self.pending.contains_key(k));
        self.used = used;
        waiting
    }

    /// Counts a frame naming `used`, then, past [`KEEP_PROGRAMS`], lets go of the programs used
    /// longest ago. The store is shared by several renderers, each naming only its own: a
    /// program any of them named in the last [`RECENT_FRAMES`] frames stays.
    fn forget_unused(&mut self, used: &[u64]) {
        self.frames += 1;
        for k in used {
            self.last_used.insert(*k, self.frames);
        }
        if self.made.len() > KEEP_PROGRAMS {
            let last = |k: &u64| self.last_used.get(k).copied().unwrap_or(0);
            let mut old: Vec<(u64, u64)> =
                self.made.keys().map(|k| (last(k), *k)).filter(|(at, _)| at + RECENT_FRAMES <= self.frames).collect();
            old.sort_unstable();
            let extra = self.made.len() - KEEP_PROGRAMS;
            for (_, k) in old.into_iter().take(extra) {
                self.made.remove(&k);
            }
        }
        let (made, pending) = (&self.made, &self.pending);
        self.last_used.retain(|k, _| made.contains_key(k) || pending.contains_key(k));
    }

    /// The pipeline program `key` draws with (in a layer whose things blend with each other by
    /// `each`), when it is built.
    pub(crate) fn pipeline(&self, key: u64, each: Option<u32>) -> Option<wgpu::RenderPipeline> {
        let made = self.made.get(&key)?.as_ref().ok()?;
        match each {
            Some(code) => made.each.get(&code).cloned(),
            None => Some(made.normal.clone()),
        }
    }

    /// The pipeline shapes with program `key` draw with (in a layer whose things blend with each
    /// other by `each`), when it is built.
    pub(crate) fn shape_pipeline(&self, key: u64, each: Option<u32>) -> Option<wgpu::RenderPipeline> {
        let made = self.made.get(&key)?.as_ref().ok()?;
        match each {
            Some(code) => made.shape_each.get(&code).cloned(),
            None => Some(made.shape.clone()),
        }
    }

    /// Why programs the last frame named didn't build, by key.
    pub(crate) fn errors(&self) -> impl Iterator<Item = (u64, &str)> {
        self.used.iter().filter_map(|k| match self.made.get(k) {
            Some(Err(e)) => Some((*k, e.as_str())),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_background_store_builds_the_lighting_once_for_every_renderer() {
        let Ok((device, _)) = crate::test_device() else { return };
        let store = ProgramStore::in_background();
        if store.lock().light_kit(&device).is_none() {
            assert!(store.pending(), "building on its own thread");
            store.wait();
        }
        assert!(!store.pending());
        let a = store.lock().light_kit(&device).expect("built");
        let b = store.lock().light_kit(&device).expect("kept");
        assert!(a.same_as(&b), "every renderer gets the one kit");
        // A store that builds at once (exports, tests) has its kit straight away: another one.
        let now = ProgramStore::default();
        assert!(now.lock().light_kit(&device).is_some());
        assert!(!now.same_light(&store) && store.same_light(&store));
    }

    #[test]
    fn a_renderer_sharing_the_store_never_drops_a_program_another_one_still_draws_with() {
        // The preview draws with program 0 every frame while the ball view names a new program
        // each frame (own code being typed): program 0 stays, and the store stays small.
        let mut p = Programs::default();
        p.made.insert(0, Err(String::new()));
        for k in 1..=200 {
            p.made.insert(k, Err(String::new()));
            p.forget_unused(&[k]);
            p.forget_unused(&[0]);
            assert!(p.made.contains_key(&0), "the preview's program went at {k}");
            assert!(p.made.contains_key(&k), "the ball view's own went at {k}");
            assert!(p.made.len() <= KEEP_PROGRAMS + 1, "{} programs kept", p.made.len());
        }
        // Past the most kept, the ones named longest ago go first.
        assert!(!p.made.contains_key(&1) && p.made.contains_key(&199));
        assert!(p.last_used.len() <= p.made.len());
    }
}
