#!/usr/bin/env pwsh
# Scenario: quick-grab (Windows)
# On screen: launches Fidget with the Static Director. The sprite comes to
#   rest, the cursor parks away from it, then a press on the sprite starts a
#   drag that leaves the sprite before the next tick. Fidget quits when the
#   scenario ends. No screenshots.
# Input: SetCursorPos and a real button-down. The cursor moves. Keep hands
#   off the mouse for the run.
# Duration: a few seconds once the sprite is up, 30 s at most.
# Grants: a desktop session.
# Asserts: the click log's press point is on the sprite while the cursor the
#   tick sampled has already left, and that press starts a Grab. It does not
#   wait for the hit-test flip line.
# Fixture: none.
#
# Usage: quick-grab.win.ps1 --go <fidget binary>
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
    Write-Error "usage: quick-grab.win.ps1 --go <fidget binary>"
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

# process.log puts epoch seconds before each line.
$clickPattern = 'click: down .* cursor\([-\d.]+,[-\d.]+\)->\((-?\d+),(-?\d+)\) press=\((-?\d+),(-?\d+)\)'
$grabPattern = '^(?:\d+ )?verbs: .*\[Grab\]'
$framePattern = '^(?:\d+ )?frame: '

function Near([int]$X, [int]$Y, [int]$ToX, [int]$ToY, [int]$Slop) {
    return ([Math]::Abs($X - $ToX) -le $Slop) -and ([Math]::Abs($Y - $ToY) -le $Slop)
}

function Last-ClickDown([string]$File) {
    $line = Get-Content -LiteralPath $File | Where-Object { $_ -match $clickPattern } | Select-Object -Last 1
    if (-not $line -or $line -notmatch $clickPattern) { return $null }
    [pscustomobject]@{
        CursorX = [int]$Matches[1]
        CursorY = [int]$Matches[2]
        PressX  = [int]$Matches[3]
        PressY  = [int]$Matches[4]
    }
}

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class QuickGrab {
    [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);
    const uint Down = 0x0002, Up = 0x0004;
    public static void Move(int x, int y) { SetCursorPos(x, y); }
    public static void Release() { mouse_event(Up, 0, 0, 0, UIntPtr.Zero); }
    // Down and leave in one call, so the tick that sees the button finds the
    // cursor already off the sprite. The hook records the down point first.
    public static void PressAndLeave(int x, int y, int leaveX, int leaveY) {
        SetCursorPos(x, y);
        mouse_event(Down, 0, 0, 0, UIntPtr.Zero);
        SetCursorPos(leaveX, leaveY);
    }
}
"@

$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$out = Join-Path $env:TEMP "fidget-scenario-quick-grab-$stamp"
$script:Evidence = $out
$data = Initialize-ScenarioData $out
$log = Join-Path $out "app.log"
$err = Join-Path $out "app.err"
$trace = Join-Path $data "process.log"

$env:FIDGET_DIRECTOR_API_KEY = "x"
$env:FIDGET_TRACE_FRAMES = "1"
$env:FIDGET_CHARACTERS = Join-Path $root "characters"

$script:proc = Start-FidgetPiped $Bin @() $log $err
$startMs = Get-NowMs
try {
    if (-not (Wait-ForFile 3 $trace)) { Fail "process.log never appeared; see $err" }
    if (-not (Wait-ForUp $err)) { Fail "Fidget never logged overlay:; see $err" }
    $rest = Wait-ForRest $trace
    if (-not $rest) { Fail "the sprite never came to rest; see $trace" }

    $overlay = @(Get-Content -LiteralPath $trace | Where-Object { $_ -match 'sprite (\d+)x\d+' })
    $sizeLine = $overlay | Select-Object -First 1
    if (-not ($sizeLine -match 'sprite (\d+)x\d+')) { Fail "no sprite size in $trace" }
    $size = [int]$Matches[1]
    $half = [int][Math]::Floor($size / 2)

    $grabbed = $false
    for ($try = 0; $try -lt 4 -and -not $grabbed; $try++) {
        [QuickGrab]::Release()
        $spot = Get-LastSpriteSpot $trace
        if (-not $spot) { Fail "no sprite position in $trace" }
        $cx = $spot.X + $half
        $cy = $spot.Y + $half
        $farY = $spot.Y - (2 * $size)
        if ($farY -lt 0) { $farY = $spot.Y + (2 * $size) }
        $farX = $spot.X
        [QuickGrab]::Move($farX, $farY)
        $frames = @(Select-String -LiteralPath $trace -Pattern $framePattern -CaseSensitive).Count
        if (-not (Wait-ForCount 3 $trace $framePattern $frames)) {
            Fail "no tick after the cursor left the sprite; see $trace"
        }
        $spot = Get-LastSpriteSpot $trace
        if (-not $spot) { Fail "no sprite position in $trace" }
        $cx = $spot.X + $half
        $cy = $spot.Y + $half
        $farY = $spot.Y - (2 * $size)
        if ($farY -lt 0) { $farY = $spot.Y + (2 * $size) }

        $clicks = @(Select-String -LiteralPath $trace -Pattern 'click: down ' -CaseSensitive).Count
        $grabs = @(Select-String -LiteralPath $trace -Pattern $grabPattern -CaseSensitive).Count
        [QuickGrab]::PressAndLeave($cx, $cy, $farX, $farY)
        if (-not (Wait-ForCount 3 $trace 'click: down ' $clicks)) { continue }
        $click = Last-ClickDown $trace
        if (-not $click) { continue }
        $pressOnSprite = Near $click.PressX $click.PressY $cx $cy 8
        $cursorLeft = -not (Near $click.CursorX $click.CursorY $cx $cy $half)
        if (-not $pressOnSprite) {
            Fail "press hit-tested at ($($click.PressX),$($click.PressY)), not the sprite at ($cx,$cy); cursor ($($click.CursorX),$($click.CursorY)); see $trace"
        }
        if (-not $cursorLeft) { continue }
        if (-not (Wait-ForCount 3 $trace $grabPattern $grabs)) {
            Fail "press at ($($click.PressX),$($click.PressY)) with the cursor at ($($click.CursorX),$($click.CursorY)) did not start a Grab; see $trace"
        }
        $grabbed = $true
        Write-Output "ok: press ($($click.PressX),$($click.PressY)) on the sprite, cursor already ($($click.CursorX),$($click.CursorY))"
    }
    if (-not $grabbed) {
        Fail "every press was sampled while the cursor was still on the sprite; see $trace"
    }
} finally {
    [QuickGrab]::Release()
    Stop-FidgetTree $script:proc
}

$elapsedMs = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() - $startMs
Write-Output "PASS: evidence in $out (${elapsedMs}ms)"
