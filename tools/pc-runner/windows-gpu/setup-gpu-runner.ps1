# The administrator part of the GPU picture-checks runner setup. Needs "Run as administrator"
# (Windows shows a UAC prompt). Safe to run again: every step checks what is already there.
#
#   -Step Probe  : create the bpe-gpu user and F:\bpe-gpu, then check whether a background job
#                  (session 0, like a service) can see the graphics card.     -> probe\vulkaninfo.txt
#   -Step Lock   : keep bpe-gpu out of everything on D:, E: and F: except F:\bpe-gpu.
#   -Step Smoke  : run bpe-gfx's tests once as bpe-gpu in the background.     -> probe\smoke.log
#   -Step Runner : register the GitHub Actions runner as a Windows service running as bpe-gpu.
#   -Step All    : Probe, Lock, then Runner.
#
# The bpe-gpu password is random, made fresh on every run, and handed straight to Windows (the user account, the
# scheduled task and the service). It is never written to disk or shown. The GitHub registration token is fetched
# from the WSL test machine's `beat` login and handed straight to the runner's config.cmd; it expires in an hour.
param([ValidateSet('Probe', 'Lock', 'Smoke', 'Runner', 'All')][string]$Step = 'All')

. "$PSScriptRoot\common.ps1"
if (-not (Test-Admin)) { throw 'Run this script as administrator.' }
New-Item -ItemType Directory -Force $LogDir | Out-Null
Start-Transcript -Path "$LogDir\setup-gpu-runner-$Step-$(Get-Date -Format yyyyMMdd-HHmmss).log" | Out-Null

$ServiceName = "actions.runner.$($Repo -replace '/', '-').$RunnerName"

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
        # Created with a throwaway password; New-RunnerPassword sets the real one straight after.
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

    Write-Step "Folder $Root (full control for $RunnerUser)"
    foreach ($d in $Root, $LogDir, $ProbeDir, $WorkDir, $TargetDir, $DownloadDir) {
        New-Item -ItemType Directory -Force $d | Out-Null
    }
    & icacls.exe $Root /grant "${RunnerUser}:(OI)(CI)F" /T /C /Q | Out-Null
    & icacls.exe $Root
}

# Runs probe.ps1 once as bpe-gpu from a task set to "run whether user is logged on or not" (session 0), waits for it,
# then removes the task again.
function Invoke-AsRunner([string]$Mode, [int]$TimeoutMinutes) {
    $task = "BPE GPU $Mode check"
    $pw = New-RunnerPassword
    $action = New-ScheduledTaskAction -Execute 'powershell.exe' `
        -Argument "-NoProfile -ExecutionPolicy Bypass -File `"$SetupDir\probe.ps1`" -Mode $Mode"
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
    Write-Step 'Can a background job (session 0) see the graphics card?'
    Invoke-AsRunner -Mode Vulkan -TimeoutMinutes 5
    Get-Content "$ProbeDir\vulkaninfo.txt" | Select-String 'user:|GPU\d|deviceName|deviceType|driverName|driverInfo|exit code|ERROR'
}

function Invoke-Smoke {
    Write-Step "Trial run of bpe-gfx's tests as $RunnerUser"
    if (-not (Test-Path "$ScratchDir\Cargo.toml")) { throw "No scratch clone at $ScratchDir; run install-tools.ps1 first" }
    # The clone was made by the owner's account; hand it to bpe-gpu so git doesn't refuse it as someone else's.
    & icacls.exe $ScratchDir /setowner $RunnerUser /T /C /Q | Out-Null
    Invoke-AsRunner -Mode Smoke -TimeoutMinutes 120
    Get-Content "$ProbeDir\smoke.log" -Tail 15
}

