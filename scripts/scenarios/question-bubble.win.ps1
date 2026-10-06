#!/usr/bin/env pwsh
# Scenario: question-bubble (Windows)
# On screen: launches Fidget as BMO with a fixture Harness. Chat opens by
#   itself and shows a permission question. A Poke is sent to the sprite while
#   the question waits. Two UI Automation dumps of the overlay: one before the
#   Poke, one after it. Fidget quits when the scenario ends.
# Input: one real click on the sprite (the Poke).
# Duration: about 20 s, 1 min at most.
# Grants: a desktop session. UI Automation for the terminal that runs it. bash
#   on PATH for the fixture Harness wrapper.
# Asserts: no "Question for you" bubble before the Poke; the click lands as a
#   Poke; after it, the bubble reads "Question for you in the" with a "chat"
#   link button.
#
# Usage: question-bubble.win.ps1 --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
# Set FIDGET_SCENARIO_AX_BEFORE and FIDGET_SCENARIO_AX_AFTER to assert those
# dumps and skip the GUI. That checks the assertion, not the live window.
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
    Write-Error "usage: question-bubble.win.ps1 --go <fidget binary> <fidget test binary>"
    exit 1
}

$selfTestFrameLine = "1791259416 frame: 1791259416038 Grounded pos(1720,1392) sprite(1657,1264) idle#0 <id>"
$selfTestVerbsLine = "1791259408 overlay: 2 display(s); sprite 126x128; BMO as BMO"
$selfTestPokeLine = "1791259420 verbs: Poke"
$selfTestPresenceHidden = "1791260872 presence: hidden over 500ms"
$selfTestClickHitTrue = "1791260862 click: down hits=[true] target=Some(0) cursor=(2554,1328) visible=true sprite=untargeted(2563,1264)"
$selfTestClickHitFalse = "1791260862 click: down hits=[false] target=None cursor=(2554,1328) visible=true sprite=untargeted(2563,1264)"
$selfTestSettledFrame = "1791260861.975 frame: 1791260861975 Grounded pos(1720,1328) sprite(2491,1264) land#1"
if (-not ($selfTestFrameLine -match '^\d+ frame: .* sprite\(')) { Write-Error "self-test: frame pattern failed"; exit 1 }
if (-not ($selfTestFrameLine -match '^\d+ frame: .* sprite\((-?\d+),(-?\d+)\) ')) { Write-Error "self-test: frame coordinate pattern failed"; exit 1 }
if (-not ($selfTestPokeLine -match '^\d+ verbs: .*Poke')) { Write-Error "self-test: Poke pattern failed"; exit 1 }
if ($selfTestVerbsLine -match '^\d+ verbs: ') { Write-Error "self-test: should not match non-verbs line"; exit 1 }
if (-not ($selfTestPresenceHidden -match '^\d+ presence: hidden')) { Write-Error "self-test: presence:hidden pattern failed"; exit 1 }
if (-not ($selfTestClickHitTrue -match '^\d+ click: down hits=\[true\]')) { Write-Error "self-test: click hits=[true] pattern failed"; exit 1 }
if ($selfTestClickHitFalse -match '^\d+ click: down hits=\[true\]') { Write-Error "self-test: should not match hits=[false]"; exit 1 }
if (-not ($selfTestSettledFrame -match '^\d+\.?\d* frame: .* Grounded .* sprite\((-?\d+),(-?\d+)\) ')) { Write-Error "self-test: settled frame Grounded pattern failed"; exit 1 }
if ($selfTestSettledFrame -match 'walk#|Falling') { Write-Error "self-test: settled frame should not match walk# or Falling"; exit 1 }

$root = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$script:Evidence = $null

function Fail([string]$Message) {
    [Console]::Error.WriteLine("FAIL: $Message")
    if ($script:Evidence) { [Console]::Error.WriteLine("evidence in $($script:Evidence)") }
    $traceFile = Join-Path $out "home\AppData\Roaming\fidget\process.log"
    if (-not (Test-Path -LiteralPath $traceFile -ErrorAction SilentlyContinue)) {
        [Console]::Error.WriteLine("data_dir did not resolve under $out\home")
    }
    exit 1
}

function Read-Dump([string]$File) { @(Get-Content -LiteralPath $File -Encoding utf8) }

function Check-Before([string]$File) {
    if (Read-Dump $File | Where-Object { $_.Contains("Question for you") }) {
        Fail "the question cue showed before the Poke; see $File"
    }
    Write-Output "ok: no question cue before the Poke"
}

