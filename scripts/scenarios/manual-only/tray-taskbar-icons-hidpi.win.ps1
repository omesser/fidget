#!/usr/bin/env pwsh
# Scenario: tray-taskbar-icons-hidpi (Windows)
# On screen: launches Fidget with the Static Director. Its sprite, tray icon
#   and taskbar button appear, and nothing takes focus. The scenario captures
#   two small regions, the taskbar button and the tray icon. Fidget quits when
#   the scenario ends. It never changes the display scale or any setting: set
#   the primary display to 150% or 200% yourself first (sign out and back in
#   if Windows asks), and put it back afterwards.
# Input: none.
# Duration: a few seconds, 60 s at most.
# Grants: a desktop session with the taskbar showing. UI Automation for the
#   terminal that runs it. Fidget's tray icon must not sit in the hidden-icons
#   overflow.
# Asserts: the anchor window is DPI aware; its WM_GETICON ICON_SMALL and
#   GCLP_HICONSM icons are SM_CXSMICON x SM_CYSMICON at Fidget's own system DPI
#   (GetSystemMetricsForDpi), which is what Fidget asked GetSystemMetrics for;
#   its ICON_BIG and GCLP_HICON icons are 256x256; the taskbar button and the
#   tray icon are found and captured. Whether they look sharp is for the eye:
#   compare taskbar-button.png and tray-icon.png with a 100% run.
# Skips (exit 2) when Fidget is already running, or when the icon checks pass
#   but Fidget's system DPI is 96 (100%), since that run proves nothing about
#   HiDPI.
# Fixture: FIDGET_SCENARIO_ICONS="aware=2 dpi=144 taskbar=144 small=24x24
#   classsmall=24x24 big=256x256 classbig=256x256" judges those readings and
#   launches nothing.
#
# Usage: tray-taskbar-icons-hidpi.win.ps1 --go <fidget binary>
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
    Write-Error "usage: tray-taskbar-icons-hidpi.win.ps1 --go <fidget binary>"
    exit 1
}

$script:Evidence = $null

