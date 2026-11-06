#!/usr/bin/env pwsh
# Launch Fidget once and promote its tray icon out of the hidden-icons overflow,
# where a scenario's click cannot reach it. Waits on the NotifyIconSettings
# registry change, not a clock, for up to 30 s.
#
# Usage: show-tray-icon-win.ps1 <fidget.exe>
$ErrorActionPreference = "Stop"
$Bin = (Resolve-Path -LiteralPath $args[0]).Path

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Threading;
public static class RegistryChange {
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode)]
    static extern int RegOpenKeyEx(UIntPtr key, string subKey, int options, int access, out UIntPtr result);
    [DllImport("advapi32.dll")]
    static extern int RegNotifyChangeKeyValue(UIntPtr key, bool subtree, int filter, IntPtr evt, bool async);
    static readonly UIntPtr CurrentUser = new UIntPtr(0x80000001u);
    const int KeyNotify = 0x0010;
    const int NameOrValue = 0x1 | 0x4;
    public static UIntPtr Open(string subKey) {
        UIntPtr key;
        if (RegOpenKeyEx(CurrentUser, subKey, 0, KeyNotify, out key) != 0) throw new InvalidOperationException("cannot watch HKCU\\" + subKey);
        return key;
    }
    // Arm before each check, so a change between the check and the wait still wakes it.
    public static void Arm(UIntPtr key, EventWaitHandle changed) {
        if (RegNotifyChangeKeyValue(key, true, NameOrValue, changed.SafeWaitHandle.DangerousGetHandle(), true) != 0)
            throw new InvalidOperationException("RegNotifyChangeKeyValue failed");
    }
}
"@

# The parent key, so the watch also sees NotifyIconSettings being created.
$watched = [RegistryChange]::Open("Control Panel")
$changed = New-Object System.Threading.AutoResetEvent $false
$deadline = [DateTime]::UtcNow.AddSeconds(30)
$launched = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()

# Which side was late (#2084): Fidget's `tray: menu set` line, which it writes
# once the icon exists, against Explorer's NotifyIconSettings entry. Both are
# timed from launch; process.log stamps whole seconds, the registry milliseconds.
function Write-Timeline([string]$Registry) {
    $log = if ($env:FIDGET_DATA_DIR) { Join-Path $env:FIDGET_DATA_DIR "process.log" } else { "" }
    $lines = @(if ($log -and (Test-Path -LiteralPath $log)) { Get-Content -LiteralPath $log -Encoding utf8 })
    $tray = $lines | Where-Object { $_ -match '^\d+ tray: menu set' } | Select-Object -First 1
    $fidget = if ($tray) { "{0:+0;-0} s" -f ([long]($tray -split ' ')[0] - [math]::Floor($launched / 1000)) } else { "never" }
    $alive = if ($proc.HasExited) { "exited $($proc.ExitCode)" } else { "running" }
    Write-Host "tray timeline: Fidget icon $fidget, Explorer entry $Registry, Fidget $alive"
    return $lines
}

$proc = Start-Process $Bin -PassThru
try {
    while ($true) {
        [RegistryChange]::Arm($watched, $changed)
        $icon = Get-ChildItem 'HKCU:\Control Panel\NotifyIconSettings' -ErrorAction SilentlyContinue |
            Where-Object { (Get-ItemProperty $_.PSPath).ExecutablePath -eq $Bin } |
            Select-Object -First 1
        if ($icon) { break }
        $left = $deadline - [DateTime]::UtcNow
        if ($left -le [TimeSpan]::Zero -or -not $changed.WaitOne($left)) {
            $lines = Write-Timeline "never"
            $paths = @(Get-ChildItem 'HKCU:\Control Panel\NotifyIconSettings' -ErrorAction SilentlyContinue |
                    ForEach-Object { (Get-ItemProperty $_.PSPath).ExecutablePath }) -join "; "
            Write-Output "NotifyIconSettings holds: $paths"
            Write-Output "process.log, last 20 lines:"
            $lines | Select-Object -Last 20 | ForEach-Object { Write-Output "  $_" }
            Write-Error "Windows recorded no tray icon for $Bin within 30 s"
            exit 1
        }
    }
    $null = Write-Timeline ("+{0} ms" -f ([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() - $launched))
    Set-ItemProperty -Path $icon.PSPath -Name IsPromoted -Value 1 -Type DWord
    Write-Output "promoted the tray icon for $Bin"
} finally {
    & taskkill.exe /PID $proc.Id /T /F | Out-Null
}
# taskkill errs when a child is already gone; the promotion is what counts.
exit 0
