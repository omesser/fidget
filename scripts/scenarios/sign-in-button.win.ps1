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
. (Join-Path $PSScriptRoot "wait-helpers.win.ps1")
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

. (Join-Path $PSScriptRoot "bash-setup-win.ps1")
if (-not $bashPath) {
    [Console]::Error.WriteLine("SKIP: bash not found (tried Git Bash, then PATH)")
    exit 2
}

$TestBin = (Resolve-Path -LiteralPath $TestBin).Path
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$out = Join-Path $env:TEMP "fidget-scenario-sign-in-button-$stamp"
$script:Evidence = $out
$data = Initialize-ScenarioData $out
$marks = Join-Path $out "harness.log"
$log = Join-Path $out "app.log"
Set-Content -LiteralPath $marks -Value "" -Encoding ascii

# Chat names the Harness by its launcher, the first word of the line.
$harness = "$bashPath $root/scripts/scenarios/fixture-harness.sh $TestBin script=auth-sign-in-link count=$marks"
$paths = @($harness -split '\s+' | Select-Object -Skip 1)
if ($paths.Count -ne 4) { Fail "a path in the Harness line holds a space: $harness" }
$Launcher = "bash"

$env:FIDGET_HARNESS = $harness
$env:FIDGET_DIRECTOR_WAKE_SECS = "1"
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

$startMs = Get-NowMs
$proc = Start-FidgetPiped $Bin @() $log (Join-Path $out "app.err")
try {
    function Invoke-Ax([string]$Label, [hashtable]$AxArgs) {
        return (Invoke-AxWin (Join-Path $out "$Label.txt") $AxArgs)
    }
    function Invoke-Dump([string]$Label) {
        if (-not (Invoke-Ax "$Label.ax" @{ Command = "dump"; ProcessId = $proc.Id; Title = "BMO"; Arg3 = "frames" })) {
            Fail "${Label}: UI Automation dump failed; see $out\$Label.ax.txt"
        }
        return (Join-Path $out "$Label.ax.txt")
    }

    if (-not (Wait-ForMarks 3 $marks '^spawn$')) { Fail "Harness never spawned; see $log" }
    # The 1 s timed wake sends session/new (refused: auth required), showing
    # needs-login; session/new comes only from a wake, not from attach alone.
    if (-not (Invoke-Ax "open" @{ Command = "tray"; ProcessId = $proc.Id; Title = "BMO"; Arg3 = "Chat" })) {
        Fail "the tray's Chat row did not open Chat; see $out\open.txt"
    }
    if (-not (Wait-ForMarks 2 $marks '^new$')) { Fail "timed wake did not send session/new; see $marks" }

    Check-NeedsLogin (Invoke-Dump "needs-login")
    if (-not (Invoke-Ax "press" @{ Command = "press"; ProcessId = $proc.Id; Title = "BMO"; Arg3 = "Fake login" })) {
        Fail "could not press Fake login button; see $out\press.txt"
    }
    if (-not (Wait-ForMarks 2 $marks '^authenticate$')) { Fail "sign-in button did not trigger authenticate; see $log" }
    Check-Waiting (Invoke-Dump "waiting")

    if (-not (Invoke-Ax "press-open" @{ Command = "press"; ProcessId = $proc.Id; Title = "BMO"; Arg3 = "Open" })) {
        Fail "could not press Open on the sign-in link; see $out\press-open.txt"
    }
    if (-not (Wait-ForMarks 2 $marks '^elicit-url:accept$')) { Fail "sign-in form never answered; see $log" }
    Write-Output "ok: Open accepted the link"
    if (-not (Wait-ForCount 3 $marks '^new$' 1)) { Fail "session did not open after sign-in; see $log" }
    Check-SignedIn (Invoke-Dump "signed-in")
    Write-Output "PASS: evidence in $out ($((Get-NowMs) - $startMs)ms)"
} finally {
    Stop-FidgetTree $proc "count=$marks"
}
