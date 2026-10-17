#!/usr/bin/env pwsh
# Scenario: chat-close-pill-type (Windows)
# On screen: Notepad on a uniquely named scratch file, as a tab of a Notepad
#   already open or a window of its own, and Fidget as a standing BMO with the
#   fixture Harness. Cleanup closes only that tab, so it never comes back in
#   Notepad's restored session. If this run started Notepad, it also closes
#   that one window, and only when every tab left in it was there at launch.
# Input: SendInput cursor moves, clicks, typed words, Escape, Enter, Ctrl+Home,
#   Ctrl+S. Case 5 opens and closes a small window of the script's own.
#   Each press is held until Fidget traces it down. A pill word is one SendInput;
#   a Notepad word goes one character at a time, each saved and read back first.
# Duration: about 30 s (an estimate until the first Windows run). Every wait
#   gives up 3 s after its trigger; at startup that is the first frame after
#   the launch, the overlay's page load after that frame, and its first report
#   after the page load. Waits wake on each line of Fidget's piped stderr, on
#   file changes, or on UI Automation events; nothing polls.
# WebView2's profile is one folder every run reuses,
#   %LOCALAPPDATA%\fidget-scenarios\webview2. The first run on a PC makes it
#   cold; if that run misses a startup step it says to rerun. A WebView2 still
#   holding its EBWebView\lockfile (a run that died) fails the run before
#   launch, named by pid when the Restart Manager knows it; it is never killed.
# Grants: a desktop session with no Fidget running (it skips otherwise). Other
#   Notepad tabs and windows, and any fullscreen app, stay as they are; a
#   fullscreen window is only logged. bash for the fixture Harness wrapper.
# Asserts: (1) a sprite click leaves typing in Notepad; (2) hover opens the
#   pill, Fidget prints the pill-open SetForegroundWindow line, and "two" lands
#   in the pill (text="two" focused=true), with no close or hand-back line
#   since the pill opened; (3) Escape and (4) Enter close the
#   pill and hand typing back to Notepad; (5) with the window that was in front
#   closed while the pill was open, Escape steps aside to Notepad; (6) a click
#   into Notepad closes the empty pill (the page blur finds another window in
#   front) and typing lands in Notepad; (7) hover opens a pill, a click on it
#   keeps typing in it, and Escape hands it back.
#   It prints the pill-open line, which says whether Windows refused the pill
#   the foreground (#1577).
# Self-checks that launch nothing: FIDGET_SCENARIO_NOTEPAD_TABS (names split by
#   |), FIDGET_SCENARIO_DOC and FIDGET_SCENARIO_NOTEPAD_BEFORE=1|0 print the
#   Notepad cleanup plan, and adding FIDGET_SCENARIO_NOTEPAD_LEFT (the tabs left
#   once ours closes, or none) prints whether the window may close;
#   FIDGET_SCENARIO_READY_LOG=<process.log> prints whether the overlay was
#   listening; FIDGET_SCENARIO_AIM_LOG=<process.log> prints the sprite's middle;
#   FIDGET_SCENARIO_TRACE_LINE=<line> prints which waited-on trace patterns it
#   matches; FIDGET_SCENARIO_PROFILE_LOCK=<folder> prints whether
#   its EBWebView\lockfile is missing, free or in use;
#   FIDGET_SCENARIO_PILL_LOG=<process.log> prints whether "two" (or the pattern
#   named by FIDGET_SCENARIO_PILL_TYPED) typed into the pill landed or the pill
#   went first, judged from FIDGET_SCENARIO_PILL_OPENED_AT (0-based); FIDGET_SCENARIO_READ_DOC=<file> prints
#   the file as Notepad's is read, or busy;
#   FIDGET_SCENARIO_UNEXPECTED=<results.txt> throws, to check the FAIL line;
#   FIDGET_SCENARIO_PREPARE_ONLY=1 makes the run folder and stops.
# Each run writes to a new chat-close-pill-type-<time>-<hex> folder in
#   [evidence dir], or in $env:TEMP without one.
#
# Usage: chat-close-pill-type.win.ps1 --go <fidget binary> <fidget test binary> [evidence dir]
# Without --go it prints this header, which is the takeover prompt, and exits 2.
# No param() block: Windows PowerShell would bind `--go` as -Go.
$Go = if ($args.Count -gt 0) { $args[0] } else { "" }
$Bin = if ($args.Count -gt 1) { $args[1] } else { "" }
$TestBin = if ($args.Count -gt 2) { $args[2] } else { "" }
$Out = if ($args.Count -gt 3) { $args[3] } else { "" }
$ErrorActionPreference = "Stop"
# An error nothing here expected still ends the run with a FAIL line in
# results.txt once that exists (Fail writes its own, then throws "scenario failed").
$results = $null
trap {
    if ("$_" -ne "scenario failed") {
        $line = "FAIL: unexpected error: $($_.Exception.Message) (line $($_.InvocationInfo.ScriptLineNumber))"
        [Console]::Error.WriteLine($line)
        if ($results) { $line | Add-Content -LiteralPath $results }
    }
    break
}
if ($env:FIDGET_SCENARIO_UNEXPECTED) {
    $results = $env:FIDGET_SCENARIO_UNEXPECTED
    throw "boom from the self-check"
}

function Show-Header {
    $header = @()
    foreach ($line in Get-Content -LiteralPath $PSCommandPath | Select-Object -Skip 1) {
        if ($line.StartsWith("#")) { $header += ($line -replace '^# ?', '') } else { break }
    }
    $header -join "`n"
}
if ($Go -ne "--go") {
    Show-Header
    exit 2
}
if (-not $Bin -or -not $TestBin) {
    Write-Error "usage: chat-close-pill-type.win.ps1 --go <fidget binary> <fidget test binary> [evidence dir]"
    exit 1
}

