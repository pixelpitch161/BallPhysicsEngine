# Picture checks on the owner's graphics card

## Intent

Since PR #205 the checks run in WSL Ubuntu on the owner's PC, but every picture test still draws with Mesa's
software driver (lavapipe), so the expected pictures match GitHub's machines. The owner asked (2026-11-22) for the
picture tests to check only the expected pictures of their own card, an AMD Radeon RX 9070 XT on Windows 11: faster,
and testing exactly what Ball Studio draws for them. They accepted the costs: when the PC is off the picture checks
wait instead of running in the cloud, and a graphics driver update can need a one-time refresh of the expected
pictures, which a thread must be able to do without the owner running anything.

## Goals

- Every test that draws (calls `bpe_gfx::test_device()`) runs natively on Windows on the RX 9070 XT, in a new CI job
  "Pictures (owner's graphics card)".
- The expected pictures that are pinned to a device (hash and tile goldens) are made on that card and are the only
  reference; the lavapipe ones are replaced.
- Everything that doesn't draw keeps running where it runs today (WSL on the PC, GitHub's machines when it is off),
  with the physics and sound goldens unchanged.
- A refresh of the card's expected pictures is one workflow a thread can start (Actions, "Refresh GPU pictures") on
  any branch; it commits the new files there.
- Timings of the heavy scenes (liquid, lighting) on the real card are printed in the job summary on every run, so
  the liquid speed work has real numbers.

## Non-goals

- Re-blessing the CPU goldens (physics, sound) on Windows. They stay pinned on Ubuntu 24.04.
- Building or installing the Ball Studio exe (the job builds tests only).
- Changing which crates a pull request tests (`affected.py` stays).

## Anti-goals

- A picture test that silently runs nowhere. A drawing test is either run on the card or the run is not green.
- Picture checks quietly passing in the cloud while the PC is off. They wait (queued) and the gate stays pending.
- Test jobs reaching the owner's own files or credentials more than they must.
- Results that depend on what else is open (OBS, Steam and Overwolf overlays install Vulkan layers on this PC).

## The PC (from the survey, 2026-11-22)

Windows 11 Home build 26300, Ryzen 7 9800X3D (16 threads), 61.6 GB RAM, F: has 571 GB free. Vulkan sees one device,
the RX 9070 XT (AMD proprietary driver 26.11.2, Vulkan 1.4.359); the integrated Radeon isn't a Vulkan device. Rust
host toolchain is GNU (x86_64-pc-windows-gnu) with WinLibs MinGW gcc from winget; no MSVC. git, ffmpeg, python 3.13
and winget are installed. The owner's account is an administrator through UAC. The pinned Rust 1.97.0 is not
installed for Windows (a broken half-install was rolled back), and this PC's network damages downloads now and then,
so every download retries.

## Approaches

1. **Native Windows runner (picked).** A GitHub Actions runner on Windows with the real AMD Vulkan driver, exactly
   what Ball Studio uses. Tests build natively for `x86_64-pc-windows-gnu`.
2. WSL with the GPU through `/dev/dxg` and Mesa's `dzn` (Vulkan on Direct3D 12). Keeps glibc, but Ubuntu's Mesa has
   no dzn, dzn is not a conformant Vulkan driver, and it isn't what Ball Studio uses. Turned down.
3. Keep lavapipe goldens and add a Windows smoke check without picture comparison. Not what the owner asked for.

## Design

### Which tests run where

- **WSL / GitHub's machines** run the whole workspace as now, with `BPE_SKIP_GPU_TESTS=1` set on the test job.
  `test_device()` then makes no device: under nextest (one process per test, `NEXTEST` set) it ends the test there
  as passed with `std::process::exit(0)`, so every drawing test skips the same way whatever it does with the result
  (some `.expect()` the device, some sit in `OnceLock` helpers); under plain `cargo test` it returns the error as
  before.
- **Windows ("Pictures (owner's graphics card)")** runs the workspace's tests natively with the nextest profile
  `gpu`, whose `default-filter` leaves out only the tests pinned to Ubuntu's maths library: the CPU golden binaries
  (`golden`, `golden_health`, `golden_particles*`, `golden_rules`, `golden_text`) and bpe-sound's golden tests.
  Everything else runs there too, so the non-drawing tests also get a native Windows run. A non-drawing test that
  fails on Windows only because Windows' maths library rounds differently goes into that filter with a comment
  (it still runs on Ubuntu); a real Windows bug is fixed.
- The same affected-crates filter from "What changed" applies to both.
- `golden_liquid` draws (the drops move on the card), so it runs on Windows only and its golden is made there.
- Release: the Windows job also runs the device-pinned pictures in `--release` (`unlit_pictures`, `lit_flat_pinned`,
  `golden_liquid`); they leave the Linux "Goldens (release)" job, which keeps the physics and sound goldens.

### Expected pictures pinned to a device

- Files made on the card: `crates/bpe-kit/tests/golden-unlit{,-debug}.txt`, `golden-lit-flat{,-debug}.txt`,
  `golden-liquid{,-debug}.txt`. Their first line names the device they were made on:
  `# made on: AMD Radeon RX 9070 XT (AMD proprietary driver 26.11.2)`, from a new `bpe_gfx::test_device_name()`
  (wgpu `AdapterInfo` name, driver name and driver info). The PC check is the reference; nothing else is.
- When the test runs on a device whose **name** differs from the file's (a cloud container's lavapipe, say), it
  draws everything and checks that it drew, prints "not compared: made on X, this is Y", and passes. This replaces
  `BPE_ANY_GPU`.
