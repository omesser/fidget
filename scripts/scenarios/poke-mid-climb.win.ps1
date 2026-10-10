#!/usr/bin/env pwsh
# Scenario: poke-mid-climb (Windows)
# On screen: launches Fidget with the Static Director. The sprite is dragged
#   past a display's side edge and let go, so it climbs that wall, and is
#   poked while it climbs. Fidget quits when the scenario ends. No screenshots.
# Input: a real drag that carries the sprite over the edge named by
#   FIDGET_SCENARIO_EDGE (left or right, default left), then clicks on it
#   until one lands as a Poke. The cursor moves; keep hands off the mouse for
#   the run.
# Duration: about 20 s, 1 min at most.
# Grants: a desktop session.
# Asserts: the Poke starts react over, then the sprite stays Climbing at one
#   position in one climb frame through the cooldown, and climbs on after it.
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

$root = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$script:Evidence = $null

function Fail([string]$Message) {
    [Console]::Error.WriteLine("FAIL: $Message")
    if ($script:Evidence) { [Console]::Error.WriteLine("evidence in $($script:Evidence)") }
    exit 1
}

# process.log puts epoch seconds before each line; a saved stderr log does not.
$framePattern = '^(?:\d+ )?frame: (\d+) (\w+) pos\((-?\d+),(-?\d+)\) \S+ (\S+)#(\d+) '
$pokePattern = '^(?:\d+ )?verbs:.*Poke'

# poke-mid-climb-check.py in PowerShell, so no Windows host needs Python. Case
# sensitive and word for word like it: the same broken fixtures fail both.
function Check-Trace([string]$File) {
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
    if ($null -eq $pokeAt) { Fail "no Poke landed on a climbing sprite" }
    $after = @($frames | Select-Object -Skip $pokeAt)
    # POKE_COOLDOWN_MS is 2500; the windows leave a tick either side of it.
    $span = if ($after.Count -gt 0) { $after[-1].At - $after[0].At } else { 0 }
    if ($span -lt 2300) {
        $seconds = ($span / 1000).ToString("0.0", [Globalization.CultureInfo]::InvariantCulture)
        Fail "the trace stops $seconds s after the Poke"
    }
    $start = $after[0].At
    $paused = @($after | Where-Object { $_.At - $start -lt 2300 })
    $resumed = @($after | Where-Object { $_.At - $start -ge 2600 -and $_.At - $start -le 3500 })
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
$homeDir = Join-Path $out "home"
New-Item -ItemType Directory -Force -Path (Join-Path $homeDir "AppData\Roaming\fidget") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $homeDir "AppData\Local") | Out-Null
$utf8 = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllText((Join-Path $homeDir "AppData\Roaming\fidget\settings.json"), '{"hide_in_fullscreen": false, "first_run_tour_shown": true}', $utf8)

Add-Type -AssemblyName System.Windows.Forms

$preflightCode = @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class PreflightCheck {
    public delegate bool Callback(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(Callback cb, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr hWnd, int index);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr hWnd, StringBuilder lp, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr hWnd, StringBuilder lp, int n);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr hwnd, int attr, out int value, int size);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
    public const int GWL_STYLE = -16;
    public const int GWL_EXSTYLE = -20;
    public const int WS_VISIBLE = 0x10000000;
    public const int WS_EX_TOOLWINDOW = 0x00000080;
    public const int DWMWA_CLOAKED = 14;
}
"@
Add-Type -TypeDefinition $preflightCode

