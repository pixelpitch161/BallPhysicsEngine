# Puts every tool the runner needs into F:\bpe-gpu. Runs as the owner's normal account (no administrator needed);
# the files inherit "full control for bpe-gpu" from F:\bpe-gpu (set by setup-gpu-runner.ps1 -Step Probe, run first).
# Safe to run again: each part is skipped when it is already in place.
#
#   Rust 1.97.0 (x86_64-pc-windows-gnu, clippy, rustfmt)   -> F:\bpe-gpu\rustup, F:\bpe-gpu\cargo
#   cargo-nextest                                          -> F:\bpe-gpu\cargo\bin
#   WinLibs MinGW (copied from the owner's winget install) -> F:\bpe-gpu\mingw64
#   ffmpeg.exe (copied from the owner's winget install)    -> F:\bpe-gpu\bin
#   GitHub Actions runner, latest, SHA-256 checked         -> F:\bpe-gpu\runner
#   scratch clone of the design branch for the trial run   -> F:\bpe-gpu\scratch
param([switch]$SkipScratch)

. "$PSScriptRoot\common.ps1"
New-Item -ItemType Directory -Force $LogDir, $DownloadDir, $BinDir | Out-Null
Start-Transcript -Path "$LogDir\install-tools-$(Get-Date -Format yyyyMMdd-HHmmss).log" | Out-Null

# Only this script's own Rust: never the owner's own rustup in C:\Users\aedan\.rustup.
$env:RUSTUP_HOME = $RustupHome
$env:CARGO_HOME  = $CargoHome
$env:PATH = "$CargoHome\bin;$MingwDir\bin;$GitDir;$env:PATH"
Set-Location $Root   # away from any rust-toolchain.toml

Write-Step 'MinGW compiler'
$wingetMingw = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Directory -Filter 'BrechtSanders.WinLibs*' |
    ForEach-Object { Join-Path $_.FullName 'mingw64' } | Where-Object { Test-Path "$_\bin\gcc.exe" } | Select-Object -First 1
if (-not $wingetMingw) { throw 'WinLibs MinGW not found in the winget packages folder' }
& robocopy.exe $wingetMingw $MingwDir /MIR /NFL /NDL /NJH /NP /R:3 /W:2 | Out-Null
if ($LASTEXITCODE -ge 8) { throw "Copying MinGW failed (robocopy $LASTEXITCODE)" }
& "$MingwDir\bin\gcc.exe" --version | Select-Object -First 1

Write-Step 'ffmpeg'
$ffmpeg = (Get-Item "$env:LOCALAPPDATA\Microsoft\WinGet\Links\ffmpeg.exe").Target
if (-not $ffmpeg) { $ffmpeg = (Get-Command ffmpeg.exe).Source }
Copy-Item $ffmpeg "$BinDir\ffmpeg.exe" -Force
& "$BinDir\ffmpeg.exe" -hide_banner -version | Select-Object -First 1

Write-Step "Rust $RustVersion"
if (-not (Test-Path "$CargoHome\bin\rustup.exe")) {
    $init = "$DownloadDir\rustup-init.exe"
    $base = 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-gnu/rustup-init.exe'
    Get-Download "$base.sha256" "$DownloadDir\rustup-init.exe.sha256"
    $sha = ((Get-Content "$DownloadDir\rustup-init.exe.sha256" -Raw) -split '\s+')[0]
    Get-Download $base $init $sha
    Invoke-Retry 5 { & $init -y --no-modify-path --profile minimal `
        --default-host x86_64-pc-windows-gnu --default-toolchain none }
}
Invoke-Retry 8 { & rustup toolchain install $RustVersion --profile minimal -c clippy -c rustfmt -t x86_64-pc-windows-gnu }
Invoke-Retry 3 { & rustup default $RustVersion }
& rustc -vV

Write-Step "cargo-nextest $NextestVersion"
$have = if (Test-Path "$CargoHome\bin\cargo-nextest.exe") { & "$CargoHome\bin\cargo-nextest.exe" nextest --version } else { '' }
if ("$have" -notmatch [regex]::Escape($NextestVersion)) {
    # Built from crates.io source: --locked uses its checked-in Cargo.lock and cargo checks every crate's checksum.
    $env:CARGO_TARGET_DIR = "$TargetDir\nextest-build"
    Invoke-Retry 5 { & cargo install cargo-nextest --version $NextestVersion --locked }
    Remove-Item Env:CARGO_TARGET_DIR
    Remove-Item "$TargetDir\nextest-build" -Recurse -Force -ErrorAction SilentlyContinue
}
& cargo nextest --version

Write-Step 'GitHub Actions runner'
$release = Invoke-RestMethod 'https://api.github.com/repos/actions/runner/releases/latest' -Headers @{ 'User-Agent' = 'bpe-setup' }
$version = $release.tag_name.TrimStart('v')
$zipName = "actions-runner-win-x64-$version.zip"
# Each release's notes list the SHA-256 of every package between <!-- BEGIN SHA win-x64 --> markers.
if ($release.body -notmatch '<!-- BEGIN SHA win-x64 -->([0-9a-fA-F]{64})<!-- END SHA win-x64 -->') {
    throw 'The runner release notes have no SHA-256 for win-x64'
}
$runnerSha = $Matches[1]
$current = if (Test-Path "$RunnerDir\bin\Runner.Listener.exe") { (Get-Item "$RunnerDir\bin\Runner.Listener.exe").VersionInfo.ProductVersion } else { '' }
if ("$current" -notlike "$version*") {
    if (Test-Path "$RunnerDir\.runner") {
        Write-Warning "Runner $current is registered; it updates itself, so it is left as it is."
    } else {
        $zip = "$DownloadDir\$zipName"
        $url = "https://github.com/actions/runner/releases/download/v$version/$zipName"
        Get-Download $url $zip $runnerSha
        New-Item -ItemType Directory -Force $RunnerDir | Out-Null
        Expand-Archive $zip -DestinationPath $RunnerDir -Force
    }
}
Write-Host "  runner $version in $RunnerDir"

if (-not $SkipScratch) {
    Write-Step "Scratch clone of $ScratchBranch for the trial run"
    if (-not (Test-Path "$ScratchDir\.git")) {
        Invoke-Retry 5 { & git clone --branch $ScratchBranch --single-branch "https://github.com/$Repo.git" $ScratchDir }
    } else {
        Invoke-Retry 5 { & git -C $ScratchDir fetch origin $ScratchBranch }
        & git -C $ScratchDir checkout --force FETCH_HEAD
    }
    & git -C $ScratchDir log -1 --oneline
    # Fetch every crate now (on the owner's account, where downloads are easy to watch and retry) so the trial run
    # only builds. Same CARGO_HOME as the runner.
    Push-Location $ScratchDir
    Invoke-Retry 8 { & cargo fetch --locked }
    Pop-Location
}

Write-Step 'Done'
Stop-Transcript | Out-Null
