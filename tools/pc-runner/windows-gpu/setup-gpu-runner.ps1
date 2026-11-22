# The administrator part of the GPU picture-checks runner setup. Needs "Run as administrator"
# (Windows shows a UAC prompt). Safe to run again: every step checks what is already there.
#
#   -Step Probe   : create the bpe-gpu user and F:\bpe-gpu (Folders below), then check whether a background job
#                   (session 0, like a service) can see the graphics card.     -> probe\vulkaninfo.txt
#   -Step Lock    : keep bpe-gpu out of everything on D:, E: and F: (they let every signed-in user in by default).
#   -Step Folders : set who may do what in F:\bpe-gpu (see Set-Folders). Part of Probe; safe to run on its own.
#   -Step Tools   : install Rust, cargo-nextest and the scratch clone AS bpe-gpu (install-tools.ps1 -Part Rust).
#                   Run install-tools.ps1 -Part Copy as the owner first.              -> probe\tools.log
#   -Step Smoke   : run bpe-gfx's tests once as bpe-gpu in the background.     -> probe\smoke.log
#   -Step Runner  : register the GitHub Actions runner as a Windows service running as bpe-gpu (normal priority).
#   -Step All     : Probe, Lock, Tools, then Runner.
#
# -ScratchBranch picks the branch the scratch clone is made from (default main).
# -OwnerCheckout is the owner's own checkout, only used to show that bpe-gpu can't reach it.
#
# The bpe-gpu password is random, made fresh on every run, and handed straight to Windows (the user account, the
# scheduled task and the service). It is never written to disk or shown. The GitHub registration token is fetched
# from the WSL test machine's `beat` login and handed straight to the runner's config.cmd; it expires in an hour.
#
# This script runs as administrator, so it never runs a program bpe-gpu could have changed, and it writes only to
# F:\bpe-gpu\admin and to folders it has just checked or made itself.
param(
    [ValidateSet('Probe', 'Lock', 'Folders', 'Tools', 'Smoke', 'Runner', 'All')][string]$Step = 'All',
    [string]$ScratchBranch = 'main',
    [string]$OwnerCheckout = 'F:\!Ai Stuff\BallPhysicsEngine'
)

. "$PSScriptRoot\common.ps1"
if (-not (Test-Admin)) { throw 'Run this script as administrator.' }
New-Item -ItemType Directory -Force $AdminDir | Out-Null
Start-Transcript -Path "$AdminDir\setup-gpu-runner-$Step-$(Get-Date -Format yyyyMMdd-HHmmss).log" | Out-Null

$ServiceName = "actions.runner.$($Repo -replace '/', '-').$RunnerName"
$Admins = '*S-1-5-32-544'   # the Administrators group, by its fixed id (its name differs between languages)
$System = '*S-1-5-18'
$Owner  = "$env:USERDOMAIN\$env:USERNAME"   # the owner's account (this script runs elevated as the owner)

# --- Windows account rights (Log on as a batch job, and so on) through the LSA API, which Windows Home has ----------
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class BpeLsa {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct LSA_UNICODE_STRING { public ushort Length; public ushort MaximumLength; public string Buffer; }
    [StructLayout(LayoutKind.Sequential)]
    struct LSA_OBJECT_ATTRIBUTES { public int Length; public IntPtr RootDirectory; public IntPtr ObjectName;
        public uint Attributes; public IntPtr SecurityDescriptor; public IntPtr SecurityQualityOfService; }
    [DllImport("advapi32.dll")] static extern uint LsaOpenPolicy(IntPtr system, ref LSA_OBJECT_ATTRIBUTES attr,
        uint access, out IntPtr handle);
    [DllImport("advapi32.dll")] static extern uint LsaAddAccountRights(IntPtr handle, byte[] sid,
        LSA_UNICODE_STRING[] rights, uint count);
    [DllImport("advapi32.dll")] static extern uint LsaClose(IntPtr handle);
    [DllImport("advapi32.dll")] static extern int LsaNtStatusToWinError(uint status);
    public static void Grant(byte[] sid, string right) {
        var attr = new LSA_OBJECT_ATTRIBUTES();
        IntPtr h;
        uint s = LsaOpenPolicy(IntPtr.Zero, ref attr, 0x00000810, out h); // POLICY_CREATE_ACCOUNT | LOOKUP_NAMES
        if (s != 0) throw new System.ComponentModel.Win32Exception(LsaNtStatusToWinError(s));
        try {
            var r = new LSA_UNICODE_STRING[1];
            r[0].Buffer = right; r[0].Length = (ushort)(right.Length * 2); r[0].MaximumLength = (ushort)(right.Length * 2 + 2);
            s = LsaAddAccountRights(h, sid, r, 1);
            if (s != 0) throw new System.ComponentModel.Win32Exception(LsaNtStatusToWinError(s));
        } finally { LsaClose(h); }
    }
}
'@

