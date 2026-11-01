#!/usr/bin/env pwsh
# Scenario: pill-dual-display (Windows)
# On screen: launches Fidget on a desktop with two or more displays, any layout.
#   A poke against a local Director that never answers shows the Thinking bubble;
#   the pill is typed into and carried to another display. Fidget quits at the end.
# Input: SendInput cursor moves, one click, typed text and one left-button drag.
#   The cursor pokes the sprite and hovers until the pill opens, "hi" is typed,
#   and the sprite is pressed, carried to the middle of another display and let go.
#   Keep hands off the mouse and keyboard for the run.
# Duration: a few seconds. Every wait gives up 3 s after its trigger (launch,
#   landing, poke, hover, keys, press, move, drop), and page load 5 s after rest.
# Grants: a desktop session with two or more displays. A loopback TCP port.
#   Nothing is pressed unless Fidget owns the window under the cursor, and
#   nothing is typed unless Fidget holds the foreground.
# Asserts: painted bubble rect included in Windows region while visible,
#   a typed pill reopens on the new owner overlay on display cross.
# Fixture: FIDGET_SCENARIO_TRACE=<saved stderr> runs only the check and
#   launches nothing; FIDGET_SCENARIO_PLAN_LOG=<process.log> prints the drag.
#
# Usage: powershell -NoProfile -ExecutionPolicy Bypass -File pill-dual-display.win.ps1 --go <fidget binary>
#   Windows PowerShell 5.1, as the fidget-verify runner calls it; pwsh 7 is not supported.
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
$landedPattern = '(?:^|\s)frame: \d+ (?:Grounded|Perched) '
$pokePattern = '(?:^|\s)verbs: \S+ \[[^\]]*\bPoke\b'

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
        if (-not $onIt) { return @{ X = $x; Y = $y; At = $at; Left = $left; Top = $top; W = $w; H = $h; ToX = $left + [int]($w / 2); ToY = $top + [int]($h / 2) } }
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

# Hover the sprite's newest spot until Fidget owns the window there, chasing it
# if it walks. The region is art-shaped, so a landing frame can leave it bare.
function Aim-AtSprite($p, [string]$What) {
    $script:aim = $null
    $owned = Wait-Until 3 {
        if (-not ($at = Get-LastSpriteSpot $log)) { return $false }
        $x = $at.X + $p.X - $p.At.X; $y = $at.Y + $p.Y - $p.At.Y
        $null = [PillDrag]::SetCursorPos($x, $y)
        $script:aim = @{ X = $x; Y = $y; At = $at }
        [PillDrag]::OwnerAt($x, $y) -eq $script:proc.Id
    }
    if (-not $script:aim) { Fail "no sprite position to $What at in 3 s; see $log" }
    if (-not $owned) { Fail "refusing to $What at ($($script:aim.X),$($script:aim.Y)): Fidget did not own the window there in 3 s; see $log" }
    $script:aim
}

function Assert-Still($a, [string]$What) {
    $now = Get-LastSpriteSpot $log
    if (-not $now -or [Math]::Abs($now.X - $a.At.X) -gt 2 -or [Math]::Abs($now.Y - $a.At.Y) -gt 2) { Fail "the sprite kept moving between aiming and the $What, from ($($a.At.X),$($a.At.Y)) to ($($now.X),$($now.Y)); see $log" }
}

