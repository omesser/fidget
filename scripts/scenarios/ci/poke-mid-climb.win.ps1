#!/usr/bin/env pwsh
# Scenario: poke-mid-climb (Windows)
# On screen: launches Fidget with the Static Director. The sprite is dragged
#   past a display's side edge and let go, so it climbs that wall, and is
#   poked while it climbs. Fidget quits when the scenario ends. No screenshots.
# Input: a real drag that carries the sprite over the edge named by
#   FIDGET_SCENARIO_EDGE (left or right, default left), then clicks on it
#   until one lands as a Poke. The cursor moves; keep hands off the mouse for
#   the run.
# Duration: a few seconds, 10 s at most.
# Grants: a desktop session.
# Asserts: the Poke starts react over, then the sprite stays Climbing at one
#   position in one climb frame through the cooldown, and climbs on after it.
# Skips (exit 2, preflight.txt) only when a fullscreen window is stacked over
#   the click and would take it. Any other fullscreen app is left alone.
#   hide-in-fullscreen is off, so Fidget neither fades nor moves.
# Fixture: FIDGET_SCENARIO_TRACE=<saved app log> runs only the check and
#   launches nothing.
#
# Usage: poke-mid-climb.win.ps1 --go <fidget binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
# No param() block: Windows PowerShell binds `--go` as -Go and swallows the
# next token, so $Go never equals "--go" and the leaf always exits 2.
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
    Write-Error "usage: poke-mid-climb.win.ps1 --go <fidget binary>"
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

# process.log puts epoch seconds before each line; a saved stderr log does not.
$framePattern = '^(?:\d+ )?frame: (\d+) (\w+) pos\((-?\d+),(-?\d+)\) \S+ (\S+)#(\d+) '
$pokePattern = '^(?:\d+ )?verbs:.*Poke'
$hitPattern = '^(?:\d+ )?hit-test: .* HIT +click-through OFF'

# poke-mid-climb-check.py in PowerShell, so no Windows host needs Python. Case
# sensitive and word for word like it: the same broken fixtures fail both.
# POKE_COOLDOWN_MS is 2500; the windows leave a tick either side of it.
$PausedUntilMs = 2300
$ResumedFromMs = 2600
$ResumedByMs = 3500

# The frames from the first Poke on a climbing sprite, or $null before one.
function Get-FramesAfterPoke([string]$File) {
    $frames = [System.Collections.Generic.List[object]]::new()
    $pokeAt = $null
    $state = $null
    foreach ($line in Get-Content -LiteralPath $File -Encoding utf8) {
        if ($line -cmatch $framePattern) {
            $frames.Add([pscustomobject]@{
                    At = [long]$Matches[1]; State = $Matches[2]; X = [int]$Matches[3]; Y = [int]$Matches[4]
                    Animation = $Matches[5]; Index = [int]$Matches[6]
                })
            $state = $Matches[2]
        } elseif ($null -eq $pokeAt -and $line -cmatch $pokePattern -and $state -ceq "Climbing") {
            $pokeAt = $frames.Count
        }
    }
    if ($null -eq $pokeAt) { return $null }
    return , @($frames | Select-Object -Skip $pokeAt)
}

function Check-Trace([string]$File) {
    $after = Get-FramesAfterPoke $File
    if ($null -eq $after) { Fail "no Poke landed on a climbing sprite" }
    $span = if ($after.Count -gt 0) { $after[-1].At - $after[0].At } else { 0 }
    if ($span -lt $PausedUntilMs) {
        $seconds = ($span / 1000).ToString("0.0", [Globalization.CultureInfo]::InvariantCulture)
        Fail "the trace stops $seconds s after the Poke"
    }
    $start = $after[0].At
    $paused = @($after | Where-Object { $_.At - $start -lt $PausedUntilMs })
    $resumed = @($after | Where-Object { $_.At - $start -ge $ResumedFromMs -and $_.At - $start -le $ResumedByMs })
    if ($paused | Where-Object { $_.State -cne "Climbing" }) { Fail "the sprite left the wall during the pause" }
    if (@($paused | ForEach-Object { "$($_.X),$($_.Y)" } | Sort-Object -Unique).Count -ne 1) {
        Fail "the sprite moved during the pause"
    }
    if ($after[0].Animation -cne "react" -or $after[0].Index -ne 0) { Fail "the Poke did not start react over" }
    if (@($paused | Where-Object { $_.Animation -ceq "climb" } | ForEach-Object { $_.Index } | Sort-Object -Unique).Count -gt 1) {
        Fail "the sprite climbed in place during the pause"
    }
    $top = $paused[0].Y
    if (-not ($resumed | Where-Object { $_.State -ceq "Climbing" -and $_.Y -lt $top })) {
        Fail "the sprite did not climb on after the cooldown"
    }
    Write-Output "ok: paused at ($($paused[0].X), $top) for $($paused[-1].At - $start) ms in one climb pose, then climbed on"
}

