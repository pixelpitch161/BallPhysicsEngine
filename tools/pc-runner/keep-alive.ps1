# Keeps the bpe-ci WSL machine (the checks' runners) running while the owner is logged in, at low priority so music
# and games come first. Started at logon by the "BPE checks (WSL)" scheduled task; see docs/notes/checks.md.
$ErrorActionPreference = 'Continue'
Start-Process -FilePath wsl.exe -ArgumentList '-d', 'bpe-ci', '--exec', '/bin/sleep', 'infinity' -WindowStyle Hidden
Start-Sleep -Seconds 20
Get-Process -Name 'vmmem*' -ErrorAction SilentlyContinue | ForEach-Object {
    try { $_.PriorityClass = 'BelowNormal' } catch { }
}
