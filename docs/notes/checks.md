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
| Tests 1/4 ... 4/4 | every test in the workspace, split four ways by cargo-nextest, two at a time per machine | see the PR |
| Tests (debug, software GPU) | green only when all four test machines passed: the one name to look for | seconds |
| Goldens (release) | physics goldens, pinned pictures and sound goldens in `--release` | 10-15 min |
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

Design `docs/superpowers/specs/2026-11-21-pc-test-runner-design.md`, plan `docs/superpowers/plans/2026-11-21-pc-test-runner.md`,
setup scripts `tools/pc-runner/`.

- **Where the jobs run**: "What changed" also picks the machines. When the repository variable `BPE_PC_HEARTBEAT`
  is under 180 seconds old and the change isn't a pull request from a fork, Clippy, the tests, Goldens (release) and
  the Windows cross-build run on the owner's PC (`runs-on: [self-hosted, bpe-pc]`) and the tests are one job
  (`Tests 1/1`) with the `pc` nextest profile. Otherwise they run on GitHub's machines as above. Format, "What
  changed" and the gate always run on GitHub's machines. The step summary of "What changed" says which and why.
- **The PC side**: WSL2 distro `bpe-ci` (Ubuntu 24.04, on `D:\WSL\bpe-ci`, `.wslconfig` 44 GB / 16 threads) with
  three runners `bpe-pc-1..3` as systemd services `bpe-runner@1..3` (user `runner`, low priority). Same glibc and
  Mesa lavapipe as GitHub's machines (each runner's `.env` forces lavapipe; WSL's real-GPU driver `dzn` is never
  used), so the goldens and pinned pictures match. Windows drives aren't mounted and Windows programs can't be
  started from it. Build files stay in `/home/runner/target/bpe-pc-<n>` between runs (swept weekly past 150 GB).
  `bpe-heartbeat.timer` sets `BPE_PC_HEARTBEAT` every minute with the `runner` user's own `gh` login. The Windows
  task "BPE checks (WSL)" starts the distro at logon and lowers the VM's priority.
- **Steps skipped on the PC** (`if: runner.environment == 'github-hosted'`): Free disk space, the apt action (the
  packages are installed once by `setup-wsl.sh`) and rust-cache.
- **If the PC switches off mid-run**, its picked jobs stay queued until it is back (GitHub cancels them after 24
  hours). Re-run the run to send them to GitHub's machines.
- **Looking after it** (from Windows): `wsl -d bpe-ci -u root -- systemctl status 'bpe-runner@*'`; stop with
  `systemctl stop bpe-heartbeat.timer 'bpe-runner@*'` (the next run picks GitHub's machines after 3 minutes); remove
  each runner with `./config.sh remove` in `/home/runner/actions-runner-<n>` and `wsl --unregister bpe-ci`.

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
- Goldens: re-bless with `BLESS_GOLDEN=1`, once in debug and once with `--release` (CLAUDE.md).
