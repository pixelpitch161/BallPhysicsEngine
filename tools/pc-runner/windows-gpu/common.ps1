# Shared settings and helpers for the GPU picture-checks runner setup.
# Every other script in this folder loads this file first (". $PSScriptRoot\common.ps1").
# The scripts run from F:\bpe-gpu\setup (a copy of tools/pc-runner/windows-gpu in the repository), because the
# background steps run as bpe-gpu, which can't read the owner's checkout. Nothing secret lives here.

$ErrorActionPreference = 'Stop'

# --- Where everything goes ---------------------------------------------------------------------------------------
# bpe-gpu may only READ F:\bpe-gpu itself, setup, mingw64 and bin, has NO access to admin, and may WRITE only the
# folders in $RunnerWritable. That way nothing bpe-gpu can change is ever run by the owner or as administrator
# (see Set-Folders in setup-gpu-runner.ps1).
$Root        = 'F:\bpe-gpu'
$SetupDir    = "$Root\setup"         # these scripts (bpe-gpu: read only)
$AdminDir    = "$Root\admin"         # administrator logs and the checked runner download (bpe-gpu: no access)
$MingwDir    = "$Root\mingw64"       # a copy of the WinLibs MinGW compiler (bpe-gpu: read only)
$BinDir      = "$Root\bin"           # ffmpeg.exe (bpe-gpu: read only)
$LogDir      = "$Root\logs"          # what the bpe-gpu steps printed
$ProbeDir    = "$Root\probe"         # results of the background checks
$RustupHome  = "$Root\rustup"        # Rust toolchains (written only by bpe-gpu)
$CargoHome   = "$Root\cargo"         # cargo, rustup and cargo-nextest, plus downloaded crates (written only by bpe-gpu)
$RunnerDir   = "$Root\runner"        # the GitHub Actions runner (it updates itself, so bpe-gpu writes here)
$WorkDir     = "$Root\work"          # where the runner checks out the repository for each job
$TargetDir   = "$Root\target"        # build files (BPE_GPU_TARGET; one sub-folder per job)
$ScratchDir  = "$Root\scratch"       # a clone used for the trial run
$DownloadDir = "$Root\downloads"     # rustup-init, downloaded by bpe-gpu
$RunnerWritable = @($LogDir, $ProbeDir, $RustupHome, $CargoHome, $RunnerDir, $WorkDir, $TargetDir, $ScratchDir, $DownloadDir)
$RunnerReadOnly = @($SetupDir, $MingwDir, $BinDir)

$RunnerUser   = 'bpe-gpu'                # standard (not admin) local Windows user the runner runs as
$RunnerName   = 'bpe-gpu-1'              # the runner's name on GitHub
$RunnerLabels = 'bpe-gpu'                # GitHub adds self-hosted, Windows and X64 itself
$Repo         = 'pixelpitch161/BallPhysicsEngine'
$WslDistro    = 'bpe-ci'                 # the WSL test machine; only its `beat` user's GitHub login is used, nothing changed
$RustVersion  = '1.97.0'                 # must match rust-toolchain.toml
$NextestVersion = '0.9.152'              # must match the version CI pins
# Branch for the scratch clone. A script that takes -ScratchBranch sets it before loading this file.
if (-not (Get-Variable ScratchBranch -ErrorAction SilentlyContinue) -or -not $ScratchBranch) { $ScratchBranch = 'main' }

$GitDir = 'C:\Program Files\Git\cmd'

# Network on this PC damages downloads now and then, so everything retries.
$env:RUSTUP_MAX_RETRIES = '10'
$env:CARGO_NET_RETRY    = '10'

function Write-Step([string]$Text) {
    Write-Host ''
    Write-Host "=== $Text ===" -ForegroundColor Cyan
}

# Downloads a file with curl (which ships with Windows), retrying through dropped or damaged connections.
# If a SHA-256 is given the file must match it, otherwise it is deleted and the download is tried again.
function Get-Download([string]$Url, [string]$OutFile, [string]$Sha256 = '') {
    New-Item -ItemType Directory -Force (Split-Path $OutFile) | Out-Null
    for ($try = 1; $try -le 5; $try++) {
        & curl.exe --fail --location --silent --show-error --retry 10 --retry-all-errors --retry-delay 3 `
            --output $OutFile $Url
        if ($LASTEXITCODE -eq 0 -and (Test-Path $OutFile)) {
            if (-not $Sha256) { return }
            $got = (Get-FileHash $OutFile -Algorithm SHA256).Hash
            if ($got -ieq $Sha256) { Write-Host "  checked: $(Split-Path $OutFile -Leaf) SHA-256 $got"; return }
            Write-Warning "  $(Split-Path $OutFile -Leaf) came through damaged (SHA-256 $got), downloading again"
        }
        Remove-Item $OutFile -Force -ErrorAction SilentlyContinue
        Start-Sleep -Seconds (5 * $try)
    }
    throw "Could not download $Url intact after 5 tries"
}

# Runs a command again (up to $Tries times) until it exits with 0. For rustup, cargo and git, whose own retries
# sometimes give up on this network.
function Invoke-Retry([int]$Tries, [scriptblock]$Command) {
    for ($try = 1; $try -le $Tries; $try++) {
        & $Command
        if ($LASTEXITCODE -eq 0) { return }
        Write-Warning "  attempt $try of $Tries failed (exit $LASTEXITCODE), trying again"
        Start-Sleep -Seconds (5 * $try)
    }
    throw "Command failed $Tries times: $Command"
}

function Test-Admin {
    ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
        [Security.Principal.WindowsBuiltInRole]::Administrator)
}
