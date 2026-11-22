# GPU picture-checks runner (Windows)

This folder sets up the GitHub Actions runner that runs Ball Studio's picture tests on the owner's graphics card
(AMD Radeon RX 9070 XT). The design is `docs/superpowers/specs/2026-11-22-gpu-picture-checks-design.md`.
Claude runs these scripts through a Remote Control session. The owner only clicks Yes on the Windows (UAC) prompts.

The scripts are kept in the repository at `tools/pc-runner/windows-gpu/`, but they **run from `F:\bpe-gpu\setup`**:
copy the folder there first. The background checks run as `bpe-gpu`, which cannot read the owner's checkout.

As built (2026-11-22): a background job as bpe-gpu sees the RX 9070 XT (AMD driver 26.11.2), so the runner is a
Windows service running as bpe-gpu, and there's no logon task. bpe-gpu is locked out of D:, E: and F: (except
`F:\bpe-gpu`) because those drives let every signed-in user in by default; the owner agreed to that.

## What it makes

| Thing | Where |
| --- | --- |
| Standard (not admin) Windows user `bpe-gpu`, hidden from the sign-in screen, background logons only | Windows accounts |
| Everything the runner uses or writes | `F:\bpe-gpu` (bpe-gpu has full control here and nowhere else of its own) |
| Rust 1.97.0 for `x86_64-pc-windows-gnu` with clippy and rustfmt | `F:\bpe-gpu\rustup`, `F:\bpe-gpu\cargo` |
| cargo-nextest 0.9.152 | `F:\bpe-gpu\cargo\bin` |
| MinGW (copy of the owner's WinLibs) and ffmpeg | `F:\bpe-gpu\mingw64`, `F:\bpe-gpu\bin` |
| GitHub Actions runner `bpe-gpu-1`, labels `self-hosted, Windows, X64, bpe-gpu` | `F:\bpe-gpu\runner` |
| Windows service `actions.runner.pixelpitch161-BallPhysicsEngine.bpe-gpu-1`, runs as bpe-gpu, starts with Windows, below-normal priority | Services |
| Job checkouts / build files | `F:\bpe-gpu\work` / `F:\bpe-gpu\target` |
| Logs of every setup run, results of the checks | `F:\bpe-gpu\logs`, `F:\bpe-gpu\probe` |

The runner gives every job: `VK_LOADER_LAYERS_DISABLE=~implicit~` (OBS, Steam and Overwolf overlay layers are
ignored), `RUSTUP_HOME`, `CARGO_HOME`, `CARGO_NET_RETRY=10`, `RUSTUP_MAX_RETRIES=10`,
`BPE_GPU_TARGET=F:\bpe-gpu\target`, and a `PATH` of cargo, MinGW, ffmpeg, git (and Git's bash), then the system
`PATH`. That's the `.env` file in `F:\bpe-gpu\runner`.

## Order (each step is safe to run again)

1. `setup-gpu-runner.ps1 -Step Probe` (administrator): creates the user and the folder, and checks that a background
   job (session 0, where services run) sees the graphics card. The result goes to `probe\vulkaninfo.txt`.
   Then `-Step Lock` (administrator) keeps bpe-gpu out of everything on D:, E: and F: except `F:\bpe-gpu`. Windows
   copies the rule onto every folder, which takes about 15 minutes on F:. Don't close its window, or run it hidden.
2. `install-tools.ps1` (owner's normal account): the tools above, the runner download (SHA-256 checked against the
   release notes) and a scratch clone for the trial run.
3. `setup-gpu-runner.ps1 -Step Smoke` (administrator): runs `cargo nextest run -p bpe-gfx --lib` once as bpe-gpu in
   the background. The result goes to `probe\smoke.log`.
4. `setup-gpu-runner.ps1 -Step Runner` (administrator): registers the runner with GitHub as a service running as
   bpe-gpu. The one-hour registration token comes from the WSL test machine's `beat` GitHub login and goes straight
   to the runner. Nothing in WSL is changed.

To run a step as administrator from a normal PowerShell (this shows the UAC prompt):

```powershell
Start-Process powershell.exe -Verb RunAs -Wait -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File','F:\bpe-gpu\setup\setup-gpu-runner.ps1','-Step','Runner'
```

## Secrets

None are stored. The bpe-gpu password is random, made fresh on every administrator run, and handed straight to
Windows. Each run also gives the new password to the runner service. Nobody needs to know it. The registration
token is used once and expires in an hour. The runner keeps its own GitHub credentials in `F:\bpe-gpu\runner`
(`.credentials`, `.credentials_rsaparams`), as every runner does.

## Common fixes

- **Runner shows offline on GitHub:** restart the service: `Restart-Service actions.runner.pixelpitch161-BallPhysicsEngine.bpe-gpu-1`
  (as administrator), or run `-Step Runner` again.
- **GitHub deleted the runner after 14 days offline:** delete `F:\bpe-gpu\runner\.runner` and run `-Step Runner` again.
- **A download came through damaged:** run the script again. Downloads retry by themselves and are checked against
  their SHA-256 where one is published.
- **Remove everything:** as administrator, run `F:\bpe-gpu\runner\config.cmd remove --token <removal token>`, then
  `Remove-LocalUser bpe-gpu` and delete `F:\bpe-gpu`.
