#!/usr/bin/env pwsh
# Scenario: possession-displays (Windows)
# On screen: launches Fidget as BMO with a fixture Harness on a desktop with
#   two or more displays. Notepad opens on a scratch file and is brought to the
#   front, taking focus. The scenario right-clicks the sprite, presses Possess,
#   and holds an arrow until BMO walks onto the display beside it. There it
#   walks and jumps BMO, clicks Notepad, clicks the paused card, and presses
#   Escape. Fidget quits when the scenario ends, and so does the Notepad this
#   run started.
# Input: one real right-click on the sprite, a click on Possess, an arrow held
#   for the walk across (about 10 s) and again briefly, Space, a click in
#   Notepad, a click on the paused card, and Escape. Keep hands off the mouse
#   and keyboard for the run.
# Duration: about 25 s, 60 s at most.
# Grants: a desktop session with two or more displays side by side. UI
#   Automation for the terminal that runs it. bash on PATH for the fixture
#   Harness wrapper.
# Asserts: the held arrow walks the possessed sprite across the border and the
#   new display's overlay takes the keyboard; the arrow's keyup lands there and
#   stops the sprite; an arrow and Space drive it on that display; the card
#   follows it there; a click away pauses it and a click on the card resumes it
#   on that display; Escape releases possession. Fails before a click or key
#   whose target is not ready: another window covers Notepad's centre,
#   click-through stays on over the card, or the Shell never raises the overlay.
# Skips (exit 2) when Notepad is already open, when no display sits beside the
#   sprite's display at its height, or when a fullscreen window would take the
#   right-click.
#
# Usage: possession-displays.win.ps1 --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
# No param() block: Windows PowerShell binds `--go` as -Go and swallows the
# next token, so $Go never equals "--go" and the leaf always exits 2.
$Go = if ($args.Count -gt 0) { $args[0] } else { "" }
$Bin = if ($args.Count -gt 1) { $args[1] } else { "" }
$TestBin = if ($args.Count -gt 2) { $args[2] } else { "" }
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
if (-not $Bin -or -not $TestBin) {
    Write-Error "usage: possession-displays.win.ps1 --go <fidget binary> <fidget test binary>"
    exit 1
}

function Fail([string]$Message) {
    [Console]::Error.WriteLine("FAIL: $Message")
    if ($script:Evidence) { [Console]::Error.WriteLine("evidence in $($script:Evidence)") }
    exit 1
}

function Skip([string]$Message) {
    [Console]::Error.WriteLine("SKIP: $Message")
    exit 2
}

$root = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
. (Join-Path $PSScriptRoot "..\helpers\wait-helpers.win.ps1")
. (Join-Path $PSScriptRoot "..\helpers\bash-setup-win.ps1")
if (-not $bashPath) { Skip "bash not found (tried Git Bash, then PATH)" }
if (@(Get-Process -Name Notepad -ErrorAction SilentlyContinue).Count -gt 0) {
    Skip "Notepad is running. Quit it first: this run opens and closes its own."
}

$TestBin = (Resolve-Path -LiteralPath $TestBin).Path
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$out = Join-Path $env:TEMP "fidget-scenario-possession-displays-$stamp"
$script:Evidence = $out
$data = Initialize-ScenarioData $out @{ hide_in_fullscreen = $false }
$log = Join-Path $out "app.log"
$err = Join-Path $out "app.err"
$marks = Join-Path $out "harness.log"
$doc = Join-Path $out "possession-displays-letter.txt"
Set-Content -LiteralPath $marks -Value "" -Encoding ascii
[System.IO.File]::WriteAllText($doc, "mark ")

$rootBash = $root -replace '\\', '/'
$testBinBash = $TestBin -replace '\\', '/'
$marksBash = $marks -replace '\\', '/'
$harness = "$bashPath $rootBash/scripts/scenarios/helpers/fixture-harness.sh $testBinBash script=nop count=$marksBash"
$paths = @($harness -split '\s+' | Select-Object -Skip 1)
if ($paths.Count -ne 4) { Fail "a path in the Harness line holds a space: $harness" }

