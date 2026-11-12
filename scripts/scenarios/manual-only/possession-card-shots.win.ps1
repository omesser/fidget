#!/usr/bin/env pwsh
# Scenario: possession-card-shots (Windows)
# On screen: launches Fidget as BMO, seats a plain grey window behind the
#   bindings card's corner, possesses BMO through the sprite menu, holds Space
#   for a part-full charge bar, then activates the grey window so the overlay
#   loses focus. Fidget quits when the script ends.
# Input: one right-click on the sprite and a click on Possess, Space held,
#   then the grey window taking focus. No prompts.
# Duration: about 20 s after the build, a few minutes if cargo has to build.
# Grants: a desktop session. UI Automation for the Possess menu.
# Asserts: both card shots are saved only when every margin pixel is the grey
#   backdrop. The charging shot has a part-full bar; the paused shot is the
#   paused card. The crop comes from the card's trace line plus the charge bar,
#   scaled by the display's effective DPI so physical pixels match the capture.
# Skips (exit 2) when a fullscreen window would take the right-click.
#
# Usage: possession-card-shots.win.ps1 [-OutDir <dir>] [--go <fidget binary>]
# Without --go it builds or uses <checkout>/target/debug/fidget.exe. The
# checkout is three levels above this script. --self-test runs the helper
# checks and launches nothing.
# No param() block: Windows PowerShell binds `--go` as -Go and swallows the
# next token.
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

$Bin = ""
$OutDir = ""
$SelfTest = $false
$i = 0
while ($i -lt $args.Count) {
    $a = [string]$args[$i]
    if ($a -ceq "--self-test") { $SelfTest = $true; $i++; continue }
    if ($a -ceq "--go") {
        if ($i + 1 -ge $args.Count) { [Console]::Error.WriteLine("usage: $(Show-Header)"); exit 2 }
        $Bin = [string]$args[$i + 1]
        $i += 2
        if ($i -lt $args.Count -and [string]$args[$i] -notmatch '^-') { $i++ }
        continue
    }
    if ($a -ceq "-OutDir" -or $a -ceq "--out-dir") {
        if ($i + 1 -ge $args.Count) { [Console]::Error.WriteLine("usage: $(Show-Header)"); exit 2 }
        $OutDir = [string]$args[$i + 1]
        $i += 2
        continue
    }
    if ($a -ceq "-h" -or $a -ceq "--help") { Write-Output (Show-Header); exit 2 }
    [Console]::Error.WriteLine("unknown argument: $a")
    [Console]::Error.WriteLine("usage: $(Show-Header)")
    exit 2
}

$root = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
. (Join-Path $PSScriptRoot "..\helpers\possession-card-shots.ps1")

if ($SelfTest) {
    $test = Join-Path $PSScriptRoot "..\helpers\test-possession-card-shots.ps1"
    & $test
    exit $LASTEXITCODE
}

. (Join-Path $PSScriptRoot "..\helpers\wait-helpers.win.ps1")
. (Join-Path $PSScriptRoot "..\helpers\fullscreen-over-click.win.ps1")

function Fail([string]$Message) {
    [Console]::Error.WriteLine("FAIL: $Message")
    if ($script:Evidence) { [Console]::Error.WriteLine("evidence in $($script:Evidence)") }
    Show-ProcessLogHead
    exit 1
}

# The first 60 non-frame process.log lines, so a FAIL names what the sprite
# was doing (held launch fall, page load, first-draw) without the frame flood.
function Show-ProcessLogHead {
    if (-not $script:traceFile -or -not (Test-Path -LiteralPath $script:traceFile)) { return }
    $lines = @(Get-Content -LiteralPath $script:traceFile -ErrorAction SilentlyContinue |
            Where-Object { $_ -notmatch '^\d+\.?\d* frame: ' } |
            Select-Object -First 60)
    [Console]::Error.WriteLine("process.log (first $($lines.Count) non-frame lines):")
    foreach ($line in $lines) { [Console]::Error.WriteLine($line) }
}

# True once the Shell has released the launch-fall hold: the sprite has left
# Falling, the fall animation has advanced past frame 0, or its feet have moved.
function Test-SpriteClockRunning([string]$TraceFile, $Origin) {
    if (-not (Test-Path -LiteralPath $TraceFile)) { return $false }
    $frames = @(Get-Content -LiteralPath $TraceFile -ErrorAction SilentlyContinue |
            Where-Object { $_ -match '^\d+\.?\d* frame: ' } |
            Select-Object -Last 5)
    foreach ($line in $frames) {
        if ($line -match ' frame: ' -and $line -notmatch ' Falling ') { return $true }
        if ($line -match 'fall#(\d+)' -and [int]$Matches[1] -gt 0) { return $true }
    }
    $now = Get-LastSpriteSpot $TraceFile
    return [bool]($Origin -and $now -and ($now.X -ne $Origin.X -or $now.Y -ne $Origin.Y))
}

