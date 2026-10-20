$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
if (-not (Test-Path (Join-Path $Root "src-tauri"))) { $Root = (Get-Location).Path }
Set-Location $Root

function Info($m) { Write-Host "[INFO] $m" -ForegroundColor Green }
function Fail($m) {
  Write-Host "[FAIL] $m" -ForegroundColor Red
  if ($script:AppProc -and -not $script:AppProc.HasExited) {
    Stop-Process -Id $script:AppProc.Id -Force -ErrorAction SilentlyContinue
  }
  exit 1
}

$Bin = $env:FIDGET_VERIFY_BIN
if (-not $Bin) {
  $debug = Join-Path $Root "target\debug\fidget.exe"
  $release = Join-Path $Root "target\release\fidget.exe"
  if (Test-Path $debug) { $Bin = $debug }
  elseif (Test-Path $release) { $Bin = $release }
  else { Fail "binary missing. cargo build -p fidget, or set FIDGET_VERIFY_BIN" }
}

$Stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$Out = Join-Path $Root ".verify\anchor-win-$Stamp"
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Log = Join-Path $Out "app.log"

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Collections.Generic;
using System.Text;
public class AnchorVerify {
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
  public delegate bool MonitorProc(IntPtr hMonitor, IntPtr hdc, ref RECT rc, IntPtr data);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc lpEnumFunc, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumDisplayMonitors(IntPtr hdc, IntPtr clip, MonitorProc lpfnEnum, IntPtr data);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT r);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr hWnd, StringBuilder lp, int nMax);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr hWnd, StringBuilder lp, int nMax);
  public static List<IntPtr> WindowsFor(uint pid) {
    var list = new List<IntPtr>();
    EnumWindows((h, l) => {
      uint wpid; GetWindowThreadProcessId(h, out wpid);
      if (wpid == pid && IsWindowVisible(h)) list.Add(h);
      return true;
    }, IntPtr.Zero);
    return list;
  }
  public static List<RECT> Monitors() {
    var list = new List<RECT>();
    EnumDisplayMonitors(IntPtr.Zero, IntPtr.Zero, (IntPtr h, IntPtr hdc, ref RECT r, IntPtr d) => {
      list.Add(r);
      return true;
    }, IntPtr.Zero);
    return list;
  }
}
"@

# The shared waits wake on each line of the app's piped stderr; a dead app
# fails them ($script:proc).
. (Join-Path $Root "scripts\scenarios\wait-helpers.win.ps1")
$env:FIDGET_TRACE_FRAMES = "1"
$script:AppProc = Start-FidgetPiped $Bin @() (Join-Path $Out "app.out") $Log
Info "pid $($script:AppProc.Id) log $Log"
$script:proc = $script:AppProc
$script:WatchDir = $Out
if (-not (Wait-ForLine 3 $Log '^overlay:')) {
  if ($script:AppProc.HasExited) { Fail "app exited during startup" }
  Fail "app published no overlay line within 3 s"
}
if (-not (Wait-ForLine 3 $Log '^anchor:')) { Fail "app logged no anchor line within 3 s" }

function Intersects($a, $b) {
  return ($a.Left -lt $b.Right) -and ($a.Right -gt $b.Left) -and ($a.Top -lt $b.Bottom) -and ($a.Bottom -gt $b.Top)
}

# Whether a visible window of the app counts as anchor pixels on the desktop.
# $win carries Class, Title, Left, Top, Right, Bottom; $monitors are rects.
function Test-AnchorOnDesktop($win, $monitors) {
  # Tao, the window library under Tauri, registers this class for its event
  # loop: a visible 16x16 window at the origin that draws nothing. Not the anchor.
  if ($win.Class -eq "Tao Thread Event Target") { return $false }
  if ($win.Title -eq "Settings") { return $false }
  $w = $win.Right - $win.Left
  $h = $win.Bottom - $win.Top
  $onMonitor = $false
  foreach ($mon in $monitors) {
    if (Intersects $win $mon) { $onMonitor = $true }
    $mw = $mon.Right - $mon.Left
    $mh = $mon.Bottom - $mon.Top
    if ($w -ge ($mw * 0.8) -and $h -ge ($mh * 0.8)) { return $false }
  }
  return $onMonitor
}

$monitors = [AnchorVerify]::Monitors()
if ($monitors.Count -eq 0) { Fail "no monitors" }
$pid32 = [uint32]$script:AppProc.Id
$wins = [AnchorVerify]::WindowsFor($pid32)
$failed = $false
foreach ($hwnd in $wins) {
  $rect = New-Object AnchorVerify+RECT
  [void][AnchorVerify]::GetWindowRect($hwnd, [ref]$rect)
  $titleSb = New-Object System.Text.StringBuilder 512
  $classSb = New-Object System.Text.StringBuilder 256
  [void][AnchorVerify]::GetWindowText($hwnd, $titleSb, $titleSb.Capacity)
  [void][AnchorVerify]::GetClassName($hwnd, $classSb, $classSb.Capacity)
  $title = $titleSb.ToString()
  $class = $classSb.ToString()
  $w = $rect.Right - $rect.Left
  $h = $rect.Bottom - $rect.Top
  Info ("hwnd {0} {1}x{2} at {3},{4} class={5} title={6}" -f $hwnd, $w, $h, $rect.Left, $rect.Top, $class, $title)
  $win = [pscustomobject]@{
    Class = $class; Title = $title
    Left = $rect.Left; Top = $rect.Top; Right = $rect.Right; Bottom = $rect.Bottom
  }
  if (-not (Test-AnchorOnDesktop $win $monitors)) { continue }
  Write-Host "[FAIL] on-desktop hwnd $hwnd ${w}x${h} class=$class title=$title is the anchor surface" -ForegroundColor Red
  $failed = $true
}

Stop-Process -Id $script:AppProc.Id -Force -ErrorAction SilentlyContinue
if ($failed) { exit 1 }
Info "no anchor pixels on a monitor"