function Get-FrontmostFullscreenApp {
    $screens = [System.Windows.Forms.Screen]::AllScreens
    $state = @{ Hit = $null }
    [PreflightCheck]::EnumWindows({
        param($hwnd, $lParam)
        if ($null -eq $state.Hit) {
            $visible = [PreflightCheck]::IsWindowVisible($hwnd)
            if (-not $visible) { return $true }
            $style = [PreflightCheck]::GetWindowLong($hwnd, [PreflightCheck]::GWL_STYLE)
            if (($style -band [PreflightCheck]::WS_VISIBLE) -eq 0) { return $true }
            $exStyle = [PreflightCheck]::GetWindowLong($hwnd, [PreflightCheck]::GWL_EXSTYLE)
            if (($exStyle -band [PreflightCheck]::WS_EX_TOOLWINDOW) -ne 0) { return $true }
            $cloaked = 0
            $hr = [PreflightCheck]::DwmGetWindowAttribute($hwnd, [PreflightCheck]::DWMWA_CLOAKED, [ref]$cloaked, [System.Runtime.InteropServices.Marshal]::SizeOf([type][int]))
            if ($hr -eq 0 -and $cloaked -ne 0) { return $true }
            $rect = New-Object PreflightCheck+RECT
            if (-not [PreflightCheck]::GetWindowRect($hwnd, [ref]$rect)) { return $true }
            $width = $rect.Right - $rect.Left
            $height = $rect.Bottom - $rect.Top
            if ($width -le 0 -or $height -le 0) { return $true }
            foreach ($screen in $screens) {
                $sb = $screen.Bounds
                if ([Math]::Abs($rect.Left - $sb.Left) -le 1 -and [Math]::Abs($rect.Top - $sb.Top) -le 1 -and [Math]::Abs($width - $sb.Width) -le 1 -and [Math]::Abs($height - $sb.Height) -le 1) {
                    $cls = New-Object System.Text.StringBuilder 256
                    [PreflightCheck]::GetClassName($hwnd, $cls, $cls.Capacity) | Out-Null
                    $title = New-Object System.Text.StringBuilder 512
                    [PreflightCheck]::GetWindowText($hwnd, $title, $title.Capacity) | Out-Null
                    $state.Hit = @{Class=$cls.ToString(); Title=$title.ToString()}
                    return $false
                }
            }
        }
        return $true
    }, [IntPtr]::Zero) | Out-Null
    return $state.Hit
}

$fullscreenApp = Get-FrontmostFullscreenApp
if ($fullscreenApp) {
    $preflightFile = Join-Path $out "preflight.txt"
    Set-Content -LiteralPath $preflightFile -Value "Class: $($fullscreenApp.Class)`nTitle: $($fullscreenApp.Title)" -Encoding utf8
    [Console]::Error.WriteLine("SKIP: fullscreen app '$($fullscreenApp.Title)' is frontmost; close or minimize it and re-run")
    [Console]::Error.WriteLine("evidence in $out")
    exit 2
}

$log = Join-Path $out "app.log"
$err = Join-Path $out "app.err"
$inputLog = Join-Path $out "input.txt"
$trace = Join-Path $homeDir "AppData\Roaming\fidget\process.log"

$env:HOME = $homeDir
$env:USERPROFILE = $homeDir
$env:APPDATA = Join-Path $homeDir "AppData\Roaming"
$env:FIDGET_DIRECTOR_API_KEY = "x"
$env:FIDGET_TRACE_FRAMES = "1"
$env:FIDGET_CHARACTERS = Join-Path $root "characters"

# In-process, not ax-window-win.ps1 click: a new PowerShell compiles its
# Add-Type before the click, and a climbing sprite has moved its own height by
# then. Timings are click-cursor.swift's, and X11's for the drag.
Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Threading;
public class ClimbInput {
    [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);
    const uint Down = 0x0002, Up = 0x0004;
    public static void Place(int x, int y, int toX) {
        SetCursorPos(x, y);
        Thread.Sleep(120);
        mouse_event(Down, 0, 0, 0, UIntPtr.Zero);
        Thread.Sleep(250);
        for (int step = 1; step <= 10; step++) {
            SetCursorPos(x + (toX - x) * step / 10, y);
            Thread.Sleep(30);
        }
        Thread.Sleep(300);
        mouse_event(Up, 0, 0, 0, UIntPtr.Zero);
    }
    public static void Move(int x, int y) {
        SetCursorPos(x, y);
    }
    public static void Click(int x, int y) {
        SetCursorPos(x, y);
        Thread.Sleep(120);
        mouse_event(Down, 0, 0, 0, UIntPtr.Zero);
        Thread.Sleep(60);
        mouse_event(Up, 0, 0, 0, UIntPtr.Zero);
    }
}
"@

# The newest frame's state and the sprite's top-left, logged after pos(...).
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

function Pokes { @(Select-String -LiteralPath $trace -Pattern $pokePattern -CaseSensitive).Count }

# A tall-display climb can outlast the grounded rest budget. This is the wall-clock stop.
$RestCeilingMs = 90000

# Falling and Climbing do not spend the rest budget; the ceiling stops a stuck sprite.
function Get-RestDecision([string]$State, [bool]$Still, [int]$RestMs, [int]$WallMs, [int]$BudgetMs) {
    if ($WallMs -ge $RestCeilingMs) { return "give-up" }
    if ($State -cmatch '^(Falling|Climbing)$') { return "wait" }
    if ($Still -and $State -cmatch '^(Grounded|Perched)$') { return "done" }
    if ($RestMs -ge $BudgetMs) { return "give-up" }
    "wait"
}

