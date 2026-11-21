# Run the checks on the owner's PC

## Intent

The online checks take about 54 minutes on four GitHub-hosted machines (4 threads, 16 GB each, picture tests two at
a time, build files downloaded from the cache every run). The owner's PC (Ryzen 7 9800X3D, 16 threads, 62 GB,
Windows 11, lots of free disk) is idle most of the time and they are happy to keep it on. They want the checks much
faster so more work gets done, and they are the only user of the software, so using their own machine is fine.

## Goals

- Pull request and main checks run on the PC whenever it is on, much faster than today (target: under 15 minutes
  for a full run once the build files are warm).
- Exactly the same results as today: the physics goldens, sound goldens and pinned pictures are not re-blessed.
- When the PC is off, the checks run on GitHub's machines exactly as they do now, with no one doing anything.
- Strangers' pull requests (forks of the public repo) never run on the PC.

## Non-goals

- Testing on native Windows or the real AMD card (option 2, turned down for now: most goldens would need a second
  Windows-only set). A short native Windows smoke check that skips goldens is a possible follow-up.
- Building or installing the Ball Studio exe on the PC.
- Changing which tests run or how they are picked (`affected.py` stays as it is).

## Anti-goals

- A check that turns green on the PC but would fail on GitHub's machines (different maths library or graphics
  driver). The PC must match the hosted Ubuntu 24.04 + Mesa lavapipe setup.
- Checks that hang forever because the PC switched off.
- The checks making the PC unusable for music or games while they run.

## Approaches

1. **WSL2 Ubuntu 24.04 on the PC, hosted fallback (picked by the owner).** Same glibc and Mesa as the hosted
   runners, so every golden matches. Needs WSL installed once (one admin prompt and possibly a restart).
2. Native Windows runner with the real AMD GPU. Tests the owner's real setup, but libm trig and the GPU differ, so
   most goldens and pinned pictures would need a separate Windows set. Turned down for now.
3. Docker Desktop on Windows. Same Linux result as 1 but a heavier install with its own licence terms and a GUI app
   that must stay running. No advantage over 1.

## Design

### On the PC

- WSL2 with a dedicated distro named `bpe-ci` (Ubuntu 24.04), stored on `D:\WSL\bpe-ci` (lots of space, off the
  system drive). It is used only for the checks.
- `.wslconfig`: `memory=44GB`, `swap=16GB`, `processors=16`, `vmIdleTimeout=-1`. Windows keeps 18 GB for itself.
- Inside `bpe-ci`, `/etc/wsl.conf`: systemd on, Windows drives not mounted, Windows programs can't be started
  (`[automount] enabled=false`, `[interop] enabled=false`). A job can't reach the owner's files or start Windows
  programs.
- User `runner` (no sudo). The packages the hosted jobs install (`libasound2-dev pkg-config libudev-dev
  mesa-vulkan-drivers ffmpeg libxkbcommon-x11-0 libxkbcommon0 gcc-mingw-w64-x86-64 python3 git curl build-essential`)
  are installed once at setup. rustup for `runner`; `rust-toolchain.toml` picks the version as on the hosted runners.
- Graphics: Mesa's software driver lavapipe, the same as the hosted runners. WSL also offers the real GPU through
  Mesa's `dzn` driver, which would change pictures, so the runner's environment forces lavapipe:
  `VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json` (and the older `VK_ICD_FILENAMES` with the same
  value) and `GALLIUM_DRIVER=llvmpipe`, and `/dev/dxg` passthrough is not relied on.
- Three GitHub Actions runners, `bpe-pc-1`, `bpe-pc-2`, `bpe-pc-3`, each a systemd service, labels
  `self-hosted, linux, x64, bpe-pc`. Three so the tests, the release goldens and clippy / the Windows cross-build run
  at the same time like they do on GitHub. Each has `CARGO_TARGET_DIR=/home/runner/target/bpe-pc-<n>` in its `.env`,
  outside the checkout, so `actions/checkout`'s clean doesn't delete the build files and every run reuses them.