function Fail([string]$Message) {
    [Console]::Error.WriteLine("FAIL: $Message")
    if ($script:Evidence) { [Console]::Error.WriteLine("evidence in $($script:Evidence)") }
    exit 1
}

Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class TrayIconsProbe {
    [DllImport("user32.dll")] static extern bool SetProcessDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] static extern IntPtr GetThreadDpiAwarenessContext();
    [DllImport("user32.dll")] static extern IntPtr GetWindowDpiAwarenessContext(IntPtr hWnd);
    [DllImport("user32.dll")] static extern int GetAwarenessFromDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] static extern int GetSystemMetricsForDpi(int index, uint dpi);
    [DllImport("user32.dll")] static extern uint GetSystemDpiForProcess(IntPtr process);
    [DllImport("user32.dll")] static extern IntPtr MonitorFromWindow(IntPtr hWnd, uint flags);
    [DllImport("shcore.dll")] static extern int GetDpiForMonitor(IntPtr monitor, int type, out uint x, out uint y);
    [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string title);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr FindWindowEx(IntPtr parent, IntPtr after, string cls, string title);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll")] static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam, uint flags, uint ms, out IntPtr result);
    [DllImport("user32.dll", EntryPoint = "GetClassLongPtrW")] static extern IntPtr GetClassLongPtr64(IntPtr hWnd, int index);
    [DllImport("user32.dll", EntryPoint = "GetClassLongW")] static extern uint GetClassLong32(IntPtr hWnd, int index);
    [StructLayout(LayoutKind.Sequential)] struct ICONINFO { public bool fIcon; public int xHotspot; public int yHotspot; public IntPtr hbmMask; public IntPtr hbmColor; }
    [StructLayout(LayoutKind.Sequential)] struct BITMAP { public int bmType; public int bmWidth; public int bmHeight; public int bmWidthBytes; public ushort bmPlanes; public ushort bmBitsPixel; public IntPtr bmBits; }
    [DllImport("user32.dll")] static extern bool GetIconInfo(IntPtr icon, out ICONINFO info);
    [DllImport("gdi32.dll")] static extern int GetObject(IntPtr h, int size, out BITMAP bitmap);
    [DllImport("gdi32.dll")] static extern bool DeleteObject(IntPtr h);

    const int SM_CXSMICON = 49, SM_CYSMICON = 50;
    const uint PROCESS_QUERY_LIMITED_INFORMATION = 0x1000, MONITOR_DEFAULTTONEAREST = 2;
    const int MDT_EFFECTIVE_DPI = 0;
    const uint WM_GETICON = 0x7F, SMTO_ABORTIFHUNG = 2;
    public const int ICON_SMALL = 0, ICON_BIG = 1, GCLP_HICONSM = -34, GCLP_HICON = -14;

    // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, so UI Automation rectangles
    // and CopyFromScreen both speak physical pixels. The process call fails
    // when a manifest already set it; the thread call covers that case.
    static readonly IntPtr PerMonitorV2 = new IntPtr(-4);
    public static int BeAware() {
        if (!SetProcessDpiAwarenessContext(PerMonitorV2)) { SetThreadDpiAwarenessContext(PerMonitorV2); }
        return GetAwarenessFromDpiAwarenessContext(GetThreadDpiAwarenessContext());
    }

    public static string SmallIconSize(uint dpi) {
        return GetSystemMetricsForDpi(SM_CXSMICON, dpi) + "x" + GetSystemMetricsForDpi(SM_CYSMICON, dpi);
    }

    public static IntPtr FindAnchor(uint pid) {
        IntPtr h = IntPtr.Zero;
        while ((h = FindWindowEx(IntPtr.Zero, h, "FidgetAnchor", null)) != IntPtr.Zero) {
            uint owner;
            GetWindowThreadProcessId(h, out owner);
            if (owner == pid) return h;
        }
        return IntPtr.Zero;
    }

    public static IntPtr[] Taskbars() {
        var all = new System.Collections.Generic.List<IntPtr>();
        IntPtr h = FindWindow("Shell_TrayWnd", null);
        if (h != IntPtr.Zero) all.Add(h);
        h = IntPtr.Zero;
        while ((h = FindWindowEx(IntPtr.Zero, h, "Shell_SecondaryTrayWnd", null)) != IntPtr.Zero) all.Add(h);
        return all.ToArray();
    }

    // 0 unaware, 1 system aware, 2 per monitor.
    public static int Awareness(IntPtr hWnd) {
        return GetAwarenessFromDpiAwarenessContext(GetWindowDpiAwarenessContext(hWnd));
    }

    // System DPI is per process since Windows 10 1803: the one Fidget's own
    // GetSystemMetrics answered at, not this script's.
    public static uint SystemDpiOf(uint pid) {
        IntPtr process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
        if (process == IntPtr.Zero) return 0;
        try { return GetSystemDpiForProcess(process); } finally { CloseHandle(process); }
    }

    public static uint MonitorDpiOf(IntPtr hWnd) {
        uint x, y;
        return GetDpiForMonitor(MonitorFromWindow(hWnd, MONITOR_DEFAULTTONEAREST), MDT_EFFECTIVE_DPI, out x, out y) == 0 ? x : 0;
    }

    public static IntPtr SentIcon(IntPtr hWnd, int which) {
        IntPtr icon;
        if (SendMessageTimeout(hWnd, WM_GETICON, new IntPtr(which), IntPtr.Zero, SMTO_ABORTIFHUNG, 2000, out icon) == IntPtr.Zero) return IntPtr.Zero;
        return icon;
    }

    public static IntPtr ClassIcon(IntPtr hWnd, int index) {
        return IntPtr.Size == 8 ? GetClassLongPtr64(hWnd, index) : new IntPtr((int)GetClassLong32(hWnd, index));
    }

    // An HICON is a session-wide handle, so another process's icon reads here.
    public static string IconSize(IntPtr icon) {
        if (icon == IntPtr.Zero) return "none";
        ICONINFO info;
        if (!GetIconInfo(icon, out info)) return "unreadable";
        BITMAP bitmap;
        bool color = info.hbmColor != IntPtr.Zero;
        int read = GetObject(color ? info.hbmColor : info.hbmMask, Marshal.SizeOf(typeof(BITMAP)), out bitmap);
        if (info.hbmColor != IntPtr.Zero) DeleteObject(info.hbmColor);
        if (info.hbmMask != IntPtr.Zero) DeleteObject(info.hbmMask);
        if (read == 0) return "unreadable";
        // A monochrome icon stacks its AND and XOR masks in one bitmap.
        return bitmap.bmWidth + "x" + (color ? bitmap.bmHeight : bitmap.bmHeight / 2);
    }
}
"@

function Scale([int]$Dpi) { "$([int][Math]::Round($Dpi * 100 / 96))%" }