# Resting and not walking: a press on a walking sprite lands where it was, and
# the drag then throws whatever window is underneath.
function Wait-Still([int]$Tenths) {
    $budgetMs = $Tenths * 100
    $restMs = 0
    $wall = [System.Diagnostics.Stopwatch]::StartNew()
    while ($true) {
        if ($proc.HasExited) { Fail "Fidget exited; see $err" }
        $a = Last-Frame
        $state = if ($a) { $a.State } else { "" }
        $still = $false
        $airborne = $state -cmatch '^(Falling|Climbing)$'
        if ($a -and $state -cmatch '^(Grounded|Perched)$') {
            Start-Sleep -Milliseconds 200
            $b = Last-Frame
            if ($b.X -eq $a.X -and $b.Y -eq $a.Y -and $b.State -cmatch '^(Grounded|Perched)$') { $still = $true }
            $restMs += 200
        }
        $decision = Get-RestDecision $state $still $restMs ([int]$wall.ElapsedMilliseconds) $budgetMs
        if ($decision -ceq "done") { return $true }
        if ($decision -ceq "give-up") { return $false }
        Start-Sleep -Milliseconds 100
        if (-not $airborne) { $restMs += 100 }
    }
}

function Wait-Climbing([int]$Tenths) {
    for ($n = $Tenths; $n -gt 0; $n--) {
        if (In-State "Climbing") { return $true }
        Start-Sleep -Milliseconds 100
    }
    $false
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

$proc = Start-Process -FilePath $Bin -RedirectStandardOutput $log -RedirectStandardError $err -PassThru -WindowStyle Normal
try {
    for ($n = 120; $n -gt 0 -and -not (Test-Path -LiteralPath $trace); $n--) { Start-Sleep -Milliseconds 250 }
    if (-not (Test-Path -LiteralPath $trace)) { Fail "process.log never appeared; see $err" }

    if (-not (Wait-Still 300)) { Fail "the sprite never came to rest; see $trace" }
    $overlay = @(Get-Content -LiteralPath $trace | Where-Object { $_ -match '^(?:\d+ )?overlay: ' })
    $sizeLine = $overlay | Where-Object { $_ -match ' sprite (\d+)x\d+;' } | Select-Object -First 1
    if (-not ($sizeLine -match ' sprite (\d+)x\d+;')) { Fail "no sprite size in $trace" }
    $size = [int]$Matches[1]
    $half = [int][Math]::Floor($size / 2)
    # Every display's left and right edge.
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
    # The sprite is held by a point a quarter of its width inboard of its
    # centre, so with the cursor at the edge its centre is already past it: a
    # sprite whose centre is over no display falls to the nearest edge and
    # climbs it, whatever speed the hand had. The release is held still.
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
    [ClimbInput]::Place($grabX, $sy + $half, $targetX)
    Add-Content -LiteralPath $inputLog -Value "placed from ($grabX,$($sy + $half)) to x=$targetX"

    if (-not (Wait-Climbing 20)) { Fail "the sprite let go over the $side edge and did not climb; see $trace" }
    $frame = Last-Frame
    $sx = $frame.X; $sy = $frame.Y
    # A wall climb is centred on the display edge it was let go over.
    $mid = $sx + $half
    if ([Math]::Abs($mid - $edge) -gt 2) { Fail "the sprite climbs at x=$mid, not the $side edge at x=$edge; see $trace" }

    # Only the display's half of the sprite is drawn. Clicks a double-click
    # interval apart are separate Pokes; stop at the first. Aim near the top,
    # since it rises while the click travels, but never above the display.
    # Into the display: right of a left edge, left of a right edge.
    $px = $edge + $into * [int][Math]::Floor($half / 2)
    $before = Pokes
    $poked = $false
    for ($i = 0; $i -lt 6; $i++) {
        $frame = Last-Frame
        if (-not $frame -or $frame.State -cne "Climbing") { break }
        $sy = $frame.Y
        if ($sy -le $size) { break }
        $py = $sy + [int][Math]::Floor($half / 9)
        [ClimbInput]::Click($px, $py)
        Add-Content -LiteralPath $inputLog -Value "clicked ($px,$py)"
        Start-Sleep -Milliseconds $clickGap
        if ((Pokes) -gt $before) {
            $poked = $true
            break
        }
    }
    if (-not $poked) { Fail "no Poke landed mid-climb over the $side edge; see $inputLog" }

    # The quick-message pill holds a climb while the cursor rests on the sprite.
    # Move further into the display than the click landed.
    [ClimbInput]::Move($px + $into * 4 * $size, $sy)

    Start-Sleep -Seconds 4
} finally {
    if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
}

Check-Trace $trace
Write-Output "PASS: evidence in $out"