function Install-Runner {
    Write-Step "GitHub Actions runner $RunnerName as a Windows service running as $RunnerUser"
    if (-not (Test-Path "$RunnerDir\config.cmd")) { throw "No runner in $RunnerDir; run install-tools.ps1 first" }

    # The environment every job gets.
    $machinePath = [Environment]::GetEnvironmentVariable('PATH', 'Machine')
    @(
        'VK_LOADER_LAYERS_DISABLE=~implicit~'
        "RUSTUP_HOME=$RustupHome"
        "CARGO_HOME=$CargoHome"
        'CARGO_NET_RETRY=10'
        'RUSTUP_MAX_RETRIES=10'
        "BPE_GPU_TARGET=$TargetDir"
        "PATH=$CargoHome\bin;$MingwDir\bin;$BinDir;$GitDir;C:\Program Files\Git\bin;$machinePath"
    ) | Set-Content "$RunnerDir\.env" -Encoding ascii

    $pw = New-RunnerPassword
    $svc = Get-Service $ServiceName -ErrorAction SilentlyContinue
    $registered = (Test-Path "$RunnerDir\.runner") -and $svc
    if (-not $registered) {
        if ($svc) { & "$RunnerDir\config.cmd" remove --local | Out-Host }   # half-done earlier attempt
        $token = (& wsl.exe -d $WslDistro -u beat -- gh api -X POST "repos/$Repo/actions/runners/registration-token" --jq .token)
        if (-not $token) { throw 'Could not get a registration token from the WSL beat login' }
        Push-Location $RunnerDir
        & .\config.cmd --unattended --url "https://github.com/$Repo" --token $token --name $RunnerName `
            --labels $RunnerLabels --work $WorkDir --replace --runasservice `
            --windowslogonaccount ".\$RunnerUser" --windowslogonpassword $pw | Out-Host
        $code = $LASTEXITCODE
        Pop-Location
        $token = $null
        if ($code -ne 0) { throw "config.cmd failed ($code)" }
    } else {
        Write-Host '  already registered; refreshed its password and .env'
    }
    $pw = $null

    # Below-normal priority for the runner and, through inheritance, every build and test it starts.
    foreach ($exe in 'Runner.Listener.exe', 'Runner.Worker.exe') {
        $k = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\$exe\PerfOptions"
        New-Item -Path $k -Force | Out-Null
        New-ItemProperty -Path $k -Name CpuPriorityClass -Value 5 -PropertyType DWord -Force | Out-Null   # 5 = below normal
    }

    Set-Service $ServiceName -StartupType Automatic
    Restart-Service $ServiceName
    Get-Service $ServiceName | Format-List Name, Status, StartType
    (Get-CimInstance Win32_Service -Filter "Name='$ServiceName'").StartName
}

# D:, E: and F: let every signed-in Windows user read and change files (Authenticated Users: Modify), so bpe-gpu
# could reach the owner's music, video and project folders. This adds one "deny everything to bpe-gpu" rule that
# applies to everything inside each drive (not the drive's top level itself, so tools can still look at the path).
# Nobody else's access changes. F:\bpe-gpu stays open because its own "full control for bpe-gpu" rule is set on the
# folder itself, and Windows checks a folder's own rules before rules it inherits from the drive.
function Lock-Drives {
    Write-Step "Keep $RunnerUser out of the owner's drives"
    foreach ($drive in 'D:\', 'E:\', 'F:\') {
        if (-not (Test-Path $drive)) { continue }
        $already = (& icacls.exe $drive) -match "\\${RunnerUser}:\(OI\)\(CI\)\(IO\)\(N\)"
        if ($already) { Write-Host "  $drive already locked"; continue }
        Write-Host "  locking $drive (Windows copies the rule onto every folder, this can take a while)"
        & icacls.exe $drive /deny "${RunnerUser}:(OI)(CI)(IO)(F)" /C /Q | Out-Null
        Write-Host "  $drive done (icacls exit $LASTEXITCODE)"
    }
    # Confirm.
    foreach ($p in 'C:\Users\aedan', 'F:\!Ai Stuff', 'F:\', $Root) {
        Write-Host "  $p :"; & icacls.exe $p | Select-String $RunnerUser, 'Authenticated|Users:|aedan' | Out-Host
    }
}

switch ($Step) {
    'Lock'   { Lock-Drives }
    'Probe'  { Invoke-Probe }
    'Smoke'  { Invoke-Smoke }
    'Runner' { Install-Runner }
    'All'    { Invoke-Probe; Lock-Drives; Install-Runner }
}
Write-Step 'Done'
Stop-Transcript | Out-Null