function Grant-Right([string]$Right) {
    $sid = (Get-LocalUser $RunnerUser).SID
    $bytes = New-Object byte[] $sid.BinaryLength
    $sid.GetBinaryForm($bytes, 0)
    [BpeLsa]::Grant($bytes, $Right)
    Write-Host "  $RunnerUser has $Right"
}

# icacls, stopping loudly if it fails.
function Invoke-Icacls {
    & icacls.exe @args | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "icacls $($args -join ' ') failed (exit $LASTEXITCODE)" }
}

# A fresh random password, set on the account and on the runner service if it exists. Kept only in memory.
function New-RunnerPassword {
    $chars = [char[]]'ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789!#%+-=?@_'
    $bytes = New-Object byte[] 40
    [Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
    $plain = -join ($bytes | ForEach-Object { $chars[$_ % $chars.Length] })
    Set-LocalUser -Name $RunnerUser -Password (ConvertTo-SecureString $plain -AsPlainText -Force)
    if (Get-Service $ServiceName -ErrorAction SilentlyContinue) {
        & sc.exe config $ServiceName obj= ".\$RunnerUser" password= $plain | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "Could not give the runner service the new password (sc.exe $LASTEXITCODE)" }
    }
    return $plain
}

function Initialize-Account {
    Write-Step "Windows user $RunnerUser"
    if (-not (Get-LocalUser $RunnerUser -ErrorAction SilentlyContinue)) {
        # Created with a throwaway password; New-RunnerPassword sets the real one when it's first needed.
        $tmp = ConvertTo-SecureString ([guid]::NewGuid().ToString() + 'aA1!') -AsPlainText -Force
        New-LocalUser -Name $RunnerUser -Password $tmp -PasswordNeverExpires -UserMayNotChangePassword `
            -AccountNeverExpires -FullName 'BPE picture checks' `
            -Description 'Ball Studio picture checks (GitHub runner)' | Out-Null
        Write-Host "  created $RunnerUser"
    } else { Write-Host "  $RunnerUser already exists" }
    $users = Get-LocalGroupMember -Group Users | Where-Object { $_.Name -like "*\$RunnerUser" }
    if (-not $users) { Add-LocalGroupMember -Group Users -Member $RunnerUser }
    # Never an administrator.
    if (Get-LocalGroupMember -Group Administrators | Where-Object { $_.Name -like "*\$RunnerUser" }) {
        throw "$RunnerUser is an administrator; it must not be"
    }
    # Background logons only: no sign-in at the screen or over Remote Desktop, and hidden from the sign-in screen.
    Grant-Right 'SeBatchLogonRight'
    Grant-Right 'SeServiceLogonRight'
    Grant-Right 'SeDenyInteractiveLogonRight'
    Grant-Right 'SeDenyRemoteInteractiveLogonRight'
    $hide = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon\SpecialAccounts\UserList'
    New-Item -Path $hide -Force | Out-Null
    New-ItemProperty -Path $hide -Name $RunnerUser -Value 0 -PropertyType DWord -Force | Out-Null
}

