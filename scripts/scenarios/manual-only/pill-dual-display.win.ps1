#!/usr/bin/env pwsh
# Scenario: pill-dual-display (Windows)
# On screen: launches Fidget with dual displays (3440x1440@(0,0) + 1200x1920@(-1200,-209)).
#   A static-director wake shows Dialogue; the pill is typed into and carried across the seam.
#   Fidget quits when the scenario ends. No screenshots.
# Input: SendInput cursor moves, typed text and one left-button drag. The
#   cursor hovers the sprite until the pill opens, "hi" is typed into it, and
#   the sprite is pressed, carried to the middle of another display and let go.
#   Keep hands off the mouse and keyboard for the run.
# Duration: a few seconds. Every wait gives up 3 s after its trigger.
# Grants: a desktop session with dual displays matching geometry above.
#   Nothing is pressed unless Fidget owns the window under the cursor, and
#   nothing is typed unless Fidget holds the foreground.
# Asserts: painted bubble rect included in Windows region while visible,
#   a typed pill reopens on the new owner overlay on display cross.
# Fixture: FIDGET_SCENARIO_TRACE=<saved stderr> runs only the check and
#   launches nothing; FIDGET_SCENARIO_PLAN_LOG=<process.log> prints the drag.
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

$root = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
. (Join-Path $PSScriptRoot "..\helpers\wait-helpers.win.ps1")
$script:Evidence = $null

function Fail([string]$Message) {
    [Console]::Error.WriteLine("FAIL: $Message")
    if ($script:Evidence) { [Console]::Error.WriteLine("evidence in $($script:Evidence)") }
    exit 1
}

# Patterns for FIDGET_TRACE_BUBBLE=1 stderr output
$regionPattern = '^(?:\d+ )?overlay (\S+): region rebuild (\d+) rects \(art \d+ \+ hotspots \d+ \+ painted (\d+)\)'
$followPattern = '^(?:\d+ )?overlay (\S+): pill follows instance=(\S+) chars=(\d+) focused=(\S+)'
$bubbleShowPattern = '^(?:\d+ )?overlay (\S+): (?:showSpeech|showThinking)'
$bubbleHidePattern = '^(?:\d+ )?overlay (\S+): (?:hideSpeech|hideThinking)'

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

# process.log lines carry a seconds prefix; each matches its eprintln! in src-tauri/src.
$spriteSizePattern = '(?:^|\s)overlay: \d+ display\(s\); sprite (\d+)x(\d+);'
$displayPattern = '(?:^|\s)overlay: \S+ covers (\d+)x(\d+) at \((-?\d+),(-?\d+)\)'
$pillOpenedPattern = '(?:^|\s)overlay: draft instance=\S+ text="" focused=true$'
$pillTypedPattern = '(?:^|\s)overlay: draft instance=\S+ text="hi" focused=true$'
$pressPattern = '(?:^|\s)click: down '
$releasePattern = '(?:^|\s)click: up '
$draggedPattern = '(?:^|\s)frame: \d+ Dragged '

# The middle of the sprite at rest, and of a display it is not on.
function Get-DragPlan([string]$LogFile) {
    if (-not ($at = Get-SpriteAtRest $LogFile)) { return $null }
    $size = Get-Content -LiteralPath $LogFile | Where-Object { $_ -match $spriteSizePattern } | Select-Object -First 1
    if (-not ($size -match $spriteSizePattern)) { return $null }
    $x = $at.X + [int]([int]$Matches[1] / 2); $y = $at.Y + [int]([int]$Matches[2] / 2)
    foreach ($line in Get-Content -LiteralPath $LogFile | Where-Object { $_ -match $displayPattern }) {
        $null = $line -match $displayPattern
        $w = [int]$Matches[1]; $h = [int]$Matches[2]; $left = [int]$Matches[3]; $top = [int]$Matches[4]
        $onIt = $x -ge $left -and $x -lt $left + $w -and $y -ge $top -and $y -lt $top + $h
        if (-not $onIt) { return @{ X = $x; Y = $y; ToX = $left + [int]($w / 2); ToY = $top + [int]($h / 2) } }
    }
    $null
}

if ($env:FIDGET_SCENARIO_PLAN_LOG) {
    $plan = Get-DragPlan $env:FIDGET_SCENARIO_PLAN_LOG
    Write-Output "plan: $(if ($plan) { "($($plan.X),$($plan.Y)) -> ($($plan.ToX),$($plan.ToY))" } else { 'none' })"
    exit 0
}

