# Blank project on start, no built-in scenes

Asked by Aedan on 2026-11-03: "Always open to a blank project with just the Camera thing. Scene settings are a grey
background, no bloom. Arena and ball have no glow. Ball has no gravity. Remove all built-in scenes. (Do not create
built-in or example scenes unless I ask)."

Small, fully specified change, so no brainstorm or spec. This plan is the record.

## Decisions

1. **The blank project** is Scene settings plus one Camera, nothing else. `bpe_kit::new_studio_scene` builds it
   (name "New scene"). The editor opens to it every time: it no longer reopens the last scene and no longer shows the
   Start window at launch. A scene file passed on the command line (double-clicking a `.ron`) still opens that file.
2. **New defaults** live in the settings' `Default` impls, so "reset" and new things agree with the blank project:
   - `SceneSettings`: background a single mid grey (`#808080`), `bloom` 0. Scene gravity stays 500.
   - `BasicLook`: `glow` 0 (so new arenas and balls have no glow).
   - `BasicMotion`: `gravity_scale` ("Gravity ×") 0. Reading of "ball has no gravity": the ball's own gravity
     setting is 0, so raising it makes the ball fall straight away under the scene's gravity.
3. **Built-in scenes removed from the program**: the Start window (and its starter cards), `scenes::STARTERS`,
   `scenes::starters`, `scenes::long_drop`, the "Started from" path, and every file in `scenes/next/`. The toolbar's
   New button (and Ctrl+N) makes a blank project directly, asking to save changes first. The recent files the Start
   window listed move to a small "Recent" menu next to Open, so they aren't lost.
4. **Test scenes stay, but only for tests.** The determinism goldens need fixed scenes. `bpe_kit::scenes` and the
   arena-plus-ball `bpe_kit::new_scene` compile only under `cfg(test)` or a new `test-scenes` feature, which
   `bpe-kit`, `bpe-cli` and `bpe-studio` turn on as a dev-dependency only. Release exes don't contain them.
5. **The test scenes keep their old values** so goldens and picture tests don't move: `new_scene` and the scene
   builder pin the old background, bloom 0.15, glow 0.12 and gravity × 1 (`classic_*` helpers in `scenes.rs`).
   Golden files must be byte-identical to main.

## Steps

1. `bpe-kit/Cargo.toml`: `[features] test-scenes = []`; dev-dependency on itself with that feature.
   `bpe-cli` and `bpe-studio` dev-dependencies: `bpe-kit` with `test-scenes`.
2. `bpe-kit/src/lib.rs`: gate `pub mod scenes` and `new_scene`; `new_studio_scene` = `Scene::empty` + camera.
   `new_scene` pins classic values on its root, arena and ball modules.
3. `things/scene/mod.rs`, `modules/look/basic.rs`, `modules/motion/mod.rs`: new defaults.
4. `scenes.rs` / `scenes_stage5.rs`: drop starters, long drop and the scene-file tests; use `classic_look()` /
   `classic_motion()` in place of the defaults; builder's shared modules get classic values.
5. `tests/scene_files.rs`: check the saved text of every test scene (instead of files on disk) for unknown keys.
   Delete `scenes/next/`.
6. `bpe-studio`: delete `starters.rs` (and `thumbs.rs` if only it used it); `app.rs` opens blank, New makes a blank
   project, Recent menu; drop `last_scene` from `settings.rs`. `headless()` (tests) keeps the arena-plus-ball scene.
7. README and CLAUDE.md: no `scenes/next`; note the blank project and the `test-scenes` feature.

## Verify

- New tests: the blank project has exactly the root and a camera, grey background, bloom 0; a new arena and ball
  added to it have glow 0 and gravity × 0; the editor's launch scene is the blank project.
- `cargo test --workspace` (debug) and the goldens in `--release`: all pass, `git diff main -- crates/bpe-kit/tests/golden*`
  empty.
- `cargo clippy --workspace --all-targets`, `cargo fmt --check`, and `cargo build --release -p bpe-studio` (no
  `scenes` in the release build).
- One Opus review of the diff before the PR.

## Not doing

- No replacement example scenes, presets or galleries.
- Scene gravity itself is not changed.