# Judges one line of readings, live or from the fixture, and returns the exit
# code: 1 on a wrong icon, 2 when right but at 100%, else 0. It prints through
# [Console], since Write-Output would land in the return value.
function Test-Icons([string]$Readings) {
    $r = @{}
    foreach ($pair in $Readings.Trim() -split '\s+') {
        if ($pair -match '^(\w+)=(.*)$') { $r[$Matches[1]] = $Matches[2] }
    }
    foreach ($key in @('aware', 'dpi', 'taskbar', 'small', 'classsmall', 'big', 'classbig')) {
        if (-not $r.ContainsKey($key)) { Fail "the readings have no ${key}: $Readings" }
    }
    $aware = [int]$r.aware
    $dpi = [int]$r.dpi
    $taskbar = [int]$r.taskbar
    if ($dpi -le 0) { Fail "could not read Fidget's system DPI (GetSystemDpiForProcess needs Windows 10 1803)" }
    [Console]::Out.WriteLine("scale: Fidget's system DPI is $dpi ($(Scale $dpi)), the taskbar's display $taskbar ($(Scale $taskbar)); anchor awareness $aware")

    $failures = @()
    if ($aware -eq 0) { $failures += "the anchor window is DPI unaware, so Windows stretches its icons" }
    # GetSystemMetrics answers at 96 DPI to an unaware thread, else at the system DPI.
    $asked = if ($aware -eq 0) { 96 } else { $dpi }
    $small = [TrayIconsProbe]::SmallIconSize([uint32]$asked)
    $why = "(SM_CXSMICON x SM_CYSMICON at $asked dpi)"
    foreach ($check in @(
            @('small', 'WM_GETICON ICON_SMALL', $small, $why),
            @('classsmall', 'GCLP_HICONSM', $small, $why),
            @('big', 'WM_GETICON ICON_BIG', '256x256', '(the jumbo slot)'),
            @('classbig', 'GCLP_HICON', '256x256', '(the jumbo slot)'))) {
        $got = $r[$check[0]]
        if ($got -ceq 'none') {
            $failures += "$($check[1]) returned no icon"
        } elseif ($got -cne $check[2]) {
            $failures += "$($check[1]) is $got, want $($check[2]) $($check[3])"
        } else {
            [Console]::Out.WriteLine("ok: $($check[1]) is $got $($check[3])")
        }
    }
    foreach ($f in $failures) { [Console]::Error.WriteLine("FAIL: $f") }
    if ($failures.Count -gt 0) { return 1 }

    if ($dpi -le 96) {
        $skip = "SKIP: Fidget's system DPI is $dpi ($(Scale $dpi)), so its small icon is the 100% size. Set the primary display to 150% or 200% and rerun."
        if ($taskbar -gt 96) {
            $skip += " The taskbar's display is at $(Scale $taskbar) but Fidget took 100% as its system DPI at launch: sign out and back in, then rerun."
        }
        [Console]::Error.WriteLine($skip)
        return 2
    }
    if ($taskbar -ne $dpi) {
        [Console]::Out.WriteLine("note: the taskbar's display runs at $(Scale $taskbar), not Fidget's system $(Scale $dpi); the shell scales the button from these icons")
    }
    return 0
}

if ($env:FIDGET_SCENARIO_ICONS) {
    $code = Test-Icons $env:FIDGET_SCENARIO_ICONS
    if ($code -eq 0) { Write-Output "PASS: fixture icons" }
    exit $code
}

# A Fidget already here owns a taskbar button and tray icon of its own.
$already = @(Get-Process -Name fidget -ErrorAction SilentlyContinue | ForEach-Object { "fidget $($_.Id)" })
if ($already.Count) {
    [Console]::Error.WriteLine("SKIP: Fidget is already running: $($already -join ', ')")
    exit 2
}

$root = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
. (Join-Path $PSScriptRoot "..\helpers\wait-helpers.win.ps1")
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing

$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$out = Join-Path $env:TEMP "fidget-scenario-tray-taskbar-icons-hidpi-$stamp"
$script:Evidence = $out
$data = Initialize-ScenarioData $out
$log = Join-Path $out "app.log"
$err = Join-Path $out "app.err"

$leafAware = [TrayIconsProbe]::BeAware()
if ($leafAware -ne 2) { Write-Output "note: this script runs at DPI awareness $leafAware, so its crops may be offset" }