# The trailing space of "in the " may not survive the accessibility name, and
# "in the chat" as one label is the variant with no link to click.
function Check-After([string]$File) {
    $lines = Read-Dump $File
    if (-not ($lines | Where-Object { $_ -cmatch '^label\|Question for you in the ?$' })) {
        Fail "the bubble does not read 'Question for you in the'; see $File"
    }
    if (-not ($lines | Where-Object { $_ -ceq "button|chat" })) { Fail "the bubble has no 'chat' link button; see $File" }
    Write-Output "ok: bubble shows 'Question for you in the' and a 'chat' link button"
}

$before = $env:FIDGET_SCENARIO_AX_BEFORE
$after = $env:FIDGET_SCENARIO_AX_AFTER
if ($before -or $after) {
    if (-not $before -or -not $after) { Fail "set both FIDGET_SCENARIO_AX_BEFORE and FIDGET_SCENARIO_AX_AFTER" }
    Check-Before $before
    Check-After $after
    Write-Output "PASS: fixture dumps"
    exit 0
}

function Get-ShortPath([string]$Path) {
    $code = @"
    [DllImport("kernel32.dll", CharSet = CharSet.Auto, SetLastError = true)]
    public static extern uint GetShortPathName(string lpszLongPath, System.Text.StringBuilder lpszShortPath, uint cchBuffer);
"@
    $kernel32 = Add-Type -MemberDefinition $code -Name "Kernel32" -Namespace "Win32" -PassThru -ErrorAction SilentlyContinue
    $buffer = New-Object System.Text.StringBuilder 260
    $result = $kernel32::GetShortPathName($Path, $buffer, $buffer.Capacity)
    if ($result -gt 0) { return $buffer.ToString() }
    return $Path
}

$bashPath = $null
$gitBash = "C:\Program Files\Git\bin\bash.exe"
if (Test-Path -LiteralPath $gitBash -PathType Leaf) {
    $bashPath = Get-ShortPath $gitBash
} else {
    $bashCmd = Get-Command bash -ErrorAction SilentlyContinue
    if ($bashCmd) { $bashPath = Get-ShortPath $bashCmd.Path }
}
if (-not $bashPath) {
    [Console]::Error.WriteLine("SKIP: bash not found (tried Git Bash, then PATH)")
    exit 2
}

$TestBin = (Resolve-Path -LiteralPath $TestBin).Path
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$out = Join-Path $env:TEMP "fidget-scenario-question-bubble-$stamp"
$script:Evidence = $out
New-Item -ItemType Directory -Force -Path (Join-Path $out "home\AppData\Roaming") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $out "home\AppData\Local") | Out-Null

$settingsDir = Join-Path $out "home\AppData\Roaming\fidget"
New-Item -ItemType Directory -Force -Path $settingsDir | Out-Null
$settingsFile = Join-Path $settingsDir "settings.json"
$utf8 = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllText($settingsFile, '{"hide_in_fullscreen": false}', $utf8)

$marks = Join-Path $out "harness.log"
$log = Join-Path $out "app.log"
$err = Join-Path $out "app.err"
Set-Content -LiteralPath $marks -Value "" -Encoding ascii

# Convert Windows paths to forward slashes for bash.
$rootBash = $root -replace '\\', '/'
$testBinBash = $TestBin -replace '\\', '/'
$marksBash = $marks -replace '\\', '/'
$harness = "$bashPath $rootBash/scripts/scenarios/fixture-harness.sh $testBinBash script=scenario-asking count=$marksBash"
$paths = @($harness -split '\s+' | Select-Object -Skip 1)
if ($paths.Count -ne 4) { Fail "a path in the Harness line holds a space: $harness" }

$env:HOME = Join-Path $out "home"
$env:USERPROFILE = $env:HOME
$env:APPDATA = Join-Path $env:HOME "AppData\Roaming"
$env:FIDGET_HARNESS = $harness
$env:FIDGET_DIRECTOR_WAKE_SECS = "6"
$env:FIDGET_DIRECTOR_API_KEY = "x"
$env:FIDGET_CAPTURABLE = "1"
$env:FIDGET_TRACE_FRAMES = "1"
$env:FIDGET_TRACE_WINDOWS = "1"
$env:FIDGET_CHARACTER = "bmo"
$env:FIDGET_CHARACTERS = Join-Path $root "characters"