function Note([string]$Message) {
    $line = "{0} possession-card-shots: {1}" -f (Get-Date -Format "HH:mm:ss.fff"), $Message
    if ($script:RunLog) { Add-Content -LiteralPath $script:RunLog -Value $line -Encoding utf8 }
    Write-Host $line
}

function Get-StillManifest([string[]]$Lines) {
    $section = ""
    foreach ($line in $Lines) {
        if ($line.StartsWith("[")) { $section = $line }
        if ($line.StartsWith("weight = ") -and $section.StartsWith("[behaviors.") -and ($section -ne "[behaviors.fidget]")) {
            "weight = 0"
        } else {
            $line
        }
    }
}

if (-not $OutDir) { $OutDir = Join-Path $root ".verify\possession-card-shots" }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$script:RunLog = Join-Path $OutDir "run.log"
Set-Content -LiteralPath $script:RunLog -Value "" -Encoding utf8

if (-not $Bin) {
    $Bin = Join-Path $root "target\debug\fidget.exe"
    if (-not (Test-Path -LiteralPath $Bin)) {
        Note "cargo build -p fidget"
        Push-Location $root
        $eap = $ErrorActionPreference
        try {
            $ErrorActionPreference = "Continue"
            & cargo build -p fidget --manifest-path (Join-Path $root "Cargo.toml")
        } finally {
            $ErrorActionPreference = $eap
            Pop-Location
        }
        if ($LASTEXITCODE -ne 0) { Fail "cargo build failed with exit $LASTEXITCODE" }
    }
}
if (-not (Test-Path -LiteralPath $Bin)) { Fail "no build at $Bin" }
$Bin = (Resolve-Path -LiteralPath $Bin).Path

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
if (-not ("GreyBackdrop" -as [type])) {
Add-Type -ReferencedAssemblies System.Windows.Forms, System.Drawing @"
using System;
using System.Drawing;
using System.Text;
using System.Runtime.InteropServices;
using System.Windows.Forms;
public class GreyBackdrop : Form {
    protected override bool ShowWithoutActivation { get { return true; } }
    public GreyBackdrop() {
        FormBorderStyle = FormBorderStyle.None;
        StartPosition = FormStartPosition.Manual;
        TopMost = true;
        ShowInTaskbar = false;
        ControlBox = false;
        MinimizeBox = false;
        MaximizeBox = false;
    }
}
public static class PossessionCardShotsWin {
    public const uint SWP_NOSIZE = 0x0001;
    public const uint SWP_NOMOVE = 0x0002;
    public const uint SWP_NOACTIVATE = 0x0010;
    [DllImport("user32.dll")] public static extern IntPtr SetProcessDpiAwarenessContext(IntPtr v);
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr v);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern bool BringWindowToTop(IntPtr h);
    [DllImport("user32.dll")] static extern bool AttachThreadInput(uint attach, uint to, bool on);
    [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] static extern bool IsGUIThread(bool convert);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] static extern bool EnumWindows(Callback cb, IntPtr l);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder t, int n);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern IntPtr MonitorFromPoint(POINT p, uint flags);
    [DllImport("user32.dll")] static extern IntPtr MonitorFromWindow(IntPtr h, uint flags);
    [DllImport("shcore.dll")] static extern int GetDpiForMonitor(IntPtr m, int type, out uint x, out uint y);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int attr, out int value, int size);
    [DllImport("user32.dll", SetLastError = true)] static extern uint SendInput(uint n, INPUT[] i, int size);
    public delegate bool Callback(IntPtr h, IntPtr l);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] struct POINT { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] struct KEYBDINPUT { public ushort vk, scan; public uint flags, time; public IntPtr extra; }
    [StructLayout(LayoutKind.Explicit)] struct INPUT { [FieldOffset(0)] public uint type; [FieldOffset(8)] public KEYBDINPUT k; }
    static bool Shown(IntPtr h) {
        int cloaked;
        return IsWindowVisible(h) && !(DwmGetWindowAttribute(h, 14, out cloaked, 4) == 0 && cloaked != 0);
    }
    static string Title(IntPtr h) { var t = new StringBuilder(512); GetWindowText(h, t, t.Capacity); return t.ToString(); }
    static IntPtr found;
    static uint wantPid;
    static int wantLeft, wantTop;
    static bool wantExact;
    static Callback overlayCb;
    static bool OnOverlay(IntPtr h, IntPtr l) {
        uint pid;
        GetWindowThreadProcessId(h, out pid);
        if (pid != wantPid || Title(h) != "Fidget" || !Shown(h)) return true;
        RECT r;
        if (!GetWindowRect(h, out r)) return true;
        if (wantExact) {
            double scale = DpiOfMonitor(MonitorFromPoint(new POINT { X = r.Left, Y = r.Top }, 2)) / 96.0;
            int left = (int)Math.Round(wantLeft * scale);
            int top = (int)Math.Round(wantTop * scale);
            if (r.Left != left || r.Top != top) return true;
        }
        found = h;
        return false;
    }
    static uint DpiOfMonitor(IntPtr m) {
        uint x, y;
        if (m == IntPtr.Zero || GetDpiForMonitor(m, 0, out x, out y) != 0 || x == 0) return 96;
        return x;
    }
    public static uint DpiAt(int x, int y) {
        return DpiOfMonitor(MonitorFromPoint(new POINT { X = x, Y = y }, 2));
    }
    public static uint DpiOf(IntPtr h) {
        if (h == IntPtr.Zero) return 96;
        return DpiOfMonitor(MonitorFromWindow(h, 2));
    }
    public static IntPtr FindOverlay(uint pid, int left, int top) {
        found = IntPtr.Zero;
        wantPid = pid; wantLeft = left; wantTop = top; wantExact = true;
        if (overlayCb == null) overlayCb = OnOverlay;
        EnumWindows(overlayCb, IntPtr.Zero);
        if (found != IntPtr.Zero) return found;
        wantExact = false;
        EnumWindows(overlayCb, IntPtr.Zero);
        return found;
    }
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
}
"@
}