if ($env:FIDGET_SCENARIO_TRACE) {
    Check-Trace $env:FIDGET_SCENARIO_TRACE
    Write-Output "PASS: fixture trace"
    exit 0
}

$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$out = Join-Path $env:TEMP "fidget-scenario-poke-mid-climb-$stamp"
$script:Evidence = $out
$data = Initialize-ScenarioData $out @{ hide_in_fullscreen = $false }

$log = Join-Path $out "app.log"
$err = Join-Path $out "app.err"
$inputLog = Join-Path $out "input.txt"
$trace = Join-Path $data "process.log"

$env:FIDGET_DIRECTOR_API_KEY = "x"
$env:FIDGET_TRACE_FRAMES = "1"
$env:FIDGET_TRACE_HITTEST = "1"
$env:FIDGET_CHARACTERS = Join-Path $root "characters"

# In-process, not ax-window-win.ps1 click: a new PowerShell compiles its
# Add-Type before the click, and a climbing sprite has moved its own height by then.
Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Threading;
public class ClimbInput {
    [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);
    const uint Down = 0x0002, Up = 0x0004;
    public static void Drag(int x, int y, int toX) {
        mouse_event(Down, 0, 0, 0, UIntPtr.Zero);
        // A press held past the drag threshold time becomes a grab, not a click.
        Thread.Sleep(250);
        for (int step = 1; step <= 10; step++) {
            SetCursorPos(x + (toX - x) * step / 10, y);
            // One step per frame tick or two, so the drag follows the path instead of one jump.
            Thread.Sleep(30);
        }
        // Held still before release, so the release measures no speed and nothing is thrown.
        Thread.Sleep(300);
        mouse_event(Up, 0, 0, 0, UIntPtr.Zero);
    }
    public static void Move(int x, int y) {
        SetCursorPos(x, y);
    }
    public static void Click(int x, int y) {
        SetCursorPos(x, y);
        // A climbing sprite's loop ticks every frame, so 120 ms turns click-through off
        // under the cursor. Drag's hit-test file wait is slower than the climb stays under it.
        Thread.Sleep(120);
        mouse_event(Down, 0, 0, 0, UIntPtr.Zero);
        // A press and release in one tick read as no click; 60 ms spans a tick.
        Thread.Sleep(60);
        mouse_event(Up, 0, 0, 0, UIntPtr.Zero);
    }
}
"@

function Last-Frame {
    if (-not (Test-Path -LiteralPath $trace)) { return $null }
    $line = Get-Content -LiteralPath $trace -Tail 40 | Where-Object { $_ -cmatch $framePattern } | Select-Object -Last 1
    if ($line -cmatch ' (\w+) pos\(\S+ sprite\((-?\d+),(-?\d+)\) ') {
        [pscustomobject]@{ State = $Matches[1]; X = [int]$Matches[2]; Y = [int]$Matches[3] }
    }
}

function In-State([string]$Pattern) {
    $frame = Last-Frame
    [bool]($frame -and $frame.State -cmatch "^($Pattern)$")
}

# Whether the trace already runs as far past the Poke as Check-Trace reads.
function Cooldown-Traced {
    $after = Get-FramesAfterPoke $trace
    return ($null -ne $after -and $after.Count -gt 0 -and $after[-1].At - $after[0].At -ge $ResumedByMs)
}