# Who may do what in F:\bpe-gpu. F:\bpe-gpu is owned by Administrators and takes nothing from the drive (F: lets
# every signed-in user change files). Everything in it gives full control to Administrators, SYSTEM and the owner.
# bpe-gpu gets:
#   F:\bpe-gpu itself         : look inside only (it can't delete, rename or add folders there)
#   setup, mingw64, bin       : read and run only, so it can't change what the owner or an administrator runs
#   admin                     : nothing (administrator logs and the checked runner download)
#   logs, probe, rustup, cargo, runner, work, target, scratch, downloads : full control (CI writes these)
# The programs in rustup and cargo are therefore only ever run as bpe-gpu (Tools, Smoke, the runner).
function Set-Folders {
    Write-Step "Folder $Root"
    foreach ($d in @($Root, $AdminDir) + $RunnerReadOnly + $RunnerWritable) { New-Item -ItemType Directory -Force $d | Out-Null }
    Invoke-Icacls $Root /setowner $Admins /C /Q
    # /grant:r only replaces a rule with the same inheritance, so the old full-control rule is removed first.
    Invoke-Icacls $Root /remove:g $RunnerUser /C /Q
    Invoke-Icacls $Root /inheritance:r /grant:r "${Admins}:(OI)(CI)F" "${System}:(OI)(CI)F" "${Owner}:(OI)(CI)F" `
        "${RunnerUser}:(RX)" /C /Q
    # Clear every rule naming bpe-gpu below F:\bpe-gpu (the first setup gave it full control of everything), then
    # give back exactly what's listed above. /L works on links themselves, never on what they point to.
    Invoke-Icacls "$Root\*" /remove:g $RunnerUser /T /L /C /Q
    foreach ($d in $RunnerReadOnly) { Invoke-Icacls $d /grant:r "${RunnerUser}:(OI)(CI)RX" /C /Q }
    foreach ($d in $RunnerWritable) { Invoke-Icacls $d /grant:r "${RunnerUser}:(OI)(CI)F" /C /Q }
    foreach ($d in @($SetupDir, $AdminDir, $MingwDir, $BinDir)) { Invoke-Icacls $d /setowner $Admins /T /L /C /Q }
    foreach ($d in @($Root, $SetupDir, $AdminDir, $MingwDir, $CargoHome)) {
        & icacls.exe $d | Select-String $RunnerUser, 'Administrators', 'SYSTEM', $env:USERNAME, 'Authenticated' |
            ForEach-Object { "  $($_.Line.Trim())" }
    }
}

# Runs probe.ps1 once as bpe-gpu from a task set to "run whether user is logged on or not" (session 0), waits for it,
# then removes the task again.
function Invoke-AsRunner([string]$Mode, [int]$TimeoutMinutes) {
    $task = "BPE GPU $Mode check"
    $pw = New-RunnerPassword
    $action = New-ScheduledTaskAction -Execute 'powershell.exe' `
        -Argument "-NoProfile -ExecutionPolicy Bypass -File `"$SetupDir\probe.ps1`" -Mode $Mode -ScratchBranch `"$ScratchBranch`""
    $settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes $TimeoutMinutes) `
        -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -Priority 7
    Unregister-ScheduledTask -TaskName $task -Confirm:$false -ErrorAction SilentlyContinue
    Register-ScheduledTask -TaskName $task -Action $action -Settings $settings -User $RunnerUser -Password $pw `
        -RunLevel Limited | Out-Null
    $pw = $null
    Start-ScheduledTask -TaskName $task
    Write-Host "  started '$task' as $RunnerUser, waiting (up to $TimeoutMinutes minutes)"
    $deadline = (Get-Date).AddMinutes($TimeoutMinutes + 2)
    do { Start-Sleep -Seconds 10 } while ((Get-ScheduledTask -TaskName $task).State -eq 'Running' -and
        (Get-Date) -lt $deadline)
    $info = Get-ScheduledTaskInfo -TaskName $task
    Write-Host "  finished, result $($info.LastTaskResult)"
    Unregister-ScheduledTask -TaskName $task -Confirm:$false
}

