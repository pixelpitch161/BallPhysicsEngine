# Plan: run the checks on the owner's PC

Design: `docs/superpowers/specs/2026-11-21-pc-test-runner-design.md`. Everything on the PC is done by Claude over
Remote Control; the owner only clicks Yes on one Windows admin prompt, restarts if Windows asks, and types one
GitHub one-time code. Do not build or install the Ball Studio exe.

## Step 1: Windows side (`tools/pc-runner/setup-windows.ps1`)

Run elevated (one UAC prompt, started with `Start-Process -Verb RunAs`). Idempotent: each part checks first.

- `wsl --install --no-distribution` if `wsl --status` fails. If it reports a restart is needed, stop and ask the
  owner to restart.
- Write `%UserProfile%\.wslconfig` `[wsl2]` keys `memory=44GB`, `swap=16GB`, `processors=16`, `vmIdleTimeout=-1`,
  keeping any keys already there (there is no file today).
- `wsl --install Ubuntu-24.04 --name bpe-ci --location D:\WSL\bpe-ci --no-launch` (fallback if `--name` is not
  supported: download the Ubuntu 24.04 WSL rootfs from cloud-images.ubuntu.com, check its SHA256 against the
  published SHA256SUMS, `wsl --import bpe-ci D:\WSL\bpe-ci <file>`).
- Scheduled task `BPE checks (WSL)`: trigger at the owner's logon, run with highest privileges, hidden; action
  `powershell -WindowStyle Hidden -File <repo>\tools\pc-runner\keep-alive.ps1`. `keep-alive.ps1` starts
  `wsl.exe -d bpe-ci --exec /bin/sleep infinity` hidden, waits 20 s and sets every `vmmem*` process to
  BelowNormal. The task is copied to `C:\ProgramData\bpe-ci\keep-alive.ps1` first so it doesn't depend on the
  repo checkout's branch.

Verify: `wsl -l -v` lists `bpe-ci` as version 2; `Get-ScheduledTask "BPE checks (WSL)"` exists; running the task
leaves `bpe-ci` Running after the shell exits.

## Step 2: Inside `bpe-ci` (`tools/pc-runner/setup-wsl.sh`, run as root)

- `/etc/wsl.conf`: `[boot] systemd=true`, `[automount] enabled=false`, `[interop] enabled=false appendWindowsPath=false`,
  `[user] default=runner`. Then `wsl --terminate bpe-ci` and start it again.
- apt: the package list from the design plus `gh` (from Ubuntu's archive or GitHub's apt repo).
- User `runner`, no sudo. rustup (default profile minimal, no default toolchain; `rust-toolchain.toml` installs
  1.97.0 with rustfmt, clippy and the Windows target on first use).
- Download the latest `actions/runner` Linux x64 release into `/home/runner/actions-runner-<n>` for n = 1..3,
  checking the SHA256 the release notes publish.
- Unit files from `tools/pc-runner/`:
  - `bpe-runner@.service`: `User=runner`, `WorkingDirectory=/home/runner/actions-runner-%i`,
    `ExecStart=/home/runner/actions-runner-%i/run.sh`, `Nice=10`, `CPUWeight=50`, `IOWeight=50`, `Restart=always`,
    `KillMode=process`, `KillSignal=SIGTERM`, `TimeoutStopSec=5min`.
  - `bpe-heartbeat.service` + `.timer` (every 60 s, `User=runner`): runs `heartbeat.sh`, which exits quietly unless
    some `bpe-runner@*` is active, then `gh variable set BPE_PC_HEARTBEAT --repo pixelpitch161/BallPhysicsEngine
    --body "$(date +%s)"`.
  - `bpe-target-sweep.service` + `.timer` (weekly): `du -s /home/runner/target`, delete its contents over 150 GB.
- Each runner's `.env`: `CARGO_TARGET_DIR=/home/runner/target/bpe-pc-<n>`,
  `VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json`, `VK_ICD_FILENAMES=` the same,
  `GALLIUM_DRIVER=llvmpipe`, `PATH` including `/home/runner/.cargo/bin`.

