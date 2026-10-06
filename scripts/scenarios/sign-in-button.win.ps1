#!/usr/bin/env pwsh
# Scenario: sign-in-button (Windows)
# On screen: launches Fidget as BMO with a fixture Harness that advertises
#   agent sign-in. The taskbar icon's Chat... row opens Chat on the needs-login
#   landing, and Chat takes focus. Two button presses. Open hands the sign-in
#   URL to the default browser, which shows an example.test error page.
#   Three UI Automation dumps of the Chat window. Fidget quits when it ends.
# Input: one real click on the taskbar icon; Invoke on Chat... and on the
#   sign-in button and on Open.
# Duration: about 30 s, 2 min at most.
# Grants: a desktop session. UI Automation for the terminal that runs it. bash
#   on PATH for the fixture Harness wrapper. The fidget icon shown on the
#   taskbar, not in the hidden-icons overflow.
# Asserts: the sign-in button shows on the needs-login landing; pressing it
#   shows the waiting line; once authenticate completes, a session opens and
#   the waiting line disappears. Windows hands Open's URL to ShellExecuteW, not
#   a program on PATH, so which URL it got is not recorded here.
#
# Usage: sign-in-button.win.ps1 --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
# Set FIDGET_SCENARIO_AX_NEEDS_LOGIN, FIDGET_SCENARIO_AX_WAITING and
# FIDGET_SCENARIO_AX_SIGNED_IN to assert those dumps and skip the GUI. That
# checks the assertion, not the live window.
# No param() block: Windows PowerShell binds `--go` as -Go and swallows the
# next token, so $Go never equals "--go" and the leaf always exits 2.
$Go = if ($args.Count -gt 0) { $args[0] } else { "" }
$Bin = if ($args.Count -gt 1) { $args[1] } else { "" }
$TestBin = if ($args.Count -gt 2) { $args[2] } else { "" }
$ErrorActionPreference = "Stop"
$Dot = [string][char]0x00B7
$Waiting = "Finish signing in in your browser"

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
    Write-Error "usage: sign-in-button.win.ps1 --go <fidget binary> <fidget test binary>"
    exit 1
}

$root = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$script:Evidence = $null

function Fail([string]$Message) {
    [Console]::Error.WriteLine("FAIL: $Message")
    if ($script:Evidence) { [Console]::Error.WriteLine("evidence in $($script:Evidence)") }
    exit 1
}

function Has([string]$File, [string]$Text) {
    [bool](Get-Content -LiteralPath $File -Encoding utf8 | Where-Object { $_.Contains($Text) })
}

function Check-NeedsLogin([string]$File) {
    if (-not (Has $File "button|Fake login|")) { Fail "no Fake login button on needs-login; see $File" }
    if (-not (Has $File "Or run this in a terminal:")) { Fail "no terminal command hint on needs-login; see $File" }
    Write-Output "ok: needs-login shows Fake login button"
}

function Check-Waiting([string]$File) {
    $line = Get-Content -LiteralPath $File -Encoding utf8 | Where-Object { $_.StartsWith("label|") -and $_.Contains($Waiting) }
    if (-not $line) { Fail "no waiting line after button press; see $File" }
    if (-not (Has $File "came from $Launcher, so")) { Fail "waiting line does not name the Harness; see $File" }
    Write-Output "ok: waiting line appears after button press"
}

function Check-SignedIn([string]$File) {
    if (-not (Has $File "|$Launcher $Dot session ")) { Fail "mind line does not name the session; see $File" }
    if (Has $File $Waiting) { Fail "waiting line still present after sign-in; see $File" }
    Write-Output "ok: session opened, waiting line cleared"
}

