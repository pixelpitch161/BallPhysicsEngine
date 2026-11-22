# Puts the tools the runner needs into F:\bpe-gpu. Safe to run again: each part is skipped when it is in place.
#
#   -Part Copy  (the owner's normal account, no administrator needed). Copies, nothing is run from F:\bpe-gpu:
#       WinLibs MinGW from the owner's winget install -> F:\bpe-gpu\mingw64   (bpe-gpu can only read it)
#       ffmpeg.exe and ffprobe.exe from winget         -> F:\bpe-gpu\bin       (bpe-gpu can only read it)
#
#   -Part Rust  (runs AS bpe-gpu: setup-gpu-runner.ps1 -Step Tools starts it through a one-off background task).
#       Rust 1.97.0 (x86_64-pc-windows-gnu, clippy, rustfmt) -> F:\bpe-gpu\rustup, F:\bpe-gpu\cargo
#       cargo-nextest                                        -> F:\bpe-gpu\cargo\bin
#       scratch clone of -ScratchBranch (default main)       -> F:\bpe-gpu\scratch, with its crates fetched
#     bpe-gpu can change everything in cargo and rustup (CI needs that), so those programs are only ever run as
#     bpe-gpu, never by the owner or as administrator.
param([ValidateSet('Copy', 'Rust')][string]$Part = 'Copy', [string]$ScratchBranch = 'main')

. "$PSScriptRoot\common.ps1"
Start-Transcript -Path "$LogDir\install-tools-$Part-$(Get-Date -Format yyyyMMdd-HHmmss).log" | Out-Null

if ($Part -eq 'Copy') {
    if ((whoami) -like "*\$RunnerUser") { throw '-Part Copy runs as the owner (bpe-gpu cannot read the winget folder)' }

    Write-Step 'MinGW compiler'
    $wingetMingw = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Directory -Filter 'BrechtSanders.WinLibs*' |
        ForEach-Object { Join-Path $_.FullName 'mingw64' } | Where-Object { Test-Path "$_\bin\gcc.exe" } |
        Select-Object -First 1
    if (-not $wingetMingw) { throw 'WinLibs MinGW not found in the winget packages folder' }
    & robocopy.exe $wingetMingw $MingwDir /MIR /NFL /NDL /NJH /NP /R:3 /W:2 | Out-Null
    if ($LASTEXITCODE -ge 8) { throw "Copying MinGW failed (robocopy $LASTEXITCODE)" }
    & "$MingwDir\bin\gcc.exe" --version | Select-Object -First 1

    Write-Step 'ffmpeg and ffprobe'
    # Both from the same winget package. bpe-cli's export tests use ffprobe to read back what ffmpeg wrote.
    $ffmpeg = (Get-Item "$env:LOCALAPPDATA\Microsoft\WinGet\Links\ffmpeg.exe").Target
    if (-not $ffmpeg) { $ffmpeg = (Get-Command ffmpeg.exe).Source }
    foreach ($exe in 'ffmpeg.exe', 'ffprobe.exe') {
        Copy-Item (Join-Path (Split-Path $ffmpeg) $exe) "$BinDir\$exe" -Force
        & "$BinDir\$exe" -hide_banner -version | Select-Object -First 1
    }
} else {
    if ((whoami) -notlike "*\$RunnerUser") { throw "-Part Rust runs as $RunnerUser (setup-gpu-runner.ps1 -Step Tools)" }
    $env:RUSTUP_HOME = $RustupHome
    $env:CARGO_HOME  = $CargoHome
    $env:PATH = "$CargoHome\bin;$MingwDir\bin;$GitDir;$env:PATH"
    Set-Location $Root   # away from any rust-toolchain.toml

    Write-Step "Rust $RustVersion"
    if (-not (Test-Path "$CargoHome\bin\rustup.exe")) {
        $init = "$DownloadDir\rustup-init.exe"
        $base = 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-gnu/rustup-init.exe'
        Get-Download "$base.sha256" "$DownloadDir\rustup-init.exe.sha256"
        $sha = ((Get-Content "$DownloadDir\rustup-init.exe.sha256" -Raw) -split '\s+')[0]
        Get-Download $base $init $sha
        Invoke-Retry 5 { & $init -y --no-modify-path --profile minimal --default-host x86_64-pc-windows-gnu `
            --default-toolchain none }
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

    Write-Step "Scratch clone of $ScratchBranch for the trial run"
    # The repository is public, so bpe-gpu clones it without any login.
    $env:GIT_CONFIG_COUNT = '1'; $env:GIT_CONFIG_KEY_0 = 'safe.directory'; $env:GIT_CONFIG_VALUE_0 = '*'
    if (-not (Test-Path "$ScratchDir\.git")) {
        Invoke-Retry 5 { & git clone --branch $ScratchBranch --single-branch "https://github.com/$Repo.git" $ScratchDir }
    } else {
        Invoke-Retry 5 { & git -C $ScratchDir fetch origin $ScratchBranch }
        & git -C $ScratchDir checkout --force FETCH_HEAD
    }
    & git -C $ScratchDir log -1 --oneline
    Push-Location $ScratchDir
    Invoke-Retry 8 { & cargo fetch --locked }
    Pop-Location
}

Write-Step 'Done'
Stop-Transcript | Out-Null
