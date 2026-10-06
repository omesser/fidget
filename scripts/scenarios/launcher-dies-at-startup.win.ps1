#!/usr/bin/env pwsh
# Scenario: launcher-dies-at-startup (Windows)
# On screen: launches Fidget as BMO with a fixture Harness whose first launch
#   aborts before initialize. The taskbar icon's menu opens and its Chat... row
#   is invoked, so Chat opens and takes focus. Chat is resized to 420 and 320
#   wide, then Codex is pressed. Four UI Automation dumps. Fidget quits at the end.
# Input: one real click on the taskbar icon; Invoke on Chat... and Codex; the
#   resizes go through Win32 SetWindowPos; no keys.
# Duration: about 30 s, 2 min at most.
# Grants: a desktop session. UI Automation for the terminal that runs it. bash
#   on PATH for the fixture Harness wrapper. The fidget icon shown on the
#   taskbar, not in the hidden-icons overflow.
# Asserts: what only a live run can: the tray's Chat... row opens the Harness
#   error landing; at 420 and 320 its Error output and Command boxes end inside
#   the window and Error output starts above the composer; Codex, a re-pick
#   under FIDGET_HARNESS, launches the Harness again at once. Copy, labels and
#   capture are chat-landing-format.test.js's and the Rust tests'.
#
# Usage: launcher-dies-at-startup.win.ps1 --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
# Set FIDGET_SCENARIO_AX_FAILED, FIDGET_SCENARIO_AX_420, FIDGET_SCENARIO_AX_320
# and FIDGET_SCENARIO_AX_LIVE to assert those dumps and skip the GUI. That
# checks the assertion, not the live window.
# No param() block: Windows PowerShell binds `--go` as -Go and swallows the
# next token, so $Go never equals "--go" and the leaf always exits 2.
$Go = if ($args.Count -gt 0) { $args[0] } else { "" }
$Bin = if ($args.Count -gt 1) { $args[1] } else { "" }
$TestBin = if ($args.Count -gt 2) { $args[2] } else { "" }
$ErrorActionPreference = "Stop"
$Dyld = "dyld[0]: Library not loaded"
$Dot = [string][char]0x00B7

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
    Write-Error "usage: launcher-dies-at-startup.win.ps1 --go <fidget binary> <fidget test binary>"
    exit 1
}

$root = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$script:Evidence = $null

function Fail([string]$Message) {
    [Console]::Error.WriteLine("FAIL: $Message")
    if ($script:Evidence) { [Console]::Error.WriteLine("evidence in $($script:Evidence)") }
    exit 1
}

function Read-Dump([string]$File) { @(Get-Content -LiteralPath $File -Encoding utf8) }
function First-Line([string]$File, [string]$Text) {
    Read-Dump $File | Where-Object { $_.Contains($Text) } | Select-Object -First 1
}
function Box([string]$File, [string]$Text) {
    Read-Dump $File | Where-Object { $_.StartsWith("label|") -and $_.Contains($Text) } | Select-Object -First 1
}
function Get-Rect([string]$Line) { @(($Line -split '\|')[-1] -split ',' | ForEach-Object { [int]$_ }) }

function Test-Failed([string]$File) { [bool](First-Line $File "|Harness couldn't start|") }
function Test-Live([string]$File) {
    [bool]((First-Line $File "|$Name $Dot session ") -or (First-Line $File "|$Name $Dot no session yet|"))
}

function Check-Failed([string]$File) {
    if (-not (Test-Failed $File)) { Fail "Chat shows no Harness error landing; see $File" }
    Write-Output "ok: the tray's Chat row opened the Harness error landing"
}

function Check-Fits([int]$Width, [string]$File) {
    $frameLine = Read-Dump $File | Where-Object { $_ -match '^frame\|' } | Select-Object -First 1
    if (-not $frameLine) { Fail "${Width}: no frame line in $File" }
    $frame = Get-Rect $frameLine
    if ($frame[2] -ne $Width) { Fail "${Width}: frame is $($frame[2]) wide in $File" }
    $edge = $frame[0] + $frame[2]
    foreach ($text in @($Dyld, "|$Harness|")) {
        $row = Box $File $text
        if (-not $row) { Fail "${Width}: no box holds '$text' in $File" }
        $r = Get-Rect $row
        if (($r[0] + $r[2]) -gt ($edge + 1)) { Fail "${Width}: a box ends at $($r[0] + $r[2]), past the window edge at $edge" }
    }
    # The composer covers the log's foot, so the fold is its top, not the window's.
    $y = (Get-Rect (Box $File $Dyld))[1]
    $composer = Read-Dump $File | Where-Object { $_.StartsWith("entry|Nothing can answer yet|") } | Select-Object -First 1
    if (-not $composer) { Fail "${Width}: no composer in $File" }
    $fold = (Get-Rect $composer)[1]
    if ($y -ge $fold) { Fail "${Width}: Error output starts at y $y, under the composer at $fold" }
    Write-Output "ok: ${Width}: both boxes inside the window, Error output at y $y above the composer at $fold"
}

function Check-Live([string]$File) {
    if (-not (Test-Live $File)) { Fail "after the re-pick the mind line names no live Harness; see $File" }
    Write-Output "ok: the re-pick launched it again at once"
}