function Pokes { @(Select-String -LiteralPath $trace -Pattern $pokePattern -CaseSensitive).Count }

# A frame where the sprite moved (walking, climbing or falling) is progress,
# however long the walk or climb. 3 s with no progress gives up.
function Get-RestDecision([string]$State, [bool]$Moved, [bool]$Still, [int]$SinceProgressMs) {
    if ($Still -and $State -cmatch '^(Grounded|Perched)$') { return "done" }
    if ($Moved) { return "progress" }
    if ($SinceProgressMs -ge 3000) { return "give-up" }
    "wait"
}

# Resting and not walking: a press on a walking sprite lands where it was, and
# the drag then throws whatever window is underneath.
function Wait-Still {
    $script:restDecision = "wait"
    $script:lastFrame = Last-Frame
    while ($true) {
        $clock = [System.Diagnostics.Stopwatch]::StartNew()
        # Re-decided on each trace change; progress restarts the 3 s window.
        $null = Wait-OnChanges 3 (Split-Path -Parent $trace) {
            $a = Last-Frame
            $state = if ($a) { $a.State } else { "" }
            $last = $script:lastFrame
            $moved = [bool]($a -and $last -and ($a.X -ne $last.X -or $a.Y -ne $last.Y))
            $script:lastFrame = $a
            $still = [bool](Get-SpriteAtRest $trace)
            $script:restDecision = Get-RestDecision $state $moved $still ([int]$clock.ElapsedMilliseconds)
            $script:restDecision -cne "wait"
        }
        if ($proc.HasExited) { Fail "Fidget exited; see $err" }
        if ($script:restDecision -ceq "done") { return $true }
        if ($script:restDecision -cne "progress") { return $false }
    }
}

function Wait-Climbing([int]$Tenths) {
    return (Wait-OnChanges ($Tenths / 10) (Split-Path -Parent $trace) { In-State "Climbing" })
}

function Get-ClimbEdges([string[]]$Lines) {
    $displays = @($Lines | Where-Object { $_ -match ' covers (\d+)x\d+ at \((-?\d+),-?\d+\)' } | ForEach-Object {
            [void]($_ -match ' covers (\d+)x\d+ at \((-?\d+),-?\d+\)')
            # The log's x is the origin. The far side is origin plus width, parenthesized so + is arithmetic.
            $origin = [int]$Matches[2]
            $width = [int]$Matches[1]
            , @($origin, ($origin + $width))
        })
    if ($displays.Count -eq 0) { return $null }
    $left = ($displays | ForEach-Object { $_[0] } | Measure-Object -Minimum).Minimum
    $right = ($displays | ForEach-Object { $_[1] } | Measure-Object -Maximum).Maximum
    [pscustomobject]@{
        Left = [int]$left
        Right = [int]$right
        LeftTarget = [int]$left
        RightTarget = [int]$right - 1
    }
}