$null = [PossessionCardShotsWin]::SetProcessDpiAwarenessContext([IntPtr](-4))
$null = [PossessionCardShotsWin]::SetThreadDpiAwarenessContext([IntPtr](-4))

$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$out = Join-Path $OutDir "run-$stamp"
$script:Evidence = $out
$data = Initialize-ScenarioData $out @{ hide_in_fullscreen = $false }
$appLog = Join-Path $out "app.log"
$errLog = Join-Path $out "app.err"
$script:traceFile = Join-Path $data "process.log"
$traceFile = $script:traceFile

Remove-Item Env:\XAI_API_KEY -ErrorAction SilentlyContinue
Remove-Item Env:\FIDGET_HARNESS -ErrorAction SilentlyContinue
$env:FIDGET_DIRECTOR = "0"
$env:FIDGET_CAPTURABLE = "1"
$env:FIDGET_TRACE_FRAMES = "1"
$env:FIDGET_TRACE_HITTEST = "1"
$env:FIDGET_TRACE_BUBBLE = "1"
$env:FIDGET_CHARACTER = "bmo"
$chars = Join-Path $out "characters"
New-Item -ItemType Directory -Force -Path $chars | Out-Null
Copy-Item -Recurse (Join-Path (Join-Path $root "characters") "bmo") $chars
$manifest = Join-Path (Join-Path $chars "bmo") "character.manifest"
$still = Get-StillManifest @(Get-Content -LiteralPath $manifest)
$utf8 = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllLines($manifest, [string[]]$still, $utf8)
$env:FIDGET_CHARACTERS = $chars

$grey = Get-BackdropGrey
$script:form = $null
$script:proc = $null

function Get-TraceLines { return @(Get-Content -LiteralPath $traceFile -ErrorAction SilentlyContinue) }

function Get-ShotBoxFromTrace {
    $lines = Get-TraceLines
    $card = Get-LatestPossessionCard $lines
    if (-not $card) { return $null }
    $origin = Get-OverlayOrigin $lines $card.Label
    if (-not $origin) { Fail "no covers line for $($card.Label) in $traceFile" }
    $overlay = [IntPtr]::Zero
    if ($script:proc) {
        $overlay = [PossessionCardShotsWin]::FindOverlay([uint32]$script:proc.Id, $origin.X, $origin.Y)
    }
    $dpi = 96
    if ($overlay -ne [IntPtr]::Zero) {
        $dpi = [PossessionCardShotsWin]::DpiOf($overlay)
    } else {
        $dpi = [PossessionCardShotsWin]::DpiAt($origin.X, $origin.Y)
    }
    $scale = Get-DpiScale $dpi
    $box = Get-CardShotBox $card $origin $script:PossessionBarPx $script:PossessionGapPx $script:PossessionShotMarginPx $scale
    $box.Label = $card.Label
    $box.Paused = $card.Paused
    $box.OriginX = $origin.X
    $box.OriginY = $origin.Y
    $box.Dpi = $dpi
    $box.Scale = $scale
    return $box
}