Verify: `systemctl is-system-running` is running or degraded with nothing of ours failed; `ls /mnt` has no Windows
drives; `cmd.exe` is not found; `vulkaninfo --summary` (from `vulkan-tools`, install it) as `runner` with the `.env`
values shows only `llvmpipe`.

## Step 3: GitHub login and registration

- As `runner`: `gh auth login --hostname github.com --git-protocol https --web` (scopes `repo`); the owner types the
  one-time code shown, in their own browser. Only the code goes to the owner, never a token.
- For n = 1..3: `gh api -X POST repos/pixelpitch161/BallPhysicsEngine/actions/runners/registration-token`, then
  `./config.sh --unattended --url https://github.com/pixelpitch161/BallPhysicsEngine --token <t> --name bpe-pc-<n>
  --labels bpe-pc --work _work --replace`. The token is passed straight from one command to the next, never printed.
- `systemctl enable --now bpe-runner@{1,2,3} bpe-heartbeat.timer bpe-target-sweep.timer`.
- Repository setting: fork PR workflows from outside collaborators require approval for all outside collaborators
  (`gh api -X PUT repos/.../actions/permissions/fork-pr-contributor-approval -f approval_policy=all_external_contributors`).

Verify: `gh api repos/.../actions/runners` lists three runners online with label `bpe-pc`; `gh variable get
BPE_PC_HEARTBEAT` is within a minute of now.

## Step 4: CI (`.github/workflows/ci.yml`, `.config/nextest.toml`)

- `changes`: new step "Pick the machines" after "Pick the checks", bash:
  ```
  now=$(date +%s); beat=${{ vars.BPE_PC_HEARTBEAT || 0 }}
  own=true  # false when a pull request comes from a fork
  [ "${{ github.event_name }}" = pull_request ] && [ "${{ github.event.pull_request.head.repo.full_name }}" != "${{ github.repository }}" ] && own=false
  if $own && [ $((now - beat)) -lt 180 ]; then runner='["self-hosted","bpe-pc"]'; shards='[1]'; where="the owner's PC"
  else runner='"ubuntu-24.04"'; shards='[1,2,3,4]'; where="GitHub's machines"; fi
  ```
  Pass the head repo name and heartbeat through `env:` rather than inline `${{ }}` in the script (they are
  attacker-controlled on fork PRs). Outputs `runner`, `shards`, `profile` (`pc` or `ci`); reason to the step summary.
- `clippy`, `test-shard`, `release-goldens`, `windows`: `runs-on: ${{ fromJSON(needs.changes.outputs.runner) }}`.
  `test-shard`: `matrix.shard: ${{ fromJSON(needs.changes.outputs.shards) }}`, name `Tests ${{ matrix.shard }}/${{
  strategy.job-total }}`, partition `slice:${{ matrix.shard }}/${{ strategy.job-total }}`, profile from output.
- Skip on the PC (`if: runner.environment == 'github-hosted'`): Free disk space, every `./.github/actions/apt` use,
  every `Swatinem/rust-cache` use. Release jobs keep their env.
- `.config/nextest.toml`: `[profile.pc]` = the `ci` profile with `test-threads` from step 5.
- `docs/notes/checks.md`: a section "On the owner's PC" (what runs where, how the pick works, how to restart or remove
  it, the 24-hour queued caveat). `CLAUDE.md`: one line under Building and testing that checks normally run on the
  owner's PC.

Verify: `actionlint` if available (else careful read); push the branch and watch the run.

## Step 5: Measure and tune

- First PC run with `pc` test-threads = 8. Watch memory inside `bpe-ci` (`free -g` sampled every 10 s to a log by
  a background loop during the run). If peak > 36 GB or OOM kills, drop to 6; if peak < 20 GB and CPU not saturated,
  try 12. Optionally `LP_NUM_THREADS=4` in the runner `.env` if lavapipe oversubscribes.
