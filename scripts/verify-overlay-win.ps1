#!/usr/bin/env pwsh
# Windows Spatial e2e, twin of verify-overlay-x11.sh. Dual display and a built
# binary ($env:FIDGET_VERIFY_BIN overrides). Missing WDA_EXCLUDEFROMCAPTURE only
# warns: capture exclusion is off by default. Logs under .verify/.

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
if (-not (Test-Path (Join-Path $Root "src-tauri"))) { $Root = (Get-Location).Path }
Set-Location $Root

function Info($m) { Write-Host "[INFO] $m" -ForegroundColor Green }
function Pass($m) { Write-Host "[PASS] $m" -ForegroundColor Green }
function Fail($m) {
  Write-Host "[FAIL] $m" -ForegroundColor Red
  if ($script:AppProc -and -not $script:AppProc.HasExited) {
    Stop-Process -Id $script:AppProc.Id -Force -ErrorAction SilentlyContinue
  }
  Get-Process Notepad -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
  exit 1
}

$Stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$Out = Join-Path $Root ".verify\win-$Stamp"
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Log = Join-Path $Out "app.log"
Info "Output: $Out"

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Collections.Generic;
public class WinVerify {
  public const int GWL_EXSTYLE = -20;
  public const int WS_EX_NOACTIVATE = 0x08000000;
  public const uint WDA_EXCLUDEFROMCAPTURE = 0x11;
  public const uint MOUSEEVENTF_LEFTDOWN = 0x0002;
  public const uint MOUSEEVENTF_LEFTUP = 0x0004;
  public const uint MONITORINFOF_PRIMARY = 1;
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr hWnd, int nIndex);
  [DllImport("user32.dll")] public static extern bool GetWindowDisplayAffinity(IntPtr hWnd, out uint affinity);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, IntPtr lParam);
  public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr hWnd, int X, int Y, int nWidth, int nHeight, bool bRepaint);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int X, int Y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint dwFlags, uint dx, uint dy, uint dwData, UIntPtr dwExtraInfo);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT r);
  [DllImport("user32.dll")] public static extern bool EnumDisplayMonitors(IntPtr hdc, IntPtr lprcClip, MonitorEnumProc lpfnEnum, IntPtr dwData);
  public delegate bool MonitorEnumProc(IntPtr hMonitor, IntPtr hdcMonitor, ref RECT lprcMonitor, IntPtr dwData);
  [DllImport("user32.dll")] public static extern bool GetMonitorInfo(IntPtr hMonitor, ref MONITORINFO lpmi);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct MONITORINFO {
    public int cbSize; public RECT rcMonitor; public RECT rcWork; public uint dwFlags;
  }
  public static List<IntPtr> FindWindowsByPid(uint pid) {
    var list = new List<IntPtr>();
    EnumWindows((h, l) => {
      uint wpid; GetWindowThreadProcessId(h, out wpid);
      if (wpid == pid && IsWindowVisible(h)) list.Add(h);
      return true;
    }, IntPtr.Zero);
    return list;
  }
  public static RECT? GetSecondaryWorkArea() {
    RECT? secondary = null;
    EnumDisplayMonitors(IntPtr.Zero, IntPtr.Zero, (IntPtr h, IntPtr hdc, ref RECT r, IntPtr d) => {
      MONITORINFO mi = new MONITORINFO();
      mi.cbSize = Marshal.SizeOf(typeof(MONITORINFO));
      if (GetMonitorInfo(h, ref mi) && (mi.dwFlags & MONITORINFOF_PRIMARY) == 0) {
        secondary = mi.rcWork;
      }
      return true;
    }, IntPtr.Zero);
    return secondary;
  }
}
"@

$Bin = if ($env:FIDGET_VERIFY_BIN) { $env:FIDGET_VERIFY_BIN } else { Join-Path $Root "target\debug\fidget.exe" }
if (-not (Test-Path $Bin)) { Fail "missing $Bin - build with VsDevCmd first, or set FIDGET_VERIFY_BIN" }
Pass "Binary ready"