$axNeeds = $env:FIDGET_SCENARIO_AX_NEEDS_LOGIN
$axWaiting = $env:FIDGET_SCENARIO_AX_WAITING
$axSigned = $env:FIDGET_SCENARIO_AX_SIGNED_IN
if ($axNeeds -or $axWaiting -or $axSigned) {
    if (-not $axNeeds -or -not $axWaiting -or -not $axSigned) {
        Fail "set FIDGET_SCENARIO_AX_NEEDS_LOGIN, FIDGET_SCENARIO_AX_WAITING and FIDGET_SCENARIO_AX_SIGNED_IN"
    }
    # The launcher the fixture dumps name.
    $Launcher = if ($env:FIDGET_SCENARIO_LAUNCHER) { $env:FIDGET_SCENARIO_LAUNCHER } else { "/opt/fidget/scripts/scenarios/fixture-harness.sh" }
    Check-NeedsLogin $axNeeds
    Check-Waiting $axWaiting
    Check-SignedIn $axSigned
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
$out = Join-Path $env:TEMP "fidget-scenario-sign-in-button-$stamp"
$script:Evidence = $out
New-Item -ItemType Directory -Force -Path (Join-Path $out "home") | Out-Null
$marks = Join-Path $out "harness.log"
$log = Join-Path $out "app.log"
Set-Content -LiteralPath $marks -Value "" -Encoding ascii

# Chat names the Harness by its launcher, the first word of the line.
$harness = "$bashPath $root/scripts/scenarios/fixture-harness.sh $TestBin script=auth-sign-in-link count=$marks"
$paths = @($harness -split '\s+' | Select-Object -Skip 1)
if ($paths.Count -ne 4) { Fail "a path in the Harness line holds a space: $harness" }
$Launcher = "bash"

$env:HOME = Join-Path $out "home"
$env:USERPROFILE = $env:HOME
$env:FIDGET_HARNESS = $harness
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

$proc = Start-Process -FilePath $Bin -RedirectStandardOutput $log -RedirectStandardError (Join-Path $out "app.err") -PassThru -WindowStyle Normal
try {
    function Wait-For([int]$Seconds, [scriptblock]$Probe) {
        $n = $Seconds * 4
        while ($n -gt 0) {
            if (& $Probe) { return $true }
            if ($proc.HasExited) { Fail "Fidget exited; see $log" }
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
    function Invoke-Dump([string]$Label) {
        if (-not (Invoke-Ax "$Label.ax" @("dump", "-ProcessId", $proc.Id, "-Title", "BMO", "frames"))) {
            Fail "${Label}: UI Automation dump failed; see $out\$Label.ax.txt"
        }
        return (Join-Path $out "$Label.ax.txt")
    }
    function Marked([string]$Line) { [bool](Get-Content -LiteralPath $marks | Where-Object { $_ -eq $Line }) }

    if (-not (Wait-For 30 { Marked "spawn" })) { Fail "Harness never spawned; see $log" }
    # Needs-login does not open Chat by itself; only a link Fidget's own sign-in
    # raises does (docs/harness.md).
    if (-not (Wait-For 15 { Marked "new" })) { Fail "the Harness never asked for a session; see $marks" }
    if (-not (Invoke-Ax "open" @("tray", "-ProcessId", $proc.Id, "-Title", "BMO", "Chat"))) {
        Fail "the tray's Chat row did not open Chat; see $out\open.txt"
    }

    Check-NeedsLogin (Invoke-Dump "needs-login")
    if (-not (Invoke-Ax "press" @("press", "-ProcessId", $proc.Id, "-Title", "BMO", "Fake login"))) {
        Fail "could not press Fake login button; see $out\press.txt"
    }
    Start-Sleep -Seconds 1
    Check-Waiting (Invoke-Dump "waiting")

    if (-not (Invoke-Ax "press-open" @("press", "-ProcessId", $proc.Id, "-Title", "BMO", "Open"))) {
        Fail "could not press Open on the sign-in link; see $out\press-open.txt"
    }
    if (-not (Wait-For 10 { Marked "elicit-url:accept" })) { Fail "sign-in form never answered; see $log" }
    Write-Output "ok: Open accepted the link"
    $sessions = { @(Get-Content -LiteralPath $marks | Where-Object { $_ -eq "new" }).Count -ge 2 }
    if (-not (Wait-For 15 $sessions)) { Fail "session did not open after sign-in; see $log" }
    Start-Sleep -Milliseconds 1500
    Check-SignedIn (Invoke-Dump "signed-in")
    Write-Output "PASS: evidence in $out"
} finally {
    if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
    Get-CimInstance Win32_Process -Filter "Name = 'bash.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.CommandLine -like "*count=$marks*" } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}