function Invoke-Probe {
    Initialize-Account
    Set-Folders
    Write-Step 'Can a background job (session 0) see the graphics card?'
    Invoke-AsRunner -Mode Vulkan -TimeoutMinutes 5
    Get-Content "$ProbeDir\vulkaninfo.txt" | Select-String 'user:|GPU\d|deviceName|deviceType|driverName|driverInfo|exit code|ERROR'
}

function Invoke-Tools {
    Write-Step "Rust, cargo-nextest and the scratch clone, installed as $RunnerUser"
    if (-not (Test-Path "$MingwDir\bin\gcc.exe")) { throw "No MinGW in $MingwDir; run install-tools.ps1 -Part Copy first" }
    Invoke-AsRunner -Mode Tools -TimeoutMinutes 90
    Get-Content "$ProbeDir\tools.log" -Tail 12
}

function Invoke-Smoke {
    Write-Step "Trial run of bpe-gfx's tests as $RunnerUser"
    if (-not (Test-Path "$ScratchDir\Cargo.toml")) { throw "No scratch clone at $ScratchDir; run -Step Tools first" }
    Invoke-AsRunner -Mode Smoke -TimeoutMinutes 120
    Get-Content "$ProbeDir\smoke.log" -Tail 15
}

# The environment every job gets. The runner folder is bpe-gpu's, so refuse to write through a link it planted.
function Write-RunnerEnv {
    $envFile = "$RunnerDir\.env"
    if (Test-Path $envFile) {
        if ((Get-Item $envFile -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "$envFile is a link; not writing it" }
        Remove-Item $envFile -Force
    }
    $machinePath = [Environment]::GetEnvironmentVariable('PATH', 'Machine')
    @(
        'VK_LOADER_LAYERS_DISABLE=~implicit~'
        "RUSTUP_HOME=$RustupHome"
        "CARGO_HOME=$CargoHome"
        'CARGO_NET_RETRY=10'
        'RUSTUP_MAX_RETRIES=10'
        "BPE_GPU_TARGET=$TargetDir"
        "PATH=$CargoHome\bin;$MingwDir\bin;$BinDir;$GitDir;C:\Program Files\Git\bin;$machinePath"
    ) | Set-Content $envFile -Encoding ascii
}

function Install-Runner {
    Write-Step "GitHub Actions runner $RunnerName as a Windows service running as $RunnerUser"
    $svc = Get-Service $ServiceName -ErrorAction SilentlyContinue
    $registered = (Test-Path "$RunnerDir\.runner") -and $svc
    if (-not $registered) {
        # A fresh copy of the latest runner, checked against the SHA-256 in its release notes, unpacked into a new
        # folder bpe-gpu can't touch until config.cmd (run here as administrator) has finished with it.
        if ($svc) { Stop-Service $ServiceName -Force; & sc.exe delete $ServiceName | Out-Null }
        $release = Invoke-RestMethod 'https://api.github.com/repos/actions/runner/releases/latest' -Headers @{ 'User-Agent' = 'bpe-setup' }
        $version = $release.tag_name.TrimStart('v')
        if ($release.body -notmatch '<!-- BEGIN SHA win-x64 -->([0-9a-fA-F]{64})<!-- END SHA win-x64 -->') {
            throw 'The runner release notes have no SHA-256 for win-x64'
        }
        $zip = "$AdminDir\actions-runner-win-x64-$version.zip"
        Get-Download "https://github.com/actions/runner/releases/download/v$version/actions-runner-win-x64-$version.zip" $zip $Matches[1]
        if (Test-Path $RunnerDir) { & cmd.exe /c rmdir /s /q "$RunnerDir" }   # rmdir never follows links
        New-Item -ItemType Directory $RunnerDir | Out-Null   # inherits only Administrators, SYSTEM and the owner
        Expand-Archive $zip -DestinationPath $RunnerDir
        Write-RunnerEnv

        $pw = New-RunnerPassword
        $token = (& wsl.exe -d $WslDistro -u beat -- gh api -X POST "repos/$Repo/actions/runners/registration-token" --jq .token)
        if (-not $token) { throw 'Could not get a registration token from the WSL beat login' }
        Push-Location $RunnerDir
        & .\config.cmd --unattended --url "https://github.com/$Repo" --token $token --name $RunnerName `
            --labels $RunnerLabels --work $WorkDir --replace --runasservice `
            --windowslogonaccount ".\$RunnerUser" --windowslogonpassword $pw | Out-Host
        $code = $LASTEXITCODE
        Pop-Location
        $token = $null; $pw = $null
        if ($code -ne 0) { throw "config.cmd failed ($code)" }
        # Now the runner is bpe-gpu's: it writes its logs here and updates itself.
        Invoke-Icacls $RunnerDir /grant:r "${RunnerUser}:(OI)(CI)F" /C /Q
    } else {
        Write-RunnerEnv
        $null = New-RunnerPassword
        Write-Host '  already registered; refreshed its password and .env'
    }

    # Normal priority, on purpose. Below-normal was tried first (2026-11-22), but builds at normal priority on the same
    # CPU (the WSL test machine's, or anyone's) starved the runner: a job's worker started so slowly that it missed the
    # runner's 30-second handoff, and GitHub reported "lost communication". So any priority setting from that first
    # setup is removed here.
    foreach ($exe in 'Runner.Listener.exe', 'Runner.Worker.exe') {
        $k = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\$exe"
        if (Test-Path $k) { Remove-Item -Path $k -Recurse -Force; Write-Host "  removed the priority setting for $exe" }
    }

    Set-Service $ServiceName -StartupType Automatic
    Restart-Service $ServiceName
    Get-Service $ServiceName | Format-List Name, Status, StartType
    (Get-CimInstance Win32_Service -Filter "Name='$ServiceName'").StartName
}

# D:, E: and F: let every signed-in Windows user read and change files (Authenticated Users: Modify), so bpe-gpu
# could reach the owner's music, video and project folders. This adds one "deny everything to bpe-gpu" rule that
# applies to everything inside each drive (not the drive's top level itself, so tools can still look at the path).
# Nobody else's access changes. F:\bpe-gpu has its own rules and takes nothing from the drive (Set-Folders).
function Lock-Drives {
    Write-Step "Keep $RunnerUser out of the owner's drives"
    $rule = "\\${RunnerUser}:\(OI\)\(CI\)\(IO\)\(N\)"
    foreach ($drive in 'D:\', 'E:\', 'F:\') {
        if (-not (Test-Path $drive)) { continue }
        if ((& icacls.exe $drive) -match $rule) { Write-Host "  $drive already locked"; continue }
        Write-Host "  locking $drive (Windows copies the rule onto every folder, this can take a while)"
        & icacls.exe $drive /deny "${RunnerUser}:(OI)(CI)(IO)(F)" /C /Q | Out-Null
        $code = $LASTEXITCODE
        if (-not ((& icacls.exe $drive) -match $rule)) { throw "Locking $drive didn't take (icacls exit $code)" }
        if ($code -ne 0) { Write-Warning "  $drive locked, but icacls reported $code (some protected system folders can't be changed)" }
        Write-Host "  $drive done"
    }
    # Confirm.
    foreach ($p in @($env:USERPROFILE, $OwnerCheckout, 'F:\', $Root)) {
        if (-not (Test-Path $p)) { continue }
        Write-Host "  $p :"; & icacls.exe $p | Select-String $RunnerUser, 'Authenticated|Users:', $env:USERNAME | Out-Host
    }
}

switch ($Step) {
    'Probe'   { Invoke-Probe }
    'Lock'    { Lock-Drives }
    'Folders' { Set-Folders }
    'Tools'   { Invoke-Tools }
    'Smoke'   { Invoke-Smoke }
    'Runner'  { Install-Runner }
    'All'     { Invoke-Probe; Lock-Drives; Invoke-Tools; Install-Runner }
}
Write-Step 'Done'
Stop-Transcript | Out-Null