- The services run at low priority (`Nice=10`, `CPUWeight=50`, `IOWeight=50`) so they yield inside the VM. Windows
  sees one VM, so to keep the PC usable the VM's process is also set to below-normal priority by the start task (see
  next point).
- Keeping it running: a Windows scheduled task `BPE checks (WSL)`, run at the owner's logon, hidden, starts
  `wsl.exe -d bpe-ci --exec /bin/sleep infinity` so the VM stays up, and lowers the `vmmem*` process priority to
  BelowNormal. If the PC restarts, the runners come back when the owner logs in.
- Heartbeat: a systemd timer in `bpe-ci` runs every minute. When at least one runner service is active it sets the
  repository variable `BPE_PC_HEARTBEAT` to the current Unix time with `gh variable set`. The GitHub login it uses
  lives only in `bpe-ci` (`gh auth login`, done once with the owner typing a one-time code on github.com). No token
  is stored in GitHub.
- Disk: a weekly systemd timer deletes `/home/runner/target/*` when it passes 150 GB (the next run rebuilds).
- All of this is scripted in `tools/pc-runner/` (`setup-windows.ps1`, `setup-wsl.sh`, the unit files and the
  heartbeat script) so it can be redone or changed. The scripts take no secrets as arguments.

### In CI (`.github/workflows/ci.yml`)

- `changes` (hosted, as now) also picks where the heavy jobs run and outputs `runner` (a JSON `runs-on` value) and
  `shards` (a JSON list):
  - PC: the event is not a pull request from a fork (`github.event.pull_request.head.repo.full_name ==
    github.repository`, or not a pull request at all) and `vars.BPE_PC_HEARTBEAT` is less than 180 seconds old →
    `runner = ["self-hosted","bpe-pc"]`, `shards = [1]`.
  - Otherwise `runner = "ubuntu-24.04"`, `shards = [1,2,3,4]`.
  - The choice and the reason go in the step summary.
- `clippy`, `test-shard`, `release-goldens` and `windows` use `runs-on: ${{ fromJSON(needs.changes.outputs.runner)
  }}`. `fmt`, `changes` and the `test` gate stay on `ubuntu-24.04` (seconds each, and the gate must not wait on the
  PC).
- Test split: `--partition slice:<k>/<count>` with the count from `shards`, so the PC runs one job with every test.
  On the PC nextest uses the new `pc` profile (same as `ci` but more tests at a time; the number is set from a
  measured run, starting at 8). The job name becomes `Tests <k>/<n>`.
- Steps that only make sense on GitHub's machines are skipped on the PC with `if: runner.environment ==
  'github-hosted'`: "Free disk space", the apt action and `Swatinem/rust-cache` (the PC keeps its build files).
  `rustup toolchain install` and the pinned nextest install still run (both no-ops when already there).
- The gate job's rule is unchanged: every test job must pass.
- If the PC switches off after being picked, its queued jobs wait; GitHub cancels a job queued over 24 hours. The
  fix is to re-run the run, which then picks GitHub's machines. This is rare enough to accept.

### Repository settings

- "Fork pull request workflows from outside collaborators" set to "Require approval for all outside collaborators",
  so a fork's edited workflow can't name the PC's label without the owner approving. With the routing above this is a
  second lock, not the only one.

## Verification

- A pushed branch whose run shows the PC picked, all jobs green and the same tests run as the hosted run of the same
  commit (count of passed tests equal).
- The pinned pictures and goldens pass on the PC without re-blessing: proof the PC matches the hosted setup.
- Stopping the runners and waiting 3 minutes, then re-running: GitHub's machines are picked and pass.
- A fork pull request is routed to GitHub's machines (checked by reading the routing rule's output on a test run with
  the fork condition, via the step summary logic; no real fork needed).
- Timing of a full warm run recorded in `docs/notes/checks.md`.

## Left to the builder

- The exact tests-at-a-time number for the `pc` profile and any `LP_NUM_THREADS` limit, from measured memory use.
- Script details, as long as they follow the above.