$sec = [WinVerify]::GetSecondaryWorkArea()
if ($null -eq $sec) { Fail "No secondary monitor (dual-display required)" }
$secLeft = $sec.Left; $secTop = $sec.Top
$secW = $sec.Right - $sec.Left; $secH = $sec.Bottom - $sec.Top
Info "Secondary work area: ${secLeft},${secTop} ${secW}x${secH}"

$npW = [Math]::Min(900, [Math]::Max(400, $secW - 80))
$npH = [Math]::Min(500, [Math]::Max(300, [int]($secH / 3)))
$npX = $secLeft + [int](($secW - $npW) / 2)
$npY = $secTop + [int]($secH / 4)
Info "Notepad perch at $npX,$npY ${npW}x${npH}"
Start-Process notepad.exe | Out-Null
$npHwnd = [IntPtr]::Zero
$npPid = 0
for ($i = 0; $i -lt 40; $i++) {
  foreach ($p in @(Get-Process -Name Notepad -ErrorAction SilentlyContinue)) {
    if ($p.MainWindowHandle -ne [IntPtr]::Zero) {
      $npHwnd = $p.MainWindowHandle
      $npPid = $p.Id
      break
    }
  }
  if ($npHwnd -ne [IntPtr]::Zero) { break }
  Start-Sleep -Milliseconds 100
}
if ($npHwnd -eq [IntPtr]::Zero) { Fail "Notepad window never appeared" }
[WinVerify]::MoveWindow($npHwnd, $npX, $npY, $npW, $npH, $true) | Out-Null
Start-Sleep -Milliseconds 200
$rect = New-Object WinVerify+RECT
[WinVerify]::GetWindowRect($npHwnd, [ref]$rect) | Out-Null
Pass "Notepad pid=$npPid left=$($rect.Left) top=$($rect.Top)"

$env:FIDGET_TRACE_FRAMES = "1"
$env:FIDGET_CHARACTER = "buddy-bot"
if ($env:FIDGET_TRACE_HITTEST -ne "1") {
  Remove-Item Env:FIDGET_TRACE_HITTEST -ErrorAction SilentlyContinue
}

$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $Bin
$psi.WorkingDirectory = $Root
$psi.UseShellExecute = $false
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError = $true
$psi.CreateNoWindow = $true
$script:AppProc = New-Object System.Diagnostics.Process
$script:AppProc.StartInfo = $psi
$null = $script:AppProc.Start()
$sw = [System.IO.StreamWriter]::new($Log, $false)
$handler = {
  if ($EventArgs.Data) {
    $sw.WriteLine($EventArgs.Data)
    $sw.Flush()
  }
}
Register-ObjectEvent -InputObject $script:AppProc -EventName OutputDataReceived -Action $handler | Out-Null
Register-ObjectEvent -InputObject $script:AppProc -EventName ErrorDataReceived -Action $handler | Out-Null
$script:AppProc.BeginOutputReadLine()
$script:AppProc.BeginErrorReadLine()

function Await-Log([string]$Pattern, [int]$Attempts = 50) {
  for ($i = 0; $i -lt $Attempts; $i++) {
    if ((Test-Path $Log) -and (Select-String -Path $Log -Pattern $Pattern -Quiet -ErrorAction SilentlyContinue)) {
      return $true
    }
    Start-Sleep -Milliseconds 100
  }
  return $false
}

if (-not (Await-Log "frame:" 40)) { Fail "No TRACE frames - $Log" }
Pass "TRACE frames"

$noAct = $false
$affOk = $false
for ($i = 0; $i -lt 25; $i++) {
  foreach ($h in [WinVerify]::FindWindowsByPid([uint32]$script:AppProc.Id)) {
    $ex = [WinVerify]::GetWindowLong($h, [WinVerify]::GWL_EXSTYLE)
    $aff = [uint32]0
    $has = [WinVerify]::GetWindowDisplayAffinity($h, [ref]$aff)
    if (($ex -band [WinVerify]::WS_EX_NOACTIVATE) -ne 0) { $noAct = $true }
    if ($has -and $aff -eq [WinVerify]::WDA_EXCLUDEFROMCAPTURE) { $affOk = $true }
  }
  if ($noAct -and $affOk) { break }
  Start-Sleep -Milliseconds 100
}
if (-not $noAct) { Fail "WS_EX_NOACTIVATE missing" }
Pass "WS_EX_NOACTIVATE"
if ($affOk) { Pass "WDA_EXCLUDEFROMCAPTURE" } else {
  Write-Host "[WARN] capture affinity not seen" -ForegroundColor Yellow
}

