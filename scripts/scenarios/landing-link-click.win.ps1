#!/usr/bin/env pwsh
# Scenario: landing-link-click (Windows)
# On screen: launches Fidget as BMO with Codex picked and no `npx` on PATH, so
#   no Harness runs. The taskbar icon's menu opens and its Chat... row is
#   invoked, so Chat opens on the "Codex needs `npx`" landing and takes focus.
#   The install link is clicked, which opens https://nodejs.org/ in the default
#   browser. Two UI Automation dumps. Fidget quits at the end.
# Input: one real click on the taskbar icon, Invoke on Chat..., Invoke on the
#   link (a real click only if it has no Invoke); no keys.
# Duration: a few seconds, 10 s at most.
# Grants: a desktop session. UI Automation for the terminal that runs it. The
#   fidget icon shown on the taskbar, not in the hidden-icons overflow.
# Asserts: the landing draws `npx` and https://nodejs.org/ as their own
#   elements, with no backtick left in the copy; the click raises no "That
#   link did not open" note, so ShellExecuteW took the URL; Chat still shows
#   the landing after it, so the webview did not navigate. Windows hands the
#   URL to ShellExecuteW, not a program on PATH, so which URL it got is not
#   recorded here; tests/chat-landing-format.test.js asserts that.
#
# Usage: landing-link-click.win.ps1 --go <fidget binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
# Set FIDGET_SCENARIO_AX_LANDING and FIDGET_SCENARIO_AX_AFTER to assert those
# dumps and skip the GUI. That checks the assertion, not the live window.
# No param() block: Windows PowerShell binds `--go` as -Go and swallows the
# next token, so $Go never equals "--go" and the leaf always exits 2.
$Go = if ($args.Count -gt 0) { $args[0] } else { "" }
$Bin = if ($args.Count -gt 1) { $args[1] } else { "" }
$ErrorActionPreference = "Stop"
$Url = "https://nodejs.org/"

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
    Write-Error "usage: landing-link-click.win.ps1 --go <fidget binary>"
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

function Read-Dump([string]$File) { @(Get-Content -LiteralPath $File -Encoding utf8) }
function Test-Landing([string]$File) { [bool](Read-Dump $File | Where-Object { $_.Contains("|Codex needs ") }) }

# Sets $script:Link to the link's frame as x, y, w, h.
function Check-Landing([string]$File) {
    $lines = Read-Dump $File
    if (-not (Test-Landing $File)) { Fail "Chat shows no Codex needs npx landing; see $File" }
    if ($lines | Where-Object { $_.Contains('`') }) { Fail "a backtick is left in the landing copy; see $File" }
    if (-not ($lines | Where-Object { ($_ -split '\|')[1] -eq "npx" })) { Fail "npx is not drawn as its own element; see $File" }
    $row = $lines | Where-Object { ($_ -split '\|')[1] -eq $Url } | Select-Object -First 1
    if (-not $row) { Fail "$Url is not drawn as its own element; see $File" }
    $script:Link = @(($row -split '\|')[-1] -split ',' | ForEach-Object { [int]$_ })
    Write-Output "ok: the landing draws npx and $Url apart from the copy, no backtick left"
}

function Check-After([string]$File) {
    if (-not (Test-Landing $File)) { Fail "Chat left the landing after the click, so the webview navigated; see $File" }
    $refused = Read-Dump $File | Where-Object { $_.Contains("That link did not open") } | Select-Object -First 1
    if ($refused) { Fail "the click did not reach ShellExecuteW: $refused" }
    Write-Output "ok: Chat still shows the landing, and no note says the link did not open"
}

function Get-LinkOutcome([string[]]$Dump, [string[]]$NewTitles) {
    if ($Dump | Where-Object { $_.Contains("That link did not open") }) { return "refused" }
    if (-not ($Dump | Where-Object { $_.Contains("|Codex needs ") })) { return "navigated" }
    if ($NewTitles | Where-Object { $_ -and $_ -notin @("BMO", "Fidget") }) { return "opened" }
    return $null
}