$env:FIDGET_HARNESS = $harness
$env:FIDGET_DIRECTOR_WAKE_SECS = "3600"
$env:FIDGET_DIRECTOR_API_KEY = "x"
$env:FIDGET_CAPTURABLE = "1"
$env:FIDGET_TRACE_FRAMES = "1"
$env:FIDGET_TRACE_HITTEST = "1"
$env:FIDGET_TRACE_BUBBLE = "1"
$env:FIDGET_CHARACTER = "bmo"
# Greet chains to patrol, which would walk BMO off the spot the menu click aims
# at. Every ambient weight but fidget goes to 0; the walk animation stays.
$chars = Join-Path $out "characters"
New-Item -ItemType Directory -Force -Path $chars | Out-Null
Copy-Item -Recurse (Join-Path (Join-Path $root "characters") "bmo") $chars
$manifest = Join-Path (Join-Path $chars "bmo") "character.manifest"
$section = ""
$still = foreach ($line in @(Get-Content -LiteralPath $manifest)) {
    if ($line.StartsWith("[")) { $section = $line }
    if ($line.StartsWith("weight = ") -and $section.StartsWith("[behaviors.") -and ($section -ne "[behaviors.fidget]")) { "weight = 0" } else { $line }
}
$utf8 = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllLines($manifest, [string[]]$still, $utf8)
$env:FIDGET_CHARACTERS = $chars

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class PossessionDisplaysInput {
    public delegate bool Callback(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool EnumWindows(Callback cb, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int count);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr hWnd, int attr, out int value, int size);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] static extern bool BringWindowToTop(IntPtr h);
    [DllImport("user32.dll")] static extern bool AttachThreadInput(uint attach, uint to, bool on);
    [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] static extern bool IsGUIThread(bool convert);
    [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr h, uint flags);
    [StructLayout(LayoutKind.Sequential)] struct POINT { public int X; public int Y; }
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
    [StructLayout(LayoutKind.Sequential)] struct KEYBDINPUT { public ushort vk; public ushort scan; public uint flags; public uint time; public IntPtr extra; }
    [StructLayout(LayoutKind.Sequential)] struct MOUSEINPUT { public int dx; public int dy; public uint data; public uint flags; public uint time; public IntPtr extra; }
    [StructLayout(LayoutKind.Explicit)] struct INPUT { [FieldOffset(0)] public uint type; [FieldOffset(8)] public KEYBDINPUT k; [FieldOffset(8)] public MOUSEINPUT m; }
    [DllImport("user32.dll", SetLastError = true)] static extern uint SendInput(uint n, INPUT[] i, int size);
    public static IntPtr RootAt(int x, int y) { var p = new POINT(); p.X = x; p.Y = y; return GetAncestor(WindowFromPoint(p), 2); }
    // Windows refuses the foreground to a background script, so for the call
    // this thread's input joins the foreground thread's.
    public static bool Bring(IntPtr h) {
        if (SetForegroundWindow(h) && GetForegroundWindow() == h) return true;
        IsGUIThread(true);
        uint unused;
        uint front = GetWindowThreadProcessId(GetForegroundWindow(), out unused);
        uint me = GetCurrentThreadId();
        bool joined = front != 0 && front != me && AttachThreadInput(me, front, true);
        try { BringWindowToTop(h); SetForegroundWindow(h); }
        finally { if (joined) AttachThreadInput(me, front, false); }
        return GetForegroundWindow() == h;
    }
    static IntPtr found;
    static string wantTitle;
    static uint wantPid;
    static int wantLeft, wantTop;
    static Callback titledCb, overlayCb;
    static bool Shown(IntPtr h) {
        int cloaked;
        // 14 is DWMWA_CLOAKED. A cloaked window still reports visible.
        return IsWindowVisible(h) && !(DwmGetWindowAttribute(h, 14, out cloaked, 4) == 0 && cloaked != 0);
    }
    static string Title(IntPtr h) { var t = new StringBuilder(512); GetWindowText(h, t, t.Capacity); return t.ToString(); }
    static bool OnTitled(IntPtr h, IntPtr l) {
        if (!Shown(h) || Title(h).IndexOf(wantTitle, StringComparison.Ordinal) < 0) return true;
        found = h;
        return false;
    }
    public static IntPtr FindTitled(string titlePart) {
        found = IntPtr.Zero;
        if (string.IsNullOrEmpty(titlePart)) return found;
        wantTitle = titlePart;
        if (titledCb == null) titledCb = OnTitled;
        EnumWindows(titledCb, IntPtr.Zero);
        return found;
    }
    // Every overlay is titled Fidget, so the one covering a display is told
    // apart by its top-left corner.
    static bool OnOverlay(IntPtr h, IntPtr l) {
        uint pid;
        GetWindowThreadProcessId(h, out pid);
        if (pid != wantPid || Title(h) != "Fidget") return true;
        RECT r;
        if (!GetWindowRect(h, out r) || r.Left != wantLeft || r.Top != wantTop) return true;
        found = h;
        return false;
    }
    public static IntPtr FindOverlay(uint pid, int left, int top) {
        found = IntPtr.Zero;
        wantPid = pid; wantLeft = left; wantTop = top;
        if (overlayCb == null) overlayCb = OnOverlay;
        EnumWindows(overlayCb, IntPtr.Zero);
        return found;
    }
    static void Send(INPUT[] all) {
        uint sent = SendInput((uint)all.Length, all, Marshal.SizeOf(typeof(INPUT)));
        if (sent != all.Length) throw new InvalidOperationException("SendInput sent " + sent + " of " + all.Length);
    }
    public static void Key(ushort vk, bool up) {
        var i = new INPUT();
        i.type = 1;
        i.k.vk = vk;
        i.k.flags = up ? 2u : 0u;
        Send(new INPUT[] { i });
    }
    public static void Click() {
        var down = new INPUT();
        down.type = 0;
        down.m.flags = 2;
        var up = new INPUT();
        up.type = 0;
        up.m.flags = 4;
        Send(new INPUT[] { down, up });
    }
}
"@