$axFailed = $env:FIDGET_SCENARIO_AX_FAILED
$ax420 = $env:FIDGET_SCENARIO_AX_420
$ax320 = $env:FIDGET_SCENARIO_AX_320
$axLive = $env:FIDGET_SCENARIO_AX_LIVE
if ($axFailed -or $ax420 -or $ax320 -or $axLive) {
    if (-not $axFailed -or -not $ax420 -or -not $ax320 -or -not $axLive) {
        Fail "set FIDGET_SCENARIO_AX_FAILED, FIDGET_SCENARIO_AX_420, FIDGET_SCENARIO_AX_320 and FIDGET_SCENARIO_AX_LIVE"
    }
    # The launcher line the fixture dumps name.
    $Harness = if ($env:FIDGET_SCENARIO_HARNESS) { $env:FIDGET_SCENARIO_HARNESS } else {
        "/opt/fidget/scripts/scenarios/fixture-harness.sh /opt/fidget/target/debug/deps/fidget-0 script=abort-first count=/tmp/harness.log"
    }
    $Name = ($Harness -split ' ')[0]
    Check-Failed $axFailed
    Check-Fits 420 $ax420
    Check-Fits 320 $ax320
    Check-Live $axLive
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
$out = Join-Path $env:TEMP "fidget-scenario-launcher-dies-at-startup-$stamp"
$script:Evidence = $out
New-Item -ItemType Directory -Force -Path (Join-Path $out "home") | Out-Null
$marks = Join-Path $out "harness.log"
$log = Join-Path $out "app.log"
$errLog = Join-Path $out "app.err"
Set-Content -LiteralPath $marks -Value "" -Encoding ascii

# `abort-first` prints a dyld line and aborts on its first spawn, then answers.
$Harness = "$bashPath $root/scripts/scenarios/fixture-harness.sh $TestBin script=abort-first count=$marks"
$paths = @($Harness -split '\s+' | Select-Object -Skip 1)
if ($paths.Count -ne 4) { Fail "a path in the Harness line holds a space: $Harness" }
$Name = "bash"

# No ambient wake: one would respawn the Harness and pass the re-pick for it.
$env:HOME = Join-Path $out "home"
$env:USERPROFILE = $env:HOME
$env:FIDGET_HARNESS = $Harness
$env:FIDGET_DIRECTOR_WAKE_SECS = "600"
$env:FIDGET_DIRECTOR_API_KEY = "x"
$env:FIDGET_CAPTURABLE = "1"
$env:FIDGET_CHARACTER = "bmo"
$env:FIDGET_CHARACTERS = Join-Path $root "characters"

$utf8 = New-Object System.Text.UTF8Encoding $false
[Console]::OutputEncoding = $utf8
$OutputEncoding = $utf8
$axPs1 = Join-Path $out "ax.ps1"
$ax = Join-Path $root "scripts\ax-window-win.ps1"
[System.IO.File]::WriteAllLines($axPs1, @(
        '[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false',
        '$OutputEncoding = [Console]::OutputEncoding',
        "& '$ax' @args"
    ), $utf8)

$proc = Start-Process -FilePath $Bin -RedirectStandardOutput $log -RedirectStandardError $errLog -PassThru -WindowStyle Normal
try {
    function Wait-For([int]$Seconds, [scriptblock]$Probe) {
        $n = $Seconds * 4
        while ($n -gt 0) {
            if (& $Probe) { return $true }
            if ($proc.HasExited) { Fail "Fidget exited; see $errLog" }
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
    function Invoke-Dump([string]$Name) {
        return (Invoke-Ax "$Name.ax" @("dump", "-ProcessId", $proc.Id, "-Title", "BMO", "frames"))
    }
    function Get-Spawns { @(Get-Content -LiteralPath $marks | Where-Object { $_ -eq "spawn" }).Count }

    if (-not (Wait-For 20 { Select-String -LiteralPath $errLog -Pattern 'exited before initialize' -Quiet })) {
        Fail "the launcher never died before initialize; see $errLog"
    }
    if ((Get-Spawns) -ne 1) { Fail "want one spawn before Chat opens, the fixture saw $(Get-Spawns)" }
    if (-not (Invoke-Ax "open" @("tray", "-ProcessId", $proc.Id, "-Title", "BMO", "Chat"))) {
        Fail "the tray's Chat row did not open Chat; see $out\open.txt"
    }
    $failed = Join-Path $out "failed.ax.txt"
    $null = Wait-For 15 { (Invoke-Dump "failed") -and (Test-Failed $failed) }
    Check-Failed $failed

    foreach ($w in @(420, 320)) {
        if (-not (Invoke-Ax "$w.frame" @("size", "-ProcessId", $proc.Id, "-Title", "BMO", $w, 560))) {
            Fail "${w}: resize failed; see $out\$w.frame.txt"
        }
        $got = [int]((Get-Content -LiteralPath (Join-Path $out "$w.frame.txt") -Encoding utf8 | Select-Object -First 1) -split ',')[2]
        if ($got -ne $w) { Fail "${w}: Chat is $got wide after the resize" }
        Start-Sleep -Seconds 1
        if (-not (Invoke-Dump "$w")) { Fail "${w}: UI Automation dump failed; see $out\$w.ax.txt" }
        Check-Fits $w (Join-Path $out "$w.ax.txt")
    }

    if ((Get-Spawns) -ne 1) { Fail "the Harness launched again before the re-pick ($(Get-Spawns) spawns)" }
    if (-not (Invoke-Ax "press" @("press", "-ProcessId", $proc.Id, "-Title", "BMO", "Codex"))) {
        Fail "could not press Codex on the landing; see $out\press.txt"
    }
    if (-not (Wait-For 10 { (Get-Spawns) -ge 2 })) { Fail "the re-pick did not launch the Harness again; see $errLog" }
    $live = Join-Path $out "live.ax.txt"
    $null = Wait-For 15 { (Invoke-Dump "live") -and (Test-Live $live) }
    Check-Live $live
    Write-Output "PASS: evidence in $out"
} finally {
    if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
    Get-CimInstance Win32_Process -Filter "Name = 'bash.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.CommandLine -like "*count=$marks*" } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}