$utf8 = New-Object System.Text.UTF8Encoding $false
[Console]::OutputEncoding = $utf8
$OutputEncoding = $utf8

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
    $frontmost = $null
    $frontmostZ = [int]::MaxValue
    $currentZ = 0
    $foundFrontmost = $false
    [PreflightCheck]::EnumWindows({
        param($hwnd, $lParam)
        if (-not $foundFrontmost) {
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
                    $script:frontmost = @{Class=$cls.ToString(); Title=$title.ToString()}
                    $script:foundFrontmost = $true
                    return $false
                }
            }
        }
        return $true
    }, [IntPtr]::Zero) | Out-Null
    return $frontmost
}

$fullscreenApp = Get-FrontmostFullscreenApp
if ($fullscreenApp) {
    $preflightFile = Join-Path $out "preflight.txt"
    Set-Content -LiteralPath $preflightFile -Value "Class: $($fullscreenApp.Class)`nTitle: $($fullscreenApp.Title)" -Encoding utf8
    [Console]::Error.WriteLine("SKIP: fullscreen app '$($fullscreenApp.Title)' is frontmost; close or minimize it and re-run")
    [Console]::Error.WriteLine("evidence in $out")
    exit 2
}

$axPs1 = Join-Path $out "ax.ps1"
$ax = Join-Path $root "scripts\ax-window-win.ps1"
[System.IO.File]::WriteAllLines($axPs1, @(
        '[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false',
        '$OutputEncoding = [Console]::OutputEncoding',
        "& '$ax' @args"
    ), $utf8)

