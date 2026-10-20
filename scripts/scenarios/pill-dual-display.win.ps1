#!/usr/bin/env pwsh
# Scenario: pill-dual-display (Windows)
# On screen: launches Fidget with dual displays (3440x1440@(0,0) + 1200x1920@(-1200,-209)).
#   A static-director wake shows speech; the pill is typed into and carried across the seam.
#   Fidget quits when the scenario ends. No screenshots.
# Input: none automated. A person hovers the sprite, types into the pill, and
#   drags the sprite to the other display. The run ends the moment the pill
#   reopens on the other display, or after 60 s of waiting on the person.
# Duration: a few seconds plus however long the person takes, 70 s at most.
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
. (Join-Path $PSScriptRoot "wait-helpers.win.ps1")
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

if ($env:FIDGET_SCENARIO_TRACE) {
    Check-Trace $env:FIDGET_SCENARIO_TRACE
    exit 0
}

$displays = Get-CimInstance -ClassName Win32_DesktopMonitor | Measure-Object
if ($displays.Count -lt 2) {
    [Console]::Error.WriteLine("SKIP: need dual displays, have $($displays.Count)")
    exit 2
}

# Run Fidget with trace enabled, in a private profile with the first-run tour
# already shown, so the tour never covers the sprite.
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$out = Join-Path $env:TEMP "fidget-scenario-pill-dual-display-$stamp"
$script:Evidence = $out
$data = Initialize-ScenarioData $out
$traceFile = Join-Path $out "app.err"

Write-Host "scenario: launching Fidget with FIDGET_TRACE_BUBBLE=1, static director"
$env:FIDGET_TRACE_BUBBLE = "1"
$env:FIDGET_DIRECTOR = "static"
$env:FIDGET_DIRECTOR_WAKE_SECS = "1"
$startMs = Get-NowMs
$script:proc = Start-FidgetPiped $Bin @() (Join-Path $out "app.log") $traceFile
try {
    # The static director's wake brings up a speech bubble on its own.
    if (-not (Wait-ForTrace 3 $traceFile $bubbleShowPattern)) { Fail "the static wake showed no speech bubble; see $traceFile" }

    # Opening the pill, typing, and dragging the sprite across the seam have no
    # hook yet, so a person does them. The wait ends on the trace line that the
    # pill reopened with its draft; 60 s is a person's limit, not a fixed sleep.
    Write-Host "scenario: hover the sprite, type into the pill, drag the sprite to the other display (60 s at most)."
    if (-not (Wait-ForTrace 60 $traceFile $followPattern)) { Fail "the pill never reopened on another overlay within 60 s; see $traceFile" }
    # The region rebuild that clears the painted rects follows the hide.
    $null = Wait-ForTrace 3 $traceFile $bubbleHidePattern
} finally {
    Stop-FidgetTree $script:proc $null
}

Check-Trace $traceFile
Write-Host "evidence in $out ($((Get-NowMs) - $startMs)ms)"
