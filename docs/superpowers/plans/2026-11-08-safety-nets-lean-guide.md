# W1: safety nets and a lean guide

Workstream W1 of the 2026-11-08 improvement review (report "Ball Studio code review: what to improve", findings C1,
C2, C3, C12 and the "render at full size and look" lesson). Aedan picked it on 2026-11-08; it runs first and alone
because every later workstream leans on it. No physics, picture or sound changes: goldens and picture hashes must not
move.

## Goal

1. Every pull request is checked automatically (C2).
2. A picture test can't pass on a machine without a graphics adapter unless that was asked for (C3).
3. A standing full-size render check that a thread runs and looks at before a visual PR (review lesson).
4. CLAUDE.md drops from ~143 KB to ~15 KB; the feature detail moves to one notes file per area (C1).
5. Specs and plans that describe the removed Version 1 engine move to an archive (C12).

## Decisions

- **CI is GitHub Actions** in `.github/workflows/ci.yml`, on `pull_request` and on `push` to `main`. The repository
  is public, so standard runners cost nothing. Jobs run side by side:
  - `fmt`: `cargo fmt --all --check`.
  - `clippy`: `cargo clippy --workspace --all-targets` with `-D warnings`, after making the tree clean (only
    mechanical fixes; anything that would change behaviour gets an `#[allow]` with a reason instead).
  - `test`: `cargo test --workspace` (debug, so the `-debug` goldens are checked) on Mesa's software Vulkan driver
    (lavapipe, `mesa-vulkan-drivers`) with ffmpeg installed, so the export tests run. `bpe-gfx`'s own tests run with
    `--test-threads=2` (they run out of memory with more on a 16 GB machine) and the rest of the workspace with the
    default.
  - `release-goldens`: the physics and sound goldens in `--release` (`cargo test --release -p bpe-kit --test 'golden*'`
    and `-p bpe-sound --test 'golden*'`). The release profile's fat LTO and one codegen unit are switched off for this
    job through `CARGO_PROFILE_RELEASE_*` variables: Rust does no fast-math or float contraction, so LTO can't move a
    float result; checked locally by running the release goldens both ways before relying on it.
  - `windows`: `cargo build --release --target x86_64-pc-windows-gnu -p bpe-studio -p bpe-cli` with MinGW, LTO off
    the same way (it checks the cross-build compiles and links, not the shipped binary). Nothing is uploaded: the exe
    is only built for Aedan when he asks.
  - All jobs use `Swatinem/rust-cache` and `CARGO_INCREMENTAL=0`; a `concurrency` group cancels a PR's older runs.
  - Not done: branch protection (a repository setting the owner turns on; threads read the check result before they
    merge instead).
- **GPU-required tests**: one helper, `bpe_gfx::test_device()`, with the same return type as `headless_device()`.
  It returns the device when there is one. Without one it panics with a message naming the fix (install
  `mesa-vulkan-drivers`), unless `BPE_SKIP_GPU_TESTS=1` is set, when it returns the error and the tests skip as they
  did before. Every test that called `headless_device()` calls `test_device()` instead (a word-for-word swap only in
  test code: test files and `#[cfg(test)]` modules); the program's own calls (`bpe-cli/src/main.rs`,
  `final_frame.rs`, `export_window.rs`) keep `headless_device()`. The early `return`s stay; they now only happen when
  skipping was asked for. A unit test checks the helper's skip/panic decision through a pure function
  (`gpu_missing(skip_set) -> Skip | Fail(message)`), so it needs no GPU to test.
- **Full-size look check**: `crates/bpe-kit/tests/full_size_look.rs`, one `#[ignore]`d test, so CI compiles it
  (it can't rot) but never runs it. Built from the review's harness (`ballphysics/visual-review/lighting/
  all-prototypes-with-harness.patch`, harness half only; the shader prototypes belong to W2). It renders a fixed set of
  test scenes at the real 1080 x 1920: the lighting stress scenes from the harness at every Quality, plus one frame of
  each existing test-scene family (`lit_demo`, `stage_demo`, `surfaces_demo`, particle, text, rule, emitter scenes as
  they exist in `bpe_kit::scenes`). For each picture it writes the full PNG and four 1:1 quarter crops
  (`<name>-q1..q4.png`) so edges can be seen at real size, and a `contact-sheet.png` at quarter size. Options by
  environment: `LOOK_OUT` (folder, default `target/full-size-look`), `LOOK_ONLY` (comma list of name parts),
  `LOOK_QUALITIES`. The working rules say: after any change to what is drawn, run it, open the pictures (the crops at
  real size) and look for jagged edges, leaks, banding, seams and anything that doesn't make sense, before the PR.
- **Lean CLAUDE.md**: notes files in `docs/notes/`, the text moved word for word (nothing lost), one per area:
  `foundations.md` (the strip-down, own settings, Vary across balls), `lighting.md` (with Light shows and the Effects
  stack), `glass-light.md`, `rules.md`, `gradients.md`, `particles.md`, `text.md`, `engine.md` (the long crate map for
  settings-derive, core, runtime, gfx, kit and cli), `sound.md` (bpe-dsp, bpe-sound), `editor.md` (bpe-studio), and a
  `README.md` index. CLAUDE.md keeps: what the program is and who it is for, the crates table, the core ideas every
  thread needs in a paragraph, an index with one line per area pointing at its notes file, spec and plans, a short
  crate map, the working rules, building and testing, goldens, the egui and engine pitfalls. New rule: a finished
  part adds its "as built" summary to its area's notes file; CLAUDE.md only changes for a new area or a new rule, so
  parallel threads stop colliding on it.
- **Archive**: everything in `docs/superpowers/specs`, `plans` and `reports` dated before the 2026-11-03 strip-down
  moves (`git mv`) to `docs/superpowers/archive/{specs,plans,reports}/` with a `README.md` saying these describe
  Version 1 (kept on the `version-1` branch) and are history, not instructions. CLAUDE.md's "Design spec" row (the
  2026-10-29 modular studio spec) is dropped. Later docs that mention removed features keep their place.

## Steps and how each is verified

1. Install the container's packages; `cargo test --workspace --no-run` to time a cold build.
2. `test_device` in `crates/bpe-gfx/src/lib.rs` beside `headless_device`, plus its unit test. Swap the calls with
   `sed -i 's/\bheadless_device\b/test_device/g'` on the test files only (listed by grep; each `#[cfg(test)]` module
   checked by hand). Verify: `grep -rn headless_device` shows only the definition and the three program calls;
   `cargo test --workspace` passes with lavapipe; with `VK_ICD_FILENAMES=/nonexistent` a picture test fails with the
   message, and with `BPE_SKIP_GPU_TESTS=1` too it passes (skips).
3. `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --all --check` clean.
4. `full_size_look.rs`; run it once (`--ignored`) here, open a few pictures and crops to check they are right.
5. Release goldens with and without the LTO override: both pass, so the CI shortcut is safe.
6. `.github/workflows/ci.yml`; push, open the PR as a draft, read the run, fix until green.
7. Notes files + new CLAUDE.md. Verify: the notes files together with the new CLAUDE.md contain every line of the old
   CLAUDE.md's feature text (a script checks each old paragraph is present somewhere); CLAUDE.md is about 15 KB;
   every path it names exists.
8. Archive with `git mv`; grep for links to moved files and fix them.
9. One Opus review of the whole branch; fix; merge once CI is green.

## Not doing

- No shader, physics or look changes (W2 and W3 take the review's prototype patches).
- No exe build or install.
- No branch protection or required checks (owner's setting).