$proc = Start-Process -FilePath $Bin -RedirectStandardOutput $log -RedirectStandardError $err -PassThru -WindowStyle Normal
try {
    function Wait-For([int]$Seconds, [scriptblock]$Probe) {
        $n = $Seconds * 4
        while ($n -gt 0) {
            if (& $Probe) { return $true }
            if ($proc.HasExited) { Fail "Fidget exited; see $err" }
            Start-Sleep -Milliseconds 250
            $n--
        }
        return $false
    }
    function Invoke-Ax([string]$Label, [string[]]$AxArgs) {
        & powershell -NoProfile -ExecutionPolicy Bypass -File $axPs1 @AxArgs |
            Out-File -FilePath (Join-Path $out "$Label.txt") -Encoding utf8
        return ($LASTEXITCODE -eq 0)
    }
    # The overlay window is titled Fidget; Chat takes the Character's name.
    function Invoke-Dump([string]$Label) {
        if (-not (Invoke-Ax "$Label.ax" @("dump", "-ProcessId", $proc.Id, "-Title", "Fidget"))) {
            Fail "${Label}: UI Automation dump failed; see $out\$Label.ax.txt"
        }
        return (Join-Path $out "$Label.ax.txt")
    }
    function Marked([string]$Line) { [bool](Get-Content -LiteralPath $marks | Where-Object { $_ -eq $Line }) }
    function Traced([string]$Pattern) {
        $traceFile = Join-Path $out "home\AppData\Roaming\fidget\process.log"
        if (Test-Path -LiteralPath $traceFile) {
            [bool](Get-Content -LiteralPath $traceFile | Where-Object { $_ -match $Pattern })
        } else {
            return $false
        }
    }
    function Wait-Settled {
        $deadline = (Get-Date).AddSeconds(15)
        $lastX = $null
        $lastY = $null
        $firstSeenAt = $null
        while ((Get-Date) -lt $deadline) {
            if ($proc.HasExited) { Fail "Fidget exited; see $err" }
            $trace = Get-Content -LiteralPath $traceFile
            $recent = $trace | Select-String -Pattern '^\d+\.?\d* frame: .* Grounded .* sprite\((-?\d+),(-?\d+)\) ' | Select-Object -Last 1
            if (-not $recent) {
                Start-Sleep -Milliseconds 100
                continue
            }
            if ($recent.Line -match 'walk#|Falling') {
                $lastX = $null
                $lastY = $null
                $firstSeenAt = $null
                Start-Sleep -Milliseconds 100
                continue
            }
            $x = [int]$recent.Matches[0].Groups[1].Value
            $y = [int]$recent.Matches[0].Groups[2].Value
            if ($null -eq $lastX) {
                $lastX = $x
                $lastY = $y
                $firstSeenAt = Get-Date
                Start-Sleep -Milliseconds 100
                continue
            }
            $dx = [Math]::Abs($x - $lastX)
            $dy = [Math]::Abs($y - $lastY)
            if ($dx -le 2 -and $dy -le 2) {
                $elapsed = ((Get-Date) - $firstSeenAt).TotalMilliseconds
                if ($elapsed -ge 400) {
                    return @{X=$x; Y=$y}
                }
            } else {
                $lastX = $x
                $lastY = $y
                $firstSeenAt = Get-Date
            }
            Start-Sleep -Milliseconds 100
        }
        Fail "sprite did not settle within 15s; see $traceFile"
    }

    $traceFile = Join-Path $out "home\AppData\Roaming\fidget\process.log"
    if (-not (Wait-For 30 { Test-Path -LiteralPath $traceFile })) { Fail "process.log never appeared; see $err" }

    $settingsFile = Join-Path $out "home\AppData\Roaming\fidget\settings.json"
    if (Test-Path -LiteralPath $settingsFile) {
        $settingsContent = Get-Content -LiteralPath $settingsFile -Raw | ConvertFrom-Json
        if ($settingsContent.hide_in_fullscreen -ne $false) {
            Fail "settings.json does not have hide_in_fullscreen: false as seeded"
        }
    }

    if (-not (Wait-For 30 { Marked "asked" })) { Fail "no wake reached the Harness; see $err" }

    if (Traced '^\d+ presence: hidden') {
        Fail "overlay hidden (presence: hidden) - see process.log window_source lines"
    }

    Start-Sleep -Seconds 1

    Check-Before (Invoke-Dump "before-poke")

    $settled = Wait-Settled
    $trace = Get-Content -LiteralPath $traceFile
    $size = $trace | Select-String -Pattern 'sprite (\d+)x(\d+);' | Select-Object -First 1
    if (-not $size) { Fail "no sprite size in $traceFile" }
    $w = [int]$size.Matches[0].Groups[1].Value
    $h = [int]$size.Matches[0].Groups[2].Value

    $attemptsFile = Join-Path $out "poke-attempts.txt"
    $maxAttempts = 3
    $success = $false
    for ($attempt = 1; $attempt -le $maxAttempts; $attempt++) {
        $attemptLog = "Attempt $attempt of $maxAttempts"
        Add-Content -LiteralPath $attemptsFile -Value $attemptLog
        $trace = Get-Content -LiteralPath $traceFile
        $clickCountBefore = @($trace | Select-String -Pattern '^\d+ click: down ').Count
        if (-not (Invoke-Ax "poke-$attempt" @("click", "-TraceFile", $traceFile, "-SpriteW", $w, "-SpriteH", $h))) {
            $msg = "could not click the sprite on attempt $attempt; see $out\poke-$attempt.txt"
            Add-Content -LiteralPath $attemptsFile -Value $msg
            if ($attempt -eq $maxAttempts) { Fail $msg }
            Start-Sleep -Milliseconds 600
            continue
        }
        Start-Sleep -Milliseconds 500
        $trace = Get-Content -LiteralPath $traceFile
        $clicksAfter = @($trace | Select-String -Pattern '^\d+ click: down ')
        $thisAttemptClick = $null
        if ($clicksAfter.Count -gt $clickCountBefore) {
            $thisAttemptClick = $clicksAfter[$clickCountBefore]
            Add-Content -LiteralPath $attemptsFile -Value $thisAttemptClick.Line
        }
        if ($thisAttemptClick -and ($thisAttemptClick.Line -match '^\d+ click: down hits=\[true\]')) {
            $verbsAfter = @($trace | Select-String -Pattern '^\d+ verbs: ')
            $grabFound = $false
            $pokeFound = $false
            for ($i = $verbsAfter.Count - 1; $i -ge 0; $i--) {
                if ($verbsAfter[$i].Line -match '\[Grab\]') { $grabFound = $true; break }
                if ($verbsAfter[$i].Line -match 'Poke') { $pokeFound = $true; break }
            }
            if ($grabFound) {
                $msg = "click held past DRAG_DELAY_MS, became Grab instead of Poke on attempt $attempt"
                Add-Content -LiteralPath $attemptsFile -Value $msg
                if ($attempt -eq $maxAttempts) { Fail $msg }
            } elseif ($pokeFound) {
                $success = $true
                break
            }
        }
        if ($attempt -lt $maxAttempts) {
            Start-Sleep -Milliseconds 600
        }
    }
    if (-not $success) {
        Fail "click did not land as Poke after $maxAttempts attempts; see $attemptsFile"
    }
    Start-Sleep -Seconds 1
    Check-After (Invoke-Dump "after-poke")
    Write-Output "PASS: evidence in $out"
} finally {
    if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
    Get-CimInstance Win32_Process -Filter "Name = 'bash.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.CommandLine -like "*count=$marks*" } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}