- Record full-run time (cold and warm) in `docs/notes/checks.md`.

## Step 6: Fallback check

- `systemctl stop bpe-heartbeat.timer bpe-runner@{1,2,3}`, wait 3+ minutes, re-run the workflow: GitHub's machines
  picked and green. Start everything again; next run picks the PC.

## Step 7: Review and merge

- One Opus review of the whole diff (workflow + scripts) before marking the PR ready.
- Merge when checks pass and the review's points are handled.

## Things not to do

- Don't re-bless any golden: a golden mismatch on the PC means the PC setup is wrong.
- Don't put any token in GitHub secrets, the repo, logs or chat.
- Don't mount Windows drives or enable interop in `bpe-ci`.
- Don't route `fmt` or the gate job to the PC.
- Don't build or install the Ball Studio exe.

## As built (2026-11-22)

What changed from the plan while building it on the PC:

- **WSL install**: Virtual Machine Platform reported Enabled, but its files (vmcompute.exe, vmwp.exe) were never
  put in place, because Windows kept deferring the change ("Deferring shutdown processing at users request" in
  CBS.log) while a .NET update was waiting, and the Start menu had no "Update and restart". A restart through
  `InitiateShutdownW` with `SHUTDOWN_INSTALL_UPDATES` applied it. `wsl --install` then failed on TLS errors, so the
  Ubuntu 24.04.5 image was downloaded with a resuming curl loop, checked against Microsoft's published SHA-256 and
  installed with `wsl --install --from-file`.
- **This PC's network damages downloads now and then** (TLS "bad record mac", apt hash mismatches), so apt, curl,
  rustup and cargo all retry (`Acquire::Retries`, `--retry-all-errors`, `RUSTUP_MAX_RETRIES`, `CARGO_NET_RETRY`).
- **gh** comes from GitHub's apt repository: Ubuntu's 2.45 gives up on the device login with `slow_down`.
- **Users**: jobs run as `runner`; the GitHub login lives with user `beat` (heartbeat, registration), whose home the
  jobs can't read (review finding). The heartbeat checks that GitHub lists a `bpe-pc` runner online, not just that
  the services run.
- **Runner environment**: the runner's own `.path` didn't reach the jobs, so PATH and the `.env` file are set in
  `bpe-runner@.service`; `KillMode=control-group` (with `process`, a restart left old listeners taking jobs).
- **Four runners**, one per heavy job, so the release goldens don't wait for a free runner.
- **Build files**: one folder per job (`/home/runner/target/<job>`, a `ci.yml` step), not per runner: a job lands on
  any runner, and per-runner folders meant rebuilding whenever it changed runner. The weekly sweep timer became a
  job-start hook (`job-started.sh`, the name must end in `.sh`) that empties them past 150 GB when no other job runs.
- **Toolchain and nextest** are installed once by `setup-wsl.sh`; CI's `rustup toolchain install` runs under
  `flock` (three jobs installing at once broke ~/.rustup on the first run) and the nextest install is skipped on
  the PC.
- **Lavapipe**: the ICD file is `lvp_icd.json`, and Ubuntu's Mesa 25.2 has no `dzn` driver, so only llvmpipe is
  seen anyway. `LP_NUM_THREADS=2` and 12 tests at a time (bpe-gfx unit tests: 160 s at 8 x all threads, 127 s at
  12 x 2, 123 s at 16 x 1). The slowest tests start first (nextest `priority`).
- **The VM priority** can't be lowered from Windows (access denied, even elevated), so keep-alive only starts the
  distro; the runners' `Nice=10` applies inside.
- **Manual runs** can tick "Run on GitHub's machines" (`github_machines` input), the way to move a run off the PC.
- **Timings**: first cold run 41 min, then 37 min (rebuilds on runner changes), 23.5 min with the tests' build
  files warm; GitHub's machines took about 54 min.