$VK_LEFT = 0x25
$VK_RIGHT = 0x27
$VK_SPACE = 0x20
$VK_ESCAPE = 0x1B

function Release-Keys {
    if (-not ("PossessionDisplaysInput" -as [type])) { return }
    foreach ($vk in @($VK_LEFT, $VK_RIGHT, $VK_SPACE)) {
        try { [PossessionDisplaysInput]::Key($vk, $true) } catch { }
    }
}

function Owner-Of([IntPtr]$H) {
    $owner = [uint32]0
    [void][PossessionDisplaysInput]::GetWindowThreadProcessId($H, [ref]$owner)
    $owner
}

function Count-Lines([string]$Pattern) {
    @(Get-Content -LiteralPath $script:traceFile | Where-Object { $_ -match $Pattern }).Count
}

# Frames traced after the first $From lines, as sprite x and y and whether
# the line walks, jumps or falls.
function Get-FramesFrom([int]$From) {
    $lines = @(Get-Content -LiteralPath $script:traceFile)
    for ($i = $From; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -notmatch '^\d+ frame: .* sprite\((-?\d+),(-?\d+)\)') { continue }
        @{
            X = [int]$Matches[1]; Y = [int]$Matches[2]
            Walk = $lines[$i] -match ' walk#'
            Air = $lines[$i] -match ' Falling | jump#'
        }
    }
}

function Get-LineCount { @(Get-Content -LiteralPath $script:traceFile).Count }

