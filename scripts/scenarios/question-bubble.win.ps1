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

$root = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$script:Evidence = $null

function Fail([string]$Message) {
    [Console]::Error.WriteLine("FAIL: $Message")
    if ($script:Evidence) { [Console]::Error.WriteLine("evidence in $($script:Evidence)") }
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

if (-not (Get-Command bash -ErrorAction SilentlyContinue)) {
    [Console]::Error.WriteLine("SKIP: bash is not on PATH, so the fixture Harness wrapper cannot run.")
    exit 2
}

$TestBin = (Resolve-Path -LiteralPath $TestBin).Path
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$out = Join-Path $env:TEMP "fidget-scenario-question-bubble-$stamp"
$script:Evidence = $out
New-Item -ItemType Directory -Force -Path (Join-Path $out "home") | Out-Null
$marks = Join-Path $out "harness.log"
$log = Join-Path $out "app.log"
$err = Join-Path $out "app.err"
Set-Content -LiteralPath $marks -Value "" -Encoding ascii

# Convert Windows paths to forward slashes for bash.
$rootBash = $root -replace '\\', '/'
$testBinBash = $TestBin -replace '\\', '/'
$marksBash = $marks -replace '\\', '/'
$harness = "bash $rootBash/scripts/scenarios/fixture-harness.sh $testBinBash script=scenario-asking count=$marksBash"
$paths = @($harness -split '\s+' | Select-Object -Skip 1)
if ($paths.Count -ne 4) { Fail "a path in the Harness line holds a space: $harness" }

$env:HOME = Join-Path $out "home"
$env:USERPROFILE = $env:HOME
$env:FIDGET_HARNESS = $harness
$env:FIDGET_DIRECTOR_WAKE_SECS = "6"
$env:FIDGET_DIRECTOR_API_KEY = "x"
$env:FIDGET_CAPTURABLE = "1"
$env:FIDGET_TRACE_FRAMES = "1"
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
    function Traced([string]$Pattern) { [bool](Get-Content -LiteralPath $err | Where-Object { $_ -match $Pattern }) }

    if (-not (Wait-For 30 { Marked "asked" })) { Fail "no wake reached the Harness; see $err" }
    Start-Sleep -Seconds 1

    # The overlay spans the display, so its centre is not the sprite. The
    # newest frame trace line says where the sprite is drawn.
    $trace = Get-Content -LiteralPath $err
    $size = $trace | Select-String -Pattern 'sprite (\d+)x(\d+);' | Select-Object -First 1
    if (-not $size) { Fail "no sprite size in $err" }
    $at = $trace | Select-String -Pattern '^frame: .* sprite\((-?\d+),(-?\d+)\) ' | Select-Object -Last 1
    if (-not $at) { Fail "no frame trace in $err" }
    $w = [int]$size.Matches[0].Groups[1].Value
    $h = [int]$size.Matches[0].Groups[2].Value
    $x = [int]$at.Matches[0].Groups[1].Value + [int]($w / 2)
    $y = [int]$at.Matches[0].Groups[2].Value + [int]($h / 2)

    Check-Before (Invoke-Dump "before-poke")
    if (-not (Invoke-Ax "poke" @("click", "-X", $x, "-Y", $y))) { Fail "could not click the sprite; see $out\poke.txt" }
    if (-not (Wait-For 3 { Traced '^verbs: .*Poke' })) { Fail "the click did not land as a Poke; see $err" }
    Start-Sleep -Seconds 1
    Check-After (Invoke-Dump "after-poke")
    Write-Output "PASS: evidence in $out"
} finally {
    if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
    Get-CimInstance Win32_Process -Filter "Name = 'bash.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.CommandLine -like "*count=$marks*" } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}
