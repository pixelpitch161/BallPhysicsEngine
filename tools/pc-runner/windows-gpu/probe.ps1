# Runs as the runner account (bpe-gpu) from a scheduled task set to "run whether user is logged on or not",
# which is the same hidden background session (session 0) a Windows service uses. setup-gpu-runner.ps1 starts it.
#
#   -Mode Vulkan : can a background job see the graphics card? Writes probe\vulkaninfo.txt
#   -Mode Tools  : installs Rust, cargo-nextest and the scratch clone as bpe-gpu (install-tools.ps1 -Part Rust)
#   -Mode Smoke  : the trial run. Builds and runs bpe-gfx's own tests on the card. Writes probe\smoke.log
param([ValidateSet('Vulkan', 'Tools', 'Smoke')][string]$Mode = 'Vulkan', [string]$ScratchBranch = 'main')

. "$PSScriptRoot\common.ps1"
$ErrorActionPreference = 'Continue'   # record every problem in the log instead of stopping at the first

# The same settings the runner's .env gives every job (see Install-Runner in setup-gpu-runner.ps1).
$env:VK_LOADER_LAYERS_DISABLE = '~implicit~'   # ignore the OBS / Steam / Overwolf overlay layers
$env:RUSTUP_HOME = $RustupHome
$env:CARGO_HOME  = $CargoHome
$env:PATH = "$CargoHome\bin;$MingwDir\bin;$BinDir;$GitDir;$env:PATH"
# The scratch clone may have been made by another account; git may use it anyway.
$env:GIT_CONFIG_COUNT = '1'; $env:GIT_CONFIG_KEY_0 = 'safe.directory'; $env:GIT_CONFIG_VALUE_0 = '*'

$whoami = whoami
$session = (Get-Process -Id $PID).SessionId

if ($Mode -eq 'Vulkan') {
    $out = "$ProbeDir\vulkaninfo.txt"
    "user: $whoami   session: $session   time: $(Get-Date -Format s)" | Set-Content $out -Encoding utf8
    & "$env:windir\System32\vulkaninfo.exe" --summary 2>&1 | Out-File $out -Append -Encoding utf8
    "exit code: $LASTEXITCODE" | Out-File $out -Append -Encoding utf8
    return
}

if ($Mode -eq 'Tools') {
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$PSScriptRoot\install-tools.ps1" -Part Rust `
        -ScratchBranch $ScratchBranch *>&1 | Out-File "$ProbeDir\tools.log" -Encoding utf8
    "exit code: $LASTEXITCODE" | Out-File "$ProbeDir\tools.log" -Append -Encoding utf8
    return
}

# Smoke
$log = "$ProbeDir\smoke.log"
"user: $whoami   session: $session   start: $(Get-Date -Format s)" | Set-Content $log -Encoding utf8
$env:CARGO_TARGET_DIR = "$TargetDir\scratch"
Set-Location $ScratchDir
$sw = [Diagnostics.Stopwatch]::StartNew()
& cargo --version 2>&1 | Out-File $log -Append -Encoding utf8
& cargo nextest run -p bpe-gfx --lib --no-fail-fast 2>&1 | Out-File $log -Append -Encoding utf8
"exit code: $LASTEXITCODE   total minutes (build + tests): $([math]::Round($sw.Elapsed.TotalMinutes, 1))" |
    Out-File $log -Append -Encoding utf8
"end: $(Get-Date -Format s)" | Out-File $log -Append -Encoding utf8
