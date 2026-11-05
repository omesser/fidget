#!/usr/bin/env pwsh
# Scenario: possession-live (Windows)
# On screen: launches Fidget as BMO with a fixture Harness. Notepad opens on a
#   scratch file. The scenario right-clicks the sprite, presses Possess, holds
#   Right and Space, clicks Notepad, clicks the paused card, and presses
#   Escape. Fidget quits when the scenario ends, and so does the Notepad this
#   run started.
# Input: one real right-click on the sprite, a click on Possess, Right and
#   Space held, a click in Notepad, the letter k, a click on the paused card,
#   and Escape. Fidget treats Control-click as the menu gesture only on macOS;
#   here the right button is the menu gesture.
# Duration: about 15 s, 45 s at most.
# Grants: a desktop session. UI Automation for the terminal that runs it. bash
#   on PATH for the fixture Harness wrapper.
# Asserts: the menu's Possess row possesses the sprite; holding Right walks it
#   at least 8 px; the click away pauses it without launching the held Space;
#   k reaches Notepad; a click on the paused card returns the keyboard; Escape
#   releases possession. Frame waits go through Wait-ForTrace.
# Skips (exit 2) when Notepad is already open, or when a fullscreen window
#   would take the right-click.
#
# Usage: possession-live.win.ps1 --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
# Set FIDGET_SCENARIO_TRACE and FIDGET_SCENARIO_LETTER to check a saved log and
# the editor text, and skip the GUI.
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

# possession-live-check.sh is the bash copy; keep the messages identical.
function Test-PossessionLive([string]$TracePath, [string]$LetterPath, [string]$Mode = "done") {
    $xs = [System.Collections.Generic.List[int]]::new()
    $fall = [System.Collections.Generic.List[bool]]::new()
    $walk = [System.Collections.Generic.List[bool]]::new()
    $on = $false
    $escaped = $false
    foreach ($raw in @(Get-Content -LiteralPath $TracePath)) {
        $line = $raw.TrimEnd([char]13)
        if ($line -match '^(\d+) (.*)$') {
            $rest = $Matches[2]
            if ($rest -match '^(frame:|menu:|possession:|overlay )') { $line = $rest }
        }
        if (-not $on) {
            if ($line -eq "menu: possession on") { $on = $true }
            continue
        }
        # The release plays react. That motion is after the pause this check owns.
        if ($escaped) { continue }
        if ($line -eq "possession: escape") { $escaped = $true; continue }
        if ($line -notmatch '^frame: ') { continue }
        if ($line -notmatch 'sprite\((-?\d+),(-?\d+)\)') { continue }
        $spriteX = [int]$Matches[1]
        $isFall = $line -match ' Falling '
        $isWalk = $line -match ' walk#'
        [void]$xs.Add($spriteX)
        [void]$fall.Add([bool]$isFall)
        [void]$walk.Add([bool]$isWalk)
    }
    if (-not $on) { [Console]::Error.WriteLine("possession never turned on"); return $false }
    if ($Mode -ne "pause" -and -not $escaped) {
        [Console]::Error.WriteLine("Escape did not release possession")
        return $false
    }
    if ($xs.Count -lt 1) {
        [Console]::Error.WriteLine("the held Right did not walk the sprite")
        return $false
    }
    $origin = $xs[0]
    $lastWalk = -1
    $maxX = $origin
    for ($i = 0; $i -lt $xs.Count; $i++) {
        if ($fall[$i]) {
            [Console]::Error.WriteLine("the pause launched a jump")
            return $false
        }
        if ($walk[$i]) {
            $lastWalk = $i
            if ($xs[$i] -gt $maxX) { $maxX = $xs[$i] }
        }
    }
    if ($lastWalk -lt 0 -or ($maxX - $origin) -lt 8) {
        [Console]::Error.WriteLine("the held Right did not walk the sprite")
        return $false
    }
    if (($xs.Count - $lastWalk - 1) -lt 5) {
        [Console]::Error.WriteLine("the sprite kept walking after the other window took focus")
        return $false
    }
    $spot = $xs[$lastWalk + 1]
    for ($i = $lastWalk + 1; $i -lt $xs.Count; $i++) {
        if ($walk[$i] -or [Math]::Abs($xs[$i] - $spot) -gt 2) {
            [Console]::Error.WriteLine("the sprite kept walking after the other window took focus")
            return $false
        }
    }
    if ($Mode -eq "pause") {
        [Console]::Out.WriteLine("ok: Right walked the sprite and the pause held the jump")
        return $true
    }
    $letter = ""
    if (Test-Path -LiteralPath $LetterPath) { $letter = [System.IO.File]::ReadAllText($LetterPath) }
    if ($letter -notmatch '[Kk]') {
        [Console]::Error.WriteLine("the letter did not reach the other window")
        return $false
    }
    [Console]::Out.WriteLine("ok: Right walked the sprite, the pause held, Escape released it, and the letter landed")
    return $true
}

