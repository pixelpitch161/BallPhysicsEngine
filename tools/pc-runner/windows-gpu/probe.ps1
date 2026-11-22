# Runs as the runner account (bpe-gpu) from a scheduled task set to "run whether user is logged on or not",
# which is the same hidden background session (session 0) a Windows service uses. setup-gpu-runner.ps1 starts it.
#
#   -Mode Vulkan : can a background job see the graphics card? Writes probe\vulkaninfo.txt
#   -Mode Smoke  : the trial run. Builds and runs bpe-gfx's own tests on the card. Writes probe\smoke.log
param([ValidateSet('Vulkan', 'Smoke')][string]$Mode = 'Vulkan')

. "$PSScriptRoot\common.ps1"
$ErrorActionPreference = 'Continue'   # record every problem in the log instead of stopping at the first
New-Item -ItemType Directory -Force $ProbeDir | Out-Null

# The same settings the runner's .env gives every job (see install-runner part of setup-gpu-runner.ps1).
$env:VK_LOADER_LAYERS_DISABLE = '~implicit~'   # ignore the OBS / Steam / Overwolf overlay layers
$env:RUSTUP_HOME = $RustupHome
$env:CARGO_HOME  = $CargoHome
$env:PATH = "$CargoHome\bin;$MingwDir\bin;$BinDir;$GitDir;$env:PATH"

$whoami = whoami
$session = (Get-Process -Id $PID).SessionId

if ($Mode -eq 'Vulkan') {
    $out = "$ProbeDir\vulkaninfo.txt"
    "user: $whoami   session: $session   time: $(Get-Date -Format s)" | Set-Content $out -Encoding utf8
    & "$env:windir\System32\vulkaninfo.exe" --summary 2>&1 | Out-File $out -Append -Encoding utf8
    "exit code: $LASTEXITCODE" | Out-File $out -Append -Encoding utf8
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
