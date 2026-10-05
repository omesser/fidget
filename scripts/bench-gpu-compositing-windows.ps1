#!/usr/bin/env pwsh
$ErrorActionPreference = "Stop"
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

$script:Seconds = 15
$script:WalkTimeout = 180
$script:Shot = ""
$script:Out = ""
$script:LogPath = ""
$script:Scenario = ""
$script:AppProc = $null
$script:CoverProc = $null
$script:CursorOk = $false
$script:CursorError = ""
$script:CursorScale = "N/A"
$script:CursorNote = ""
$script:PhysicalWidth = 0
$script:ShotNoted = $false

function Show-Usage {
    [Console]::Error.WriteLine(@"
Usage: scripts\bench-gpu-compositing-windows.ps1 <env|baseline|idle|walking|chat|multi|hidden|matrix|parse-log|aim-check> [--seconds N] [--walk-timeout N] [--shot FILE] [--out DIR] [--log FILE] [--physical-width N]

parse-log counts mask_rebuild lines in --log and divides by --seconds.
aim-check prints the walking-over cursor target for --log. It does not move the cursor.
GPU% is dwm.exe engtype_3D when that counter exists, else nvidia-smi for the whole adapter.
Checkout and a debug build on DESKTOP-UQIE144 are authorized.
idle, walking, chat, hidden, and matrix launch fidget and move the cursor. Wait for an explicit green light before those scenarios.
"@)
    exit 2
}

$i = 0
while ($i -lt $args.Count) {
    $arg = [string]$args[$i]
    switch ($arg) {
        "--seconds" {
            $i++
            if ($i -ge $args.Count) { Show-Usage }
            $script:Seconds = [int]$args[$i]
        }
        "--walk-timeout" {
            $i++
            if ($i -ge $args.Count) { Show-Usage }
            $script:WalkTimeout = [int]$args[$i]
        }
        "--shot" {
            $i++
            if ($i -ge $args.Count) { Show-Usage }
            $script:Shot = [string]$args[$i]
        }
        "--out" {
            $i++
            if ($i -ge $args.Count) { Show-Usage }
            $script:Out = [string]$args[$i]
        }
        "--log" {
            $i++
            if ($i -ge $args.Count) { Show-Usage }
            $script:LogPath = [string]$args[$i]
        }
        "--physical-width" {
            $i++
            if ($i -ge $args.Count) { Show-Usage }
            $script:PhysicalWidth = [double]$args[$i]
        }
        "--help" { Show-Usage }
        "-h" { Show-Usage }
        default {
            if ($arg -in @("env", "baseline", "idle", "walking", "chat", "multi", "hidden", "matrix", "parse-log", "aim-check")) {
                if ($script:Scenario) { Show-Usage }
                $script:Scenario = $arg
            }
            else {
                [Console]::Error.WriteLine("unknown argument: $arg")
                Show-Usage
            }
        }
    }
    $i++
}

if (-not $script:Scenario) { Show-Usage }
if ($script:Seconds -le 0) { Show-Usage }
if ($script:WalkTimeout -le 0) { Show-Usage }

$script:IsWindowsOS = $false
if ($PSVersionTable.PSVersion.Major -ge 6) {
    $script:IsWindowsOS = [bool]$IsWindows
}
elseif ($env:OS -eq "Windows_NT") {
    $script:IsWindowsOS = $true
}

$script:Bin = if ($env:FIDGET_VERIFY_BIN) { $env:FIDGET_VERIFY_BIN } else { Join-Path $Root "target\debug\fidget.exe" }

function Add-Reason([string]$tool, [string]$line) {
    $clean = ($line -replace "\s+", " ").Trim()
    if (-not $clean) { $clean = "failed" }
    if ($script:GpuReason) {
        $script:GpuReason = "$($script:GpuReason). ${tool}: $clean"
    }
    else {
        $script:GpuReason = "${tool}: $clean"
    }
}

function Format-Mean($values) {
    $avg = ($values | Measure-Object -Average).Average
    return "{0:F1}" -f $avg
}

function Format-Rate([double]$count, [double]$window) {
    if ($window -le 0) { return "N/A" }
    return "{0:F2}" -f ($count / $window)
}

function Test-CommandPresent([string]$name) {
    if (Get-Command $name -ErrorAction SilentlyContinue) { return "present" }
    return "absent"
}

function Get-DwmProcess {
    if (-not $script:IsWindowsOS) { return $null }
    # Session 0's dwm is not the interactive desktop's compositor.
    $session = (Get-Process -Id $PID).SessionId
    $match = Get-Process -Name dwm -ErrorAction SilentlyContinue |
        Where-Object { $_.SessionId -eq $session } |
        Select-Object -First 1
    if ($match) { return $match }
    return Get-Process -Name dwm -ErrorAction SilentlyContinue | Select-Object -First 1
}

function Read-DwmCpuStamp {
    $proc = Get-DwmProcess
    if (-not $proc) { return $null }
    # The cached TotalProcessorTime does not move. The next read is a new object.
    return @{ Pid = $proc.Id; Cpu = $proc.TotalProcessorTime.TotalSeconds; At = [datetime]::UtcNow }
}

function Format-DwmCpu($stamp) {
    if (-not $stamp) { return "N/A" }
    try {
        $proc = Get-Process -Id $stamp.Pid -ErrorAction Stop
    }
    catch {
        return "N/A"
    }
    $elapsed = ([datetime]::UtcNow - $stamp.At).TotalSeconds
    if ($elapsed -le 0) { return "N/A" }
    $delta = $proc.TotalProcessorTime.TotalSeconds - $stamp.Cpu
    return "{0:F1}" -f (100.0 * $delta / $elapsed)
}

function Read-NvidiaCsv {
    $errFile = [System.IO.Path]::GetTempFileName()
    $outFile = [System.IO.Path]::GetTempFileName()
    $nvidia = Get-Command nvidia-smi -ErrorAction Stop
    $proc = Start-Process -FilePath $nvidia.Source `
        -ArgumentList "--query-gpu=utilization.gpu,power.draw", "--format=csv,noheader,nounits" `
        -NoNewWindow -PassThru -Wait `
        -RedirectStandardOutput $outFile -RedirectStandardError $errFile
    $raw = ""
    $err = ""
    if (Test-Path $outFile) { $raw = [string](Get-Content $outFile -Raw -ErrorAction SilentlyContinue) }
    if (Test-Path $errFile) { $err = [string](Get-Content $errFile -Raw -ErrorAction SilentlyContinue) }
    Remove-Item $errFile, $outFile -Force -ErrorAction SilentlyContinue
    $first = @($raw -split "`r?`n" | Where-Object { $_.Trim() } | Select-Object -First 1)
    $util = $null
    $power = $null
    if ($first.Count -gt 0 -and $first[0]) {
        $parts = [string]$first[0] -split ","
        $utilText = $parts[0].Trim()
        if ($utilText -match '^[0-9]+(\.[0-9]+)?$') { $util = [double]$utilText }
        if ($parts.Count -gt 1) {
            $powerText = $parts[1].Trim()
            if ($powerText -match '^[0-9]+(\.[0-9]+)?$') { $power = [double]$powerText }
        }
    }
    $errorText = ($err -replace "\s+", " ").Trim()
    if (-not $errorText -and $proc.ExitCode -ne 0) { $errorText = "exit $($proc.ExitCode)" }
    return @{ Util = $util; Power = $power; Error = $errorText; ExitCode = $proc.ExitCode }
}

function Measure-Nvidia([int]$window) {
    $total = 0.0
    $powerTotal = 0.0
    $n = 0
    $pn = 0
    $errText = ""
    for ($k = 0; $k -lt $window; $k++) {
        $sample = Read-NvidiaCsv
        if ($null -eq $sample.Util -and $sample.Error -and -not $errText) { $errText = $sample.Error }
        if ($null -ne $sample.Util) {
            $total += $sample.Util
            $n++
        }
        if ($null -ne $sample.Power) {
            $powerTotal += $sample.Power
            $pn++
        }
        if ($k -lt ($window - 1)) { Start-Sleep -Seconds 1 }
    }
    if ($n -eq 0) {
        if (-not $errText) { $errText = "no numeric utilization.gpu sample" }
        Add-Reason "nvidia-smi" $errText
        if ($pn -eq 0) {
            $script:PowerReason = "nvidia-smi: $errText"
        }
        return $false
    }
    $script:GpuPct = "{0:F1}" -f ($total / $n)
    $script:GpuTool = "nvidia-smi"
    $script:GpuReason = "nvidia-smi mean of $n samples, first GPU. Whole adapter, not dwm.exe"
    if ($pn -gt 0) {
        $script:PowerW = "{0:F1}" -f ($powerTotal / $pn)
        $script:PowerReason = "nvidia-smi power.draw mean of $pn samples, first GPU"
    }
    else {
        $script:PowerW = "N/A"
        $script:PowerReason = "nvidia-smi returned no numeric power.draw"
    }
    return $true
}

function Read-NvidiaPowerOnce {
    if (-not (Get-Command nvidia-smi -ErrorAction SilentlyContinue)) {
        $script:PowerW = "N/A"
        $script:PowerReason = "nvidia-smi not on PATH"
        return
    }
    $sample = Read-NvidiaCsv
    if ($null -ne $sample.Power) {
        $script:PowerW = "{0:F1}" -f $sample.Power
        $script:PowerReason = "nvidia-smi power.draw, first GPU"
        return
    }
    $script:PowerW = "N/A"
    $why = $sample.Error
    if (-not $why) { $why = "no numeric power.draw" }
    $script:PowerReason = "nvidia-smi: $why"
}

function Test-Dwm3DName([string]$name, [int]$dwmPid) {
    return ($name -like "*pid_${dwmPid}_*" -and $name -like "*engtype_3D*")
}

function Measure-DwmCounter([int]$window) {
    $dwm = Get-DwmProcess
    if (-not $dwm) {
        Add-Reason "gpu-engine" "dwm.exe is not running"
        return $false
    }
    $dwmPid = [int]$dwm.Id
    try {
        $data = @(Get-Counter -Counter "\GPU Engine(*)\Utilization Percentage" -SampleInterval 1 -MaxSamples $window -ErrorAction Stop)
    }
    catch {
        Add-Reason "gpu-engine" $_.Exception.Message
        return $false
    }
    $sums = @()
    foreach ($set in $data) {
        $sum = 0.0
        $hit = $false
        foreach ($cs in @($set.CounterSamples)) {
            $name = "$($cs.InstanceName) $($cs.Path)"
            if ((Test-Dwm3DName $name $dwmPid) -and ("$($cs.CookedValue)" -match '^[0-9]+(\.[0-9]+)?$')) {
                $sum += [double]$cs.CookedValue
                $hit = $true
            }
        }
        if ($hit) { $sums += $sum }
    }
    if ($sums.Count -eq 0) {
        Add-Reason "gpu-engine" "no engtype_3D sample for dwm pid $dwmPid"
        return $false
    }
    $script:GpuPct = Format-Mean $sums
    $script:GpuTool = "gpu-engine-counter"
    $script:GpuReason = "mean of $($sums.Count) samples, sum of dwm.exe pid $dwmPid engtype_3D"
    return $true
}

function Measure-DwmWmi([int]$window) {
    $dwm = Get-DwmProcess
    if (-not $dwm) {
        Add-Reason "wmi" "dwm.exe is not running"
        return $false
    }
    $dwmPid = [int]$dwm.Id
    $sums = @()
    $errText = ""
    for ($k = 0; $k -lt $window; $k++) {
        try {
            $rows = @(Get-CimInstance -ClassName Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine -ErrorAction Stop)
        }
        catch {
            $errText = $_.Exception.Message
            break
        }
        if ($rows.Count -gt 0 -and -not ($rows[0].PSObject.Properties.Name -contains "UtilizationPercentage")) {
            $errText = "GPUEngine class has no UtilizationPercentage"
            break
        }
        $sum = 0.0
        $hit = $false
        foreach ($row in $rows) {
            $pct = [string]$row.UtilizationPercentage
            if ((Test-Dwm3DName ([string]$row.Name) $dwmPid) -and ($pct -match '^[0-9]+(\.[0-9]+)?$')) {
                $sum += [double]$pct
                $hit = $true
            }
        }
        if ($hit) { $sums += $sum }
        if ($k -lt ($window - 1)) { Start-Sleep -Seconds 1 }
    }
    if ($sums.Count -eq 0) {
        if (-not $errText) { $errText = "no engtype_3D row for dwm pid $dwmPid" }
        Add-Reason "wmi" $errText
        return $false
    }
    $script:GpuPct = Format-Mean $sums
    $script:GpuTool = "wmi-gpu-engine"
    $script:GpuReason = "mean of $($sums.Count) samples, sum of dwm.exe pid $dwmPid engtype_3D"
    return $true
}

function Measure-Gpu([int]$window) {
    $script:GpuPct = "N/A"
    $script:GpuTool = "none"
    $script:GpuReason = ""
    $script:PowerW = "N/A"
    $script:PowerReason = ""

    if ($script:IsWindowsOS) {
        if (Measure-DwmCounter $window) {
            Read-NvidiaPowerOnce
            return
        }
        if (Measure-DwmWmi $window) {
            Read-NvidiaPowerOnce
            return
        }
    }
    if (Get-Command nvidia-smi -ErrorAction SilentlyContinue) {
        if (Measure-Nvidia $window) { return }
    }
    if (-not $script:GpuReason) {
        if ($script:IsWindowsOS) {
            $script:GpuReason = "no GPU Engine counter, WMI GPUEngine class, or nvidia-smi sample"
        }
        else {
            $script:GpuReason = "not Windows and nvidia-smi is not on PATH"
        }
    }
    if (-not $script:PowerReason) {
        $script:PowerReason = "no power sample"
    }
}

function Get-LineCount([string]$log) {
    if (-not (Test-Path $log)) { return 0 }
    return @(Get-Content -Path $log -ErrorAction SilentlyContinue).Count
}

function Get-LinesSince([string]$log, [int]$from, [string]$pattern) {
    if (-not (Test-Path $log)) { return @() }
    return @(Select-String -Path $log -Pattern $pattern -ErrorAction SilentlyContinue |
        Where-Object { $_.LineNumber -gt $from })
}

function Get-MaskSince([string]$log, [int]$from) {
    return @(Get-LinesSince $log $from "mask_rebuild:").Count
}

function Get-FrameCenter([string]$text) {
    if ($text -notmatch 'pos\((-?\d+),(-?\d+)\)') { return $null }
    $feetX = [int]$Matches[1]
    $feetY = [int]$Matches[2]
    if ($text -notmatch 'sprite\((-?\d+),(-?\d+)\)') { return $null }
    $spriteY = [int]$Matches[2]
    $kind = ""
    if ($text -match ' ([A-Za-z0-9_]+)#\d+') { $kind = $Matches[1] }
    return @{ X = $feetX; Y = [int](($spriteY + $feetY) / 2); Kind = $kind }
}

function Get-SpriteCenter([string]$log, [string]$animation) {
    if (-not (Test-Path $log)) { return $null }
    $lines = @(Select-String -Path $log -Pattern "frame:" -ErrorAction SilentlyContinue)
    if ($lines.Count -eq 0) { return $null }
    $picked = $null
    if ($animation -eq "walk") {
        $picked = $lines | Where-Object { $_.Line -match ' (walk|ballwalk)#' } | Select-Object -Last 1
        if (-not $picked) { return $null }
    }
    else {
        $picked = $lines | Select-Object -Last 1
    }
    return Get-FrameCenter $picked.Line
}

function Get-OverlayWidth([string]$log) {
    if (-not $log -or -not (Test-Path $log)) { return 0 }
    $line = Select-String -Path $log -Pattern "overlay: \S+ covers \d+x\d+ at \(0,0\)" -ErrorAction SilentlyContinue | Select-Object -Last 1
    if (-not $line) { return 0 }
    if ($line.Line -notmatch 'covers (\d+)x') { return 0 }
    return [double]$Matches[1]
}

# frame: is in overlay points, which match SetCursorPos when the overlay is the
# primary's physical width. Scaling by DPI/96 under SetProcessDPIAware walks the
# point off the virtual screen.
function Convert-FrameCursor([int]$x, [int]$y, [double]$overlayWidth, [double]$physicalWidth) {
    $scale = 1.0
    if ($overlayWidth -gt 0 -and $physicalWidth -gt 0) {
        $scale = $physicalWidth / $overlayWidth
    }
    return @{
        X     = [int][Math]::Round($x * $scale)
        Y     = [int][Math]::Round($y * $scale)
        Scale = $scale
    }
}

function Initialize-Cursor {
    if (-not $script:IsWindowsOS) { return }
    $csharp = @'
using System;
using System.Runtime.InteropServices;
public class BuddyGpuBench {
  public const uint LEFTDOWN = 0x0002;
  public const uint LEFTUP = 0x0004;
  public const uint MONITOR_DEFAULTTOPRIMARY = 1;
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X; public int Y; }
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll", SetLastError=true)] public static extern bool SetCursorPos(int X, int Y);
  [DllImport("user32.dll", SetLastError=true)] public static extern bool GetCursorPos(out POINT lpPoint);
  [DllImport("user32.dll")] public static extern IntPtr MonitorFromPoint(POINT pt, uint dwFlags);
  [DllImport("user32.dll", CharSet=CharSet.Auto)] public static extern bool GetMonitorInfo(IntPtr hMonitor, ref MONITORINFO lpmi);
  [DllImport("shcore.dll")] public static extern int GetDpiForMonitor(IntPtr hmonitor, int dpiType, out uint dpiX, out uint dpiY);
  [DllImport("user32.dll")] public static extern void mouse_event(uint dwFlags, uint dx, uint dy, uint dwData, UIntPtr dwExtraInfo);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int nIndex);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct MONITORINFO {
    public int cbSize;
    public RECT rcMonitor;
    public RECT rcWork;
    public uint dwFlags;
  }
  public static string LastMove = "";
  public static double PrimaryScale() {
    SetProcessDPIAware();
    IntPtr mon = MonitorFromPoint(new POINT(), MONITOR_DEFAULTTOPRIMARY);
    uint dpiX, dpiY;
    if (GetDpiForMonitor(mon, 0, out dpiX, out dpiY) != 0 || dpiX == 0) return 0;
    return dpiX / 96.0;
  }
  public static int PrimaryPixelWidth() {
    SetProcessDPIAware();
    IntPtr mon = MonitorFromPoint(new POINT(), MONITOR_DEFAULTTOPRIMARY);
    MONITORINFO info = new MONITORINFO();
    info.cbSize = Marshal.SizeOf(typeof(MONITORINFO));
    if (!GetMonitorInfo(mon, ref info)) return 0;
    return info.rcMonitor.Right - info.rcMonitor.Left;
  }
  static bool Near(POINT p, int x, int y) {
    int dx = p.X - x;
    int dy = p.Y - y;
    if (dx < 0) dx = -dx;
    if (dy < 0) dy = -dy;
    return dx <= 1 && dy <= 1;
  }
  static void AbsoluteMove(int x, int y) {
    int vx = GetSystemMetrics(76);
    int vy = GetSystemMetrics(77);
    int vw = GetSystemMetrics(78);
    int vh = GetSystemMetrics(79);
    if (vw <= 1 || vh <= 1) return;
    int nx = (int)Math.Round((x - vx) * 65535.0 / (vw - 1));
    int ny = (int)Math.Round((y - vy) * 65535.0 / (vh - 1));
    if (nx < 0) nx = 0;
    if (ny < 0) ny = 0;
    mouse_event(0x0001 | 0x8000 | 0x4000, (uint)nx, (uint)ny, 0, UIntPtr.Zero);
  }
  public static bool MoveScreen(int x, int y) {
    POINT after;
    bool called = SetCursorPos(x, y);
    int err = Marshal.GetLastWin32Error();
    if (!GetCursorPos(out after)) {
      LastMove = "GetCursorPos failed";
      return false;
    }
    if (Near(after, x, y)) {
      LastMove = "actual " + after.X + "," + after.Y;
      return true;
    }
    AbsoluteMove(x, y);
    if (!GetCursorPos(out after)) {
      LastMove = "GetCursorPos failed";
      return false;
    }
    if (Near(after, x, y)) {
      LastMove = "actual " + after.X + "," + after.Y + " via absolute move";
      return true;
    }
    LastMove = "target " + x + "," + y + " actual " + after.X + "," + after.Y + " set=" + called + " err=" + err;
    return false;
  }
}
'@
    try {
        Add-Type -TypeDefinition $csharp -ErrorAction Stop
        $script:CursorOk = $true
    }
    catch {
        if ($_.Exception.Message -match "already exists") {
            $script:CursorOk = $true
        }
        else {
            $script:CursorError = $_.Exception.Message
            return
        }
    }
    $scale = [BuddyGpuBench]::PrimaryScale()
    if ($scale -le 0) {
        $script:CursorOk = $false
        $script:CursorError = "primary DPI unavailable"
        return
    }
    $script:CursorScale = "{0:F2}" -f $scale
}

function Move-Cursor([int]$x, [int]$y, [string]$log) {
    if (-not $script:CursorOk) {
        $script:CursorNote = "cursor unavailable. $($script:CursorError)"
        return $false
    }
    $overlay = Get-OverlayWidth $log
    $physical = $script:PhysicalWidth
    if ($physical -le 0) {
        try { $physical = [BuddyGpuBench]::PrimaryPixelWidth() } catch { $physical = 0 }
    }
    $aim = Convert-FrameCursor $x $y $overlay $physical
    $ok = [BuddyGpuBench]::MoveScreen($aim.X, $aim.Y)
    $script:CursorNote = ("frame {0},{1} cursor {2},{3} scale={4:F2} dpi_scale={5} {6}" -f $x, $y, $aim.X, $aim.Y, $aim.Scale, $script:CursorScale, [BuddyGpuBench]::LastMove)
    return $ok
}

function Invoke-Click([int]$x, [int]$y, [string]$log) {
    if (-not (Move-Cursor $x $y $log)) { return $false }
    Start-Sleep -Milliseconds 30
    [BuddyGpuBench]::mouse_event([BuddyGpuBench]::LEFTDOWN, 0, 0, 0, [UIntPtr]::Zero)
    [BuddyGpuBench]::mouse_event([BuddyGpuBench]::LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
    return $true
}

function Stop-Tracked([ref]$proc) {
    if ($proc.Value -and -not $proc.Value.HasExited) {
        Stop-Process -Id $proc.Value.Id -Force -ErrorAction SilentlyContinue
        try { $proc.Value.WaitForExit(5000) | Out-Null } catch { }
    }
    $proc.Value = $null
}

function Stop-App {
    Stop-Tracked ([ref]$script:AppProc)
}

function Stop-Cover {
    Stop-Tracked ([ref]$script:CoverProc)
}

function Assert-Binary {
    if (Test-Path $script:Bin) { return }
    [Console]::Error.WriteLine("no $($script:Bin) - run: cargo build -p fidget --bin fidget")
    exit 2
}

function Start-App([string]$log) {
    Assert-Binary
    $stdout = "$log.stdout"
    if (-not $env:FIDGET_INSTANCES) { $env:FIDGET_INSTANCES = "BMO" }
    if (-not $env:FIDGET_CHARACTERS) { $env:FIDGET_CHARACTERS = Join-Path (Get-Location) "characters" }
    $env:FIDGET_TRACE_MASK_REBUILD = "1"
    $env:FIDGET_TRACE_FRAMES = "1"
    $script:AppProc = Start-Process -FilePath $script:Bin -PassThru -WindowStyle Hidden `
        -RedirectStandardOutput $stdout -RedirectStandardError $log
}

function Wait-Overlay([string]$log) {
    for ($k = 0; $k -lt 40; $k++) {
        if (Select-String -Path $log -Pattern "overlay: [0-9]+ display" -Quiet -ErrorAction SilentlyContinue) {
            return $true
        }
        if ($script:AppProc.HasExited) { return $false }
        Start-Sleep -Milliseconds 500
    }
    return $false
}

function Start-Overlay([string]$name) {
    $log = Join-Path $script:Out "$name.log"
    Start-App $log
    if (-not (Wait-Overlay $log)) {
        [Console]::Error.WriteLine("app did not report an overlay; see $log")
        if (Test-Path $log) { Get-Content $log -Tail 40 | ForEach-Object { [Console]::Error.WriteLine($_) } }
        Stop-App
        exit 1
    }
    Start-Sleep -Seconds 2
    if ($script:CursorOk) { Move-Cursor 2 2 | Out-Null }
    Start-Sleep -Seconds 1
    return $log
}

function Emit-Row(
    [string]$name,
    [string]$gpu,
    [string]$tool,
    [string]$power,
    [string]$xperf,
    [string]$calls,
    [string]$hz,
    [string]$dwmCpu,
    [string]$notes
) {
    $notes = ($notes -replace "[`t`r`n]+", " ").Trim()
    $line = "$name`t$gpu`t$tool`t$power`t$xperf`t$calls`t$hz`t$dwmCpu`t$notes"
    Add-Content -Path (Join-Path $script:Out "rows.tsv") -Value $line -Encoding ascii
    Write-Host $line
}

function Join-SampleNotes([string]$scenarioNotes) {
    $parts = @($scenarioNotes)
    if ($script:GpuReason) { $parts += $script:GpuReason }
    if ($script:PowerReason) { $parts += "power: $($script:PowerReason)" }
    return (($parts | Where-Object { $_ }) -join ". ")
}

function Sample-Row([string]$name, [string]$log, [int]$window, [string]$notes) {
    $from = Get-LineCount $log
    $stamp = Read-DwmCpuStamp
    $started = [datetime]::UtcNow
    Measure-Gpu $window
    $elapsed = ([datetime]::UtcNow - $started).TotalSeconds
    $left = $window - $elapsed
    if ($left -gt 0.2) { Start-Sleep -Seconds ([int][Math]::Ceiling($left)) }
    $dwmCpu = Format-DwmCpu $stamp
    $calls = "N/A"
    $hz = "N/A"
    if ($name -ne "baseline") {
        $count = Get-MaskSince $log $from
        $calls = [string]$count
        $hz = Format-Rate $count $window
    }
    if ($name -eq "walking" -or $name -eq "walking-over") {
        $walks = @(Get-LinesSince $log $from " (walk|ballwalk)#").Count
        $notes = "$notes, walk_frames=$walks"
        if ($name -eq "walking-over") {
            $last = Get-LinesSince $log $from "frame:" | Select-Object -Last 1
            $kind = ""
            if ($last -and $last.Line -match ' ([^ ]+)#\d+') { $kind = $Matches[1] }
            if ($walks -eq 0 -or ($kind -and $kind -notmatch '^(walk|ballwalk)$')) {
                $notes = "$notes, walk aborted"
            }
        }
    }
    Emit-Row $name $script:GpuPct $script:GpuTool $script:PowerW "N/A" $calls $hz $dwmCpu (Join-SampleNotes $notes)
}

function Emit-Unstaged([string]$name, [string]$why) {
    Emit-Row $name "N/A" "none" "N/A" "N/A" "N/A" "N/A" "N/A" $why
}

function Assert-WindowsScenario([string]$name) {
    if ($script:IsWindowsOS) { return $true }
    Emit-Unstaged $name "not Windows. DWM compositing is not on this host"
    return $false
}

function Probe-Host {
    $script:OsName = if ($PSVersionTable.OS) { $PSVersionTable.OS } else { [Environment]::OSVersion.VersionString }
    $script:Computer = [Environment]::MachineName
    $script:Nvidia = Test-CommandPresent "nvidia-smi"
    $script:Xperf = Test-CommandPresent "xperf"
    $script:Wpr = Test-CommandPresent "wpr"
    if ($script:Xperf -eq "absent" -and $script:Wpr -eq "absent") {
        $script:FrameReason = "xperf and wpr are not on PATH"
    }
    else {
        $script:FrameReason = "xperf=$($script:Xperf) wpr=$($script:Wpr). This script does not decode an ETL into a frame time"
    }
    $script:Screens = "N/A"
    $script:ScreenReason = "not Windows"
    $script:GpuName = "N/A"
    $script:RefreshHz = "N/A"
    $script:RefreshReason = "not Windows"
    $script:DwmPid = "N/A"
    $script:Elevated = "N/A"
    if (-not $script:IsWindowsOS) { return }

    $dwm = Get-DwmProcess
    if ($dwm) { $script:DwmPid = [string]$dwm.Id }
    try {
        $id = [Security.Principal.WindowsIdentity]::GetCurrent()
        $principal = New-Object Security.Principal.WindowsPrincipal($id)
        $script:Elevated = [string]$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    }
    catch {
        $script:Elevated = "unknown"
    }
    try {
        Add-Type -AssemblyName System.Windows.Forms -ErrorAction Stop
        $script:Screens = [string][System.Windows.Forms.Screen]::AllScreens.Count
        $script:ScreenReason = "System.Windows.Forms.Screen"
    }
    catch {
        $script:ScreenReason = ($_.Exception.Message -replace "\s+", " ").Trim()
    }
    try {
        $controllers = @(Get-CimInstance Win32_VideoController -ErrorAction Stop)
        $name = $controllers | ForEach-Object { $_.Name } | Where-Object { $_ } | Select-Object -First 1
        if ($name) { $script:GpuName = [string]$name }
        $rate = $controllers | ForEach-Object { $_.CurrentRefreshRate } | Where-Object { $_ -and $_ -gt 0 } | Select-Object -First 1
        if ($rate) {
            $script:RefreshHz = [string]$rate
            $script:RefreshReason = "Win32_VideoController.CurrentRefreshRate. Mode refresh, not an xperf frame time"
        }
        else {
            $script:RefreshReason = "Win32_VideoController returned no CurrentRefreshRate"
        }
    }
    catch {
        $script:RefreshReason = ($_.Exception.Message -replace "\s+", " ").Trim()
    }
}

function Write-Env {
    Write-Host "os=$($script:OsName)"
    Write-Host "computer=$($script:Computer)"
    Write-Host "windows=$($script:IsWindowsOS)"
    Write-Host "elevated=$($script:Elevated)"
    Write-Host "screens=$($script:Screens)"
    Write-Host "screen_source=$($script:ScreenReason)"
    Write-Host "gpu_name=$($script:GpuName)"
    Write-Host "refresh_hz=$($script:RefreshHz)"
    Write-Host "refresh_source=$($script:RefreshReason)"
    Write-Host "dwm_pid=$($script:DwmPid)"
    Write-Host "nvidia_smi=$($script:Nvidia)"
    Write-Host "xperf=$($script:Xperf)"
    Write-Host "wpr=$($script:Wpr)"
    Write-Host "xperf_frame_ms=N/A"
    Write-Host "xperf_frame_reason=$($script:FrameReason)"
    Write-Host "cursor_scale=$($script:CursorScale)"
    Write-Host "seconds=$($script:Seconds)"
    Write-Host "walk_timeout=$($script:WalkTimeout)"
    if ($script:CursorError) { Write-Host "cursor_error=$($script:CursorError)" }
}

function Run-Baseline {
    $log = Join-Path $script:Out "baseline.log"
    Set-Content -Path $log -Value "" -Encoding ascii
    Sample-Row "baseline" $log $script:Seconds "no fidget"
}

function Run-Idle {
    if (-not (Assert-WindowsScenario "idle")) { return }
    $log = Start-Overlay "idle"
    $park = "pointer at 2,2"
    if (-not $script:CursorOk) { $park = "cursor unavailable. $($script:CursorError)" }
    Sample-Row "idle" $log $script:Seconds $park
    Stop-App
    if ($script:Shot) { Write-ShotNote }
}

function Write-ShotNote {
    if ($script:ShotNoted) { return }
    $script:ShotNoted = $true
    Write-Host "shot=N/A"
    Write-Host "shot_reason=Open Task Manager on Performance, GPU, during idle perched. Crop about 280px wide. Attach with gh pr comment --attach in file#alt form. Do not commit the image."
}

function Run-Walking {
    if (-not (Assert-WindowsScenario "walking")) { return }
    $log = Start-Overlay "walking"
    $deadline = [datetime]::UtcNow.AddSeconds($script:WalkTimeout)
    $saw = $false
    while ([datetime]::UtcNow -lt $deadline) {
        if (Select-String -Path $log -Pattern " (walk|ballwalk)#" -Quiet -ErrorAction SilentlyContinue) {
            $saw = $true
            break
        }
        if ($script:AppProc.HasExited) { break }
        Start-Sleep -Seconds 1
    }
    if (-not $saw) {
        Emit-Unstaged "walking" "no walk or ballwalk frame within $($script:WalkTimeout)s"
        Stop-App
        return
    }
    Sample-Row "walking" $log $script:Seconds "pointer away, walk frame seen"
    $center = Get-SpriteCenter $log "walk"
    if (-not $center) {
        Emit-Unstaged "walking-over" "no walk or ballwalk frame to aim at"
    }
    elseif (-not (Move-Cursor $center.X $center.Y $log)) {
        Emit-Unstaged "walking-over" "cursor did not land on the sprite. $($script:CursorNote)"
    }
    else {
        Start-Sleep -Seconds 1
        Sample-Row "walking-over" $log 5 "5s window, pointer on sprite. $($script:CursorNote)"
    }
    Stop-App
}

function Run-Chat {
    if (-not (Assert-WindowsScenario "chat")) { return }
    $log = Start-Overlay "chat"
    $center = Get-SpriteCenter $log ""
    if (-not $center) {
        Emit-Unstaged "chat" "no frame line to aim a double-click"
        Stop-App
        return
    }
    if (-not (Invoke-Click $center.X $center.Y $log)) {
        Emit-Unstaged "chat" "cursor did not land on the sprite. $($script:CursorNote)"
        Stop-App
        return
    }
    Start-Sleep -Milliseconds 120
    if (-not (Invoke-Click $center.X $center.Y $log)) {
        Emit-Unstaged "chat" "second click failed. $($script:CursorNote)"
        Stop-App
        return
    }
    $opened = $false
    for ($k = 0; $k -lt 20; $k++) {
        if (Select-String -Path $log -Pattern "Summon" -Quiet -ErrorAction SilentlyContinue) {
            $opened = $true
            break
        }
        Start-Sleep -Milliseconds 250
    }
    if (-not $opened) {
        Emit-Unstaged "chat" "double-click at $($center.X),$($center.Y) did not log Summon"
        Stop-App
        return
    }
    Start-Sleep -Seconds 1
    Sample-Row "chat" $log $script:Seconds "Summon logged, pointer left on sprite"
    Stop-App
}

function Run-Multi {
    if (-not (Assert-WindowsScenario "multi")) { return }
    if ($script:Screens -eq "N/A") {
        Emit-Unstaged "multi" "screen count unavailable. $($script:ScreenReason)"
        return
    }
    if ([int]$script:Screens -lt 2) {
        Emit-Unstaged "multi" "host has $($script:Screens) display"
        return
    }
    $log = Start-Overlay "multi"
    $reported = "unknown"
    $hit = Select-String -Path $log -Pattern "^overlay: ([0-9]+) display" | Select-Object -First 1
    if ($hit) { $reported = $hit.Matches[0].Groups[1].Value }
    Sample-Row "multi" $log $script:Seconds "screens=$($script:Screens) overlay displays=$reported"
    Stop-App
}

function Start-Cover([int]$window) {
    $path = Join-Path $script:Out "cover.ps1"
    # Maximized stops at the taskbar. Hide rules treat that as zoomed, not fullscreen.
    $body = @'
Add-Type @"
using System.Runtime.InteropServices;
public class BuddyCoverDpi {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
}
"@
[BuddyCoverDpi]::SetProcessDPIAware() | Out-Null
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
# One per screen: with a display left free, the Character moves there instead of hiding.
$forms = foreach ($screen in [System.Windows.Forms.Screen]::AllScreens) {
    $form = New-Object System.Windows.Forms.Form
    $form.FormBorderStyle = "None"
    $form.StartPosition = "Manual"
    $form.Bounds = $screen.Bounds
    $form.TopMost = $true
    $form.ShowInTaskbar = $true
    $form.Text = "fidget-bench-cover"
    $form.BackColor = [System.Drawing.Color]::Black
    $form
}
$timer = New-Object System.Windows.Forms.Timer
$timer.Interval = __MS__
$timer.Add_Tick({ $forms | ForEach-Object { $_.Close() } })
$timer.Start()
$forms | Select-Object -Skip 1 | ForEach-Object { $_.Show() }
[void]@($forms)[0].ShowDialog()
'@
    $body = $body.Replace("__MS__", [string](($window + 30) * 1000))
    [System.IO.File]::WriteAllText($path, $body)
    $hostExe = (Get-Process -Id $PID).Path
    $script:CoverProc = Start-Process -FilePath $hostExe -ArgumentList "-NoProfile", "-STA", "-File", $path -PassThru
}

function Run-Hidden {
    if (-not (Assert-WindowsScenario "hidden")) { return }
    if (-not [Environment]::UserInteractive) {
        Emit-Unstaged "hidden" "no interactive session for a cover window"
        return
    }
    $log = Start-Overlay "hidden"
    try {
        Start-Cover $script:Seconds
    }
    catch {
        Emit-Unstaged "hidden" "cover window failed. $($_.Exception.Message)"
        Stop-App
        return
    }
    $hidden = $false
    for ($k = 0; $k -lt 20; $k++) {
        if (Select-String -Path $log -Pattern "presence: hidden" -Quiet -ErrorAction SilentlyContinue) {
            $hidden = $true
            break
        }
        Start-Sleep -Milliseconds 250
    }
    if (-not $hidden) {
        Emit-Unstaged "hidden" "cover window did not log presence hidden"
        Stop-Cover
        Stop-App
        return
    }
    Start-Sleep -Seconds 1
    Sample-Row "hidden" $log $script:Seconds "presence hidden under fullscreen cover"
    Stop-Cover
    Stop-App
}

function Invoke-AimCheck {
    if (-not $script:LogPath) {
        [Console]::Error.WriteLine("aim-check needs --log FILE")
        exit 2
    }
    if (-not (Test-Path $script:LogPath)) {
        [Console]::Error.WriteLine("no log $($script:LogPath)")
        exit 2
    }
    $center = Get-SpriteCenter $script:LogPath "walk"
    if (-not $center) {
        Write-Host "aim=none"
        exit 1
    }
    $overlay = Get-OverlayWidth $script:LogPath
    $aim = Convert-FrameCursor $center.X $center.Y $overlay $script:PhysicalWidth
    Write-Host "aim_kind=$($center.Kind)"
    Write-Host "aim_x=$($center.X)"
    Write-Host "aim_y=$($center.Y)"
    Write-Host "cursor_x=$($aim.X)"
    Write-Host "cursor_y=$($aim.Y)"
    Write-Host ("scale={0:F2}" -f $aim.Scale)
}

function Invoke-ParseLog {
    if (-not $script:LogPath) {
        [Console]::Error.WriteLine("parse-log needs --log FILE")
        exit 2
    }
    if (-not (Test-Path $script:LogPath)) {
        [Console]::Error.WriteLine("no log $($script:LogPath)")
        exit 2
    }
    $count = @(Select-String -Path $script:LogPath -Pattern "mask_rebuild:" -ErrorAction SilentlyContinue).Count
    $hz = Format-Rate $count $script:Seconds
    Write-Host "mask_calls=$count"
    Write-Host "mask_hz=$hz"
    Write-Host "seconds=$($script:Seconds)"
}

if (-not $script:Out) {
    $script:Out = Join-Path ([System.IO.Path]::GetTempPath()) ("fidget-gpu-bench-" + [guid]::NewGuid().ToString("n").Substring(0, 8))
}
New-Item -ItemType Directory -Force -Path $script:Out | Out-Null

try {
    if ($script:Scenario -eq "parse-log") {
        Invoke-ParseLog
        exit 0
    }
    if ($script:Scenario -eq "aim-check") {
        Invoke-AimCheck
        exit 0
    }

    Probe-Host
    Initialize-Cursor
    Write-Env
    $header = "scenario`tgpu_pct`tgpu_tool`tpower_w`txperf_frame_ms`tmask_calls`tmask_hz`tdwm_cpu_pct`tnotes"
    Set-Content -Path (Join-Path $script:Out "rows.tsv") -Value $header -Encoding ascii
    Write-Host $header

    switch ($script:Scenario) {
        "env" { }
        "baseline" { Run-Baseline }
        "idle" { Run-Idle }
        "walking" { Run-Walking }
        "chat" { Run-Chat }
        "multi" { Run-Multi }
        "hidden" { Run-Hidden }
        "matrix" {
            Run-Baseline
            Run-Idle
            Run-Walking
            Run-Chat
            Run-Multi
            Run-Hidden
        }
    }

    Write-Host "out=$($script:Out)"
    if ($script:Shot) { Write-ShotNote }
}
finally {
    Stop-Cover
    Stop-App
}
