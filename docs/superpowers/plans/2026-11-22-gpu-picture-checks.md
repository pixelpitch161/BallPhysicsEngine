# Plan: picture checks on the owner's graphics card

Spec: `docs/superpowers/specs/2026-11-22-gpu-picture-checks-design.md`. Approved by the owner 2026-11-22 (project
chat: "Could we have it so it only checks the expected pictures of my gpu?", then "Yes"; "go ahead" for the Windows
changes; "lock it" for denying the runner account the data drives).

## Steps

1. **Skip mode and card goldens in `bpe-gfx`** (`crates/bpe-gfx/src/lib.rs`).
   - `test_device()`: when `BPE_SKIP_GPU_TESTS=1`, make no device; under nextest (`NEXTEST` set) print a line and
     `std::process::exit(0)`, else `bail!`. Without it, as before (fail loudly when there's no device).
   - `test_device_name() -> String`: `"<name> (<driver>, <driver info>)"` from the adapter `headless_device` picks.
   - `MADE_ON`, `enum CardGolden { Blessed, NotCompared(String), Compare { expected, driver_note } }`,
     `card_golden(path, lines)` (writes with `BLESS_GOLDEN`, else reads) and the pure `card_golden_compare(text,
     here)`. The card is the part of the name before `" ("`; the driver note is set when the full lines differ.
   - Unit test `a_card_golden_is_compared_only_on_its_own_card` (same card, newer driver, lavapipe).
   - Verify: `cargo test -p bpe-gfx --lib test_device_tests` passes.
2. **The three pinned tests** use `card_golden`: `unlit_pictures.rs` (exact lines), `lit_flat_pinned.rs` (tiles
   within 1/255), `golden_liquid.rs` (exact lines; the seek and second-run checks stay on every card).
   `BPE_ANY_GPU` goes. Doc comments say where the files are made and how to refresh them.
3. **Timing test** `crates/bpe-kit/tests/liquid_speed.rs` (ignored): each liquid test scene, ms per second of video
   after a warm-up second, drops at the end, the card's name.
4. **nextest profile `gpu`** (`.config/nextest.toml`): `default-filter` leaves out bpe-kit's `golden`,
   `golden_health`, `golden_particles*`, `golden_rules`, `golden_text` binaries and bpe-sound's `golden` tests; 8 at
   a time (tune from a measured run); the same slow-first priority and the timing test alone as `pc`.
5. **CI** (`.github/workflows/ci.yml`):
   - `changes` outputs `fork`.
   - `test-shard` sets `BPE_SKIP_GPU_TESTS=1`, renamed "Tests k/n (debug, no drawing)".
   - New `gpu-pictures` job on `[self-hosted, Windows, bpe-gpu]`, `shell: powershell`: checkout, `rustup toolchain
     install`, `CARGO_TARGET_DIR=$BPE_GPU_TARGET\<job>`, `vulkaninfo` card line, `cargo nextest run --workspace
     --profile gpu -E <filter>`, release pinned pictures, timings (`continue-on-error`, through `cmd /c` so stderr
     doesn't trip PowerShell, filtered into the step summary).
   - Gate renamed "Tests" (no branch protection names checks; checked 2026-11-22), needs `gpu-pictures`; a fork with
     code fails it with a message.
   - "Goldens (release)" runs only the CPU golden binaries by name (no Mesa package).
6. **Refresh workflow** `.github/workflows/gpu-pictures.yml` ("Refresh GPU pictures"): `workflow_dispatch` input
   `branch` (refuses main), blesses debug and release, re-runs debug without blessing (determinism on the card),
   commits the six files if changed (card line in the message), pushes, then `POST .../workflows/ci.yml/dispatches`
   with `ref: <branch>` (the token's own push starts nothing). Permissions `contents: write`, `actions: write`.
   Only usable once merged (dispatch needs the file on main), so the first card goldens for this PR are made by the
   same commands on the PC through Remote Control and pushed to this branch.
7. **Windows runner** (Remote Control on the PC; scripts in `tools/pc-runner/windows-gpu/`):
   - Standard user `bpe-gpu`, random password known only to Windows. Session-0 check: `vulkaninfo --summary` from a
     task that runs whether logged on or not shows the RX 9070 XT (done 2026-11-22: yes) → the runner is a service.
   - `F:\bpe-gpu`: `rustup` (`RUSTUP_HOME`), `cargo` (`CARGO_HOME`), Rust 1.97.0 GNU with clippy, rustfmt and the
     Windows target, cargo-nextest 0.9.152, WinLibs `mingw64`, `ffmpeg.exe`, the runner, `work`, `target`.
   - Deny `bpe-gpu` on D:, E:, F: roots, explicit allow on `F:\bpe-gpu` (owner said "lock it"); the owner's profile
     is private already.
   - Runner `bpe-gpu-1`, label `bpe-gpu`, registration token from WSL user `beat`'s `gh`, `.env` with
     `VK_LOADER_LAYERS_DISABLE=~implicit~`, `RUSTUP_HOME`, `CARGO_HOME`, retries, `BPE_GPU_TARGET`, `PATH`.
   - Verify: runner online in `actions/runners`; `cargo nextest run -p bpe-gfx --lib` as the runner account passes.
8. **First card goldens**: on the PC, in a clone of this branch with the runner's environment, `BLESS_GOLDEN=1` debug
   and release for the three tests, then a plain second run must pass; commit the six files to this branch.
9. **Docs**: `docs/notes/checks.md` (jobs table, "The owner's graphics card"), CLAUDE.md (Checks rule, goldens
   section: refresh workflow), this plan's "As built".
10. **Verify on the PR**: Linux test jobs green with drawing tests skipped (spot-check a log line "runs on the picture
    machine instead"); `gpu-pictures` green on the card; the gate green. Then one deliberate wrong line in
    `golden-unlit-debug.txt` fails `gpu-pictures` with the expected message (pushed and reverted), if cheap; at least
    checked by the unit test. Record timings.
11. **Review**: one Opus subagent review of the whole diff and scripts; fix findings.
12. **Merge** when green. Then tell the liquid threads (speed PR #206, look) that pinned pictures now refresh via the
    workflow, and save project memory.

## Not to do

- Don't re-bless CPU goldens or the sound goldens.
- Don't delete or loosen a picture test that fails on the card; debug it (it may be a real AMD problem). A non-drawing
  test that fails only on Windows maths goes in the `gpu` filter with a comment.
- Don't touch the WSL runners, the main checkout on the PC, or the Ball Studio exe.
- Don't store the runner account's password or any token on disk.

## As built (2026-11-22)

- **The runner** is a Windows service running as the standard user `bpe-gpu` (session 0 sees the RX 9070 XT through
  the AMD Vulkan driver: `vulkaninfo` from a background task as that user). The account may only log on as a batch
  job or service and is hidden from the sign-in screen; its password is made fresh on every admin run and never
  written down. Rust 1.97.0 GNU, nextest 0.9.152, WinLibs gcc 16.2 and ffmpeg live in `F:\bpe-gpu`. A native build
  needed nothing beyond MinGW (no MSVC).
- **Data drives**: D:, E: and F: let every signed-in user in by default, so (owner: "lock it") `bpe-gpu` is denied on
  them; its own allow on `F:\bpe-gpu` takes precedence. The owner's profile on C: was already private.
- **Normal priority**: below-normal on the runner (through Image File Execution Options) let the WSL checks and
  normal-priority builds starve the listener until a job missed its 30-second hand-over ("lost communication").
- **PowerShell**: steps use `powershell -NoProfile -ExecutionPolicy Bypass -Command ". '{0}'"` (the runner account's
  default policy refuses the step scripts; pwsh isn't installed).
- **One more pinned picture**: `bpe-gfx`'s `paths_tests::shapes_draw_as_before` had its hash in the code; it now uses
  `crates/bpe-gfx/tests/golden-shapes.txt` like the others. The card's pictures: unlit 109 lines changed, lit-flat 9,
  liquid unchanged (header only), shapes changed. Same pictures twice in a row on the card.
- **Review fixes**: `BPE_CARD_GOLDENS_REQUIRED=1` on the card turns "not compared" into a failure (a golden blessed by
  mistake in a container would otherwise stop being checked); `ci.yml` gets a read-only token; the refresh workflow
  keeps its write token out of `.git/config` and builds in its own folder (`target\gpu-refresh`), and checks release
  a second time too; the scripts keep `F:\bpe-gpu\setup` read-only for `bpe-gpu`.
- **First trial** (bpe-gfx unit tests as `bpe-gpu`): build 6 min, tests 461 s, 363 of 364 passed (the shapes hash
  above).
