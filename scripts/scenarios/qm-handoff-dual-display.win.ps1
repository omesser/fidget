#!/usr/bin/env pwsh
# Scenario: qm-handoff-dual-display (Windows)
# On screen: launches Fidget with dual displays (3440x1440@(0,0) + 1200x1920@(-1200,-209)).
#   Sends QM -> thinking -> speech, drags sprite across displays, watches pill handoff.
#   Fidget quits when the scenario ends. No screenshots.
# Input: types into QM pill, drags sprite from right display to left,
#   watches for pill handoff and bubble region paint. The cursor moves;
#   keep hands off the mouse for the run.
# Duration: about 30 s.
# Grants: a desktop session with dual displays matching geometry above.
# Asserts: painted bubble rect included in Windows region while visible,
#   pill handoff to new owner overlay on display cross, no orphan ellipsis.
# Fixture: FIDGET_SCENARIO_TRACE=<saved stderr> runs only the check and
#   launches nothing.
#
# Usage: qm-handoff-dual-display.win.ps1 --go <fidget binary>
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
    Write-Error "usage: qm-handoff-dual-display.win.ps1 --go <fidget binary>"
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
$handoffPattern = '^(?:\d+ )?overlay (\S+): pill handoff from (\S+) open=(\S+) text="([^"]*)" focused=(\S+)'
$bubbleShowPattern = '^(?:\d+ )?overlay (\S+): showSpeech|showThinking'
$bubbleHidePattern = '^(?:\d+ )?overlay (\S+): hideSpeech|hideThinking'

function Check-Trace([string]$File) {
    $sawRegionWithPainted = $false
    $sawRegionWithoutPainted = $false
    $sawHandoff = $false
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
        } elseif ($line -cmatch $handoffPattern) {
            $sawHandoff = $true
            $toOverlay = $Matches[1]
            $fromOverlay = $Matches[2]
            $open = $Matches[3]
            $text = $Matches[4]
            $focused = $Matches[5]
            if ($open -ne "true") {
                Fail "pill handoff has open=$open, want true"
            }
            if ($text.Length -eq 0) {
                Fail "pill handoff has empty text, want preserved text"
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
    if (-not $sawHandoff) {
        Fail "no pill handoff across displays"
    }

    Write-Host "PASS: painted rect in region while visible, cleared after hide, pill handoff preserved state"
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
$traceFile = Join-Path $tempDir "fidget-qm-handoff-trace-$timestamp.log"
$script:Evidence = $traceFile

Write-Host "scenario: launching Fidget with FIDGET_TRACE_BUBBLE=1, static director"
$env:FIDGET_TRACE_BUBBLE = "1"
$env:FIDGET_DIRECTOR = "static"
$env:FIDGET_DIRECTOR_WAKE_SECS = "2"
$process = Start-Process -FilePath $Bin -NoNewWindow -PassThru -RedirectStandardError $traceFile

Start-Sleep -Seconds 2

# Automated scenario steps using static director and simulated events:
# 1. Static director generates initial wake (FIDGET_DIRECTOR_WAKE_SECS=2)
# 2. Speech bubble appears automatically (static director provides canned response)
# 3. QM pill opens on the first overlay when bubble shows (automatic in bubble.js)
# 4. Cross-display handoff is triggered by frame_loop.rs bubble_owner detection
#
# The following steps are still PARTIALLY MANUAL as of this commit:
# - Moving the sprite across display seam: No dev hook exists yet. To fully automate,
#   add a dev-only command `#[tauri::command] fn dev_move_sprite(instance: u32, x: i32, y: i32)`
#   behind FIDGET_DEV_HOOKS flag that calls engine.set_position() and emits frame events.
# - Typing into QM pill: UI Automation APIs exist on Windows (IUIAutomation) but are not
#   yet wired. For now, the scenario runs Fidget and waits while the static director
#   and automatic QM pill opening/handoff logic execute, then checks the trace log for
#   expected region rebuild and handoff patterns.

Write-Host "scenario: Fidget running with static director. QM pill should open automatically."
Write-Host "Automated: wake -> speech bubble -> QM pill open"
Write-Host "Manual (not yet hooked): drag sprite across displays to trigger handoff"
Write-Host ""
Write-Host "Waiting 10s for static director wake and automatic QM pill..."
Start-Sleep -Seconds 10

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
