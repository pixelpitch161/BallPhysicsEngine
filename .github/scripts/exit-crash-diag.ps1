# TEMPORARY diagnosis of the 0xc0000409 aborts after "test result: ok" on the owner's card. Removed before merging.
$ErrorActionPreference = 'Continue'

"=== Vulkan implicit layers (registry) ==="
foreach ($k in 'HKLM:\SOFTWARE\Khronos\Vulkan\ImplicitLayers', 'HKLM:\SOFTWARE\WOW6432Node\Khronos\Vulkan\ImplicitLayers',
  'HKCU:\SOFTWARE\Khronos\Vulkan\ImplicitLayers') {
  if (Test-Path $k) { $k; (Get-Item $k).Property | ForEach-Object { "  $_" } }
}

function Show-Crashes([datetime]$since) {
  $ev = Get-WinEvent -FilterHashtable @{ LogName = 'Application'; Id = 1000, 1001, 1026; StartTime = $since } -ErrorAction SilentlyContinue
  $ev | Where-Object { $_.Message -match 'bpe|ball|c0000409' } | Select-Object -First 40 | ForEach-Object {
    "--- $($_.TimeCreated) id $($_.Id)"
    ($_.Message -split "`n" | Select-Object -First 30) -join "`n"
  }
}

"=== Crash events in the last 3 days ==="
Show-Crashes (Get-Date).AddDays(-3)

"=== WER report folders ==="
foreach ($d in "$env:ProgramData\Microsoft\Windows\WER\ReportArchive", "$env:LOCALAPPDATA\Microsoft\Windows\WER\ReportArchive",
  "$env:LOCALAPPDATA\CrashDumps") {
  if (Test-Path $d) {
    Get-ChildItem $d -ErrorAction SilentlyContinue | Where-Object { $_.Name -match 'bpe' } | Select-Object -First 10 |
      ForEach-Object {
        "--- $($_.FullName)"
        $wer = Join-Path $_.FullName 'Report.wer'
        if (Test-Path $wer) { Get-Content $wer -Encoding Unicode | Select-String 'Sig\[|LoadedModule|AppPath|NsAppName' | Select-Object -First 60 }
      }
  }
}

"=== Stress ==="
$json = cargo test -p bpe-gfx --lib --no-run --message-format=json 2>$null
$exe = ($json | ForEach-Object { try { $_ | ConvertFrom-Json } catch {} } |
  Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'bpe_gfx' -and $_.profile.test -and $_.executable } |
  Select-Object -First 1).executable
"test exe: $exe"
$tests = @(
  'effects::chain::tests::a_background_store_leaves_an_effect_out_until_it_is_built_then_shares_it',
  'sub_layers_tests::a_glow_layer_takes_colour_sub_layers_and_a_half_opaque_layer_s_outline_is_as_opaque_as_it',
  'liquid_sim::tests::the_card_times_a_step_when_it_can'
)
$runs = [int]$env:STRESS_RUNS
$start = Get-Date

function Stress([string]$label) {
  $bad = 0; $total = 0; $live = @(); $n = 0
  for ($i = 0; $i -lt $runs; $i++) {
    foreach ($t in $tests) {
      while (($live | Where-Object { -not $_.HasExited }).Count -ge 8) { Start-Sleep -Milliseconds 50 }
      $n++
      $p = Start-Process -FilePath $exe -ArgumentList "$t --exact --test-threads=1" -NoNewWindow -PassThru `
        -RedirectStandardOutput "$env:TEMP\diag-out-$n.txt" -RedirectStandardError "$env:TEMP\diag-err-$n.txt"
      $null = $p.Handle
      $live += $p
    }
  }
  $live | ForEach-Object { $_.WaitForExit() }
  foreach ($p in $live) {
    $total++
    if ($p.ExitCode -ne 0) { $bad++; "  exit 0x{0:x8}" -f [uint32]([int64]$p.ExitCode -band 0xffffffffL) }
  }
  "$label : $bad of $total runs failed"
}

$env:VK_LOADER_LAYERS_DISABLE = $null
Stress 'with the implicit layers'
$env:VK_LOADER_LAYERS_DISABLE = '~implicit~'
Stress 'without implicit layers'
$env:VK_LOADER_LAYERS_DISABLE = $null

"=== Crash events during the stress ==="
Show-Crashes $start
