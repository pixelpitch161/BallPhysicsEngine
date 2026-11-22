# Checks and running tests

How the online checks are laid out, and how to run only the tests a change needs. The plan is
`docs/superpowers/plans/2026-11-21-faster-checks.md`.

## The online checks (`.github/workflows/ci.yml`)

Every job starts as soon as "What changed" has picked it, each on its own machine (the gate waits for the four test
machines). A pull request runs only what its changes can affect; a push to main checks everything.

| Job | What it does | Usual time |
| --- | --- | --- |
| What changed | `.github/scripts/affected.py`: the changed crates plus every crate that depends on them | seconds |
| Format | `cargo fmt --all --check` | under 1 min |
| Clippy | `cargo clippy --workspace --all-targets -- -D warnings` | 1-2 min |
| Tests 1/4 ... 4/4 (debug, no drawing) | every test in the workspace, split four ways by cargo-nextest, two at a time per machine; the tests that draw end at once as passed (`BPE_SKIP_GPU_TESTS=1`) | see the PR |
| Pictures (owner's graphics card) | the tests that draw (and every other test but the Ubuntu-pinned goldens) on the owner's AMD card, native Windows; then the card-pinned pictures in release and timings | see the PR |
| Tests | green only when all the test machines and the card passed: the one name to look for | seconds |
| Goldens (release) | physics goldens and sound goldens in `--release` | 10-15 min |
| Windows cross-build | the editor and the renderer for Windows | 3-10 min |

- **Splitting**: `cargo nextest run --workspace --profile ci --partition slice:<k>/4`. The slice partition lines up
  the tests of every test binary together and deals them out to the four machines, so a big binary (bpe-kit's unit
  tests) is shared out too and the one-test binaries don't all land on one machine (`count:` splits each binary on
  its own and piles them onto machine 1). nextest is pinned in `ci.yml`; move it on purpose. Each test runs in its
  own process. More machines means a shorter wait; more threads per machine doesn't work (the picture tests on the
  software driver run out of memory above two).
- **Only what a change affects** (plan `docs/superpowers/plans/2026-11-21-affected-checks.md`): notes and docs
  alone run only Format. A change in `crates/<x>/` tests `x` and every crate that depends on it (nextest `-E
  'package(..)'`, filtered before the four-way split); Goldens run only when bpe-kit or bpe-sound is picked, the
  Windows build only when bpe-studio or bpe-cli is. Anything else (`Cargo.lock`, `Cargo.toml`,
  `rust-toolchain.toml`, `.github/`, `.config/`, `assets/`, `crates/*/assets/`) runs everything. Try it locally with
  `python3 .github/scripts/affected.py main`. A new file outside these places runs everything until the script
  learns about it; a crate that reads another crate's files must be added to `SHARED_IN_CRATES` there.
- **Doc-tests**: nextest doesn't run them, so shard 1 runs `cargo test --workspace --doc` (there are none yet).
- **Caches**: Rust build files through `Swatinem/rust-cache` (shard 1 saves for the four test machines), and the
  downloaded system packages through `.github/actions/apt` (a slow Ubuntu mirror once cost 32 minutes). Change a
  cache's `prefix-key` / `cache-name` version to start it fresh.
- **Settings**: `.config/nextest.toml`. The `default` profile (local runs) and the `ci` profile both run two tests at a
  time and print SLOW for a test past two minutes; `ci` also keeps going after a failure. There are no retries: a
  flaky test should stay visible.

## On the owner's PC

Design `docs/superpowers/specs/2026-11-21-pc-test-runner-design.md`, plan
`docs/superpowers/plans/2026-11-21-pc-test-runner.md`, setup scripts `tools/pc-runner/`.

- **Where the jobs run**: "What changed" also picks the machines. When the repository variable `BPE_PC_HEARTBEAT`
  is under 180 seconds old and the change isn't a pull request from a fork, Clippy, the tests, Goldens (release) and
  the Windows cross-build run on the owner's PC (`runs-on: [self-hosted, bpe-pc]`) and the tests are one job
  (`Tests 1/1`) with the `pc` nextest profile (12 at a time with two lavapipe threads each, the slowest tests first). Otherwise they run on GitHub's
  machines as above. Format, "What changed" and the gate always run on GitHub's machines. The step summary of "What
  changed" says which and why. A manual run (Actions, CI, "Run workflow") can tick "Run on GitHub's machines".