$script:proc = Start-FidgetPiped $Bin @() $log $err
$startMs = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
try {
    if (-not (Wait-ForFile 3 $trace)) { Fail "process.log never appeared; see $err" }

    if (-not (Wait-Still)) { Fail "the sprite never came to rest; see $trace" }
    $overlay = @(Get-Content -LiteralPath $trace | Where-Object { $_ -match '^(?:\d+ )?overlay: ' })
    $sizeLine = $overlay | Where-Object { $_ -match ' sprite (\d+)x\d+;' } | Select-Object -First 1
    if (-not ($sizeLine -match ' sprite (\d+)x\d+;')) { Fail "no sprite size in $trace" }
    $size = [int]$Matches[1]
    $half = [int][Math]::Floor($size / 2)
    $edges = Get-ClimbEdges $overlay
    if (-not $edges) { Fail "no display bounds in $trace" }
    $gapMs = 500
    $gapLine = $overlay | Where-Object { $_ -match 'double-click interval (\d+)ms' } | Select-Object -First 1
    if ($gapLine -and $gapLine -match 'double-click interval (\d+)ms') { $gapMs = [int]$Matches[1] }
    $clickGap = $gapMs + 100

    $frame = Last-Frame
    $sx = $frame.X; $sy = $frame.Y
    $cx = $sx + $half
    $side = if ($env:FIDGET_SCENARIO_EDGE) { $env:FIDGET_SCENARIO_EDGE } else { "left" }
    # Held a quarter of its width inboard of centre, so at the edge the sprite's
    # centre is past it: it falls to the nearest edge and climbs, whatever speed
    # the hand had. The release is held still.
    if ($side -ceq "left") {
        $edge = $edges.Left
        $into = 1
        $grabX = $cx + [int][Math]::Floor($half / 2)
        $targetX = $edges.LeftTarget
    } elseif ($side -ceq "right") {
        $edge = $edges.Right
        $into = -1
        $grabX = $cx - [int][Math]::Floor($half / 2)
        $targetX = $edges.RightTarget
    } else {
        Fail "FIDGET_SCENARIO_EDGE is '$side', want left or right"
    }
    # A resting sprite's loop polls up to 1 s apart until it sees the cursor near
    # the sprite, then 100 ms apart. A press before that is read after the drag.
    $hits = @(Select-String -LiteralPath $trace -Pattern $hitPattern -CaseSensitive).Count
    [ClimbInput]::Move($grabX, $sy + $half)
    if (-not (Wait-ForCount 3 $trace $hitPattern $hits)) { Fail "Fidget never saw the cursor over the sprite; see $trace" }
    [ClimbInput]::Drag($grabX, $sy + $half, $targetX)
    Add-Content -LiteralPath $inputLog -Value "placed from ($grabX,$($sy + $half)) to x=$targetX"

    if (-not (Wait-Climbing 20)) { Fail "the sprite let go over the $side edge and did not climb; see $trace" }
    $frame = Last-Frame
    $sx = $frame.X; $sy = $frame.Y
    # A wall climb is centred on the display edge it was let go over.
    $mid = $sx + $half
    if ([Math]::Abs($mid - $edge) -gt 2) { Fail "the sprite climbs at x=$mid, not the $side edge at x=$edge; see $trace" }

    # Only the display's half of the sprite is drawn. Clicks a double-click
    # interval apart are separate Pokes, so stop at the first. Aim a quarter of
    # the sprite in from the edge, where Buddy Bot is solid from row 15 to 59:
    # halfway in crosses its eye and misses. It climbs 27-65 px before the
    # press lands, so aim 10 px above its top; `$sy -gt $size` keeps that on
    # the display.
    $px = $edge + $into * [int][Math]::Floor($half / 4)
    . (Join-Path $PSScriptRoot "..\helpers\fullscreen-over-click.win.ps1")
    $before = Pokes
    $poked = $false
    for ($i = 0; $i -lt 6; $i++) {
        $frame = Last-Frame
        if (-not $frame -or $frame.State -cne "Climbing") { break }
        $sy = $frame.Y
        if ($sy -le $size) { break }
        $py = $sy - 10
        Skip-IfFullscreenOverClick $px $py $script:proc.Id $out "click"
        [ClimbInput]::Click($px, $py)
        Add-Content -LiteralPath $inputLog -Value "clicked ($px,$py)"
        if (Wait-ForCount ($clickGap / 1000) $trace $pokePattern $before) {
            $poked = $true
            break
        }
    }
    if (-not $poked) { Fail "no Poke landed mid-climb over the $side edge; see $inputLog" }

    # The quick-message pill holds a climb while the cursor rests on the sprite.
    # Move further into the display than the click landed.
    [ClimbInput]::Move($px + $into * 4 * $size, $sy)

    if (-not (Wait-ForTrace 3 $trace 'frame: .* Climbing .* react#\d+')) {
        Fail "sprite did not hold react during cooldown; see $trace"
    }
    $null = Wait-OnChanges 5 (Split-Path -Parent $trace) { Cooldown-Traced }
} finally {
    Stop-FidgetTree $script:proc
}

Check-Trace $trace
$elapsedMs = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() - $startMs
Write-Output "PASS: evidence in $out (${elapsedMs}ms)"