function Drive-PillAcrossSeam {
    if ([PillDrag]::SetThreadDpiAwarenessContext([IntPtr]-4) -eq [IntPtr]::Zero) { Fail "could not make this thread per-monitor DPI aware, so its pixels would not match Fidget's" }
    if (-not (Wait-Until 3 { ($script:plan = Get-DragPlan $log) })) { Fail "no sprite at rest with a display to carry it to in 3 s; see $log" }
    $p = $script:plan

    $a = Aim-AtSprite $p "poke"
    Assert-Still $a "poke"
    # Fidget samples the button each frame, so an instant click is never seen.
    $presses = Count $pressPattern
    [PillDrag]::Button($false)
    try {
        if (-not (Wait-ForCount 3 $log $pressPattern $presses)) { Fail "Fidget did not see the poke's press in 3 s; see $log" }
    } finally {
        $releases = Count $releasePattern
        [PillDrag]::Button($true)
    }
    if (-not (Wait-ForCount 3 $log $releasePattern $releases)) { Fail "Fidget did not see the poke's release in 3 s; see $log" }
    if (-not (Wait-ForTrace 3 $traceFile $bubbleShowPattern)) { Fail "the poke showed no Thinking bubble in 3 s; see $traceFile" }
    # Settled after the double-click interval, so the drag press is not a double-click.
    if (-not (Wait-ForTrace 3 $log $pokePattern)) { Fail "Fidget never settled the click as a Poke in 3 s; see $log" }

    $a = Aim-AtSprite $p "hover"
    if (-not (Wait-ForTrace 3 $log $pillOpenedPattern)) { Fail "hovering the sprite at ($($a.X),$($a.Y)) did not open a focused pill in 3 s; see $log" }
    if (-not (Wait-Until 3 { [PillDrag]::ForegroundOwner() -eq $script:proc.Id })) { Fail "the pill opened but Fidget never took the foreground, so nothing was typed; see $log" }
    [PillDrag]::Text("hi")
    if (-not (Wait-ForTrace 3 $log $pillTypedPattern)) { Fail "the pill never showed text=`"hi`" in 3 s; see $log" }

    $a = Aim-AtSprite $p "press"
    Assert-Still $a "press"
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
    $end = Get-LastSpriteSpot $log; $cx = $end.X + $p.X - $p.At.X; $cy = $end.Y + $p.Y - $p.At.Y
    Write-Host "dropped at ($($p.ToX),$($p.ToY)); sprite middle at ($cx,$cy) on the display at ($($p.Left),$($p.Top)) $($p.W)x$($p.H)"
    if ($cx -lt $p.Left -or $cx -ge $p.Left + $p.W -or $cy -lt $p.Top -or $cy -ge $p.Top + $p.H) { Fail "the sprite did not land on the display it was carried to; see $log" }
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

# A local Director that accepts and never answers keeps a poke's turn in flight,
# so the Thinking bubble shows until the 2 s timeout. No network leaves the box.
$listener = New-Object System.Net.Sockets.TcpListener([System.Net.IPAddress]::Loopback, 0)
$listener.Start()
Write-Host "scenario: launching Fidget with FIDGET_TRACE_BUBBLE=1, silent Director on port $($listener.LocalEndpoint.Port)"
$env:FIDGET_TRACE_BUBBLE = "1"
$env:FIDGET_DIRECTOR_BASE_URL = "http://127.0.0.1:$($listener.LocalEndpoint.Port)"
$env:FIDGET_DIRECTOR_TIMEOUT_SECS = "2"
$env:FIDGET_TRACE_FRAMES = "1"
$startMs = Get-NowMs
$script:proc = Start-FidgetPiped $Bin @() (Join-Path $out "app.log") $traceFile
try {
    if (-not (Wait-ForUp 3 $traceFile)) { Fail "Fidget never wrote its overlay line in 3 s; see $traceFile" }
    if (-not (Wait-ForTrace 3 $log $landedPattern)) { Fail "the sprite never landed within 3 s of startup; see $log" }
    if (-not (Wait-Until 3 { Get-SpriteAtRest $log })) { Fail "the sprite never came to rest within 3 s of landing; see $log" }
    # A bubble sent before an overlay's page loads is never drawn or traced. 5 s,
    # not 3: WebView2 page load runs late on slower machines (Oded's exception).
    $overlays = @(Get-Content -LiteralPath $traceFile | Where-Object { $_ -match '^overlay: \S+ covers ' }).Count
    if (-not (Wait-ForCount 5 $traceFile '^overlay: \S+ page loaded$' ($overlays - 1))) { Fail "not all $overlays overlay pages loaded within 5 s of rest; see $traceFile" }

    Drive-PillAcrossSeam
    # The region rebuild that clears the painted rects follows the hide.
    $null = Wait-ForTrace 3 $traceFile $bubbleHidePattern
} finally {
    Stop-FidgetTree $script:proc $null
    $listener.Stop()
}

Check-Trace $traceFile
Write-Host "evidence in $out ($((Get-NowMs) - $startMs)ms)"