- **The PC side**: WSL2 distro `bpe-ci` (Ubuntu 24.04, on `D:\WSL\bpe-ci`, `.wslconfig` 44 GB / 16 threads) with
  four runners `bpe-pc-1..4` (one per heavy job) as systemd services `bpe-runner@1..4` (user `runner`, `Nice=10`). Same glibc and Mesa
  lavapipe as GitHub's machines (each runner's `.env` forces lavapipe), so the goldens and pinned pictures match.
  Windows drives aren't mounted and Windows programs can't be started from it. Build files stay in
  `/home/runner/target/<job>` between runs (one folder per job, set by a `ci.yml` step, since a job can land on any
  runner); a job-start hook (`job-started.sh`) empties them past 150 GB when no other job is running. nextest and the pinned Rust are installed once by `setup-wsl.sh` (move nextest's version there and in
  `ci.yml` together). `bpe-heartbeat.timer` sets `BPE_PC_HEARTBEAT` every minute while GitHub lists a `bpe-pc` runner
  online, as user `beat`, whose `gh` login the jobs can't read. The Windows task "BPE checks (WSL)" starts the
  distro at logon.
- **Steps skipped on the PC** (`if: runner.environment == 'github-hosted'`): Free disk space, the apt action (the
  packages are installed once by `setup-wsl.sh`), rust-cache and the nextest install. `rustup toolchain install`
  runs under `flock`, since the four runners share one `~/.rustup`.
- **Fork pull requests**: their workflows need the owner's approval (repository setting "Require approval for all
  outside collaborators"). A fork can edit `ci.yml` to name the PC's label, and the runners keep their files between
  jobs, so **never approve a fork's run without reading its workflow changes**.
- **If the PC switches off mid-run**, its picked jobs stay queued until it is back (GitHub cancels them after 24
  hours). Cancel the run and use "Re-run all jobs" once the PC has been off 3 minutes ("Re-run failed jobs" keeps
  the old pick), or start a manual run with "Run on GitHub's machines".