function Get-Displays {
    foreach ($line in @(Get-Content -LiteralPath $script:traceFile)) {
        if ($line -match '^\d+ overlay: (overlay-\d+) covers (\d+)x(\d+) at \((-?\d+),(-?\d+)\)') {
            @{ Label = $Matches[1]; W = [int]$Matches[2]; H = [int]$Matches[3]; X = [int]$Matches[4]; Y = [int]$Matches[5] }
        }
    }
}

# The card's focus button on one overlay, through that overlay's own window:
# a dump by title alone reads whichever overlay comes first. With -Paused, only
# once the Paused heading shows, since the live card puts its button elsewhere.
function Get-FocusButton([IntPtr]$Overlay, [switch]$Paused) {
    try {
        $rootEl = [System.Windows.Automation.AutomationElement]::FromHandle($Overlay)
        $named = { param($Name) New-Object System.Windows.Automation.PropertyCondition ([System.Windows.Automation.AutomationElement]::NameProperty), $Name }
        $scope = [System.Windows.Automation.TreeScope]::Descendants
        if ($Paused -and -not $rootEl.FindFirst($scope, (& $named "Paused"))) { return $null }
        $button = $rootEl.FindFirst($scope, (& $named "Click to focus"))
        if (-not $button) { return $null }
        $r = $button.Current.BoundingRectangle
        if ($r.Width -le 0 -or $r.Height -le 0) { return $null }
        return @{ X = [int]($r.X + $r.Width / 2); Y = [int]($r.Y + $r.Height / 2) }
    } catch {
        return $null
    }
}

function Test-Inside($Point, $Display) {
    $Point.X -ge $Display.X -and $Point.X -lt ($Display.X + $Display.W) -and
    $Point.Y -ge $Display.Y -and $Point.Y -lt ($Display.Y + $Display.H)
}

# Hold an arrow until a frame after the press has moved the sprite 8 px its
# way, then let go and wait for rest.
function Step-Arrow([int]$Vk, [int]$Sign, [string]$What) {
    $from = Get-LineCount
    $start = Get-LastSpriteSpot $script:traceFile
    [PossessionDisplaysInput]::Key($Vk, $false)
    $moved = Wait-OnChanges 3 $data {
        foreach ($f in @(Get-FramesFrom $from)) {
            if ($f.Walk -and (($f.X - $start.X) * $Sign) -ge 8) { return $true }
        }
        return $false
    }
    [PossessionDisplaysInput]::Key($Vk, $true)
    if (-not $moved) { Fail "$What did not walk the sprite; see $script:traceFile" }
    if (-not (Wait-Until 3 { Get-SpriteAtRest $script:traceFile })) { Fail "the sprite kept walking after $What came up; see $script:traceFile" }
}