$env:FIDGET_DIRECTOR_API_KEY = "x"
$env:FIDGET_CHARACTERS = Join-Path $root "characters"
Remove-Item Env:FIDGET_NO_ANCHOR -ErrorAction SilentlyContinue

# The first button on any taskbar whose name matches, the primary's first.
function Find-TaskbarElement([string]$NameLike) {
    $cond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::Button)
    foreach ($taskbar in [TrayIconsProbe]::Taskbars()) {
        $buttons = [System.Windows.Automation.AutomationElement]::FromHandle($taskbar).FindAll(
            [System.Windows.Automation.TreeScope]::Descendants, $cond)
        $found = $buttons | Where-Object { $_.Current.Name -clike $NameLike } | Select-Object -First 1
        if ($found) { return $found }
    }
    return $null
}

# A few pixels of taskbar around the element, so the edge of the art shows.
function Save-Crop($Element, [string]$Path) {
    $b = $Element.Current.BoundingRectangle
    $pad = 6
    $x = [int]$b.X - $pad; $y = [int]$b.Y - $pad
    $w = [int]$b.Width + 2 * $pad; $h = [int]$b.Height + 2 * $pad
    $bitmap = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $g.CopyFromScreen($x, $y, 0, 0, (New-Object System.Drawing.Size $w, $h))
        $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    } finally {
        $g.Dispose(); $bitmap.Dispose()
    }
    Write-Output "ok: $(Split-Path -Leaf $Path) covers $x,$y ${w}x$h around `"$($Element.Current.Name)`""
}

$proc = $null
$systemDpi = 0
$code = 0
try {
    $proc = Start-FidgetPiped $Bin @() $log $err
    $script:proc = $proc
    if (-not (Wait-ForUp $err)) { Fail "Fidget never wrote its overlay line; see $err" }
    if (-not (Wait-ForLine 10 $err '^anchor: 1x1 at ')) { Fail "Fidget never placed its anchor window; see $err" }
    $anchor = [TrayIconsProbe]::FindAnchor([uint32]$proc.Id)
    if ($anchor -eq [IntPtr]::Zero) { Fail "no FidgetAnchor window belongs to pid $($proc.Id)" }

    $taskbar = @([TrayIconsProbe]::Taskbars())[0]
    $systemDpi = [int][TrayIconsProbe]::SystemDpiOf([uint32]$proc.Id)
    $readings = @(
        "aware=$([TrayIconsProbe]::Awareness($anchor))",
        "dpi=$systemDpi",
        "taskbar=$([TrayIconsProbe]::MonitorDpiOf($taskbar))",
        "small=$([TrayIconsProbe]::IconSize([TrayIconsProbe]::SentIcon($anchor, [TrayIconsProbe]::ICON_SMALL)))",
        "classsmall=$([TrayIconsProbe]::IconSize([TrayIconsProbe]::ClassIcon($anchor, [TrayIconsProbe]::GCLP_HICONSM)))",
        "big=$([TrayIconsProbe]::IconSize([TrayIconsProbe]::SentIcon($anchor, [TrayIconsProbe]::ICON_BIG)))",
        "classbig=$([TrayIconsProbe]::IconSize([TrayIconsProbe]::ClassIcon($anchor, [TrayIconsProbe]::GCLP_HICON)))"
    ) -join " "
    Set-Content -LiteralPath (Join-Path $out "readings.txt") -Value $readings -Encoding ascii
    $code = Test-Icons $readings

    # Case-sensitive: the anchor's button is "Fidget", the tray icon's tooltip "fidget".
    $button = Wait-UiaChange 0 5 { Find-TaskbarElement "Fidget*" }
    if ($button) { Save-Crop $button (Join-Path $out "taskbar-button.png") } else {
        [Console]::Error.WriteLine("FAIL: no Fidget button on the taskbar after 5 s")
        $code = 1
    }
    $tray = Wait-UiaChange 0 5 { Find-TaskbarElement "fidget*" }
    if ($tray) { Save-Crop $tray (Join-Path $out "tray-icon.png") } else {
        [Console]::Error.WriteLine("FAIL: no fidget tray icon on the taskbar after 5 s; is it in the hidden-icons overflow?")
        $code = 1
    }
} finally {
    Stop-FidgetTree $proc ""
}

if ($code -eq 0) { Write-Output "PASS: the anchor icons fit $(Scale $systemDpi); compare the crops by eye" }
Write-Output "evidence in $out"
exit $code