# Every trace line a wait or assert reads, each matched to its eprintln! in
# src-tauri/src (main.rs, frame_loop.rs). Lines in process.log may carry a
# timestamp first.
$T = [ordered]@{
    frame     = '(?:^|\s)frame: \d+ '
    loaded    = '(?:^|\s)overlay: \S+ page loaded$'
    size      = '(?:^|\s)overlay: \d+ display\(s\); sprite (\d+)x(\d+);'
    listening = '(?:^|\s)overlay: draft instance=\S+ '
    opened    = '(?:^|\s)overlay: draft instance=\S+ text="" focused=true$'
    two       = '(?:^|\s)overlay: draft instance=\S+ text="two" focused=true$'
    eight     = '(?:^|\s)overlay: draft instance=\S+ text="eight" focused=true$'
    closed    = '(?:^|\s)overlay: draft instance=\S+ closed$'
    pillGone  = '(?:^|\s)overlay: (?:draft instance=\S+ closed|hand back )'
    blurAway  = '(?:^|\s)overlay \S+: window blur foreground=false dnd='
    restore   = '(?:^|\s)overlay: hand back Restore to 0x[0-9a-f]+ -> -?\d+$'
    stepAside = '(?:^|\s)overlay: hand back StepAside to 0x[0-9a-f]+ -> -?\d+$'
    pillOpen  = '(?:^|\s)overlay: pill open SetForegroundWindow\((0x[0-9a-f]+)\) -> (-?\d+) GetForegroundWindow\(\) -> (0x[0-9a-f]+)$'
    hitTest   = '(?:^|\s)hit-test: '
    hit       = '(?:^|\s)hit-test: cursor\(.*\bHIT\s+click-through OFF'
    miss      = '(?:^|\s)hit-test: cursor\(.*\bmiss click-through '
    control   = '(?:^|\s)hit-test: cursor\(.*\bmiss click-through OFF'
    five      = '(?:^|\s)overlay: draft instance=\S+ text="five" focused=true$'
    down      = '(?:^|\s)click: down '
    up        = '(?:^|\s)click: up '
}
# The pill's text, whatever its focus, holding $Word.
function Draft-Holds([string]$Word) { "(?:^|\s)overlay: draft instance=\S+ text=`"[^`"]*$Word[^`"]*`" " }

# The verdict on typing into the pill that opened at line $OpenedAt (0-based):
# "typed" at the first $Typed line, "lost <line>" at a pillGone line before it
# (even before the typing), else $null.
function Get-TypingVerdict([string[]]$Lines, [int]$OpenedAt, [string]$Typed) {
    foreach ($l in @($Lines | Select-Object -Skip $OpenedAt)) {
        if ($l -match $Typed) { return "typed" }
        if ($l -match $T.pillGone) { return "lost $($l.Trim())" }
    }
    $null
}
if ($env:FIDGET_SCENARIO_PILL_LOG) {
    $typed = if ($env:FIDGET_SCENARIO_PILL_TYPED) { $T[$env:FIDGET_SCENARIO_PILL_TYPED] } else { $T.two }
    $verdict = Get-TypingVerdict @(Get-Content -LiteralPath $env:FIDGET_SCENARIO_PILL_LOG) ([int]$env:FIDGET_SCENARIO_PILL_OPENED_AT) $typed
    Write-Output "pill: $(if ($verdict) { $verdict } else { 'none' })"
    exit 0
}

# Notepad's file as it is now, opened with a share that lets Notepad keep it open
# for writing. $null while Notepad holds it unshared mid-save: not saved yet.
function Read-Doc([string]$Path = $doc) {
    try {
        $stream = [System.IO.FileStream]::new($Path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]"ReadWrite, Delete")
    } catch [System.IO.IOException] {
        $e = $_.Exception
        while ($e -and $e -isnot [System.IO.IOException]) { $e = $e.InnerException }
        # 32 is a sharing violation, 33 a lock violation.
        if ($e -and ($e.HResult -band 0xFFFF) -in 32, 33) { return $null }
        throw
    }
    try { [System.IO.StreamReader]::new($stream, $true).ReadToEnd().TrimStart([char]0xFEFF) } finally { $stream.Dispose() }
}
if ($env:FIDGET_SCENARIO_READ_DOC) {
    $text = Read-Doc $env:FIDGET_SCENARIO_READ_DOC
    Write-Output "doc: $(if ($null -eq $text) { 'busy' } else { $text })"
    exit 0
}

# The file after each character of $Word typed into $Before ends at $Expected,
# or "fail: ..." when no single insertion of $Word gives $Expected.
function Get-TypingSteps([string]$Before, [string]$Expected, [string]$Word) {
    for ($at = 0; $at -le $Before.Length; $at++) {
        if ($Before.Insert($at, $Word) -cne $Expected) { continue }
        return ,@(for ($k = 1; $k -le $Word.Length; $k++) { $Before.Insert($at, $Word.Substring(0, $k)) })
    }
    "fail: `"$Word`" typed into `"$Before`" can't give `"$Expected`""
}
if ($null -ne $env:FIDGET_SCENARIO_TYPE_WORD) {
    $steps = Get-TypingSteps $env:FIDGET_SCENARIO_TYPE_BEFORE $env:FIDGET_SCENARIO_TYPE_EXPECTED $env:FIDGET_SCENARIO_TYPE_WORD
    Write-Output "steps: $(if ($steps -is [string]) { $steps } else { $steps -join '|' })"
    exit 0
}

if ($null -ne $env:FIDGET_SCENARIO_TRACE_LINE) {
    $names = @($T.Keys | Where-Object { $env:FIDGET_SCENARIO_TRACE_LINE -match $T[$_] })
    Write-Output "matches: $($names -join ' ')"
    exit 0
}

# Cleanup's plan for Notepad, from its tab names: it closes only our tab, and the
# window too only when this run opened Notepad. Anything else fails loudly.
function Get-NotepadPlan([string[]]$Tabs, [string]$Ours, [bool]$WasRunning) {
    $mine = @($Tabs | Where-Object { $_.Contains($Ours) }).Count
    if ($mine -eq 0) { return "fail: no Notepad tab shows $Ours" }
    if ($mine -gt 1) { return "fail: Notepad already shows $Ours in $mine tabs" }
    if ($WasRunning) { return "close our tab only" }
    "close our tab, then the window this run opened if it holds only tabs seen at launch"
}
# After our tab is closed, whether the window this run opened may close too:
# only if each tab left in it was there at launch, ours aside. Names repeat
# (two "Untitled"), so each launch name covers one tab.
function Get-WindowVerdict([string[]]$AtLaunch, [string[]]$Left, [string]$Ours) {
    $pool = New-Object System.Collections.Generic.List[string]
    foreach ($name in $AtLaunch) { if (-not $name.Contains($Ours)) { $pool.Add($name) } }
    foreach ($name in $Left) {
        if (-not $pool.Remove($name)) { return "leave: `"$name`" was not open at launch" }
    }
    "close"
}
# WebView2's lock on a profile: the browser holds <profile>\EBWebView\lockfile
# open for writing, sharing only reads, and the file goes when it closes
# (Chromium process_singleton_win.cc L244 at bef1ce99).
function Get-ProfileLock([string]$Folder) {
    $lock = Join-Path (Join-Path $Folder "EBWebView") "lockfile"
    $missing = [pscustomobject]@{ State = "missing"; Pids = @() }
    if (-not (Test-Path -LiteralPath $lock)) { return $missing }
    try {
        # Read sharing still clashes with the browser's write handle, but lets
        # a reader (an indexer, antivirus) through.
        [System.IO.File]::Open($lock, "Open", "Read", "Read").Dispose()
        return [pscustomobject]@{ State = "free"; Pids = @() }
    } catch {
        # PowerShell wraps a .NET method's exception; look at the cause.
        $e = $_.Exception
        while ($e.InnerException -and $e -isnot [System.IO.IOException]) { $e = $e.InnerException }
        if ($e -is [System.IO.FileNotFoundException] -or $e -is [System.IO.DirectoryNotFoundException]) { return $missing }
        # ERROR_SHARING_VIOLATION (32) or ERROR_LOCK_VIOLATION (33).
        if ($e -isnot [System.IO.IOException] -or @(32, 33) -notcontains ($e.HResult -band 0xFFFF)) { throw }
    }
    $pids = @()
    try { $pids = @(Get-LockHolders $lock) } catch { }
    return [pscustomobject]@{ State = "in-use"; Pids = $pids }
}
# The pids holding a file, from the Restart Manager. Never used to stop them.
function Get-LockHolders([string]$Path) {
    if (-not ("PillRm" -as [type])) {
        Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class PillRm {
    [StructLayout(LayoutKind.Sequential)] struct UniqueProcess { public int Pid; public System.Runtime.InteropServices.ComTypes.FILETIME Start; }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] struct ProcessInfo {
        public UniqueProcess Process;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 256)] public string App;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 64)] public string Service;
        public int Type; public int Status; public int Session; [MarshalAs(UnmanagedType.Bool)] public bool Restartable;
    }
    [DllImport("rstrtmgr.dll", CharSet = CharSet.Unicode)] static extern int RmStartSession(out uint session, int flags, System.Text.StringBuilder key);
    [DllImport("rstrtmgr.dll")] static extern int RmEndSession(uint session);
    [DllImport("rstrtmgr.dll", CharSet = CharSet.Unicode)] static extern int RmRegisterResources(uint session, uint files, string[] names, uint apps, IntPtr unique, uint services, string[] serviceNames);
    [DllImport("rstrtmgr.dll")] static extern int RmGetList(uint session, out uint needed, ref uint count, [In, Out] ProcessInfo[] info, out uint reasons);
    public static int[] Holders(string path) {
        uint session;
        if (RmStartSession(out session, 0, new System.Text.StringBuilder(64)) != 0) { return new int[0]; }
        try {
            if (RmRegisterResources(session, 1, new string[] { path }, 0, IntPtr.Zero, 0, null) != 0) { return new int[0]; }
            uint needed, reasons, count = 8;
            ProcessInfo[] info = new ProcessInfo[count];
            if (RmGetList(session, out needed, ref count, info, out reasons) != 0) { return new int[0]; }
            int[] pids = new int[count];
            for (int i = 0; i < count; i++) { pids[i] = info[i].Process.Pid; }
            return pids;
        } finally { RmEndSession(session); }
    }
}
"@
    }
    return [PillRm]::Holders($Path)
}
if ($env:FIDGET_SCENARIO_PROFILE_LOCK) {
    $verdict = Get-ProfileLock $env:FIDGET_SCENARIO_PROFILE_LOCK
    Write-Output "lock: $($verdict.State)$(if ($verdict.Pids.Count) { " pid $($verdict.Pids -join ',')" })"
    exit 0
}
if ($env:FIDGET_SCENARIO_NOTEPAD_LEFT) {
    $left = if ($env:FIDGET_SCENARIO_NOTEPAD_LEFT -eq "none") { [string[]]@() } else { [string[]]($env:FIDGET_SCENARIO_NOTEPAD_LEFT -split '\|') }
    Write-Output "window: $(Get-WindowVerdict ($env:FIDGET_SCENARIO_NOTEPAD_TABS -split '\|') $left $env:FIDGET_SCENARIO_DOC)"
    exit 0
}
if ($env:FIDGET_SCENARIO_NOTEPAD_TABS) {
    $plan = Get-NotepadPlan ($env:FIDGET_SCENARIO_NOTEPAD_TABS -split '\|') $env:FIDGET_SCENARIO_DOC ($env:FIDGET_SCENARIO_NOTEPAD_BEFORE -eq "1")
    if ($plan.StartsWith("fail: ")) {
        [Console]::Error.WriteLine("FAIL: $($plan.Substring(6))")
        exit 1
    }
    Write-Output "plan: $plan"
    exit 0
}
# The overlay's first draft report: its webview is listening, about 3 s after the
# Shell's first frame on Windows. Input before that reaches no pill.
function Test-OverlayListening([string[]]$Lines) {
    @($Lines | Where-Object { $_ -match $T.listening }).Count -gt 0
}
if ($env:FIDGET_SCENARIO_READY_LOG) {
    $ready = Test-OverlayListening (Get-Content -LiteralPath $env:FIDGET_SCENARIO_READY_LOG)
    Write-Output "ready: $(if ($ready) { 'yes' } else { 'no' })"
    exit 0
}

# Where Notepad goes: a monitor no fullscreen window covers, the primary first,
# sized to fit its work area. With every monitor covered it stays on the primary,
# and coming to the front is what puts it over the fullscreen app.
function Get-NotepadPlacement([object[]]$Monitors) {
    $order = @($Monitors | Where-Object { $_.Primary }) + @($Monitors | Where-Object { -not $_.Primary })
    $pick = $order | Where-Object { -not $_.Full } | Select-Object -First 1
    $free = [bool]$pick
    if (-not $pick) { $pick = $order[0] }
    $ww = $pick.R - $pick.L
    $wh = $pick.B - $pick.T
    $w = [math]::Min(1400, $ww - 100)
    $h = [math]::Min(900, $wh - 100)
    @{ Name = $pick.Name; X = $pick.L + [math]::Max(0, [math]::Min(400, $ww - $w)); Y = $pick.T + [math]::Max(0, [math]::Min(250, $wh - $h)); W = $w; H = $h; Free = $free }
}
if ($env:FIDGET_SCENARIO_MONITORS) {
    $mons = @($env:FIDGET_SCENARIO_MONITORS -split ';' | ForEach-Object {
        $f = $_ -split ','
        @{ Name = $f[0]; Primary = $f[1] -eq '1'; Full = $f[2] -eq '1'; L = [int]$f[3]; T = [int]$f[4]; R = [int]$f[5]; B = [int]$f[6] }
    })
    $p = Get-NotepadPlacement $mons
    Write-Output "place: $($p.Name) ($($p.X),$($p.Y)) $($p.W)x$($p.H) $(if ($p.Free) { 'free' } else { 'covered' })"
    exit 0
}

$root = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
. (Join-Path $PSScriptRoot "wait-helpers.win.ps1")
$script:t0 = Get-NowMs

# The middle of the sprite at rest: its top-left from the frame line, its size
# from the overlay's startup line. $null until both are there.
function Get-SpriteAim([string]$LogFile) {
    $at = Get-SpriteAtRest $LogFile
    if (-not $at) { return $null }
    $size = Get-Content -LiteralPath $LogFile | Where-Object { $_ -match $T.size } | Select-Object -First 1
    if (-not $size) { return $null }
    $null = $size -match $T.size
    @{ X = $at.X + [int]([int]$Matches[1] / 2); Y = $at.Y + [int]([int]$Matches[2] / 2) }
}
# Whether Fidget last saw the cursor on the sprite. Only then does moving off it
# print a miss line at once (the flip); otherwise none comes until the periodic trace.
function Test-LastHit([string[]]$Lines) {
    $last = @($Lines | Where-Object { $_ -match $T.hitTest }) | Select-Object -Last 1
    [bool]($last -and $last -notmatch $T.miss)
}
if ($env:FIDGET_SCENARIO_AWAY_LOG) {
    $wait = Test-LastHit @(Get-Content -LiteralPath $env:FIDGET_SCENARIO_AWAY_LOG)
    Write-Output "away: $(if ($wait) { 'wait' } else { 'go' })"
    exit 0
}
if ($env:FIDGET_SCENARIO_AIM_LOG) {
    $aim = Get-SpriteAim $env:FIDGET_SCENARIO_AIM_LOG
    Write-Output "aim: $(if ($aim) { "($($aim.X),$($aim.Y))" } else { 'none' })"
    exit 0
}

# Windows 11 Notepad is one process for every tab, so an open Notepad may hold the
# user's docs: the run adds a tab or window of its own and touches nothing else.
$notepadWasRunning = [bool](Get-Process -Name Notepad -ErrorAction SilentlyContinue)

$TestBin = (Resolve-Path -LiteralPath $TestBin).Path
# A fresh folder per run, under the given one or $env:TEMP, so a rerun keeps
# earlier evidence and nothing is ever cleared. New-Item stops if the name is taken.
$base = if ($Out) { $Out } else { $env:TEMP }
$Out = Join-Path $base "chat-close-pill-type-$(Get-Date -Format yyyyMMdd-HHmmss)-$((Get-Random -Maximum 65536).ToString('x4'))"
New-Item -ItemType Directory -Path $Out | Out-Null
# The shared waits watch this run's folder.
$script:WatchDir = $Out
$results = Join-Path $Out "results.txt"
$focusLog = Join-Path $Out "focus.log"
Set-Content -LiteralPath $results -Value $null
# Milliseconds since the script started, per step, so a slow step shows in focus.log.
function Note-Step([string]$What) { "step +$((Get-NowMs) - $script:t0) ms: $What" | Add-Content -LiteralPath $focusLog }
Note-Step "run folder made"
$script:proc = $null
$script:notepad = $null
$script:hwnd = [IntPtr]::Zero
$script:launchTabs = [string[]]@()
$harnessTag = $null

function Fail([string]$Message) {
    [Console]::Error.WriteLine("FAIL: $Message")
    "FAIL: $Message" | Add-Content -LiteralPath $results
    [Console]::Error.WriteLine("evidence in $Out")
    throw "scenario failed"
}
function Ok([string]$Message) { "ok: $Message" | Tee-Object -FilePath $results -Append | Write-Output }

$log = Join-Path (Join-Path $Out "data") "process.log"
$marks = Join-Path $Out "harness.log"
Set-Content -LiteralPath $marks -Value "" -Encoding ascii
$docName = "fidget-pill-$(Get-Date -Format yyyyMMdd-HHmmss)-$((Get-Random -Maximum 65536).ToString('x4')).txt"
$doc = Join-Path $Out $docName
[System.IO.File]::WriteAllText($doc, "abc")
# A standing BMO: its own copy, with every walk played as idle.
$chars = Join-Path $Out "characters"
New-Item -ItemType Directory -Force -Path $chars | Out-Null
Copy-Item -Recurse (Join-Path (Join-Path $root "characters") "bmo") $chars
$manifest = Join-Path (Join-Path $chars "bmo") "character.manifest"
$standing = Get-Content -LiteralPath $manifest | ForEach-Object { if ($_ -match '^play = ') { $_ -replace '"walk"', '"idle"' } else { $_ } }
[System.IO.File]::WriteAllLines($manifest, [string[]]$standing)
if (Select-String -LiteralPath $manifest -Pattern '^play = .*"walk"' -Quiet) { Fail "BMO still walks in $manifest" }
# FIDGET_SCENARIO_PREPARE_ONLY stops here, with the run folder made and nothing launched.
if ($env:FIDGET_SCENARIO_PREPARE_ONLY) {
    Write-Output "prepared: $Out"
    exit 0
}
# A Fidget already here would share the overlay.
$already = @(Get-Process -Name fidget -ErrorAction SilentlyContinue | ForEach-Object { "fidget $($_.Id)" })
if ($already.Count) {
    [Console]::Error.WriteLine("SKIP: Fidget is already running: $($already -join ', ')")
    exit 2
}
. (Join-Path $PSScriptRoot "bash-setup-win.ps1")
if (-not $bashPath) {
    [Console]::Error.WriteLine("SKIP: bash not found (tried Git Bash, then PATH)")
    exit 2
}

Add-Type @"
using System;
using System.Collections.Generic;
using System.Text;
using System.Runtime.InteropServices;
public static class PillWin {
    [DllImport("user32.dll")] public static extern IntPtr SetProcessDpiAwarenessContext(IntPtr v);
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr v);
    [DllImport("user32.dll")] public static extern IntPtr GetThreadDpiAwarenessContext();
    [DllImport("user32.dll")] public static extern int GetAwarenessFromDpiAwarenessContext(IntPtr v);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int hh, bool r);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint processId);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr h, uint f);
    [DllImport("user32.dll")] public static extern IntPtr MonitorFromWindow(IntPtr h, uint f);
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] public struct MONITORINFOEX { public int cbSize; public RECT rcMonitor; public RECT rcWork; public uint dwFlags; [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string szDevice; }
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern bool GetMonitorInfo(IntPtr m, ref MONITORINFOEX i);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int a, out int v, int size);
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc p, IntPtr l);
    [StructLayout(LayoutKind.Sequential)] struct KEYBDINPUT { public ushort vk; public ushort scan; public uint flags; public uint time; public IntPtr extra; }
    [StructLayout(LayoutKind.Sequential)] struct MOUSEINPUT { public int dx; public int dy; public uint data; public uint flags; public uint time; public IntPtr extra; }
    [StructLayout(LayoutKind.Explicit)] struct INPUT { [FieldOffset(0)] public uint type; [FieldOffset(8)] public KEYBDINPUT k; [FieldOffset(8)] public MOUSEINPUT m; }
    [DllImport("user32.dll", SetLastError = true)] static extern uint SendInput(uint n, INPUT[] i, int size);
    static void Send(INPUT[] all) {
        uint sent = SendInput((uint)all.Length, all, Marshal.SizeOf(typeof(INPUT)));
        if (sent != all.Length) throw new InvalidOperationException("SendInput sent " + sent + " of " + all.Length + " events, error " + Marshal.GetLastWin32Error());
    }
    public static void Key(ushort vk, bool up) { var i = new INPUT(); i.type = 1; i.k.vk = vk; i.k.flags = up ? 2u : 0u; Send(new[] { i }); }
    // A whole word as one SendInput call: each character's unicode down and up.
    public static void Text(string word) {
        var all = new List<INPUT>();
        foreach (char c in word) {
            var i = new INPUT(); i.type = 1; i.k.scan = c; i.k.flags = 4; all.Add(i);
            var j = new INPUT(); j.type = 1; j.k.scan = c; j.k.flags = 6; all.Add(j);
        }
        Send(all.ToArray());
    }
    public static void Button(bool up) { var i = new INPUT(); i.type = 0; i.m.flags = up ? 4u : 2u; Send(new[] { i }); }
    public static string Title(IntPtr h) { var s = new StringBuilder(512); GetWindowText(h, s, 512); return s.ToString(); }
    public static string Class(IntPtr h) { var s = new StringBuilder(256); GetClassName(h, s, 256); return s.ToString(); }
    public static IntPtr RootAt(int x, int y) { var p = new POINT(); p.X = x; p.Y = y; return GetAncestor(WindowFromPoint(p), 2); }
    public static MONITORINFOEX Monitor(IntPtr h) { var i = new MONITORINFOEX(); i.cbSize = Marshal.SizeOf(typeof(MONITORINFOEX)); GetMonitorInfo(MonitorFromWindow(h, 2), ref i); return i; }
    [DllImport("user32.dll")] static extern bool AttachThreadInput(uint attach, uint to, bool on);
    [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] static extern bool BringWindowToTop(IntPtr h);
    [DllImport("user32.dll")] static extern bool IsGUIThread(bool convert);
    // Windows refuses the foreground to a background script, so for the call
    // this thread's input joins the foreground thread's. The window in front
    // gets no key or click and is never moved, minimized or closed.
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
    delegate bool MonitorProc(IntPtr m, IntPtr dc, ref RECT r, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumDisplayMonitors(IntPtr dc, IntPtr clip, MonitorProc p, IntPtr l);
    public static List<MONITORINFOEX> Monitors() {
        var all = new List<MONITORINFOEX>();
        EnumDisplayMonitors(IntPtr.Zero, IntPtr.Zero, (IntPtr m, IntPtr dc, ref RECT r, IntPtr l) => {
            var i = new MONITORINFOEX(); i.cbSize = Marshal.SizeOf(typeof(MONITORINFOEX));
            if (GetMonitorInfo(m, ref i)) all.Add(i);
            return true;
        }, IntPtr.Zero);
        return all;
    }
    public static List<IntPtr> Tops() {
        var all = new List<IntPtr>();
        EnumWindows((h, l) => { int v; bool cloaked = DwmGetWindowAttribute(h, 14, out v, 4) == 0 && v != 0; if (IsWindowVisible(h) && !cloaked) all.Add(h); return true; }, IntPtr.Zero);
        return all;
    }
}
"@
# Per-monitor aware (-4), so cursor and window coordinates are the physical pixels
# Fidget traces. The process may already have an awareness set by its host, so
# this thread is set too, and checked: 2 is per-monitor.
$null = [PillWin]::SetProcessDpiAwarenessContext([IntPtr]-4)
$null = [PillWin]::SetThreadDpiAwarenessContext([IntPtr]-4)
$awareness = [PillWin]::GetAwarenessFromDpiAwarenessContext([PillWin]::GetThreadDpiAwarenessContext())
if ($awareness -ne 2) { Fail "this thread is not per-monitor DPI aware (awareness $awareness), so its pixels would not match Fidget's" }
Note-Step "Win32 helper compiled"

function Proc-Name([uint32]$ProcessId) { $g = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue; if ($g) { $g.ProcessName } else { "?" } }
function Owner-Of([IntPtr]$H) { [uint32]$owner = 0; $null = [PillWin]::GetWindowThreadProcessId($H, [ref]$owner); $owner }
function Rect-Text($R) { "($($R.L),$($R.T))-($($R.R),$($R.B))" }
function Describe-Window([IntPtr]$H) {
    if ($H -eq [IntPtr]::Zero) { return "none" }
    $owner = Owner-Of $H
    $r = New-Object PillWin+RECT
    $null = [PillWin]::GetWindowRect($H, [ref]$r)
    "hwnd=0x$($H.ToString('x')) process=$owner ($(Proc-Name $owner)) class=$([PillWin]::Class($H)) title=`"$([PillWin]::Title($H))`" rect=$(Rect-Text $r) monitor=$([PillWin]::Monitor($H).szDevice)"
}
function Get-Fullscreen {
    foreach ($h in [PillWin]::Tops()) {
        if ([PillWin]::Class($h) -in @("Progman", "WorkerW")) { continue }
        $r = New-Object PillWin+RECT
        $null = [PillWin]::GetWindowRect($h, [ref]$r)
        $m = [PillWin]::Monitor($h).rcMonitor
        if ($r.L -le $m.L -and $r.T -le $m.T -and $r.R -ge $m.R -and $r.B -ge $m.B) { $h }
    }
}
function Note-Foreground([string]$When) { "foreground $When`: $(Describe-Window ([PillWin]::GetForegroundWindow()))" | Add-Content -LiteralPath $focusLog }
# Each monitor, and whether a fullscreen window covers it; Notepad's place from that.
function Get-MonitorMap([string[]]$Covered) {
    foreach ($m in [PillWin]::Monitors()) {
        @{ Name = $m.szDevice; Primary = ($m.dwFlags -band 1) -eq 1; Full = $Covered -contains $m.szDevice; L = $m.rcWork.L; T = $m.rcWork.T; R = $m.rcWork.R; B = $m.rcWork.B }
    }
}

# The shared WebView2 profile, under the real LOCALAPPDATA (read before the run
# points it at its own folder). A startup step that misses on the run that
# made it says so.
$webviewProfile = Join-Path (Join-Path $env:LOCALAPPDATA "fidget-scenarios") "webview2"
$coldProfile = -not (Test-Path -LiteralPath $webviewProfile)
$profileLock = Join-Path (Join-Path $webviewProfile "EBWebView") "lockfile"
# A WebView2 left by a run that died can still hold the profile, and a launch
# against it would look like a page that never loads. Named, never killed.
$lockState = Get-ProfileLock $webviewProfile
if ($lockState.State -eq "in-use") {
    $holder = if ($lockState.Pids.Count) { " by pid $($lockState.Pids -join ', ')" } else { "" }
    $why = "FAIL: the shared WebView2 profile is in use$holder`: $profileLock is held open; close that WebView2 (or let it exit), then rerun"
    [Console]::Error.WriteLine($why)
    $why | Add-Content -LiteralPath $results
    exit 1
}
function Fail-Startup([string]$Message) {
    if ($coldProfile) { $Message += ". This run made the shared WebView2 profile in $webviewProfile, so it started cold: rerun once" }
    Fail $Message
}
Note-Step "profile checked"
Note-Foreground "before the run"
# Evidence only: a fullscreen app keeps running, and nothing here focuses or moves it.
$full = @(Get-Fullscreen)
foreach ($h in $full) { "fullscreen: $(Describe-Window $h)" | Add-Content -LiteralPath $focusLog }
if (-not $full.Count) { "fullscreen: none" | Add-Content -LiteralPath $focusLog }
# Their monitors, kept for placing Notepad later rather than scanning again.
$fullMonitors = @($full | ForEach-Object { [PillWin]::Monitor($_).szDevice })

# Lines in Fidget's log so far; the next pill's lines start here.
function Line-Count { if (Test-Path -LiteralPath $log) { @(Get-Content -LiteralPath $log).Count } else { 0 } }
$script:pillFrom = 0
function Count([string]$Pattern) {
    if (-not (Test-Path -LiteralPath $log)) { return 0 }
    @(Get-Content -LiteralPath $log | Where-Object { $_ -match $Pattern }).Count
}
# Until $Test holds, re-checked on each UI Automation event (focus, windows,
# structure, names), for desktop state with no line or file of its own.
function Wait-Ui([double]$Seconds, [scriptblock]$Test) { [bool](Wait-UiaChange 0 $Seconds $Test $script:uia) }
# One more matching line in Fidget's log than $Before, within 3 s.
function Wait-LogCount([string]$Pattern, [int]$Before) { Wait-ForCount 3 $log $Pattern $Before }
# Counts first, then pulls the trigger, then waits 3 s for one more matching line.
function Wait-ForNext([string]$Pattern, [scriptblock]$Trigger) {
    $before = Count $Pattern
    & $Trigger
    Wait-LogCount $Pattern $before
}
# Notepad in front with our tab the open one, so keys and Ctrl+S reach only our file.
function Foreground-Is { [PillWin]::GetForegroundWindow() -eq $script:hwnd -and [PillWin]::Title($script:hwnd).Contains($docName) }
$UiaEl = [System.Windows.Automation.AutomationElement]
$Gone = [System.Windows.Automation.ElementNotAvailableException]
function Keys-InNotepad {
    try {
        $f = $UiaEl::FocusedElement
        $f.Current.ProcessId -eq $script:notepad.Id -and $f.Current.ControlType -eq [System.Windows.Automation.ControlType]::Document
    } catch [System.Windows.Automation.ElementNotAvailableException] { $false }
}
function Describe-Keys {
    try {
        $f = $UiaEl::FocusedElement
        "process=$($f.Current.ProcessId) ($(Proc-Name $f.Current.ProcessId)) type=$($f.Current.ControlType.ProgrammaticName) name=`"$($f.Current.Name)`""
    } catch [System.Windows.Automation.ElementNotAvailableException] { "an element that is already gone" }
}
function Note-Keys([string]$When) { Note-Foreground $When; "keys $When`: $(Describe-Keys)" | Add-Content -LiteralPath $focusLog }
# Fidget samples the button each frame, so the press is held until Fidget
# traces it down (any window: the sample is global), then released.
function Press-Hold([int]$X, [int]$Y, [string]$Case) {
    $null = [PillWin]::SetCursorPos($X, $Y)
    $downs = Count $T.down
    [PillWin]::Button($false)
    $seen = Wait-LogCount $T.down $downs
    $ups = Count $T.up
    [PillWin]::Button($true)
    if (-not $seen) { Fail "${Case}: Fidget did not see the press at ($X,$Y) in 3 s; see $log" }
    if (-not (Wait-LogCount $T.up $ups)) { Fail "${Case}: Fidget did not see the release at ($X,$Y) in 3 s; see $log" }
}
# A click only on our Notepad window, checked by the window under the point.
function Click-Notepad([int]$X, [int]$Y, [string]$Case) {
    $under = [PillWin]::RootAt($X, $Y)
    if ((Owner-Of $under) -ne $script:notepad.Id) { Fail "${Case}: refusing to click ($X,$Y): it would land on $(Describe-Window $under)" }
    Press-Hold $X $Y $Case
}
function Press([uint16]$Vk) { [PillWin]::Key($Vk, $false); [PillWin]::Key($Vk, $true) }
function Aim([string]$Case) {
    if (-not (Wait-Until 3 { ($script:aim = Get-SpriteAim $log) })) { Fail "${Case}: the sprite was not at rest with a known size in 3 s; see $log" }
}
# Off the sprite, and if Fidget last saw it there, until its hit test traces the
# miss (3 s), so no hover dwell carries into the next case.
function Move-Away([string]$Case) {
    $wasOn = (Test-Path -LiteralPath $log) -and (Test-LastHit @(Get-Content -LiteralPath $log))
    $misses = Count $T.miss
    $null = [PillWin]::SetCursorPos($script:aim.X + 400, $script:aim.Y)
    if ($wasOn -and -not (Wait-LogCount $T.miss $misses)) { Fail "${Case}: Fidget traced no hit-test miss in 3 s of the cursor leaving the sprite; see $log" }
}
# The overlay is click-through until its hit test sees the cursor, so the sprite is
# pressed once that line lands; a press 1.5 s late would meet a hover-opened pill.
function Click-Sprite([string]$Case) {
    Aim $Case
    Move-Away $Case
    if (-not (Wait-ForNext $T.hit { $null = [PillWin]::SetCursorPos($script:aim.X, $script:aim.Y) })) { Fail "${Case}: the sprite's click area did not follow the cursor; see $log" }
    if (Count $T.opened) { Fail "${Case}: the pill opened before the press; see $log" }
    $under = [PillWin]::RootAt($script:aim.X, $script:aim.Y)
    if ((Owner-Of $under) -ne $script:proc.Id) { Fail "${Case}: refusing to click the sprite at ($($script:aim.X),$($script:aim.Y)): it would land on $(Describe-Window $under)" }
    Press-Hold $script:aim.X $script:aim.Y $Case
    Move-Away $Case
}
function Open-Pill([string]$Case) {
    Aim $Case
    Move-Away $Case
    $script:pillFrom = Line-Count
    if (-not (Wait-ForNext $T.opened { $null = [PillWin]::SetCursorPos($script:aim.X, $script:aim.Y) })) { Fail "${Case}: hovering the sprite did not open a focused pill in 3 s; see $log" }
}
# Notepad's text: Ctrl+S with our tab in front, then the file holds what is expected (or 3 s pass).
function Notepad-Holds([string]$Expected) {
    if (-not (Foreground-Is)) { return $false }
    [PillWin]::Key(0x11, $false); Press 0x53; [PillWin]::Key(0x11, $true)
    Wait-Until 3 { (Read-Doc) -ceq $Expected }
}
# Types $Word into the open pill and judges it from the pill's opening: $Typed
# passes; the pill closing or handing the keyboard back first fails at once,
# naming the window in front then.
function Type-IntoPill([string]$Word, [string]$Typed, [string]$Case, [string]$Detail = "") {
    [PillWin]::Text($Word)
    $script:verdict = $null
    $null = Wait-Until 3 { ($script:verdict = Get-TypingVerdict @(Get-Content -LiteralPath $log) $script:pillFrom $Typed) }
    if ($script:verdict -eq "typed") { return }
    $front = Describe-Window ([PillWin]::GetForegroundWindow())
    Note-Keys "at $Case"
    if ($script:verdict) { Fail "${Case}: the pill went away before `"$Word`" reached it: $($script:verdict.Substring(5)); in front then: $front$Detail; see $log" }
    Fail "${Case}: the pill never showed text=`"$Word`" focused=true in 3 s; in front: $front$Detail; see $log"
}
# Types $Word with Notepad in front and checks it landed there, not in the pill.
# One character per SendInput, saved and read back before the next: Windows
# keeps a thread's last injected character, so a busy Notepad that reads
# several queued at once types the last one for each ("four" gave "rrrr").
function Type-IntoNotepad([string]$Word, [string]$Case, [string]$Expected) {
    if (-not (Wait-Ui 3 { Foreground-Is })) { Note-Keys "at $Case"; Fail "${Case}: Notepad was not in front to type into; see $focusLog" }
    $steps = Get-TypingSteps (Read-Doc) $Expected $Word
    if ($steps -is [string]) { Fail "${Case}: $($steps.Substring(6)); see $focusLog" }
    for ($k = 0; $k -lt $Word.Length; $k++) {
        [PillWin]::Text($Word.Substring($k, 1))
        if (-not (Notepad-Holds $steps[$k])) { Note-Keys "at $Case"; Fail "${Case}: Notepad holds `"$(Read-Doc)`", not `"$($steps[$k])`"; see $focusLog" }
    }
    if (Count (Draft-Holds $Word)) { Fail "${Case}: the pill took `"$Word`"; see $log" }
}

function Of-Type($Type) { New-Object System.Windows.Automation.PropertyCondition ($UiaEl::ControlTypeProperty), $Type }
# Read-only: every Notepad window's tab names, and our tab, wherever it is. A
# window that closes while it is read is skipped.
function Find-OurTab {
    foreach ($h in [PillWin]::Tops()) {
        $owner = Owner-Of $h
        if ((Proc-Name $owner) -ne "Notepad") { continue }
        try {
            $win = $UiaEl::FromHandle($h)
            $tabs = @($win.FindAll([System.Windows.Automation.TreeScope]::Descendants, (Of-Type ([System.Windows.Automation.ControlType]::TabItem))))
            $ours = $tabs | Where-Object { $_.Current.Name.Contains($docName) } | Select-Object -First 1
            if ($ours) { return @{ Hwnd = $h; ProcessId = $owner; Window = $win; Tab = $ours; Names = [string[]]@($tabs | ForEach-Object { $_.Current.Name }) } }
        } catch [System.Windows.Automation.ElementNotAvailableException] { "find-our-tab: window 0x$($h.ToString('x')) closed while read" | Add-Content -LiteralPath $focusLog }
    }
    $null
}
# The tab names in one Notepad window, read through UI Automation. A window
# already gone has none.
function Get-TabNames([IntPtr]$H) {
    try {
        [string[]]@($UiaEl::FromHandle($H).FindAll([System.Windows.Automation.TreeScope]::Descendants, (Of-Type ([System.Windows.Automation.ControlType]::TabItem))) | ForEach-Object { $_.Current.Name })
    } catch [System.Windows.Automation.ElementNotAvailableException] { [string[]]@() }
}
# Closes our tab by its own close button, so Notepad's session forgets it; a
# prompt about our unsaved scratch text gets Don't save. Returns "" or what failed.
function Close-OurNotepad {
    $found = Find-OurTab
    if (-not $found) { return "" }
    $plan = Get-NotepadPlan $found.Names $docName $notepadWasRunning
    if ($plan.StartsWith("fail: ")) { return $plan.Substring(6) }
    $close = $found.Tab.FindFirst([System.Windows.Automation.TreeScope]::Descendants, (Of-Type ([System.Windows.Automation.ControlType]::Button)))
    if (-not $close) { return "our tab $docName shows no close button; close it by hand" }
    $close.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    if (-not (Wait-Ui 3 { -not (Find-OurTab) })) {
        $nameIs = New-Object System.Windows.Automation.PropertyCondition ($UiaEl::NameProperty), "Don't save"
        $dont = if ([PillWin]::IsWindow($found.Hwnd)) { $found.Window.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $nameIs) } else { $null }
        if ($dont) { $dont.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke() }
        if (-not (Wait-Ui 3 { -not (Find-OurTab) })) { return "our tab $docName is still open; close it by hand" }
    }
    if ($notepadWasRunning) { return "" }
    # This run opened Notepad. Win11 Notepad is one process for every window, so
    # only our own window may go, and only while each tab left in it was there
    # at launch (a restored session). Closed, never killed.
    if (-not [PillWin]::IsWindow($script:hwnd)) { return "" }
    $left = Get-TabNames $script:hwnd
    $verdict = Get-WindowVerdict $script:launchTabs $left $docName
    if ($verdict.StartsWith("leave")) {
        "note: cleanup left the Notepad window open: $($verdict.Substring(7))" | Tee-Object -FilePath $results -Append | Write-Output
        return ""
    }
    $null = [PillWin]::PostMessage($script:hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)
    if (-not (Wait-Ui 3 { -not [PillWin]::IsWindow($script:hwnd) })) { return "the Notepad window this run opened did not close in 3 s; it may be asking about a tab restored at launch" }
    ""
}

# The middle of the open pill's text field on screen, read through UI Automation
# from Fidget's own windows, or $null. A closed pill can stay in the page,
# disabled and hidden, so only an enabled, on-screen field counts.
function Find-Pill {
    $enabled = New-Object System.Windows.Automation.PropertyCondition ($UiaEl::IsEnabledProperty), $true
    $edit = [System.Windows.Automation.AndCondition]::new([System.Windows.Automation.Condition[]]@((Of-Type ([System.Windows.Automation.ControlType]::Edit)), $enabled))
    foreach ($h in [PillWin]::Tops()) {
        if ((Owner-Of $h) -ne $script:proc.Id) { continue }
        try {
            foreach ($field in $UiaEl::FromHandle($h).FindAll([System.Windows.Automation.TreeScope]::Descendants, $edit)) {
                if ($field.Current.IsOffscreen) { continue }
                $b = $field.Current.BoundingRectangle
                if (-not $b.IsEmpty) { return @{ X = [int]($b.X + $b.Width / 2); Y = [int]($b.Y + $b.Height / 2) } }
            }
        } catch [System.Windows.Automation.ElementNotAvailableException] { continue }
    }
    $null
}
# A click on the pill once the overlay's hit test takes clicks there (the pill is
# off the art, so the line says miss, OFF), and only if Fidget's window is then under the point.
function Click-Pill([string]$Case) {
    if (-not (Wait-Ui 3 { ($script:pill = Find-Pill) })) { Fail "${Case}: no enabled pill text field in Fidget's windows through UI Automation in 3 s" }
    if (-not (Wait-ForNext $T.control { $null = [PillWin]::SetCursorPos($script:pill.X, $script:pill.Y) })) { Fail "${Case}: the pill's click area did not follow the cursor; see $log" }
    $under = [PillWin]::RootAt($script:pill.X, $script:pill.Y)
    if ((Owner-Of $under) -ne $script:proc.Id) { Fail "${Case}: refusing to click the pill at ($($script:pill.X),$($script:pill.Y)): it would land on $(Describe-Window $under)" }
    Press-Hold $script:pill.X $script:pill.Y $Case
}
# A small window of the script's own, on its own thread, for the window that
# closes while the pill is open (case 5). Nothing else is ever closed this way.
Add-Type -ReferencedAssemblies System.Windows.Forms @"
using System;
using System.Threading;
using System.Windows.Forms;
public static class PillGone {
    static Form form;
    public static IntPtr Open(string title, int x, int y) {
        ManualResetEvent shown = new ManualResetEvent(false);
        IntPtr hwnd = IntPtr.Zero;
        Thread t = new Thread(delegate() {
            form = new Form();
            form.Text = title;
            form.StartPosition = FormStartPosition.Manual;
            form.SetBounds(x, y, 360, 220);
            form.Shown += delegate { hwnd = form.Handle; shown.Set(); };
            Application.Run(form);
        });
        t.SetApartmentState(ApartmentState.STA);
        t.IsBackground = true;
        t.Start();
        shown.WaitOne(3000);
        return hwnd;
    }
    public static void Close() {
        Form f = form;
        if (f != null && !f.IsDisposed) { f.BeginInvoke(new MethodInvoker(f.Close)); }
    }
}
"@
Note-Step "window-gone helper compiled"
# Counts first, closes the pill with $Key, then waits 3 s for the closed draft
# and 3 s more for Fidget's hand-back line of the $Back kind.
function Close-HandsBack([uint16]$Key, [string]$Back, [string]$Case) {
    $backs = Count $T[$Back]
    if (-not (Wait-ForNext $T.closed { Press $Key })) { Fail "${Case}: the pill did not close; see $log" }
    if (-not (Wait-LogCount $T[$Back] $backs)) { Note-Keys "at $Case"; Fail "${Case}: the pill closed but Fidget printed no $Back hand-back line; see $log" }
}

$startMs = Get-NowMs
$cleanupFailed = $false
# One UI Automation subscription for the whole run: subscribing on the desktop
# root costs time on each wait that would otherwise make its own.
$script:uia = [FidgetUiaSignal]::new(0)
Note-Step "setup done (UI Automation subscribed)"
try {
    # Our window is the one showing our file: Notepad titles a window by its open
    # tab, and a new tab opens in front. The window and its process come from there.
    $null = Start-Process -FilePath notepad.exe -ArgumentList "`"$doc`""
    $found = Wait-UiaChange 0 3 { [PillWin]::Tops() | Where-Object { [PillWin]::Title($_).Contains($docName) } | Select-Object -First 1 } $script:uia
    if (-not $found) { Fail "Notepad did not show $docName in 3 s" }
    Note-Step "notepad window found"
    $script:hwnd = [IntPtr]$found
    $script:notepad = Get-Process -Id (Owner-Of $script:hwnd)
    if ($script:notepad.ProcessName -ne "Notepad") { Fail "the $docName window belongs to $($script:notepad.ProcessName), not Notepad" }
    $ourTab = Find-OurTab
    if (-not $ourTab) { Fail "Notepad shows no tab named $docName to UI Automation" }
    $plan = Get-NotepadPlan $ourTab.Names $docName $notepadWasRunning
    if ($plan.StartsWith("fail: ")) { Fail $plan.Substring(6) }
    # The tabs at launch, read through UI Automation, decide whether cleanup may close the window.
    $script:launchTabs = Get-TabNames $script:hwnd
    "notepad: launch tabs `"$($script:launchTabs -join '", "')`"" | Add-Content -LiteralPath $focusLog
    "notepad: process $($script:notepad.Id), tabs $($ourTab.Names.Count), was running $notepadWasRunning; cleanup will $plan" | Add-Content -LiteralPath $focusLog
    if ([PillWin]::IsIconic($script:hwnd)) { Fail "the Notepad window holding $docName is minimized; the run leaves it that way" }
    Note-Step "notepad tabs read"
    # A window already open keeps its place. One this run opened goes on a monitor
    # no fullscreen app covers (the sprite starts on the primary), and the
    # fullscreen app itself is never moved, minimized or closed.
    if (-not $notepadWasRunning) {
        $place = Get-NotepadPlacement @(Get-MonitorMap $fullMonitors)
        $null = [PillWin]::MoveWindow($script:hwnd, $place.X, $place.Y, $place.W, $place.H, $true)
        "notepad: placed on $($place.Name) at ($($place.X),$($place.Y)) $($place.W)x$($place.H)$(if (-not $place.Free) { '; every monitor has a fullscreen window, so it comes to the front over one' })" | Add-Content -LiteralPath $focusLog
    }
    # Windows can refuse a background process the foreground; Bring joins the
    # foreground thread's input for the call, and a click on our own tab is the last resort.
    $brought = [PillWin]::Bring($script:hwnd)
    "notepad: brought to the front: $brought" | Add-Content -LiteralPath $focusLog
    if (-not (Wait-Ui 3 { Foreground-Is })) {
        $b = $ourTab.Tab.Current.BoundingRectangle
        $tabX = [int]($b.X + $b.Width / 3)
        $tabY = [int]($b.Y + $b.Height / 2)
        $under = [PillWin]::RootAt($tabX, $tabY)
        if ((Owner-Of $under) -ne $script:notepad.Id) { Fail "Notepad did not take focus, and its tab at ($tabX,$tabY) is under $(Describe-Window $under)" }
        # No Fidget yet to trace the press, so this click is down and up at once.
        $null = [PillWin]::SetCursorPos($tabX, $tabY)
        [PillWin]::Button($false); [PillWin]::Button($true)
        if (-not (Wait-Ui 3 { Foreground-Is })) { Note-Keys "focusing Notepad"; Fail "Notepad did not take focus; see $focusLog" }
    }
    # A tab click leaves the keys on the tab strip; UI Automation moves them to the text.
    $text = $UiaEl::FromHandle($script:hwnd).FindFirst([System.Windows.Automation.TreeScope]::Descendants, (Of-Type ([System.Windows.Automation.ControlType]::Document)))
    if (-not $text) { Fail "Notepad shows no text area to UI Automation" }
    $text.SetFocus()
    if (-not (Wait-Ui 3 { Keys-InNotepad })) { Note-Keys "focusing Notepad's text"; Fail "Notepad's text area did not take the keys; see $focusLog" }
    [PillWin]::Key(0x11, $false); Press 0x24; [PillWin]::Key(0x11, $true)
    Note-Step "notepad in front with the keys"

    # Fidget's private folders, set after Notepad starts so Notepad keeps the real ones.
    $null = Initialize-ScenarioData $Out @{ hide_in_fullscreen = $false }
    $q = { param($p) ($p -replace '\\', '/') }
    $harnessTag = "count=$(& $q $marks)"
    $env:FIDGET_HARNESS = "$bashPath $(& $q $root)/scripts/scenarios/fixture-harness.sh $(& $q $TestBin) script=slow $harnessTag"
    $env:FIDGET_DIRECTOR_WAKE_SECS = "600"
    $env:FIDGET_DIRECTOR_API_KEY = "x"
    $env:FIDGET_TRACE_FRAMES = "1"
    $env:FIDGET_TRACE_BUBBLE = "1"
    $env:FIDGET_TRACE_HITTEST = "1"
    $env:FIDGET_CHARACTER = "bmo"
    $env:FIDGET_CHARACTERS = $chars
    # WebView2's profile lives in one folder every run reuses (WebView2 reads
    # WEBVIEW2_USER_DATA_FOLDER over the app's own choice), so only the first
    # run on a PC makes it cold. Fidget's data folder stays fresh per run.
    $env:WEBVIEW2_USER_DATA_FOLDER = $webviewProfile
    # Each startup step waits 3 s from the one before it, woken by each line of
    # Fidget's piped stderr.
    $script:proc = Start-FidgetPiped $Bin @() (Join-Path $Out "app.log") (Join-Path $Out "app.err")
    Note-Step "fidget launched"
    if (-not (Wait-LogCount $T.frame 0)) { Fail-Startup "Fidget drew no frame in 3 s of its launch; see $log" }
    if (-not (Wait-LogCount $T.loaded 0)) { Fail-Startup "the overlay's page did not load in 3 s of the first frame; see $log" }
    # The pre-launch check reads this lockfile, so it must be there now.
    if (-not (Test-Path -LiteralPath $profileLock)) { Fail "WebView2 loaded the page but holds no $profileLock, so the pre-launch profile check can't see a running WebView2" }
    if (-not (Wait-Until 3 { Test-OverlayListening (Get-Content -LiteralPath $log) })) { Fail-Startup "the overlay did not report in 3 s of its page loading; see $log" }
    if (-not (Wait-Ui 3 { Foreground-Is })) { Note-Keys "as Fidget started"; Fail "Notepad lost focus as Fidget started; see $focusLog" }
    Note-Step "fidget ready"

    # 1. No pill open: a sprite click leaves Notepad in front, and typing lands there.
    Click-Sprite sprite-click
    if (-not (Foreground-Is)) { Note-Keys "after the sprite click"; Fail "sprite-click: Notepad was not in front after the click; see $focusLog" }
    Type-IntoNotepad one sprite-click oneabc
    Note-Keys "after the sprite click and one"
    Ok "sprite-click: typing stayed in Notepad"

    # 2. Hover opens the pill, Fidget traces its SetForegroundWindow, and "two" lands in the pill.
    $opens = Count $T.pillOpen
    Open-Pill hover-type
    if (-not (Wait-LogCount $T.pillOpen $opens)) { Fail "hover-type: the pill opened but Fidget printed no pill open line; see $log" }
    $line = Get-Content -LiteralPath $log | Where-Object { $_ -match $T.pillOpen } | Select-Object -Last 1
    $null = $line -match $T.pillOpen
    $setResult = $Matches[2]
    $overlayAhead = if ($Matches[1] -eq $Matches[3]) { "yes" } else { "no" }
    "pill-open: SetForegroundWindow returned $setResult; the overlay is in front: $overlayAhead; line: $($line.Trim())" | Tee-Object -FilePath $results -Append | Write-Output
    Note-Keys "with the pill open"
    Type-IntoPill two $T.two hover-type " (SetForegroundWindow returned $setResult, overlay in front: $overlayAhead)"
    Ok "hover-type: the pill took `"two`" with no click"

    # 3. Escape closes the pill and typing goes back to Notepad (#1577).
    Close-HandsBack 0x1B restore escape
    Type-IntoNotepad three escape onethreeabc
    Ok "escape: the pill closed and typing went back to Notepad"

    # 4. Enter sends the pill's text, closes it, and typing goes back to Notepad.
    Open-Pill enter
    Type-IntoPill five $T.five enter
    Close-HandsBack 0x0D restore enter
    Type-IntoNotepad six enter onethreesixabc
    Ok "enter: the pill sent, closed, and typing went back to Notepad"

    # 5. The window in front at pill-open closes while the pill is open: Escape
    # steps aside, and typing lands in Notepad, the next window in front.
    # On Notepad's monitor, so a fullscreen app elsewhere stays untouched.
    $work = [PillWin]::Monitor($script:hwnd).rcWork
    $script:gone = [PillGone]::Open("fidget-pill-gone-$docName", $work.L + 80, $work.T + 80)
    if ($script:gone -eq [IntPtr]::Zero) { Fail "gone: the script's own window did not show in 3 s" }
    $null = [PillWin]::Bring($script:gone)
    if (-not (Wait-Ui 3 { [PillWin]::GetForegroundWindow() -eq $script:gone })) { Note-Keys "at gone"; Fail "gone: the script's own window did not come to the front; see $focusLog" }
    Open-Pill gone
    [PillGone]::Close()
    if (-not (Wait-Ui 3 { -not [PillWin]::IsWindow($script:gone) })) { Fail "gone: the script's own window did not close in 3 s" }
    Close-HandsBack 0x1B stepAside gone
    Type-IntoNotepad seven gone onethreesixsevenabc
    Ok "gone: with the window in front closed, typing stepped aside to Notepad"

    # 6. A click into Notepad below its last line closes the empty pill, and
    # typing lands at the end of Notepad's text.
    Open-Pill click-away
    $r = New-Object PillWin+RECT
    $null = [PillWin]::GetWindowRect($script:hwnd, [ref]$r)
    # Both lines within 3 s of the release Fidget logged: the field's own blur
    # timer closes the pill 3000 ms later too, so the closed line alone proves nothing.
    $blurs = Count $T.blurAway
    $closes = Count $T.closed
    Click-Notepad ([int](($r.L + $r.R) / 2)) ([int]($r.T + ($r.B - $r.T) * 7 / 10)) click-away
    $both = Wait-OnChanges 3 (Split-Path -Parent $log) { (Count $T.blurAway) -gt $blurs -and (Count $T.closed) -gt $closes }
    if (-not $both) {
        if ((Count $T.blurAway) -le $blurs) { Fail "click-away: no 'window blur foreground=false' line within 3 s of Fidget logging the release in Notepad; see $log" }
        Fail "click-away: the pill did not close within 3 s of Fidget logging the release in Notepad; see $log"
    }
    Type-IntoNotepad four click-away onethreesixsevenabcfour
    Ok "click-away: the click moved typing to Notepad"

    # 7. Hover opens a pill of its own, a click on it keeps the keyboard there
    # (judged from the opening), and Escape hands typing back to Notepad (#1577).
    Open-Pill click-pill
    Click-Pill click-pill
    Type-IntoPill eight $T.eight click-pill
    Ok "click-pill: the click put typing in the pill"
    Close-HandsBack 0x1B restore click-pill
    Type-IntoNotepad nine click-pill onethreesixsevenabcfournine
    Ok "click-pill: Escape handed typing back to Notepad"

    "PASS: evidence in $Out ($((Get-NowMs) - $startMs) ms)" | Tee-Object -FilePath $results -Append | Write-Output
} finally {
    [PillGone]::Close()
    Stop-FidgetTree $script:proc $harnessTag
    $script:proc = $null
    Note-Step "cleanup: fidget stopped"
    $why = Close-OurNotepad
    Note-Step "cleanup: notepad done"
    if ($script:uia) { $script:uia.Dispose() }
    if ($why) {
        [Console]::Error.WriteLine("FAIL: cleanup: $why")
        "FAIL: cleanup: $why" | Add-Content -LiteralPath $results
        $cleanupFailed = $true
    }
    foreach ($d in @("tmp", "home")) { Remove-Item -Recurse -Force -LiteralPath (Join-Path $Out $d) -ErrorAction SilentlyContinue }
}
if ($cleanupFailed) { exit 1 }