- When the name matches and a picture differs, the test fails. If the driver part differs too, the message says the
  driver changed and that the fix is the "Refresh GPU pictures" workflow.
- The two saved PNGs (`default-background.png`, `one-ball.png`) are historical references drawn by old commits,
  compared within a tolerance. They stay; if the card falls outside the tolerance the builder looks at why before
  changing anything.
- The other picture tests check numbers (brightness, coverage, colours) and are meant to hold on any card. A failure
  on the card is investigated as a possible real AMD problem (systematic debugging), not loosened by default.

### The Windows runner

- One runner `bpe-gpu-1`, labels `self-hosted, Windows, X64, bpe-gpu`, folder `F:\bpe-gpu\runner`, build files in
  `F:\bpe-gpu\target\<job>` (outside the checkout), registered with the same `beat`-style care: the registration
  token is fetched once from a GitHub login during setup and not stored.
- **Account and GPU access.** Preferred: a Windows service running as a dedicated standard local user `bpe-gpu`
  (no admin, no access to the owner's profile), if the AMD Vulkan driver works from the service session (checked by
  running `vulkaninfo --summary` and one picture test as that service). If it doesn't, the runner runs at logon in
  the owner's session as a hidden scheduled task ("BPE pictures (GPU)"), like the WSL keep-alive task, and the docs
  say so plainly. The builder records which one and why.
- Runner environment (`.env`): `VK_LOADER_LAYERS_DISABLE=~implicit~` (no overlay layers), `CARGO_TARGET_DIR` set per
  job by a step, `CARGO_NET_RETRY=10`, `RUSTUP_MAX_RETRIES=10`, the MinGW `bin` on `PATH`, priority below normal.
- Toolchain: rustup for the runner's account with the pinned 1.97.0 for `x86_64-pc-windows-gnu`, cargo-nextest at the
  version CI pins, MinGW (WinLibs) and ffmpeg reachable by that account.
- Disk: a job-start hook empties `F:\bpe-gpu\target` past 100 GB when the runner is idle.
- Setup is scripted in `tools/pc-runner/windows-gpu/` (PowerShell) and run once by Claude through a Remote Control
  session; the owner only answers the UAC prompts.
- Tests at a time: picked from a measured run (start at 8; the card has 16 GB).

### CI

- `changes` adds `gpu=true` when any crate with drawing tests is affected (i.e. whenever `code` is true; kept as one
  output so it can be narrowed later). Fork pull requests never route to the card.
- New job `gpu-pictures` ("Pictures (owner's graphics card)"), `runs-on: [self-hosted, Windows, bpe-gpu]`, needs
  `changes`, runs when `code == 'true'` and the run is not from a fork. Steps: checkout, toolchain (retrying), build
  files folder, `cargo nextest run --workspace --profile gpu -E '<filter>'`, then the release pinned pictures, then
  timings (`continue-on-error`, results to `$GITHUB_STEP_SUMMARY`).
- If the PC is off the job waits in the queue; GitHub cancels it after 24 hours. The gate fails then; "Re-run" once
  the PC is back.
- The gate job is renamed **"Tests"** (no branch protection names it; checked 2026-11-22) and requires the test shards
  and `gpu-pictures`. A fork pull request fails the gate with a message saying the picture checks run only on the
  owner's PC.
- The Linux test job is renamed "Tests k/n (debug, no drawing)" and sets `BPE_SKIP_GPU_TESTS=1`.
- "Goldens (release)" drops `unlit_pictures` and `lit_flat_pinned`.

### Refreshing the card's pictures

- New workflow `.github/workflows/gpu-pictures.yml`, `workflow_dispatch` with input `branch`. Runs on `bpe-gpu`,
  `permissions: contents: write, actions: write`. Checks out the branch, runs the device-pinned tests with
  `BLESS_GOLDEN=1` in debug and in release, and if any of the pinned files changed commits them ("Refresh the expected
  pictures on the owner's graphics card", the device line in the message) and pushes to that branch. It refuses
  `main` (a refresh goes through a pull request). Then, because a push made with the workflow's own token starts no
  workflow, it starts CI on that branch with a `workflow_dispatch` (allowed with that token). The step summary shows
  the diff.
- Threads use it whenever a change is meant to move pinned pictures (they can't bless them locally anymore), and after
  a driver update. CLAUDE.md gets this as a working rule.

### Heartbeat

Unchanged: it tracks the WSL runners. The Windows job always targets the card; it doesn't fall back.

## Edge cases

- PC on but Windows runner down while WSL is up: the Linux jobs run, the picture job waits; the gate is pending, the
  run summary lists the job as queued. The docs say how to restart it.
- Two runs at once: one Windows runner, so picture jobs queue one at a time. Measured time decides whether a second
  runner is worth it.
- Cloud containers: threads' local picture tests still run on lavapipe and check their numbers; the pinned ones
  print "not compared" and pass. Threads re-bless pinned pictures only through the workflow.
- In-flight branches (liquid speed PR #206, liquid look) whose pinned files change: after this merges, they run the
  refresh workflow on their branch. This PR waits for nothing; the threads are told when it merges.
- A driver update with no picture change: nothing to do.

## Verification

- Linux run with `BPE_SKIP_GPU_TESTS=1`: all green, no drawing test panics.
- Windows run on the card: all green after the pinned files are made there; the pinned files carry the RX 9070 XT line.
- Debug and release refresh through the workflow on a scratch branch: second run changes nothing (determinism on the
  card; two runs give the same hashes).
- A deliberately changed pinned line fails the Windows job with the right message; then restored.
- Timings appear in the job summary.
- Timing of a full warm run (both jobs) recorded in `docs/notes/checks.md`.