function Move-Backdrop($Box) {
    $script:form.Location = New-Object System.Drawing.Point $Box.X, $Box.Y
    $script:form.Size = New-Object System.Drawing.Size $Box.Width, $Box.Height
    [System.Windows.Forms.Application]::DoEvents()
    $overlay = [PossessionCardShotsWin]::FindOverlay([uint32]$script:proc.Id, $Box.OriginX, $Box.OriginY)
    if ($overlay -eq [IntPtr]::Zero) { Fail "no Fidget overlay window at ($($Box.OriginX),$($Box.OriginY))" }
    $flags = [PossessionCardShotsWin]::SWP_NOMOVE -bor [PossessionCardShotsWin]::SWP_NOSIZE -bor [PossessionCardShotsWin]::SWP_NOACTIVATE
    [void][PossessionCardShotsWin]::SetWindowPos($script:form.Handle, $overlay, 0, 0, 0, 0, $flags)
}

function Save-CardShot([string]$Name, $Box) {
    Start-Sleep -Milliseconds 300
    $v = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $L = [Math]::Max($Box.X, $v.Left)
    $T = [Math]::Max($Box.Y, $v.Top)
    $R = [Math]::Min($Box.X + $Box.Width, $v.Right)
    $B = [Math]::Min($Box.Y + $Box.Height, $v.Bottom)
    $w = $R - $L
    $h = $B - $T
    if ($w -le 0 -or $h -le 0) { Fail "capture ${Name}: empty crop ($L,$T)-($R,$B)" }
    $offX = $L - $Box.X
    $offY = $T - $Box.Y
    $bmp = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    try {
        $g.CopyFromScreen($L, $T, 0, 0, (New-Object System.Drawing.Size $w, $h))
        $rows = @()
        for ($y = 0; $y -lt $h; $y++) {
            $row = @()
            for ($x = 0; $x -lt $w; $x++) {
                $px = $bmp.GetPixel($x, $y)
                $row += ,(@([int]$px.R, [int]$px.G, [int]$px.B))
            }
            $rows += ,$row
        }
        $rects = @(
            @{ X = $Box.CardX - $offX; Y = $Box.CardY - $offY; Width = $Box.CardWidth; Height = $Box.CardHeight },
            @{ X = $Box.BarX - $offX; Y = $Box.BarY - $offY; Width = $Box.BarWidth; Height = $Box.BarHeight }
        )
        $verdict = Get-BackdropVerdict $rows $rects $grey
        Note ("{0}: {1}" -f $Name, $verdict.Message)
        if (-not $verdict.Ok) { Fail "$Name refused: $($verdict.Message)" }
        $path = Join-Path $OutDir $Name
        $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
        Note "saved $path ${w}x${h} at ($L,$T)"
    } finally {
        $g.Dispose()
        $bmp.Dispose()
    }
}