. (Join-Path $PSScriptRoot "..\helpers\fullscreen-over-click.win.ps1")
$script:notepad = $null
$script:launched = $null
$script:proc = $null
$startMs = Get-NowMs
try {
    $script:launched = Start-Process -FilePath notepad.exe -ArgumentList $doc -PassThru
    $proc = Start-FidgetPiped $Bin @() $log $err
    $script:proc = $proc
    # Windows 11 Store Notepad is two processes. Start-Process's pid owns no
    # window; the one titled with this file does.
    $docName = Split-Path -Leaf $doc
    $found = Wait-UiaChange 0 3 {
        $h = [PossessionDisplaysInput]::FindTitled($docName)
        if ($h -eq [IntPtr]::Zero) { return $null }
        return $h
    }
    if (-not $found) { Fail "Notepad did not open its window; see $out" }
    $pad = [IntPtr]$found
    $script:notepad = Get-Process -Id (Owner-Of $pad)
    if ($script:notepad.ProcessName -ne "Notepad") {
        $ownerName = $script:notepad.ProcessName
        $script:notepad = $null
        Fail "the $docName window belongs to $ownerName, not Notepad"
    }
    # Notepad can open behind another app's window, and the click away has to land on it.
    [void][PossessionDisplaysInput]::Bring($pad)

    if (-not (Wait-ForUp $err)) { Fail "Fidget never wrote its overlay line; see $err" }
    $script:traceFile = Join-Path $data "process.log"
    $traceFile = $script:traceFile
    if (-not (Wait-ForFile 3 $traceFile)) { Fail "process.log never appeared; see $err" }
    if (-not (Wait-ForTrace 3 $traceFile '^\d+ frame: .* sprite\(')) { Fail "Fidget traced no frame; see $traceFile" }
    $rest = Wait-ForRest $traceFile
    if (-not $rest) { Fail "the sprite never came to rest within 10s; see $traceFile" }
    $size = Get-Content -LiteralPath $traceFile | Select-String -Pattern 'sprite (\d+)x(\d+);' | Select-Object -First 1
    if (-not $size) { Fail "no sprite size in $traceFile" }
    $w = [int]$size.Matches[0].Groups[1].Value
    $h = [int]$size.Matches[0].Groups[2].Value
    $cx = $rest.X + [int]($w / 2)
    $cy = $rest.Y + [int]($h / 2)
    Write-Output "ok: sprite at ($($rest.X),$($rest.Y)) size ${w}x${h}, centre ($cx,$cy)"

    $displays = @(Get-Displays)
    $here = $displays | Where-Object { Test-Inside @{ X = $cx; Y = $cy } $_ } | Select-Object -First 1
    if (-not $here) { Fail "no overlay covers the sprite's centre; see $traceFile" }
    $next = $null
    $sign = 0
    foreach ($d in $displays) {
        if ($d.Label -eq $here.Label -or $cy -lt $d.Y -or $cy -ge ($d.Y + $d.H)) { continue }
        if (($d.X + $d.W) -eq $here.X) { $next = $d; $sign = -1; break }
        if ($d.X -eq ($here.X + $here.W)) { $next = $d; $sign = 1; break }
    }
    if (-not $next) { Skip "no display sits beside $($here.Label) at the sprite's height ($($displays.Count) display(s))" }
    $arrow = if ($sign -lt 0) { $VK_LEFT } else { $VK_RIGHT }
    $arrowName = if ($sign -lt 0) { "left" } else { "right" }
    $border = if ($sign -lt 0) { $here.X } else { $here.X + $here.W }
    Write-Output "ok: $($next.Label) at ($($next.X),$($next.Y)) $($next.W)x$($next.H) is to the $arrowName of $($here.Label)"

    Skip-IfFullscreenOverClick $cx $cy $proc.Id $out "right-click"
    if (-not (Invoke-AxWin (Join-Path $out "menu.txt") @{
                Command = "menu"; ProcessId = $proc.Id; TraceFile = $traceFile
                SpriteW = $w; SpriteH = $h; Arg3 = "Possess"
            })) {
        Fail "could not press Possess; see $out\menu.txt"
    }
    if (-not (Wait-ForTrace 3 $traceFile '^\d+ menu: possession on')) {
        Fail "the Possess row did not possess the sprite; see $traceFile"
    }
    # The menu's own focus line comes before possession. The Shell raises the
    # overlay once more after it, and a key sent before that raise is lost.
    # The success form only: a refused raise logs the same line with -> 0.
    $raisedLine = '^\d+ overlay: pill open SetForegroundWindow\(0x[0-9a-f]+\) -> 1 '
    $raisedAfterPossess = {
        $seen = $false
        foreach ($line in @(Get-Content -LiteralPath $traceFile)) {
            if ($line -match '^\d+ menu: possession on') { $seen = $true }
            elseif ($seen -and $line -match $raisedLine) { return $true }
        }
        return $false
    }
    if (-not (Wait-OnChanges 3 $data $raisedAfterPossess)) {
        Fail "the possessed overlay never took the keyboard; see $traceFile"
    }
    Write-Output "ok: possessed, and $($here.Label) has the keyboard"

    $nextFocus = "^\d+ overlay $($next.Label): window focus"
    $focusBefore = Count-Lines $nextFocus
    $from = Get-LineCount
    # Budget the walk at 100 px/s, under the ~175 px/s BMO walks.
    $distance = if ($sign -lt 0) { $rest.X + $w - $border } else { $border - $rest.X }
    $walkSeconds = [Math]::Ceiling($distance / 100) + 3
    [PossessionDisplaysInput]::Key($arrow, $false)
    $crossed = Wait-OnChanges $walkSeconds $data {
        foreach ($f in @(Get-FramesFrom $from)) {
            if (($sign -lt 0 -and ($f.X + $w) -le $border) -or ($sign -gt 0 -and $f.X -ge $border)) { return $true }
        }
        return $false
    }
    if (-not $crossed) {
        [PossessionDisplaysInput]::Key($arrow, $true)
        Fail "the held $arrowName arrow did not carry the sprite past x=$border within ${walkSeconds}s; see $traceFile"
    }
    if (-not (Wait-ForCount 3 $traceFile $nextFocus $focusBefore)) {
        [PossessionDisplaysInput]::Key($arrow, $true)
        Fail "$($next.Label) never took the keyboard after the sprite crossed; see $traceFile"
    }
    [PossessionDisplaysInput]::Key($arrow, $true)
    if (-not (Wait-ForTrace 3 $traceFile "^\d+ possession: key $($next.Label) $arrowName up")) {
        Fail "the $arrowName keyup never reached $($next.Label); see $traceFile"
    }
    $landed = $null
    if (-not (Wait-Until 3 { ($script:landed = Get-SpriteAtRest $traceFile) })) {
        Fail "the sprite kept walking after the $arrowName keyup; see $traceFile"
    }
    $landed = $script:landed
    if (-not (Test-Inside @{ X = $landed.X + [int]($w / 2); Y = $landed.Y + [int]($h / 2) } $next)) {
        Fail "the sprite came to rest at ($($landed.X),$($landed.Y)), off $($next.Label); see $traceFile"
    }
    Write-Output "ok: walked onto $($next.Label), which took the keyboard, and stopped on the keyup at ($($landed.X),$($landed.Y))"

    Step-Arrow $arrow $sign "the $arrowName arrow on $($next.Label)"
    if (-not (Wait-ForTrace 3 $traceFile "^\d+ possession: key $($next.Label) $arrowName down")) {
        Fail "the $arrowName arrow never reached $($next.Label); see $traceFile"
    }
    # Space charges while held and jumps on its keyup.
    [PossessionDisplaysInput]::Key($VK_SPACE, $false)
    if (-not (Wait-ForTrace 3 $traceFile "^\d+ possession: key $($next.Label) space down")) {
        [PossessionDisplaysInput]::Key($VK_SPACE, $true)
        Fail "Space never reached $($next.Label); see $traceFile"
    }
    $from = Get-LineCount
    [PossessionDisplaysInput]::Key($VK_SPACE, $true)
    $jumped = Wait-OnChanges 3 $data {
        foreach ($f in @(Get-FramesFrom $from)) { if ($f.Air) { return $true } }
        return $false
    }
    if (-not $jumped) { Fail "Space did not jump the sprite on $($next.Label); see $traceFile" }
    if (-not (Wait-Until 3 { Get-SpriteAtRest $traceFile })) { Fail "the sprite never landed after the jump; see $traceFile" }
    Write-Output "ok: the arrow and Space drive the sprite on $($next.Label)"

    $overlay = [PossessionDisplaysInput]::FindOverlay([uint32]$proc.Id, $next.X, $next.Y)
    if ($overlay -eq [IntPtr]::Zero) { Fail "no Fidget window covers $($next.Label) at ($($next.X),$($next.Y))" }
    $script:button = $null
    [void](Wait-ForUia 3 $proc.Id { ($script:button = Get-FocusButton $overlay) -and (Test-Inside $script:button $next) })
    if (-not $script:button -or -not (Test-Inside $script:button $next)) { Fail "the card did not follow the sprite onto $($next.Label)" }
    Write-Output "ok: the card is on $($next.Label) at ($($script:button.X),$($script:button.Y))"

    $rect = New-Object PossessionDisplaysInput+RECT
    if (-not [PossessionDisplaysInput]::GetWindowRect($pad, [ref]$rect)) { Fail "Notepad has no window to click" }
    $padX = [int](($rect.Left + $rect.Right) / 2)
    $padY = [int](($rect.Top + $rect.Bottom) / 2)
    $cover = [PossessionDisplaysInput]::RootAt($padX, $padY)
    if ((Owner-Of $cover) -ne [uint32]$script:notepad.Id) {
        $coverPid = Owner-Of $cover
        $coverName = (Get-Process -Id $coverPid -ErrorAction SilentlyContinue).ProcessName
        if (-not $coverName) { $coverName = "process $coverPid" }
        Fail "$coverName covers Notepad at ($padX,$padY), so the click would not reach it"
    }
    $blurLine = "^\d+ overlay $($next.Label): window blur"
    $blurs = Count-Lines $blurLine
    [PossessionDisplaysInput]::SetCursorPos($padX, $padY) | Out-Null
    [PossessionDisplaysInput]::Click()
    if (-not (Wait-ForCount 3 $traceFile $blurLine $blurs)) { Fail "$($next.Label) never lost focus to Notepad; see $traceFile" }
    if (-not (Wait-Until 3 { (Owner-Of ([PossessionDisplaysInput]::GetForegroundWindow())) -eq [uint32]$script:notepad.Id })) {
        Fail "Notepad did not take focus"
    }
    Write-Output "ok: the click away gave Notepad the keyboard"

    $script:button = $null
    [void](Wait-ForUia 3 $proc.Id { ($script:button = Get-FocusButton $overlay -Paused) -and (Test-Inside $script:button $next) })
    if (-not $script:button) { Fail "the paused card did not show on $($next.Label)" }
    $card = $script:button
    $focusBefore = Count-Lines $nextFocus
    $raises = Count-Lines $raisedLine
    $offLine = "^\d+ hit-test: .* on overlay \d+ .*click-through OFF"
    $offs = Count-Lines $offLine
    [PossessionDisplaysInput]::SetCursorPos($card.X, $card.Y) | Out-Null
    # A click before click-through turns off falls through the overlay.
    if (-not (Wait-ForCount 3 $traceFile $offLine $offs)) { Fail "click-through stayed on over the paused card; see $traceFile" }
    [PossessionDisplaysInput]::Click()
    if (-not (Wait-ForCount 3 $traceFile $nextFocus $focusBefore)) { Fail "the card click did not give $($next.Label) the keyboard; see $traceFile" }
    # The Shell then raises the overlay again, and a key sent before that is lost.
    if (-not (Wait-ForCount 3 $traceFile $raisedLine $raises)) { Fail "the Shell never raised the overlay after the card click; see $traceFile" }
    Step-Arrow $arrow $sign "the $arrowName arrow after the card click"
    Write-Output "ok: the card click resumed on $($next.Label)"

    [PossessionDisplaysInput]::Key($VK_ESCAPE, $false)
    [PossessionDisplaysInput]::Key($VK_ESCAPE, $true)
    if (-not (Wait-ForTrace 3 $traceFile '^\d+ possession: escape')) { Fail "Escape did not release possession; see $traceFile" }
    Write-Output "PASS: evidence in $out ($((Get-NowMs) - $startMs)ms)"
} finally {
    Release-Keys
    Stop-FidgetTree $script:proc "count=$marksBash"
    # A title match can belong to some other process. Leave that one alone.
    foreach ($held in @($script:notepad, $script:launched)) {
        if (-not $held -or $held.HasExited) { continue }
        $started = $script:launched -and ($held.Id -eq $script:launched.Id)
        if (-not $started -and $held.ProcessName -ne "Notepad") { continue }
        Stop-Process -Id $held.Id -Force -ErrorAction SilentlyContinue
    }
}