function Get-TopWindows {
    $all = [System.Windows.Automation.AutomationElement]::RootElement.FindAll(
        [System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)
    foreach ($w in $all) { [pscustomobject]@{ Handle = $w.Current.NativeWindowHandle; Title = $w.Current.Name } }
}

$landing = $env:FIDGET_SCENARIO_AX_LANDING
$after = $env:FIDGET_SCENARIO_AX_AFTER
if ($landing -or $after) {
    if (-not $landing -or -not $after) { Fail "set FIDGET_SCENARIO_AX_LANDING and FIDGET_SCENARIO_AX_AFTER" }
    Check-Landing $landing
    Check-After $after
    Write-Output "PASS: fixture dumps"
    exit 0
}

$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$out = Join-Path $env:TEMP "fidget-scenario-landing-link-click-$stamp"
$script:Evidence = $out
$data = Initialize-ScenarioData $out
$log = Join-Path $out "app.log"

$env:PATH = "$env:SystemRoot\System32;$env:SystemRoot;$env:SystemRoot\System32\WindowsPowerShell\v1.0"
if (Get-Command npx -ErrorAction SilentlyContinue) {
    [Console]::Error.WriteLine("SKIP: npx is on $env:PATH, so Codex would launch and no landing shows.")
    exit 2
}

$env:FIDGET_HARNESS = "codex"
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

$startMs = Get-NowMs
$proc = Start-FidgetPiped $Bin @() $log (Join-Path $out "app.err")
try {
    function Invoke-Ax([string]$Label, [hashtable]$AxArgs) {
        return (Invoke-AxWin (Join-Path $out "$Label.txt") $AxArgs)
    }
    function Shows-Landing([string]$Name) {
        if (-not (Invoke-Ax "$Name.ax" @{ Command = "dump"; ProcessId = $proc.Id; Title = "BMO"; Arg3 = "frames" })) { return $false }
        return (Test-Landing (Join-Path $out "$Name.ax.txt"))
    }

    if (-not (Invoke-Ax "open" @{ Command = "tray"; ProcessId = $proc.Id; Title = "BMO"; Arg3 = "Chat" })) {
        Fail "the tray's Chat row did not open Chat; see $out\open.txt"
    }
    if (-not (Wait-ForUia 3 $proc.Id { Shows-Landing "landing" })) {
        Fail "Chat shows no Codex needs npx landing; see $out\landing.ax.txt"
    }
    Check-Landing (Join-Path $out "landing.ax.txt")
    $x = $script:Link[0] + [int]($script:Link[2] / 2)
    $y = $script:Link[1] + [int]($script:Link[3] / 2)
    $before = @(Get-TopWindows | ForEach-Object { $_.Handle })
    # UI Automation's Invoke first, so the real cursor stays put; a real click
    # at the link's centre only if the webview exposes no Invoke on it.
    if (Invoke-Ax "click" @{ Command = "link"; ProcessId = $proc.Id; Title = "BMO"; Arg3 = $Url }) {
        Write-Output "ok: invoked the link through UI Automation"
    } elseif (-not (Invoke-Ax "click" @{ Command = "click"; X = $x; Y = $y })) {
        Fail "could not click the link; see $out\click.txt"
    }
    # Block on UI Automation events from any process (a browser window opening,
    # Chat changing) until the click shows an outcome, at most 3 s.
    $script:Outcome = $null
    $null = Wait-UiaChange 0 3 {
        if (-not (Invoke-Ax "after.ax" @{ Command = "dump"; ProcessId = $proc.Id; Title = "BMO"; Arg3 = "frames" })) { return $false }
        $new = @(Get-TopWindows | Where-Object { $_.Handle -notin $before } | ForEach-Object { $_.Title })
        $script:Outcome = Get-LinkOutcome (Read-Dump (Join-Path $out "after.ax.txt")) $new
        [bool]$script:Outcome
    }
    if (-not $script:Outcome) { Fail "the click showed no outcome (browser window, navigation or note) within 3 s; see $out\after.ax.txt" }
    Write-Output "ok: the click's outcome is $script:Outcome"
    Check-After (Join-Path $out "after.ax.txt")
    Write-Output "PASS: evidence in $out ($((Get-NowMs) - $startMs)ms)"
} finally {
    Stop-FidgetTree $proc
}