try {
    Note "start: $(git -C $root rev-parse --short HEAD) bin=$Bin out=$OutDir"
    $script:proc = Start-FidgetPiped $Bin @() $appLog $errLog $root
    Note "launched pid $($script:proc.Id)"
    if (-not (Wait-ForUp $errLog)) { Fail "Fidget never wrote its overlay line; see $errLog" }
    if (-not (Wait-ForFile 3 $traceFile)) { Fail "process.log never appeared; see $errLog" }
    if (-not (Wait-ForTrace 15 $traceFile '^\d+\.?\d* frame: .* sprite\(')) { Fail "Fidget traced no frame; see $traceFile" }
    $spawn = Get-LastSpriteSpot $traceFile
    # The Shell holds the launch fall still until the overlay draws it, or
    # for 3s. Wait until that clock is running, then for rest, as
    # possession-live does once the sprite can move.
    if (-not (Wait-OnChanges 15 $data { Test-SpriteClockRunning $traceFile $spawn })) {
        Fail "the launch fall was still held after 15s; see $traceFile"
    }
    Note "sprite clock running; waiting for rest"
    $rest = Wait-ForRest $traceFile
    if (-not $rest) { Fail "the sprite never came to rest within 10s; see $traceFile" }
    $size = Select-String -LiteralPath $traceFile -Pattern 'sprite (\d+)x(\d+);' | Select-Object -First 1
    if (-not $size) { Fail "no sprite size in $traceFile" }
    $sw = [int]$size.Matches[0].Groups[1].Value
    $sh = [int]$size.Matches[0].Groups[2].Value
    $cx = $rest.X + [int]($sw / 2)
    $cy = $rest.Y + [int]($sh / 2)
    Note "sprite at ($($rest.X),$($rest.Y)) size ${sw}x${sh}"
    Skip-IfFullscreenOverClick $cx $cy $script:proc.Id $out "right-click"
    if (-not (Invoke-AxWin (Join-Path $out "menu.txt") @{
                Command = "menu"; ProcessId = $script:proc.Id; TraceFile = $traceFile
                SpriteW = $sw; SpriteH = $sh; Arg3 = "Possess"
            })) {
        Fail "could not press Possess; see $out\menu.txt"
    }
    if (-not (Wait-ForTrace 3 $traceFile '^\d+ menu: possession on')) {
        Fail "the Possess row did not possess the sprite; see $traceFile"
    }
    $raisedLine = '^\d+ overlay: pill open SetForegroundWindow\(0x[0-9a-f]+\) -> 1 '
    $raisedAfterPossess = {
        $seen = $false
        foreach ($line in (Get-TraceLines)) {
            if ($line -match '^\d+ menu: possession on') { $seen = $true }
            elseif ($seen -and $line -match $raisedLine) { return $true }
        }
        return $false
    }
    if (-not (Wait-OnChanges 3 $data $raisedAfterPossess)) {
        Fail "the possessed overlay never took the keyboard; see $traceFile"
    }
    if (-not (Wait-ForTrace 5 $traceFile 'possession card at \(-?\d+,-?\d+\) \d+x\d+ paused=false')) {
        Fail "no sized possession card line; see $traceFile"
    }
    $box = Get-ShotBoxFromTrace
    if (-not $box) { Fail "could not size the crop from the card trace; see $traceFile" }
    Note ("card crop ({0},{1}) {2}x{3} card ({4},{5}) {6}x{7} at {8} dpi ({9}%)" -f $box.X, $box.Y, $box.Width, $box.Height, $box.CardX, $box.CardY, $box.CardWidth, $box.CardHeight, $box.Dpi, [int][Math]::Round(100 * $box.Scale))

    $script:form = New-Object GreyBackdrop
    $script:form.BackColor = [System.Drawing.Color]::FromArgb($grey.R, $grey.G, $grey.B)
    $script:form.Location = New-Object System.Drawing.Point $box.X, $box.Y
    $script:form.Size = New-Object System.Drawing.Size $box.Width, $box.Height
    $script:form.Show()
    [System.Windows.Forms.Application]::DoEvents()
    Move-Backdrop $box

    [PossessionCardShotsWin]::Key(0x20, $false)
    Start-Sleep -Milliseconds 450
    $box = Get-ShotBoxFromTrace
    if (-not $box) { Fail "the card trace went missing during the charge; see $traceFile" }
    Move-Backdrop $box
    Save-CardShot "win-card-charging.png" $box
    [PossessionCardShotsWin]::Key(0x20, $true)

    $blurs = @(Get-TraceLines | Where-Object { $_ -match '^\d+ overlay [^:]+: window blur' }).Count
    if (-not ([PossessionCardShotsWin]::Bring($script:form.Handle))) {
        Note "WARNING: SetForegroundWindow did not report the grey window in front; waiting for pause anyway"
    }
    $paused = {
        $latest = Get-LatestPossessionCard (Get-TraceLines)
        return [bool]($latest -and $latest.Paused)
    }
    if (-not (Wait-ForCount 3 $traceFile '^\d+ overlay [^:]+: window blur' $blurs) -and -not (Wait-Until 3 $paused)) {
        Fail "activating the grey window did not pause the card; see $traceFile"
    }
    if (-not (Wait-Until 3 $paused)) {
        Fail "the card never traced paused=true; see $traceFile"
    }
    $box = Get-ShotBoxFromTrace
    if (-not $box) { Fail "the paused card trace was missing; see $traceFile" }
    Move-Backdrop $box
    Save-CardShot "win-card-paused.png" $box
    Note "done"
} catch {
    Note "ERROR: $($_.Exception.Message)"
    throw
} finally {
    if ($script:form) {
        try { $script:form.Close(); $script:form.Dispose() } catch { }
        $script:form = $null
    }
    Stop-FidgetTree $script:proc ""
    Note "stopped Fidget"
    Write-Host "log: $script:RunLog"
    Get-ChildItem -LiteralPath $OutDir -Filter "win-card-*.png" -ErrorAction SilentlyContinue | ForEach-Object {
        Write-Host ("{0}  {1} bytes" -f $_.FullName, $_.Length)
    }
}
