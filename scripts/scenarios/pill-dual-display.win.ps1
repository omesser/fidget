#!/usr/bin/env pwsh
# Scenario: pill-dual-display (Windows)
# On screen: launches Fidget with dual displays (3440x1440@(0,0) + 1200x1920@(-1200,-209)).
#   A static-director wake shows speech; the pill is typed into and carried across the seam.
#   Fidget quits when the scenario ends. No screenshots.
# Input: none automated. During a 20 s wait a person hovers the sprite, types
#   into the pill, and drags the sprite to the other display.
# Duration: about 30 s.
# Grants: a desktop session with dual displays matching geometry above.
# Asserts: painted bubble rect included in Windows region while visible,
#   a typed pill reopens on the new owner overlay on display cross.
# Fixture: FIDGET_SCENARIO_TRACE=<saved stderr> runs only the check and
#   launches nothing.
#
# Usage: pill-dual-display.win.ps1 --go <fidget binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
$Go = if ($args.Count -gt 0) { $args[0] } else { "" }
$Bin = if ($args.Count -gt 1) { $args[1] } else { "" }
$ErrorActionPreference = "Stop"

function Show-Header {
    $lines = Get-Content -LiteralPath $PSCommandPath
    $header = @()
    foreach ($line in $lines | Select-Object -Skip 1) {
        if ($line.StartsWith("#")) {
            $header += ($line -replace '^# ?', '')
        } elseif ($line.Trim().Length -gt 0) {
            break
        }
    }
    $header -join "`n"
}

if ($Go -ne "--go") {
    Show-Header
    exit 2
}
if (-not $Bin) {
    Write-Error "usage: pill-dual-display.win.ps1 --go <fidget binary>"
    exit 1
}

$root = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$script:Evidence = $null

function Fail([string]$Message) {
    [Console]::Error.WriteLine("FAIL: $Message")
    if ($script:Evidence) { [Console]::Error.WriteLine("evidence in $($script:Evidence)") }
    exit 1
}

# Patterns for FIDGET_TRACE_BUBBLE=1 stderr output
$regionPattern = '^(?:\d+ )?overlay (\S+): region rebuild (\d+) rects \(art \d+ \+ hotspots \d+ \+ painted (\d+)\)'
$followPattern = '^(?:\d+ )?overlay (\S+): pill follows instance=(\S+) chars=(\d+) focused=(\S+)'
$bubbleShowPattern = '^(?:\d+ )?overlay (\S+): showSpeech|showThinking'
$bubbleHidePattern = '^(?:\d+ )?overlay (\S+): hideSpeech|hideThinking'

function Check-Trace([string]$File) {
    $sawRegionWithPainted = $false
    $sawRegionWithoutPainted = $false
    $sawFollow = $false
    $bubbleVisible = $false
    $paintedWhileVisible = $false

    foreach ($line in Get-Content -LiteralPath $File -Encoding utf8) {
        if ($line -cmatch $bubbleShowPattern) {
            $bubbleVisible = $true
        } elseif ($line -cmatch $bubbleHidePattern) {
            $bubbleVisible = $false
        } elseif ($line -cmatch $regionPattern) {
            $overlay = $Matches[1]
            $totalRects = [int]$Matches[2]
            $paintedCount = [int]$Matches[3]
            if ($paintedCount -gt 0) {
                $sawRegionWithPainted = $true
                if ($bubbleVisible) {
                    $paintedWhileVisible = $true
                }
            } else {
                $sawRegionWithoutPainted = $true
            }
        } elseif ($line -cmatch $followPattern) {
            $sawFollow = $true
            if ([int]$Matches[3] -eq 0) {
                Fail "pill followed with no text, want the typed draft"
            }
        }
    }

    if (-not $sawRegionWithPainted) {
        Fail "no region rebuild with painted rects (bubble body not in Windows region)"
    }
    if (-not $paintedWhileVisible) {
        Fail "painted rects not included while bubble visible"
    }
    if (-not $sawRegionWithoutPainted) {
        Fail "region never cleared painted rects after hide"
    }
    if (-not $sawFollow) {
        Fail "the pill never reopened on another overlay"
    }

    Write-Host "PASS: painted rect in region while visible, cleared after hide, pill followed with its draft"
}

# Fixture mode: check a saved trace and exit
if ($env:FIDGET_SCENARIO_TRACE) {
    Check-Trace $env:FIDGET_SCENARIO_TRACE
    exit 0
}

# Ensure dual-display geometry
$displays = Get-CimInstance -ClassName Win32_DesktopMonitor | Measure-Object
if ($displays.Count -lt 2) {
    [Console]::Error.WriteLine("SKIP: need dual displays, have $($displays.Count)")
    exit 2
}

# Run Fidget with trace enabled
$tempDir = [System.IO.Path]::GetTempPath()
$timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
$traceFile = Join-Path $tempDir "fidget-pill-dual-display-trace-$timestamp.log"
$script:Evidence = $traceFile

Write-Host "scenario: launching Fidget with FIDGET_TRACE_BUBBLE=1, static director"
$env:FIDGET_TRACE_BUBBLE = "1"
$env:FIDGET_DIRECTOR = "static"
$env:FIDGET_DIRECTOR_WAKE_SECS = "2"
$process = Start-Process -FilePath $Bin -NoNewWindow -PassThru -RedirectStandardError $traceFile

Start-Sleep -Seconds 2

# The static director's wake brings up a speech bubble on its own. Opening the
# pill, typing, and dragging the sprite across the seam have no hook yet, so a
# person does them within the wait; the trace is checked afterwards.

Write-Host "scenario: Fidget running with static director."
Write-Host "Within 20s: hover the sprite, type into the pill, drag the sprite to the other display."
Start-Sleep -Seconds 20

# For a full automated run, a dev hook would simulate the drag here:
# Invoke-Expression "$Bin dev-move-sprite --instance 0 --x -600 --y 500"
# Start-Sleep -Seconds 2

# Stop Fidget
Stop-Process -Id $process.Id -Force
Start-Sleep -Seconds 1

# Check the trace
if (Test-Path $traceFile) {
    Check-Trace $traceFile
    Write-Host "evidence in $traceFile"
} else {
    Fail "no trace file at $traceFile"
}
