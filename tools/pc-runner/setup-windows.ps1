# Windows half of the PC test runner setup (docs/superpowers/plans/2026-11-21-pc-test-runner.md, step 1).
# Run elevated. Safe to run again: each part checks first. Writes progress to C:\ProgramData\bpe-ci\setup.log.
# Continue, not Stop: Windows PowerShell 5.1 turns a native program's error output into a stopping error.
$ErrorActionPreference = 'Continue'
$dir = 'C:\ProgramData\bpe-ci'
New-Item -ItemType Directory -Force $dir | Out-Null
Start-Transcript -Path "$dir\setup.log" -Append | Out-Null

# 1. WSL itself.
& wsl.exe --status 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) {
    Write-Output 'Installing WSL'
    & wsl.exe --install --no-distribution
    Write-Output "wsl --install exit code $LASTEXITCODE"
}

# 2. .wslconfig for the owner (this script runs elevated as the same user).
$cfg = Join-Path $env:USERPROFILE '.wslconfig'
if (Test-Path $cfg) {
    Write-Output ".wslconfig already exists, left as it is: $(Get-Content $cfg -Raw)"
} else {
    # dropCache: Linux's file cache goes back to Windows once the machine is idle, so VmmemWSL shrinks between runs.
    Set-Content -Path $cfg -Encoding ascii -Value @('[wsl2]', 'memory=44GB', 'swap=16GB', 'processors=16',
        'vmIdleTimeout=-1', '', '[experimental]', 'autoMemoryReclaim=dropCache')
}

# 3. The keep-alive task, from a fixed copy so it doesn't depend on the checkout's branch.
Copy-Item -Force (Join-Path $PSScriptRoot 'keep-alive.ps1') "$dir\keep-alive.ps1"
$user = "$env:USERDOMAIN\$env:USERNAME"
$action = New-ScheduledTaskAction -Execute 'powershell.exe' `
    -Argument "-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$dir\keep-alive.ps1`""
$trigger = New-ScheduledTaskTrigger -AtLogOn -User $user
$principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Highest
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
    -ExecutionTimeLimit ([TimeSpan]::Zero) -Hidden
Register-ScheduledTask -TaskName 'BPE checks (WSL)' -Action $action -Trigger $trigger -Principal $principal `
    -Settings $settings -Force | Out-Null
Write-Output 'Scheduled task registered'
Stop-Transcript | Out-Null