if ($env:FIDGET_SCENARIO_TRACE) {
    Check-Trace $env:FIDGET_SCENARIO_TRACE
    exit 0
}

Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class PillDrag {
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr v);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [StructLayout(LayoutKind.Sequential)] struct POINT { public int X, Y; }
    [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr h, uint f);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint processId);
    [StructLayout(LayoutKind.Sequential)] struct KEYBDINPUT { public ushort vk; public ushort scan; public uint flags; public uint time; public IntPtr extra; }
    [StructLayout(LayoutKind.Sequential)] struct MOUSEINPUT { public int dx; public int dy; public uint data; public uint flags; public uint time; public IntPtr extra; }
    [StructLayout(LayoutKind.Explicit)] struct INPUT { [FieldOffset(0)] public uint type; [FieldOffset(8)] public KEYBDINPUT k; [FieldOffset(8)] public MOUSEINPUT m; }
    [DllImport("user32.dll", SetLastError = true)] static extern uint SendInput(uint n, INPUT[] i, int size);
    static void Send(INPUT[] all) {
        uint sent = SendInput((uint)all.Length, all, Marshal.SizeOf(typeof(INPUT)));
        if (sent != all.Length) throw new InvalidOperationException("SendInput sent " + sent + " of " + all.Length + " events, error " + Marshal.GetLastWin32Error());
    }
    static uint Owner(IntPtr h) { uint owner; GetWindowThreadProcessId(h, out owner); return owner; }
    public static uint ForegroundOwner() { return Owner(GetForegroundWindow()); }
    public static uint OwnerAt(int x, int y) { var p = new POINT(); p.X = x; p.Y = y; return Owner(GetAncestor(WindowFromPoint(p), 2)); }
    public static void Button(bool up) { var i = new INPUT(); i.type = 0; i.m.flags = up ? 4u : 2u; Send(new[] { i }); }
    // Each character's unicode down and up, in one call.
    public static void Text(string word) {
        var all = new INPUT[word.Length * 2];
        for (int n = 0; n < word.Length; n++) {
            all[2 * n].type = 1; all[2 * n].k.scan = word[n]; all[2 * n].k.flags = 4;
            all[2 * n + 1].type = 1; all[2 * n + 1].k.scan = word[n]; all[2 * n + 1].k.flags = 6;
        }
        Send(all);
    }
}
"@

function Count([string]$Pattern) { @(Get-Content -LiteralPath $log | Where-Object { $_ -match $Pattern }).Count }

function Drive-PillAcrossSeam {
    if ([PillDrag]::SetThreadDpiAwarenessContext([IntPtr]-4) -eq [IntPtr]::Zero) { Fail "could not make this thread per-monitor DPI aware, so its pixels would not match Fidget's" }
    if (-not (Wait-Until 3 { ($script:plan = Get-DragPlan $log) })) { Fail "no sprite at rest with a display to carry it to in 3 s; see $log" }
    $p = $script:plan

    $null = [PillDrag]::SetCursorPos($p.X, $p.Y)
    if (-not (Wait-ForTrace 3 $log $pillOpenedPattern)) { Fail "hovering the sprite at ($($p.X),$($p.Y)) did not open a focused pill in 3 s; see $log" }
    if (-not (Wait-Until 3 { [PillDrag]::ForegroundOwner() -eq $script:proc.Id })) { Fail "the pill opened but Fidget never took the foreground, so nothing was typed; see $log" }
    [PillDrag]::Text("hi")
    if (-not (Wait-ForTrace 3 $log $pillTypedPattern)) { Fail "the pill never showed text=`"hi`" in 3 s; see $log" }

    if ([PillDrag]::OwnerAt($p.X, $p.Y) -ne $script:proc.Id) { Fail "refusing to press at ($($p.X),$($p.Y)): Fidget does not own the window there" }
    # The press must land before the move, or Fidget sees it off the sprite.
    $presses = Count $pressPattern
    [PillDrag]::Button($false)
    try {
        if (-not (Wait-ForCount 3 $log $pressPattern $presses)) { Fail "Fidget did not see the press on the sprite in 3 s; see $log" }
        $null = [PillDrag]::SetCursorPos($p.ToX, $p.ToY)
        if (-not (Wait-ForTrace 3 $log $draggedPattern)) { Fail "the press did not grab the sprite in 3 s; see $log" }
    } finally {
        $releases = Count $releasePattern
        [PillDrag]::Button($true)
    }
    if (-not (Wait-ForCount 3 $log $releasePattern $releases)) { Fail "Fidget did not see the release at ($($p.ToX),$($p.ToY)) in 3 s; see $log" }
    if (-not (Wait-ForTrace 3 $traceFile $followPattern)) { Fail "the pill never reopened on another overlay within 3 s of the drop; see $traceFile" }
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
$log = Join-Path $data "process.log"

Write-Host "scenario: launching Fidget with FIDGET_TRACE_BUBBLE=1, static director"
$env:FIDGET_TRACE_BUBBLE = "1"
$env:FIDGET_DIRECTOR = "static"
$env:FIDGET_DIRECTOR_WAKE_SECS = "1"
$env:FIDGET_TRACE_FRAMES = "1"
$startMs = Get-NowMs
$script:proc = Start-FidgetPiped $Bin @() (Join-Path $out "app.log") $traceFile
try {
    # The static director's wake brings up a speech bubble on its own.
    if (-not (Wait-ForTrace 3 $traceFile $bubbleShowPattern)) { Fail "the static wake showed no speech bubble; see $traceFile" }

    Drive-PillAcrossSeam
    # The region rebuild that clears the painted rects follows the hide.
    $null = Wait-ForTrace 3 $traceFile $bubbleHidePattern
} finally {
    Stop-FidgetTree $script:proc $null
}

Check-Trace $traceFile
Write-Host "evidence in $out ($((Get-NowMs) - $startMs)ms)"