if (-not (Await-Log "Grounded" 60)) { Fail "Never Grounded - $Log" }
Pass "Grounded"

$tx = [int](($rect.Left + $rect.Right) / 2)
# y grows down: release ABOVE the title bar so feet fall onto the top edge
$ty = $rect.Top - 80
$m = Select-String -Path $Log -Pattern "sprite\((-?\d+),(-?\d+)\)" | Select-Object -Last 1
if (-not $m -or $m.Line -notmatch "sprite\((-?\d+),(-?\d+)\)") { Fail "no sprite coords" }
$sx = [int]$Matches[1]
$sy = [int]$Matches[2]
# sprite TRACE is top-left of the 90x90 art; click the center
$gx = $sx + 45
$gy = $sy + 45
Info "Grab $gx,$gy -> drop $tx,$ty (Notepad top=$($rect.Top), 80px above)"

[WinVerify]::SetCursorPos($gx, $gy) | Out-Null
Start-Sleep -Milliseconds 80
[WinVerify]::mouse_event([WinVerify]::MOUSEEVENTF_LEFTDOWN, 0, 0, 0, [UIntPtr]::Zero)
# Frame TRACE uses Dragged (not Held) while the button is down on a hit
if (-not (Await-Log "Dragged" 30)) {
  [WinVerify]::mouse_event([WinVerify]::MOUSEEVENTF_LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
  Fail "Never Dragged - $Log"
}
Pass "Dragged"

$steps = 18
for ($s = 1; $s -le $steps; $s++) {
  $cx = [int]($gx + (($tx - $gx) * $s / $steps))
  $cy = [int]($gy + (($ty - $gy) * $s / $steps))
  [WinVerify]::SetCursorPos($cx, $cy) | Out-Null
  Start-Sleep -Milliseconds 25
}
Start-Sleep -Milliseconds 150
[WinVerify]::mouse_event([WinVerify]::MOUSEEVENTF_LEFTUP, 0, 0, 0, [UIntPtr]::Zero)

Info "Waiting for Perched..."
if (Await-Log "Perched" 80) { Pass "Perched" } else {
  Get-Content $Log -Tail 30 | Set-Content (Join-Path $Out "tail.log")
  Fail "Never Perched - $Log"
}

[WinVerify]::SetForegroundWindow($npHwnd) | Out-Null
Start-Sleep -Milliseconds 100
$m2 = Select-String -Path $Log -Pattern "sprite\((-?\d+),(-?\d+)\)" | Select-Object -Last 1
if ($m2 -and $m2.Line -match "sprite\((-?\d+),(-?\d+)\)") {
  $sx2 = [int]$Matches[1]
  $sy2 = [int]$Matches[2]
  [WinVerify]::SetCursorPos($sx2 + 45, $sy2 + 45) | Out-Null
  [WinVerify]::mouse_event([WinVerify]::MOUSEEVENTF_LEFTDOWN, 0, 0, 0, [UIntPtr]::Zero)
  Start-Sleep -Milliseconds 30
  [WinVerify]::mouse_event([WinVerify]::MOUSEEVENTF_LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
  Start-Sleep -Milliseconds 150
  $fg = [WinVerify]::GetForegroundWindow()
  $fgPid = [uint32]0
  [WinVerify]::GetWindowThreadProcessId($fg, [ref]$fgPid) | Out-Null
  if ($fgPid -eq [uint32]$script:AppProc.Id) {
    Fail "Focus stolen by fidget (fg pid=$fgPid)"
  }
  Pass "No focus steal (fg pid=$fgPid)"
} else {
  Write-Host "[WARN] no perched sprite coords for focus check" -ForegroundColor Yellow
}

if ($script:AppProc -and -not $script:AppProc.HasExited) {
  Stop-Process -Id $script:AppProc.Id -Force -ErrorAction SilentlyContinue
}
Get-Process Notepad -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
$sw.Close()
Pass "E2E finished - $Out"
exit 0
