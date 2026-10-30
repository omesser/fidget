# Mask rebuild rate and cost on Windows for every #428 scenario, back to back:
# idle, walk, fast, large (BMO at 4x), small (Nim at 4x) and small at 1x.
# Each one is scripts\bench-mask-rebuild-windows-flame.ps1 -Scenario ... -Check,
# so no WPR and no admin. Results go under Out, one folder a scenario, and the
# table at the end is also in Out\table.txt.
# Usage: scripts\bench-mask-rebuild-windows-scenarios.ps1 -Bin PATH\fidget.exe [-Out DIR]
#        scripts\bench-mask-rebuild-windows-scenarios.ps1 -Report DIR
#   FIDGET_BENCH_GREEN_LIGHT=1: the operator agreed to fidget on the desktop
#   and to the pointer moving. It holds the pointer for about 2-3 minutes,
#   4 at most.
#   -Report prints the table again from a finished Out without running.
#
# Build release first, e.g.:
#   $env:CARGO_TARGET_DIR='E:\workspace\target-428'; cargo build --release -p fidget

param(
    [string]$Bin = "",
    [string]$Out = "",
    [string]$Report = ""
)

$ErrorActionPreference = "Stop"

# Scenario, scale (0: the scenario's), rebuild target (0: run out Seconds),
# and the cap in seconds. Standing scenarios update at the animation's fps,
# about 10-12/s, so 200 rebuilds take about 20 s; walk updates once a frame.
$runs = @(
    @{ Name = "idle"; Scenario = "idle"; Scale = 0; Target = 0; Seconds = 10 },
    @{ Name = "walk"; Scenario = "walk"; Scale = 0; Target = 500; Seconds = 20 },
    @{ Name = "fast"; Scenario = "fast"; Scale = 0; Target = 200; Seconds = 30 },
    @{ Name = "large"; Scenario = "large"; Scale = 0; Target = 200; Seconds = 30 },
    @{ Name = "small"; Scenario = "small"; Scale = 0; Target = 200; Seconds = 30 },
    @{ Name = "small-1x"; Scenario = "small"; Scale = 1; Target = 200; Seconds = 30 }
)

function Show-Table([string]$Dir) {
    $rows = foreach ($run in $runs) {
        $json = Join-Path (Join-Path $Dir $run.Name) "result.json"
        # Every row has every column: Format-Table takes them from the first.
        $r = if (Test-Path -LiteralPath $json) { Get-Content -Raw -LiteralPath $json | ConvertFrom-Json } else {
            [pscustomobject]@{ character = "(no result.json)"; timing = $null; rebuilds = $null; onSeconds = $null
                rate = $null; msMedian = $null; msMin = $null; msMax = $null; rectsMedian = $null }
        }
        [pscustomobject]@{
            Scenario = $run.Name; Character = $r.character; Timing = $r.timing
            Rebuilds = $r.rebuilds; Seconds = $r.onSeconds; "Rebuilds/s" = $r.rate
            "ms median" = $r.msMedian; "ms min-max" = if ($null -ne $r.msMin) { "$($r.msMin)-$($r.msMax)" } else { $null }
            Rects = $r.rectsMedian
            "Mask ms/s" = if ($null -ne $r.msMedian) { [math]::Round($r.rate * $r.msMedian, 2) } elseif ($null -ne $r.rate) { 0 } else { $null }
        }
    }
    $text = $rows | Format-Table -Property * -AutoSize | Out-String -Width 200
    $text | Set-Content -LiteralPath (Join-Path $Dir "table.txt") -Encoding UTF8
    Write-Host $text
}

if ($Report) {
    Show-Table (Resolve-Path -LiteralPath $Report).Path
    exit 0
}

if (-not $Bin) {
    [Console]::Error.WriteLine("-Bin PATH\fidget.exe is required (or -Report DIR)")
    exit 2
}
if ($env:FIDGET_BENCH_GREEN_LIGHT -ne '1') {
    [Console]::Error.WriteLine("This launches fidget on the live desktop and moves the pointer for about 2-3 minutes. Set FIDGET_BENCH_GREEN_LIGHT=1 once the operator has agreed.")
    exit 2
}
if (-not (Test-Path -LiteralPath $Bin)) {
    [Console]::Error.WriteLine("no $Bin - build: `$env:CARGO_TARGET_DIR='E:\workspace\target-428'; cargo build --release -p fidget")
    exit 2
}
if (-not $Out) {
    $Out = Join-Path "E:\workspace\fidget-428-runs" (Get-Date -Format "yyyyMMdd-HHmmss")
}
$null = New-Item -ItemType Directory -Force -Path $Out
$Out = (Resolve-Path -LiteralPath $Out).Path

# Each scenario in its own PowerShell, as the flame script exits when done.
$shell = (Get-Process -Id $PID).Path
$flame = Join-Path $PSScriptRoot "bench-mask-rebuild-windows-flame.ps1"
$clock = [Diagnostics.Stopwatch]::StartNew()
$failed = @()
foreach ($run in $runs) {
    Write-Host "=== $($run.Name) ($([int]$clock.Elapsed.TotalSeconds)s in) ==="
    $flameArgs = @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $flame, "-Bin", $Bin,
        "-Out", (Join-Path $Out $run.Name), "-Scenario", $run.Scenario, "-Check",
        "-Target", $run.Target, "-Seconds", $run.Seconds)
    if ($run.Scale -gt 0) { $flameArgs += @("-Scale", $run.Scale) }
    & $shell @flameArgs
    if ($LASTEXITCODE -ne 0) { $failed += $run.Name }
}
Write-Host "all scenarios in $([int]$clock.Elapsed.TotalSeconds)s; output: $Out"
Show-Table $Out
if ($failed.Count -gt 0) {
    [Console]::Error.WriteLine("failed: $($failed -join ', ')")
    exit 1
}