if ($Go -ne "--go") {
    Show-Header
    exit 2
}
if (-not $Bin -or -not $TestBin) {
    Write-Error "usage: possession-live.win.ps1 --go <fidget binary> <fidget test binary>"
    exit 1
}
if ($env:FIDGET_SCENARIO_TRACE -or $env:FIDGET_SCENARIO_LETTER) {
    if (-not $env:FIDGET_SCENARIO_TRACE -or -not $env:FIDGET_SCENARIO_LETTER) {
        [Console]::Error.WriteLine("FAIL: set both FIDGET_SCENARIO_TRACE and FIDGET_SCENARIO_LETTER")
        exit 1
    }
    if (-not (Test-PossessionLive $env:FIDGET_SCENARIO_TRACE $env:FIDGET_SCENARIO_LETTER)) { exit 1 }
    Write-Output "PASS: fixture trace"
    exit 0
}

function Fail([string]$Message) {
    [Console]::Error.WriteLine("FAIL: $Message")
    if ($script:Evidence) { [Console]::Error.WriteLine("evidence in $($script:Evidence)") }
    exit 1
}

$root = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
. (Join-Path $PSScriptRoot "..\helpers\wait-helpers.win.ps1")
. (Join-Path $PSScriptRoot "..\helpers\bash-setup-win.ps1")
if (-not $bashPath) {
    [Console]::Error.WriteLine("SKIP: bash not found (tried Git Bash, then PATH)")
    exit 2
}
if (@(Get-Process -Name Notepad -ErrorAction SilentlyContinue).Count -gt 0) {
    [Console]::Error.WriteLine("SKIP: Notepad is running. Quit it first: this run opens and closes its own.")
    exit 2
}

