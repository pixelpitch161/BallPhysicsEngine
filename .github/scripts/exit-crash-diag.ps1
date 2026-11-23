# TEMPORARY diagnosis of the 0xc0000409 aborts after "test result: ok" on the owner's card. Removed before merging.
$ErrorActionPreference = 'Continue'

function Show-Crashes([datetime]$since) {
  $ev = Get-WinEvent -FilterHashtable @{ LogName = 'Application'; Id = 1000; StartTime = $since } -ErrorAction SilentlyContinue
  $ev | Where-Object { $_.Message -match 'bpe' } | Select-Object -First 40 | ForEach-Object {
    "--- $($_.TimeCreated)"
    ($_.Message -split "`n" | Select-Object -First 5) -join "`n"
  }
}

$json = cargo test -p bpe-gfx --lib --no-run --message-format=json 2>$null
$exe = ($json | ForEach-Object { try { $_ | ConvertFrom-Json } catch {} } |
  Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'bpe_gfx' -and $_.profile.test -and $_.executable } |
  Select-Object -First 1).executable
"test exe: $exe"

"=== The OpenGL check, as built (expect ok) ==="
& $exe test_device_tests::a_test_device_does_not_load_the_opengl_driver --exact --nocapture
"exit $LASTEXITCODE"
"=== The OpenGL check with WGPU_BACKEND=vulkan,gl (expect a failure naming atio6axx) ==="
$env:WGPU_BACKEND = 'vulkan,gl'
& $exe test_device_tests::a_test_device_does_not_load_the_opengl_driver --exact --nocapture
"exit $LASTEXITCODE"
$env:WGPU_BACKEND = $null

"=== Stress, with the fix: every graphics test of the crate, $env:STRESS_RUNS times ==="
$start = Get-Date
$bad = 0
for ($i = 0; $i -lt [int]$env:STRESS_RUNS; $i++) {
  cargo nextest run -p bpe-gfx --lib --profile gpu --no-tests=pass 2>&1 | Select-String -Pattern 'ABORT|Summary|FAIL \['
}
"=== Crash events during the stress (expect none) ==="
Show-Crashes $start
