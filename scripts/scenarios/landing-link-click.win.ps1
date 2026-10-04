#!/usr/bin/env pwsh
# Scenario: landing-link-click (Windows)
# On screen: launches Fidget as BMO with Codex picked and no `npx` on PATH, so
#   no Harness runs. The taskbar icon's menu opens and its Chat... row is
#   invoked, so Chat opens on the "Codex needs `npx`" landing and takes focus.
#   The install link is clicked, which opens https://nodejs.org/ in the default
#   browser. Two UI Automation dumps. Fidget quits at the end.
# Input: one real click on the taskbar icon, Invoke on Chat..., one real click
#   on the link; no keys.
# Duration: about 20 s, 1 min at most.
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
New-Item -ItemType Directory -Force -Path (Join-Path $out "home") | Out-Null
$log = Join-Path $out "app.log"

$env:PATH = "$env:SystemRoot\System32;$env:SystemRoot;$env:SystemRoot\System32\WindowsPowerShell\v1.0"
if (Get-Command npx -ErrorAction SilentlyContinue) {
    [Console]::Error.WriteLine("SKIP: npx is on $env:PATH, so Codex would launch and no landing shows.")
    exit 2
}

$env:HOME = Join-Path $out "home"
$env:USERPROFILE = $env:HOME
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
    function Shows-Landing([string]$Name) {
        if (-not (Invoke-Ax "$Name.ax" @("dump", "-ProcessId", $proc.Id, "-Title", "BMO", "frames"))) { return $false }
        return (Test-Landing (Join-Path $out "$Name.ax.txt"))
    }

    Start-Sleep -Seconds 3
    if (-not (Invoke-Ax "open" @("tray", "-ProcessId", $proc.Id, "-Title", "BMO", "Chat"))) {
        Fail "the tray's Chat row did not open Chat; see $out\open.txt"
    }
    if (-not (Wait-For 15 { Shows-Landing "landing" })) {
        Fail "Chat shows no Codex needs npx landing; see $out\landing.ax.txt"
    }
    Check-Landing (Join-Path $out "landing.ax.txt")
    $x = $script:Link[0] + [int]($script:Link[2] / 2)
    $y = $script:Link[1] + [int]($script:Link[3] / 2)
    if (-not (Invoke-Ax "click" @("click", "-ProcessId", $proc.Id, "-Title", "BMO", $x, $y))) {
        Fail "could not click the link; see $out\click.txt"
    }
    Start-Sleep -Seconds 2
    if (-not (Invoke-Ax "after.ax" @("dump", "-ProcessId", $proc.Id, "-Title", "BMO", "frames"))) {
        Fail "UI Automation dump failed after the click; see $out\after.ax.txt"
    }
    Check-After (Join-Path $out "after.ax.txt")
    Write-Output "PASS: evidence in $out"
} finally {
    if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
}