$TestBin = (Resolve-Path -LiteralPath $TestBin).Path
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$out = Join-Path $env:TEMP "fidget-scenario-possession-live-$stamp"
$script:Evidence = $out
$data = Initialize-ScenarioData $out @{ hide_in_fullscreen = $false }
$log = Join-Path $out "app.log"
$err = Join-Path $out "app.err"
$marks = Join-Path $out "harness.log"
$doc = Join-Path $out "possession-live-letter.txt"
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
$env:FIDGET_TRACE_BUBBLE = "1"
$env:FIDGET_CHARACTER = "bmo"
$env:FIDGET_CHARACTERS = Join-Path $root "characters"

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class PossessionLiveInput {
    public delegate bool Callback(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(Callback cb, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int count);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
    [StructLayout(LayoutKind.Sequential)] struct KEYBDINPUT { public ushort vk; public ushort scan; public uint flags; public uint time; public IntPtr extra; }
    [StructLayout(LayoutKind.Sequential)] struct MOUSEINPUT { public int dx; public int dy; public uint data; public uint flags; public uint time; public IntPtr extra; }
    [StructLayout(LayoutKind.Explicit)] struct INPUT { [FieldOffset(0)] public uint type; [FieldOffset(8)] public KEYBDINPUT k; [FieldOffset(8)] public MOUSEINPUT m; }
    [DllImport("user32.dll", SetLastError = true)] static extern uint SendInput(uint n, INPUT[] i, int size);
    static IntPtr foundWindow;
    static string wantTitle;
    static Callback titleAlive;
    static bool OnTitled(IntPtr h, IntPtr l) {
        if (!IsWindowVisible(h)) return true;
        StringBuilder text = new StringBuilder(512);
        GetWindowText(h, text, text.Capacity);
        if (text.ToString().IndexOf(wantTitle, StringComparison.Ordinal) < 0) return true;
        foundWindow = h;
        return false;
    }
    public static IntPtr FindTitled(string titlePart) {
        foundWindow = IntPtr.Zero;
        if (string.IsNullOrEmpty(titlePart)) return foundWindow;
        wantTitle = titlePart;
        if (titleAlive == null) titleAlive = OnTitled;
        EnumWindows(titleAlive, IntPtr.Zero);
        return foundWindow;
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
    public static void Text(string word) {
        var all = new INPUT[word.Length * 2];
        for (int n = 0; n < word.Length; n++) {
            var down = new INPUT();
            down.type = 1;
            down.k.scan = word[n];
            down.k.flags = 4;
            all[n * 2] = down;
            var up = new INPUT();
            up.type = 1;
            up.k.scan = word[n];
            up.k.flags = 6;
            all[n * 2 + 1] = up;
        }
        Send(all);
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

function Release-Keys {
    if (-not ("PossessionLiveInput" -as [type])) { return }
    try {
        [PossessionLiveInput]::Key(0x27, $true)
        [PossessionLiveInput]::Key(0x20, $true)
    } catch { }
}

function Read-Shared([string]$Path) {
    try { return [System.IO.File]::ReadAllText($Path) } catch { return "" }
}

function Owner-Of([IntPtr]$H) {
    $owner = [uint32]0
    [void][PossessionLiveInput]::GetWindowThreadProcessId($H, [ref]$owner)
    $owner
}

function Get-CardCenter([string]$File) {
    # The focus button, else the Paused heading. The heading sits against the
    # quick-message field, so a click on its centre can miss the card.
    $paused = $null
    foreach ($line in @(Get-Content -LiteralPath $File)) {
        $frame = ($line -split '\|')[-1]
        $a = @($frame -split ',')
        if ($a.Count -ne 4) { continue }
        $w = 0; $h = 0
        [void][int]::TryParse($a[2], [ref]$w)
        [void][int]::TryParse($a[3], [ref]$h)
        if ($w -le 0 -or $h -le 0) { continue }
        $point = @{ X = [int]$a[0] + [int]($w / 2); Y = [int]$a[1] + [int]($h / 2) }
        if ($line -like "*Click to focus*") { return $point }
        if (-not $paused -and $line -like "*Paused*") { $paused = $point }
    }
    return $paused
}

. (Join-Path $PSScriptRoot "..\helpers\fullscreen-over-click.win.ps1")
$script:notepad = $null
$script:launched = $null
$script:pad = [IntPtr]::Zero
$script:proc = $null
$startMs = Get-NowMs
try {
    $script:launched = Start-Process -FilePath notepad.exe -ArgumentList $doc -PassThru
    $proc = Start-FidgetPiped $Bin @() $log $err
    $script:proc = $proc
    # Windows 11 Store Notepad is two processes. Start-Process's pid owns no
    # window; the one titled with this file does.
    $script:docName = Split-Path -Leaf $doc
    $found = Wait-UiaChange 0 3 {
        $h = [PossessionLiveInput]::FindTitled($script:docName)
        if ($h -eq [IntPtr]::Zero) { return $null }
        return $h
    }
    if (-not $found) { Fail "Notepad did not open its window; see $out" }
    $script:pad = [IntPtr]$found
    $script:notepad = Get-Process -Id (Owner-Of $script:pad)
    if ($script:notepad.ProcessName -ne "Notepad") {
        Fail "the $script:docName window belongs to $($script:notepad.ProcessName), not Notepad"
    }
    $traceFile = Join-Path $data "process.log"
    if (-not (Wait-ForFile 3 $traceFile)) { Fail "process.log never appeared; see $err" }
    if (-not (Wait-ForTrace 3 $traceFile '^\d+ frame: .* sprite\(')) { Fail "Fidget traced no frame; see $traceFile" }
    $rest = Wait-ForRest $traceFile
    if (-not $rest) { Fail "the sprite never came to rest within 10s; see $traceFile" }
    $trace = Get-Content -LiteralPath $traceFile
    $size = $trace | Select-String -Pattern 'sprite (\d+)x(\d+);' | Select-Object -First 1
    if (-not $size) { Fail "no sprite size in $traceFile" }
    $w = [int]$size.Matches[0].Groups[1].Value
    $h = [int]$size.Matches[0].Groups[2].Value
    $cx = $rest.X + [int]($w / 2)
    $cy = $rest.Y + [int]($h / 2)
    Write-Output "ok: sprite at ($($rest.X),$($rest.Y)) size ${w}x${h}, centre ($cx,$cy)"
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
    if (-not (Wait-ForTrace 3 $traceFile '^\d+ overlay [^:]+: window focus')) {
        Fail "the possessed overlay never took the keyboard; see $traceFile"
    }
    Write-Output "ok: possessed, and the overlay has the keyboard"
    [PossessionLiveInput]::Key(0x27, $false)
    [PossessionLiveInput]::Key(0x20, $false)
    if (-not (Wait-ForTrace 3 $traceFile '^\d+ frame: .* walk#')) {
        Fail "the held Right did not walk the sprite; see $traceFile"
    }
    $pad = $script:pad
    $rect = New-Object PossessionLiveInput+RECT
    if ($pad -eq [IntPtr]::Zero -or -not [PossessionLiveInput]::GetWindowRect($pad, [ref]$rect)) {
        Fail "Notepad has no window to click"
    }
    $blurs = @(Get-Content -LiteralPath $traceFile | Where-Object { $_ -match '^\d+ overlay [^:]+: window blur' }).Count
    [PossessionLiveInput]::SetCursorPos([int](($rect.Left + $rect.Right) / 2), [int](($rect.Top + $rect.Bottom) / 2)) | Out-Null
    [PossessionLiveInput]::Click()
    if (-not (Wait-ForCount 3 $traceFile '^\d+ overlay [^:]+: window blur' $blurs)) {
        Fail "the overlay never lost focus to Notepad; see $traceFile"
    }
    if (-not (Wait-Until 3 { Get-SpriteAtRest $traceFile })) {
        Fail "the sprite kept walking after Notepad took focus; see $traceFile"
    }
    if (-not (Test-PossessionLive $traceFile $doc "pause")) { Fail "the pause check failed; see $traceFile" }
    Release-Keys
    if (-not (Wait-Until 3 {
                $front = [PossessionLiveInput]::GetForegroundWindow()
                $owner = [uint32]0
                [void][PossessionLiveInput]::GetWindowThreadProcessId($front, [ref]$owner)
                $owner -eq [uint32]$script:notepad.Id
            })) {
        Fail "Notepad did not take focus"
    }
    [PossessionLiveInput]::Text("k")
    [PossessionLiveInput]::Key(0x11, $false)
    [PossessionLiveInput]::Key(0x53, $false)
    [PossessionLiveInput]::Key(0x53, $true)
    [PossessionLiveInput]::Key(0x11, $true)
    if (-not (Wait-Until 3 { (Read-Shared $doc) -match 'k' })) {
        Fail "the letter did not reach Notepad (it holds $(Read-Shared $doc))"
    }
    $cardFile = Join-Path $out "card.txt"
    if (-not (Invoke-AxWin $cardFile @{ Command = "dump"; ProcessId = $proc.Id; Title = "Fidget"; Arg3 = "frames" })) {
        Fail "could not read the paused card; see $cardFile"
    }
    $card = Get-CardCenter $cardFile
    if (-not $card) { Fail "the paused card did not appear; see $cardFile" }
    $focuses = @(Get-Content -LiteralPath $traceFile | Where-Object { $_ -match '^\d+ overlay [^:]+: window focus' }).Count
    [PossessionLiveInput]::SetCursorPos($card.X, $card.Y) | Out-Null
    [PossessionLiveInput]::Click()
    if (-not (Wait-ForCount 3 $traceFile '^\d+ overlay [^:]+: window focus' $focuses)) {
        Fail "the card click did not resume; see $traceFile"
    }
    Write-Output "ok: the card click resumed"
    [PossessionLiveInput]::Key(0x1B, $false)
    [PossessionLiveInput]::Key(0x1B, $true)
    if (-not (Wait-ForTrace 3 $traceFile '^\d+ possession: escape')) {
        Fail "Escape did not release possession; see $traceFile"
    }
    if (-not (Test-PossessionLive $traceFile $doc)) { Fail "the possession check failed; see $traceFile" }
    Write-Output "PASS: evidence in $out ($((Get-NowMs) - $startMs)ms)"
} finally {
    Release-Keys
    Stop-FidgetTree $script:proc "count=$marksBash"
    foreach ($held in @($script:notepad, $script:launched)) {
        if ($held -and -not $held.HasExited) {
            Stop-Process -Id $held.Id -Force -ErrorAction SilentlyContinue
        }
    }
}
