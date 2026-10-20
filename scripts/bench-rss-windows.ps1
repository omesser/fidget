# Sample a running fidget's resident set, Windows only. WebView2 is multi-process,
# so every msedgewebview2.exe that appears after launch counts as the app's.
# Usage: scripts\bench-rss-windows.ps1 [-Bin PATH] [-Settle N] [-Seconds N] [-Interval N] [-Out FILE] [-Research]

# WorkingSet alone does not compare two runs on a busy machine; PeakWorkingSet
# only ever rises. Compare scenarios on it and read the WorkingSet series for
# shape, and record the roster, display count and what the sprite was doing.

param(
    [string]$Bin = "target\debug\fidget.exe",
    [int]$Settle = 3,
    [int]$Seconds = 10,
    [int]$Interval = 2,
    [string]$Out = "",
    [switch]$Research
)

if ($Research) {
    $Settle = 300
    $Seconds = 300
    $Interval = 5
}

$ErrorActionPreference = "Stop"
Set-Location (Split-Path -Parent $PSScriptRoot)

if (-not (Test-Path -LiteralPath $Bin)) {
    [Console]::Error.WriteLine("no $Bin - run: cd src-tauri; cargo build --bin fidget")
    exit 2
}
$Bin = (Resolve-Path -LiteralPath $Bin).Path

if ($Out -eq "") {
    $Out = [System.IO.Path]::GetTempFileName() -replace '\.tmp$', '.tsv'
}
$log = "$Out.app.log"
$errLog = "$Out.app.err.log"

# Release keeps stderr in the data folder's process.log; only debug copies it
# back to stderr (process_log::init), so both are read. The log outlives runs,
# so only lines stamped at or after launch count.
$dataDir = if ($env:FIDGET_DATA_DIR) { $env:FIDGET_DATA_DIR } else { Join-Path $env:APPDATA "fidget" }
New-Item -ItemType Directory -Force -Path $dataDir | Out-Null
$processLog = Join-Path $dataDir "process.log"
$launchedAt = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()

$beforeEdge = Get-Process -Name "msedgewebview2" -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id

$start = @{ FilePath = $Bin; PassThru = $true; RedirectStandardOutput = $log; RedirectStandardError = $errLog }
# Only Windows has window styles; elsewhere is the tests' fake app.
if ([Environment]::OSVersion.Platform -eq "Win32NT") { $start.WindowStyle = "Hidden" }
$process = Start-Process @start
$app = $process.Id

function Stop-App {
    try {
        Stop-Process -Id $app -Force -ErrorAction SilentlyContinue
    } catch {}
}
Register-EngineEvent -SourceIdentifier PowerShell.Exiting -Action { Stop-App } | Out-Null
trap { Stop-App; break }

# Tails one log for its first overlay line, waiting for the file to appear.
# $Since above zero also requires the line's timestamp to be at least that.
$tail = {
    param([string]$Path, [long]$Since, [int]$Seconds)
    $watcher = New-Object System.IO.FileSystemWatcher (Split-Path -Parent $Path), (Split-Path -Leaf $Path)
    try {
        Register-ObjectEvent $watcher Created -SourceIdentifier created | Out-Null
        $watcher.EnableRaisingEvents = $true
        # Checked after the watch starts, so a file made in between is not missed.
        if (-not (Test-Path -LiteralPath $Path) -and -not (Wait-Event -SourceIdentifier created -Timeout $Seconds)) { return }
    } finally {
        $watcher.Dispose()
    }
    Get-Content -LiteralPath $Path -Wait |
        Where-Object { $_ -match 'overlay: \d+ display' -and ($Since -eq 0 -or ($_ -match '^(\d+) ' -and [long]$Matches[1] -ge $Since)) } |
        Select-Object -First 1
}

