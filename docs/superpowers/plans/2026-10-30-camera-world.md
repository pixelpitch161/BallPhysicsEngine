# Big Worlds and a Camera Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the fixed 1080 x 1920 scene into an endless world seen through one Camera thing that can follow, be keyframed, shake and punch, with Camera and World views in the editor and sound that follows the shot.

**Architecture:** The camera is a lens. Physics and things stay in world units. A camera sim writes a `CameraView` into the `World` every step (so checkpoints and scrubbing restore it) and into the `Log` (so the mixer can read it). The renderer gets the view in its globals, and every `to_clip` applies it. Things that stick to the screen are drawn into an overlay `Frame` with no camera. The studio preview's `View` maps through the camera, and a free World view adds its own pan and zoom on top.

**Tech Stack:** Rust workspace, wgpu/WGSL, egui (bpe-studio), `bpe_core` `Settings` derive.

**Spec:** `docs/superpowers/specs/2026-10-30-camera-world-design.md`

## Global Constraints

- Every scene in `scenes::all()` keeps its exact golden event hashes (`crates/bpe-kit/tests/golden*.txt`) and its picture tests (`crates/bpe-kit/tests/pictures*.rs`). Never re-bless them in this work.
- The identity camera (centre `0,0`, Zoom 1, Turn 0) must be exact in both Rust and WGSL: `rotate(p - c, -turn) * zoom` with `cos(0)=1`, `sin(0)=0` and `zoom=1` gives the input bit for bit.
- The camera sim adds no `Event`s to the log and never changes another thing's id. `ensure_camera` appends it as the newest thing.
- Zoom 1 always shows 1080 world units across the shot, whatever the export size.
- Liquid and swarm simulation areas never depend on the camera.
- Inspector settings use foldable cards and `when` conditions (see the inspector-settings work, PR #42). There are no Simple/More groups.
- BTreeMap everywhere, matching the codebase.
- The environment needs `libasound2-dev` and `mesa-vulkan-drivers` for workspace tests.
- Copy for the person stays plain, in British spelling ("colour").

## Review Focus

1. **A scene with no camera thing** (every built-in scene builder, and old files before `ensure_camera`) must render and mix exactly as today. Tested in Task 5 (`no_camera_draws_like_before`) and Task 7 (`zero_fade_mix_is_unchanged`).
2. **A follow target that has no live balls** (all removed, or a tag matching nothing) must keep the camera where it was, with no NaN. Tested in Task 3 (`follow_with_no_target_holds_still`).
3. **A world box smaller than the shot** centres the shot on the box, with no jitter. Tested in Task 4 (`box_smaller_than_shot_centres`).
4. **Extreme zoom** (0.05 and 20) and a turned camera keep preview hit testing and handles correct. Tested in Task 9 (`hit_test_under_turned_zoomed_camera`).
5. **Deleting the camera, or adding a second one**, from the tree is refused. Tested in Task 2 (`camera_cannot_be_deleted_or_doubled`).

---

### Task 1: CameraView maths, World and Log hooks

**Files:**
- Create: `crates/bpe-runtime/src/camera.rs`
- Modify: `crates/bpe-runtime/src/lib.rs` (export), `crates/bpe-runtime/src/world.rs` (field on `World`), `crates/bpe-runtime/src/events.rs` (camera track on `Log`)

**Interfaces:**
- Produces:
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub struct CameraView { pub center: Vec2, pub zoom: f32, pub turn: f32 }`, where `turn` is in radians, clockwise on screen. `impl Default` is the identity.
  - `pub const SHOT_WIDTH: f32 = 1080.0;`
  - `CameraView::to_screen(&self, world: Vec2) -> Vec2` and `to_world(&self, screen: Vec2) -> Vec2`
  - `CameraView::shot_half(&self, aspect: f32) -> Vec2`, where `aspect = height / width`, giving the half size of the shot in world units (`540/zoom`, `540*aspect/zoom`)
  - `CameraView::sees(&self, aspect: f32, center: Vec2, radius: f32) -> bool`, a conservative test using the rotated shot's bounding circle plus radius
  - `CameraView::is_identity(&self) -> bool`
  - `World.camera: CameraView` (default identity), cloned with the world
  - `Log::push_camera(&mut self, step: u64, view: CameraView)`, which stores the view only when it differs from the last one, and `Log::camera_at(&self, step: u64) -> CameraView`, which finds the last entry at or before `step` by binary search and falls back to identity. `Log::hash()` and `events()` ignore the track.

- [ ] **Step 1: Write the failing tests** in `camera.rs`:
  - `identity_is_exact`: for `p` in `[(123.456, -789.01), (0.1, 1e6)]`, assert that `CameraView::default().to_screen(p) == p` and that `to_world(p) == p`, compared exactly.
  - `round_trips`: for a view `{center: (300, -2000), zoom: 2.5, turn: 0.7}`, assert `to_world(to_screen(p))` is within `1e-3` of `p`.
  - `zoom_and_turn`: with `{center: (100, 0), zoom: 2, turn: FRAC_PI_2}`, the world point `(100, 50)` maps to screen `(100, 0)`, within `1e-4`.
  - `sees_culls`: for an identity view with aspect `1920/1080`, `sees((0,0), 10)` is true and `sees((5000,0), 10)` is false.
  - In `events.rs`, `camera_track_ignores_hash`: the log hash is equal before and after `push_camera`, `camera_at(5)` returns the view pushed at step 3, and `camera_at(1)` returns identity.
- [ ] **Step 2:** Run `cargo test -p bpe-runtime camera`. It should fail to compile.
- [ ] **Step 3:** Implement as above. `to_screen = rotate(world - center, -turn) * zoom`, and `to_world` is the inverse.
- [ ] **Step 4:** Run `cargo test -p bpe-runtime`. It should pass.
- [ ] **Step 5:** Commit with `feat(runtime): CameraView, World.camera and the log's camera track`.

### Task 2: Camera kind, Stick to screen, ensure_camera

**Files:**
- Create: `crates/bpe-kit/src/things/camera/mod.rs`
- Modify: `crates/bpe-kit/src/things/mod.rs`, `crates/bpe-kit/src/lib.rs` (`registry()`, `ensure_camera`), `crates/bpe-core/src/scene.rs` (`Thing.screen`), `crates/bpe-core/src/registry.rs` (`ThingKind::sticks_to_screen`, `ThingKind::single`), `crates/bpe-kit/src/things/text/mod.rs`, `crates/bpe-kit/src/things/scene/mod.rs` (new Scene settings), `crates/bpe-studio/src/tree.rs` (refuse delete and second add), `crates/bpe-studio/src/app.rs` and `crates/bpe-cli/src/*` (call `ensure_camera` on new, open and load)

**Interfaces:**
- Produces:
  - `pub struct CameraKind;`, which has id `"camera"`, name `"Camera"` and icon `"camera"`, with no slots.
  - `#[derive(Settings)] pub struct CameraSettings`, with these fields and cards:
    - **Shot**
      - `zoom: f32`: `range(0.05, 20)`, log slider, `keyframe`, default 1.0
      - `turn: f32`: degrees, `range(-360, 360)`, `keyframe`, default 0
      - Position uses `thing.transform.position`, and the keyframe for it uses the existing transform keys.
    - **Follow**
      - `follow: Target`: default nothing, plus a `FollowMode` Choice of `None / Target / AllBalls`. This reuses `Target` for thing and tag.
      - `follow_amount: f32`: `range(0,1)`, `keyframe`, default 1, `when = "follow_mode!=none"`
      - `smoothness: f32`: `range(0,1)`, default 0.6
      - `look_ahead: f32`: in seconds, `range(0,1)`, default 0.15
      - `slack: f32`: `range(0,0.8)`, a share of the shot, default 0.1
      - `fit_all: bool`: default false
      - `fit_margin: f32`: in px, `range(0,500)`, default 120, `when = "fit_all=true"`
    - **On hits**
      - `shake: f32`: `range(0,100)` px, default 0
      - `shake_fade: f32`: `range(0.05,2)` s, default 0.4
      - `punch: f32`: `range(0,0.5)`, default 0
      - `punch_recovery: f32`: `range(0.05,2)` s, default 0.3
      - `hits_from: Target`: default all balls
    - **World box**
      - `stay_inside: bool`: default false
      - `box_size: Vec2`: default (3240, 5760), `when = "stay_inside=true"`
      - `box_centre: Vec2`: default (0,0), `when = "stay_inside=true"`
  - `Thing.screen: Option<bool>`, which uses `#[serde(skip_serializing_if = "Option::is_none", default)]`.
  - `Thing::sticks_to_screen(&self, kind: &dyn ThingKind) -> bool`.
  - `ThingKind::sticks_to_screen(&self) -> bool { false }`, which is overridden to true by `TextKind`.
  - `ThingKind::single(&self) -> bool { false }`, which is true for the Camera and means "one per scene, can't delete".
  - New Scene settings:
    - `background_on_screen: bool`: default true, in the "Video" group
    - `off_shot_fade: f32`: `range(0,5)`, unit "shot widths", default 0, in the "Camera sound" group
  - `pub fn ensure_camera(scene: &mut Scene, reg: &Registry) -> ThingId`, which returns the existing camera or adds one under the root. It never changes other settings.
  - `pub fn new_studio_scene(reg: &Registry) -> Scene`, which is `new_scene`, then `ensure_camera`, then `off_shot_fade = 1.5`. The studio's New and the starters use it. Opening a file only calls `ensure_camera`.
  - `pub fn camera_of(scene: &Scene) -> Option<ThingId>`.

- [ ] **Step 1: Write the failing tests** in `things/camera/mod.rs` and `lib.rs`:
  - `ensure_camera_appends_once`: calling it twice gives the same id, one camera, the newest id, and every other id unchanged.
  - `old_file_loads_without_screen_or_camera`: load a stage-2 `.ron` from `scenes/next/` (or one built by `scenes::stage2`, saved and reloaded). It has no `screen` fields, and Text `sticks_to_screen` is true while Balls is false.
  - `camera_cannot_be_deleted_or_doubled` in `bpe-studio/src/tree.rs` tests: the delete action on the camera row is not offered, and the add menu doesn't list Camera when one exists.
- [ ] **Step 2:** Run `cargo test -p bpe-kit camera` and `cargo test -p bpe-studio tree`. They should fail.
- [ ] **Step 3:** Implement. Show the "Stick to screen" checkbox on the thing's first inspector card, unless the kind is `liquid`, `swarm`, `camera` or `scene`.
- [ ] **Step 4:** Run `cargo test -p bpe-core -p bpe-kit -p bpe-studio`, including goldens. They should pass with the goldens unchanged.
- [ ] **Step 5:** Commit with `feat(kit): Camera thing, Stick to screen, ensure_camera`.

### Task 3: Camera sim, shot and follow

**Files:**
- Create: `crates/bpe-kit/src/things/camera/sim.rs`
- Modify: `crates/bpe-kit/src/lib.rs` (`sims()` adds `CameraSim`), `crates/bpe-runtime/src/world.rs` (after each step, `log.push_camera(step, world.camera)` when a camera thing exists)

**Interfaces:**
- Consumes: `CameraView` and `Log::push_camera` from Task 1, and `CameraSettings` from Task 2.
- Produces:
  - `pub struct CameraSim;`, whose `ThingSim` has kind `"camera"`.
  - `#[derive(Clone)] pub struct CameraState { pub pos: Vec2, pub vel: Vec2, pub zoom: f32, pub trauma: f32, pub punch: f32 }`, stored in `World.states`.
  - `pub fn follow_point(world: &World, settings: &CameraSettings, me: ThingId) -> Option<(Vec2, Vec2, f32)>`, which returns the centre, the average velocity, and the half extent needed by Fit all. It returns `None` when there are no live targets.
  - `pub fn view_of(world: &World) -> CameraView` (this is `world.camera`).
- The algorithm, run each step after bodies move (the camera's id is the newest, so it runs last in tree order):
  - `keyed` is the transform position, `zoom` and `turn` at the step time. Keyframes are already applied by the world's physics animation.
  - `goal = follow_point.center + vel * look_ahead`.
  - Slack: if `|goal - state.pos|` is inside `slack * shot_half` on each axis, the goal becomes `state.pos`.
  - The follow blend is `target = lerp(keyed, goal, follow_amount)`.
  - Smoothing is a critically damped spring with `omega = lerp(40, 2, smoothness)` rad/s, integrated with the step's `dt`. Smoothness 0 snaps exactly.
  - Fit all sets `zoom = min(keyed_zoom, 540 / (half_extent.x + margin), 540*aspect / (half_extent.y + margin))`.
  - With no follow (`FollowMode::None`), the view is `keyed` exactly, with no spring.

- [ ] **Step 1: Write the failing tests** in `sim.rs`:
  - `unfollowed_camera_is_its_keyed_shot`: a scene with a camera at (200,-300), zoom 2 and turn 30°. After 1 s, `world.camera == {center:(200,-300), zoom:2, turn:30° in rad}` exactly.
  - `follow_converges_on_a_ball`: a ball falling with no arena, following the ball, smoothness 0.6. After 3 s the camera centre is within 60 px of the ball.
  - `scrub_equals_straight_play`: using `Playback`, `seek(4.0)` straight versus `seek(6.0)` then `seek(4.0)` give equal `world.camera` (bit-exact). Repeat with shake and punch on in Task 4.
  - `follow_with_no_target_holds_still`: following a tag that matches nothing, the camera stays at its keyed spot and all fields are finite.
  - `fit_all_keeps_spread_balls_in_shot`: two ball groups 3000 px apart, Fit all on. Every live ball satisfies `view.sees(aspect, p, r)`.
  - `camera_adds_no_events`: a golden-style run of `scenes::all()[0]` with `ensure_camera` added has a log hash equal to the run without it.
- [ ] **Step 2:** Run `cargo test -p bpe-kit camera`. It should fail.
- [ ] **Step 3:** Implement as above.
- [ ] **Step 4:** Run `cargo test -p bpe-kit`. It should pass with the goldens unchanged.
- [ ] **Step 5:** Commit with `feat(kit): camera follows, leads, fits and smooths`.

### Task 4: Shake, zoom punch, world box

**Files:**
- Modify: `crates/bpe-kit/src/things/camera/sim.rs`

**Interfaces:**
- Consumes: hit events of the current step from the log, as other sims read them, filtered by `hits_from`.
- The algorithm:
  - Each hit adds `trauma += shake * (speed / 1500).min(1)` and `punch_level += punch * (speed / 1500).min(1)`. Both are capped at `shake * 2` and `punch * 2`.
  - Each step decays them by `exp(-dt * 3 / fade)`.
  - The offset is `trauma * (n1, n2)`, where `n1` and `n2` are value noise in [-1,1] sampled at `time * 18 Hz`, seeded from `world.seed ^ camera_id`. This uses the kit's `rng.rs` and no `rand` global.
  - `view.zoom *= 1 + punch_level`.
  - The world box is applied last. Its half size is `shot_half` rotated, taken as the axis-aligned bounds of the rotated shot. Clamp the centre into `[box_centre - box_half + shot_half_aabb, box_centre + box_half - shot_half_aabb]` on each axis. Where that range is empty on an axis, use `box_centre` on that axis.

- [ ] **Step 1: Write the failing tests:**
  - `shake_fades_to_rest`: one strong hit with shake 40 and fade 0.4. 0.1 s later the centre is off the keyed spot, and 2 s later it is within 0.01 px of it.
  - `punch_zooms_and_recovers`: one hit with punch 0.2. The zoom just after is above 1.05 and returns to within 1e-3 of 1 after 2 s.
  - `box_keeps_shot_inside`: a ball followed out of a 2000 x 4000 box. At every sampled step, the shot's AABB is inside the box.
  - `box_smaller_than_shot_centres`: a 500 x 500 box at (100,100). The centre is exactly (100,100) on every step.
  - Extend `scrub_equals_straight_play` with shake and punch on.
- [ ] **Step 2:** Run `cargo test -p bpe-kit camera`. It should fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test -p bpe-kit`. It should pass.
- [ ] **Step 5:** Commit with `feat(kit): camera shake, zoom punch and world box`.

### Task 5: Renderer looks through the camera

**Files:**
- Modify:
  - `crates/bpe-gfx/src/frame.rs` (`Frame.camera`, `Frame.overlay`, `build_frame_at` routing, `Frame::cull`)
  - `crates/bpe-gfx/src/lib.rs` (`Globals`, an overlay pass)
  - `crates/bpe-gfx/src/shaders/{scene,glyph,particles,styled,trail,object,ball_look}.wgsl` (every `to_clip`)
  - `crates/bpe-kit/src/things/scene/draw.rs` (background flag)

**Interfaces:**
- Consumes: `CameraView` and `World.camera`.
- Produces:
  - `Frame.camera: CameraView`
  - `Frame.overlay: Option<Box<Frame>>`
  - `Frame.background_on_screen: bool`
  - `Globals` gains `camera: [f32; 4] = [cx, cy, zoom, turn]`. `view.z` becomes `pixel / zoom`, the world units per output pixel used for anti-aliasing.
  - The WGSL is:

    ```wgsl
    fn to_clip(p: vec2<f32>) -> vec4<f32> {
        let c = g.camera;
        let d = p - c.xy;
        let s = sin(-c.w); let k = cos(-c.w);
        let q = vec2<f32>(d.x * k - d.y * s, d.x * s + d.y * k) * c.z;
        return vec4<f32>(q.x * g.view.x, -q.y * g.view.y, 0.0, 1.0);
    }
    ```

    Put this in each shader, or in a shared include if the crate already concatenates a prelude. Check `shaders/` for one.
  - `build_frame_at` sets `frame.camera = world.camera`. A thing with `sticks_to_screen` draws into `frame.overlay`, which is created on demand with `camera` set to identity and the same size and time. The overlay is drawn after the main scene passes, using a second globals buffer with the identity camera, and before bloom and effects.
  - The background is drawn with the identity camera when `background_on_screen`. Otherwise it is drawn through the camera across the world box, or across a 1080 x 1920 area around the camera thing's start position when there is no box.
  - `Frame::cull(&mut self)` drops balls, flashes, bursts and looks whose centre and radius `!camera.sees(...)`, using a margin of 64 world units. `build_frame_at` calls it last. Only cull lists whose items each carry a centre and radius. Leave trails, soft, shapes and glyphs alone.

- [ ] **Step 1: Write the failing tests** (GPU tests use `headless_device` and skip when there is no adapter, like the existing ones):
  - `no_camera_draws_like_before`: the existing `crates/bpe-kit/tests/pictures*.rs` must still pass unchanged. Add one explicit check: a stage-2 scene rendered with and without `ensure_camera` (identity) gives byte-identical readbacks.
  - `moved_camera_moves_the_ball`: a ball at world (300,0) and a camera centred at (300,0). The brightest pixel is at the image centre within 2 px.
  - `zoom_two_doubles_size`: a ball of radius 50 at zoom 2 covers about 4 times the lit pixels (±10%).
  - `stuck_text_stays`: a Text with stick on and the camera moved to (5000,5000). The glyph pixels are in the same place as with the identity camera.
  - `culled_ball_is_not_in_frame`: after `build_frame_at` with the camera at (10000,0), `frame.balls` has no instance for a ball at (0,0).
- [ ] **Step 2:** Run `cargo test -p bpe-gfx -p bpe-kit --test pictures --test pictures7`. The new tests should fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test --workspace`. It should pass with the goldens and pictures unchanged.
- [ ] **Step 5:** Commit with `feat(gfx): draw through the camera, overlay for things stuck to the screen`.

### Task 6: Liquid, swarms and effects under the camera

**Files:**
- Modify:
  - `crates/bpe-gfx/src/flow/mod.rs` (`arena_size`)
  - `crates/bpe-gfx/src/shaders/flow/*.wgsl` (every `to_clip`, using the Task 5 version)
  - `crates/bpe-gfx/src/flow/surface.rs` (the surface splat goes through the camera globals)
  - `crates/bpe-gfx/src/pictures.rs`, `crates/bpe-gfx/src/effects/chain.rs` (hit positions)

**Interfaces:**
- Consumes: `Frame.camera`.
- Produces:
  - `arena_size(wall, fallback_half: f32)`, where the fallback is the liquid or swarm thing's own fill size passed from `FlowSource` (`960.0` when unset). It never comes from the video size.
  - `pictures::pixel_at(at, w, h, camera: &CameraView)` and the chain's hit-to-uv helper both map `camera.to_screen(at)` first, then use the existing 1080-wide screen maths.

- [ ] **Step 1: Write the failing tests:**
  - `liquid_physics_ignores_camera`: in `flow/mod.rs` tests, run the existing liquid fixture for 120 steps with the identity camera and with a camera at (400,-300), zoom 1.7. The GPU particle position readbacks are byte-equal.
  - `hit_effect_follows_camera`: in `effects/chain.rs` tests, a hit at world (300,0) with the camera centred at (300,0) maps to the centre pixel.
  - The existing flow tests at `flow/mod.rs:678` and `:706` keep passing.
- [ ] **Step 2:** Run `cargo test -p bpe-gfx flow effects pictures`. It should fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test --workspace`. It should pass.
- [ ] **Step 5:** Commit with `feat(gfx): liquid, swarms and hit effects follow the camera`.

### Task 7: Sound follows the camera

**Files:**
- Modify:
  - `crates/bpe-mix/src/lib.rs` (`MixCx`, `MixSettings`)
  - `crates/bpe-kit/src/lib.rs` (`mix_settings`, `mix_scene`)
  - `crates/bpe-kit/src/modules/sound/mix.rs:60`, `crates/bpe-kit/src/modules/melody/mix.rs:49`, `crates/bpe-kit/src/modules/drums/mix.rs:53`

**Interfaces:**
- Produces:
  - `MixSettings.off_shot_fade: f32` and `MixSettings.aspect: f32`.
  - `MixCx.screen: &dyn Fn(&Event) -> Vec2`, which gives `log.camera_at(e.step).to_screen(e.point)`.
  - `MixCx.off_shot_gain: &dyn Fn(&Event) -> f32`, which is 1 inside the shot. Outside, it is `max(0, 1 - d / (fade * SHOT_WIDTH / zoom))`, where `d` is the world distance from the point to the rotated shot rectangle. It is always 1 when `fade == 0`.
  - The three pan lines use `(cx.screen)(e).x / half_width` in place of `e.point.x / half_width`. Note velocity and gain are multiplied by `(cx.off_shot_gain)(e)`. A gain of 0 drops the note. Node-graph inputs at `sound/mix.rs:216` keep world positions. Drums on the beat (Beat events) are not faded.

- [ ] **Step 1: Write the failing tests:**
  - `zero_fade_mix_is_unchanged`: for every scene in `scenes::all()`, `mix_scene` with and without `ensure_camera` (identity, fade 0) gives equal sample hashes.
  - `off_shot_hits_fade`: a ball bouncing at world x=3000, with the camera at 0 and fade 1.5. Its notes have velocity below the in-shot run's. With fade 0 they are equal.
  - `pan_uses_screen_position`: the camera centred on a hit at world x=2000 gives that note `left == right` (within 1e-4) where stereo width > 0.
- [ ] **Step 2:** Run `cargo test -p bpe-kit -p bpe-mix mix`. It should fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test --workspace`. It should pass.
- [ ] **Step 5:** Commit with `feat(mix): off-shot fade and screen-space pan`.

### Task 8: Scores and dice use the shot

**Files:**
- Modify: `crates/bpe-kit/src/score.rs:115-150`, `crates/bpe-kit/src/random.rs`

**Interfaces:**
- Produces:
  - In `score::sample`, `on_screen` counts balls with `world.camera.sees(aspect, p, 0.0)`.
  - `random.rs` gets `fn fit_to_camera(scene: &mut Scene, reg: &Registry)`, called at the end of each roll that touches arena or obstacle size. If the scene has a camera with zoom `z` and position `c`, rolled arena sizes are scaled by `1/z`, and new arena positions are offset by `c`. A scene with no camera, or an identity camera, is untouched.

- [ ] **Step 1: Write the failing tests:**
  - `score_counts_balls_in_the_shot`: balls at world (4000,0) with the camera centred there give `on_screen == count`. With the identity camera, `on_screen == 0`.
  - `dice_fit_the_starting_shot`: a camera at (2000,0), zoom 2. After 50 rolls, every arena's bounds fit inside the shot.
- [ ] **Step 2:** Run `cargo test -p bpe-kit score random`. It should fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test -p bpe-kit`. It should pass.
- [ ] **Step 5:** Commit with `feat(kit): scores and dice follow the camera's shot`.

### Task 9: Editor, Camera and World views

**Files:**
- Modify: `crates/bpe-studio/src/preview.rs` (`View`, handles, gizmos), `crates/bpe-studio/src/app.rs` (view state, the add-at-centre rule), `crates/bpe-studio/src/inspector.rs` (the "1080 pixels wide" copy)
- Create: `crates/bpe-studio/src/preview/world_view.rs` if `preview.rs` grows past about 900 lines. Otherwise keep it in place.

**Interfaces:**
- Consumes: `CameraView`, `world.camera`, and `camera_of`.
- Produces:
  - `pub enum PreviewMode { Camera, World }`, stored per scene path in studio state, not in the scene.
  - `pub struct View { pub rect: Rect, pub scale: f32, pub camera: CameraView }`. `to_screen(p) = rect.center() + camera.to_screen(p) * scale`, and `to_world` is the inverse. In World mode, `camera` is the free view (`WorldNav { center, zoom }`, turn 0).
  - `WorldNav` handles scroll to zoom about the pointer, middle-drag or space-drag to pan, and F to frame the selection (or all things).
  - In World mode, the camera frame is drawn as the shot rectangle (rotated) with a centre move handle, corner zoom handles and a turn handle. `Handle::CameraMove`, `CameraZoom(corner)` and `CameraTurn` go into `hit_test` and `apply_drag`. Dragging writes `transform.position`, `zoom` or `turn` through the normal edit path, so a keyed setting gets a key (as `inspector.keyed_edit` does today).
  - During playback in World mode, draw the next 2 s of `log.camera_at` centres as a faint polyline. This uses the playback log's camera track, and is drawn only for steps already simulated.
  - The world box is a dashed rectangle when `stay_inside` is on. Outside the shot, a 25% black wash is drawn in World mode.
  - New things are added at `view.to_world(rect.center())`.
  - Grid and snap (merged in #45, `crates/bpe-studio/src/grid.rs`): `Grid::draw(painter, rect, centre, scale)` takes a world origin on screen and a scale with no turn. Call it with `centre = view.to_screen(Vec2::ZERO)` and `scale = view.scale * camera.zoom`. In Camera view with a turned camera, hide the grid lines but keep snapping (snapping is in world units through `snap_point`, so it is unaffected). Add a test, `grid_snaps_in_world_view`: dragging in World view at zoom 0.2 lands on grid steps.
- The copy is "World units: at Zoom 1 the shot is 1080 across."

- [ ] **Step 1: Write the failing tests** in `preview.rs` tests:
  - `hit_test_under_turned_zoomed_camera`: with the camera `{(500,800), 3.0, 0.6}` and zoom 0.05, a ball at world (520,790) is hit at `view.to_screen((520,790))` in both cases.
  - `world_view_round_trips`: for `WorldNav` with zoom 0.2 at (1000,4000), `to_world(to_screen(p)) ≈ p`.
  - `dragging_the_camera_frame_moves_the_camera`: a `CameraMove` drag by (100,0) world changes the camera thing's position by (100,0).
  - `dragging_a_keyed_zoom_adds_a_key`: zoom has a key at t=0 and the time is 2 s. A `CameraZoom` drag adds a key at 2 s.
  - `new_thing_lands_at_view_centre`: in World mode at (3000,0), an added Balls thing's position is (3000,0).
- [ ] **Step 2:** Run `cargo test -p bpe-studio preview`. It should fail.
- [ ] **Step 3:** Implement. Add the Camera / World toggle (two selectable labels) to the preview's top bar.
- [ ] **Step 4:** Run `cargo test -p bpe-studio`. It should pass. Then run the app headless as the studio's existing screenshot tests do, and save `camera-view.png` and `world-view.png` to `/mnt/project-files/ballphysics/camera/`.
- [ ] **Step 5:** Commit with `feat(studio): Camera and World preview views with a draggable camera frame`.

### Task 10: Long drop demo, Windows build, report

**Files:**
- Modify: `crates/bpe-kit/src/scenes.rs` (add `long_drop(reg)`, *not* to `scenes::all()`, so the goldens stay unchanged), `crates/bpe-studio/src/starters.rs` (list it)

**Interfaces:**
- `pub fn long_drop(reg: &Registry) -> Scene` builds:
  - a peg course 1080 wide and 7680 tall (four screens)
  - one ball released at the top
  - a camera that follows that ball with smoothness 0.5, look ahead 0.2 and punch 0.12
  - a world box around the course
  - off-shot fade 1.5
  - a stuck Text title "Long drop"

- [ ] **Step 1: Write the failing test** `long_drop_keeps_the_ball_in_shot`: sampled every 0.1 s over 20 s, the ball is always inside `world.camera`'s shot.
- [ ] **Step 2:** Run `cargo test -p bpe-kit long_drop`. It should fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test --workspace` (debug and `--release`). It should pass. Render `long-drop.mp4` with `bpe-render-next`, and copy it, the `.ron`, and Windows builds of `ball-studio.exe` and `bpe-render.exe` (cross-built as in stage 8) to `/mnt/project-files/ballphysics/camera/`. Write `REPORT.md` there, covering what landed, any gaps, and the grid/snap status.
- [ ] **Step 5:** Commit with `feat(kit): Long drop camera demo`, then push and open the PR.