- **Looking after it** (from Windows): `wsl -d bpe-ci -u root -- systemctl status 'bpe-runner@*'`; stop with
  `systemctl stop bpe-heartbeat.timer 'bpe-runner@*'` (the next run picks GitHub's machines after 3 minutes);
  `bpe-register` (as root) registers any runner GitHub dropped (it deletes runners offline for 14 days). Remove each
  runner with `./config.sh remove` in `/home/runner/actions-runner-<n>` and `wsl --unregister bpe-ci`.
- **Timings** (November 2026, all tests): see the PR that added this; cold start 41 minutes.

## The owner's graphics card

Design `docs/superpowers/specs/2026-11-22-gpu-picture-checks-design.md`, plan
`docs/superpowers/plans/2026-11-22-gpu-picture-checks.md`, setup scripts `tools/pc-runner/windows-gpu/`.

- **What runs there**: job "Pictures (owner's graphics card)" (`gpu-pictures` in `ci.yml`), on the native Windows
  runner `bpe-gpu-1` (labels `self-hosted, Windows, bpe-gpu`), an AMD Radeon RX 9070 XT. It runs the workspace with
  the nextest `gpu` profile, whose `default-filter` leaves out only the goldens pinned to Ubuntu's maths library (the
  CPU physics goldens and the sound goldens), then `unlit_pictures`, `lit_flat_pinned` and `golden_liquid` in
  release, then the liquid and light timings into the job summary. The Linux test machines set
  `BPE_SKIP_GPU_TESTS=1`, so every test that draws ends there as passed at `test_device()` (under nextest it exits
  the test's process with 0; under plain `cargo test` it returns an error).
- **No fallback**: while the PC is off this job waits in the queue and the "Tests" gate stays pending (GitHub cancels
  a job queued 24 hours; re-run it once the PC is back). A fork's pull request never runs there, so its gate fails.
- **Pictures pinned to the card**: `crates/bpe-kit/tests/golden-unlit*.txt`, `golden-lit-flat*.txt`,
  `golden-liquid*.txt` and `crates/bpe-gfx/tests/golden-shapes.txt` (`paths_tests::shapes_draw_as_before`). Their first line names the card and driver they were made on (`bpe_gfx::card_golden`). On
  another card (a cloud container's lavapipe) the tests draw and check what they can, print "not compared" and pass;
  on the same card a difference fails, and says so when the driver changed.
- **Refreshing them**: Actions, "Refresh GPU pictures" (`gpu-pictures.yml`), "Run workflow", naming the branch (not
  main). It blesses those files on the card in debug and release, checks a second run matches, commits them to
  the branch, and starts CI there (a push with the workflow's token starts nothing by itself). Threads use it when a
  change is meant to move those pictures and after a driver update; the owner runs nothing.
- **The runner**: a Windows service running as the standard local user `bpe-gpu` (session 0 reaches the card; checked
  with `vulkaninfo`). Everything lives in `F:\bpe-gpu` (runner, `rustup`, `cargo`, MinGW, ffmpeg, build files in
  `F:\bpe-gpu\target\<job>`). Its `.env` turns off the overlay Vulkan layers installed on the PC
  (`VK_LOADER_LAYERS_DISABLE=~implicit~`), so OBS or Steam can't change a picture.
- **Known on the card** (2026-11-22, the owner chose to switch first and fix after): four tests are left out of the
  `gpu` profile until each is fixed in its own change, which takes it out of the list there:
  `swarm_tests::part_3_forces_move_a_swarm_as_the_exact_engine_does` (Pull toward its ball ends 87 px off on the
  card), `shadow_quality::a_shadow_edge_moves_on_smoothly` (the edge steps in a 4-frame pattern on the card),
  `alloc::render_does_not_allocate` (a sound block allocates on Windows) and
  `library::every_scene_plays_its_full_length_and_lands_its_payoff` (Windows' maths moves stem-battle's payoff from
  26.5 s to 30.9 s). The last two still run on the Linux machines; the two drawing ones run nowhere until fixed.
- **Timings**: the job summary's "Timings on the owner's graphics card" (`tests/liquid_speed.rs`, `light_speed.rs`).

## Running only the tests a change needs

Project rule: test what the change touches, CI runs the rest. In the cloud container (4 cores, 15 GB) keep picture
tests at two at a time.

```sh
# One crate's unit tests
cargo test -p bpe-core --lib
# One test file (tests/golden.rs)
cargo test -p bpe-kit --test golden
# Tests whose name contains a word
cargo test -p bpe-kit --lib liquid
# A crate with picture tests: two at a time or it runs out of memory
cargo test -p bpe-gfx --lib -- --test-threads=2
```

With cargo-nextest (`cargo install cargo-nextest --locked`, a few minutes once), the same picks work and the
two-at-a-time limit comes from `.config/nextest.toml`:

```sh
cargo nextest run -p bpe-kit --test golden
cargo nextest run -p bpe-kit -E 'test(liquid)'
cargo nextest run --workspace --profile ci --partition slice:1/4   # what CI shard 1 runs
```

- A cold build of the tests takes about 6 minutes; keep `CARGO_INCREMENTAL=0` and delete `target/debug` if the disk
  fills.
- Slowest test binaries (CI, November 2026): bpe-kit lib (~36 min alone), bpe-gfx lib (~18 min), `liquid_sound`
  (~9 min), `golden_liquid` (~7 min, one test), `ready_look_pictures` (~6 min, one test). Avoid running them unless
  the change touches them.
- Goldens: re-bless with `BLESS_GOLDEN=1`, once in debug and once with `--release` (CLAUDE.md). The pictures pinned to
  the owner's card can't be blessed in a container: use the "Refresh GPU pictures" workflow.