# The first overlay line from either log wins.
$waitSeconds = 30
$waits = @(
    Start-Job -ScriptBlock $tail -ArgumentList $processLog, $launchedAt, $waitSeconds
    Start-Job -ScriptBlock $tail -ArgumentList $errLog, 0, $waitSeconds
)
$exited = Start-Job -ScriptBlock { param([int]$Id) Wait-Process -Id $Id -ErrorAction SilentlyContinue } -ArgumentList $app
$deadline = (Get-Date).AddSeconds($waitSeconds)
$line = $null
$why = "never reported its overlays"
$pending = @($waits) + $exited
while (-not $line -and $pending.Count -gt 0) {
    $left = [int][Math]::Ceiling(($deadline - (Get-Date)).TotalSeconds)
    if ($left -le 0) { break }
    $done = Wait-Job -Job $pending -Any -Timeout $left
    if (-not $done) { break }
    if ($done -eq $exited) { $why = "exited before reporting its overlays"; break }
    $line = Receive-Job -Job $done | Select-Object -First 1
    $pending = @($pending | Where-Object { $_ -ne $done })
}
Remove-Job -Job (@($waits) + $exited) -Force

if (-not $line) {
    Stop-App
    [Console]::Error.WriteLine("the app $why; see $processLog, $log and $errLog")
    exit 1
}
$displays = [int]([regex]::Match($line, 'overlay: (\d+) display').Groups[1].Value)

Start-Sleep -Seconds 2  # Give helpers time to spawn
$afterEdge = Get-Process -Name "msedgewebview2" -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id
$edgeHelpers = $afterEdge | Where-Object { $_ -notin $beforeEdge }

$pids = @($app) + $edgeHelpers
$pidCount = $pids.Count

Write-Host "displays: $displays   main pid: $app   total processes: $pidCount"
Write-Host "pids: $($pids -join ' ')"
Write-Host "settling ${Settle}s, then sampling ${Seconds}s every ${Interval}s -> $Out"

foreach ($procId in $pids) {
    try {
        $proc = Get-Process -Id $procId -ErrorAction SilentlyContinue
        Write-Host "  $procId $($proc.ProcessName) $($proc.WorkingSet64 / 1MB) MB"
    } catch {}
}

Start-Sleep -Seconds $Settle

$header = "epoch`ttotal_kb`t$($pids -join "`t")"
$header | Out-File -FilePath $Out -Encoding UTF8

$endTime = (Get-Date).AddSeconds($Seconds)
while ((Get-Date) -lt $endTime) {
    $rss = @()
    foreach ($procId in $pids) {
        try {
            $proc = Get-Process -Id $procId -ErrorAction SilentlyContinue
            $rss += [math]::Round($proc.WorkingSet64 / 1KB)
        } catch {
            $rss += 0
        }
    }
    $total = ($rss | Measure-Object -Sum).Sum
    $epoch = [int][double]::Parse((Get-Date -UFormat %s))
    "$epoch`t$total`t$($rss -join "`t")" | Out-File -FilePath $Out -Append -Encoding UTF8
    Start-Sleep -Seconds $Interval
}

$data = Import-Csv -Path $Out -Delimiter "`t" | Select-Object -Skip 0
$totals = $data | ForEach-Object { [int]$_.total_kb }
$sorted = $totals | Sort-Object
$min = [math]::Round($sorted[0] / 1024)
$median = [math]::Round($sorted[[math]::Floor($sorted.Count / 2)] / 1024)
$max = [math]::Round($sorted[-1] / 1024)

Write-Host "`ntotal   samples: $($sorted.Count)   min: $min MB   median: $median MB   max: $max MB"

foreach ($procId in $pids) {
    try {
        $proc = Get-Process -Id $procId -ErrorAction SilentlyContinue
        $procName = $proc.ProcessName
        $peakWS = [math]::Round($proc.PeakWorkingSet64 / 1MB)

        # The TSV column name is the pid.
        $pidRss = $data | ForEach-Object {
            if ($_.PSObject.Properties.Name -contains $procId.ToString()) {
                [int]$_.$($procId.ToString())
            } else { 0 }
        }
        $pidRssSorted = $pidRss | Sort-Object
        $pidMedian = [math]::Round($pidRssSorted[[math]::Floor($pidRssSorted.Count / 2)] / 1024)

        Write-Host ("  {0,-6} {1,-28} rss median: {2,4} MB   peak working set: {3} MB" -f $procId, $procName, $pidMedian, $peakWS)
    } catch {
        Write-Host "  $procId gone"
    }
}

Stop-App

Write-Host "`nLogs written to: $log (stdout), $errLog (stderr)"
Write-Host "TSV written to: $Out"
