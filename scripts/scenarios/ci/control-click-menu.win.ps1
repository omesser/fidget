#!/usr/bin/env pwsh
# Scenario: control-click-menu (Windows)
# On screen: launches Fidget as BMO with a fixture Harness. The scenario finds
#   the sprite, right-clicks its centre, waits for the menu to appear, and
#   reads its items. Escape closes the menu. Fidget quits when the scenario
#   ends.
# Input: one real right-click on the sprite and one Escape. Fidget treats
#   Control-click as the menu gesture only on macOS; here the right button is the menu gesture.
# Duration: a few seconds, 10 s at most.
# Grants: a desktop session. UI Automation for the terminal that runs it. bash
#   on PATH for the fixture Harness wrapper.
# Asserts: the right-click lands as Menu, and the open menu's UI Automation
#   items include Chat..., Settings... and Quit.
# Skips (exit 2, preflight.txt) only when a fullscreen window is stacked over
#   the sprite and would take the right-click. Any other fullscreen app is left
#   alone; hide-in-fullscreen is off, so Fidget neither fades nor moves.
#
# Usage: control-click-menu.win.ps1 --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
# Set FIDGET_SCENARIO_MENU to assert that item list, one name per line, and
# skip the GUI. That checks the assertion, not the live menu.
# No param() block: Windows PowerShell binds `--go` as -Go and swallows the
# next token, so $Go never equals "--go" and the leaf always exits 2.
$Go = if ($args.Count -gt 0) { $args[0] } else { "" }
$Bin = if ($args.Count -gt 1) { $args[1] } else { "" }
$TestBin = if ($args.Count -gt 2) { $args[2] } else { "" }
$ErrorActionPreference = "Stop"
$Ellipsis = [string][char]0x2026
# Before the self-check, whose FAIL line carries the ellipsis: a desktop console
# on an OEM codepage writes it as '.', and only CI's runner switches to 65001.
$utf8 = New-Object System.Text.UTF8Encoding $false
[Console]::OutputEncoding = $utf8
$OutputEncoding = $utf8

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
    Write-Error "usage: control-click-menu.win.ps1 --go <fidget binary> <fidget test binary>"
    exit 1
}

$selfTestFrameLine = "1791259416 frame: 1791259416038 Grounded pos(1720,1392) sprite(1657,1264) idle#0 <id>"
$selfTestVerbsLine = "1791259408 overlay: 2 display(s); sprite 126x128; BMO as BMO"
$selfTestMenuLine = "1791259425 verbs: [Menu]"
$selfTestPresenceHidden = "1791260872 presence: hidden over 500ms"
$selfTestClickHitTrue = "1791260862 click: down hits=[true] target=Some(0) cursor=(2554,1328) visible=true sprite=untargeted(2563,1264)"
$selfTestClickHitFalse = "1791260862 click: down hits=[false] target=None cursor=(2554,1328) visible=true sprite=untargeted(2563,1264)"
$selfTestSettledFrame = "1791260880 frame: 1791260880723 Grounded pos(1720,1328) sprite(1657,1264) land#1 <id>"
if (-not ($selfTestFrameLine -match '^\d+ frame: .* sprite\(')) { Write-Error "self-test: frame pattern failed"; exit 1 }
if (-not ($selfTestFrameLine -match '^\d+ frame: .* sprite\((-?\d+),(-?\d+)\) ')) { Write-Error "self-test: frame coordinate pattern failed"; exit 1 }
if (-not ($selfTestMenuLine -match '^\d+ verbs: .*\[Menu\]')) { Write-Error "self-test: Menu pattern failed"; exit 1 }
if ($selfTestVerbsLine -match '^\d+ verbs: ') { Write-Error "self-test: should not match non-verbs line"; exit 1 }
if (-not ($selfTestPresenceHidden -match '^\d+ presence: hidden')) { Write-Error "self-test: presence:hidden pattern failed"; exit 1 }
if (-not ($selfTestClickHitTrue -match '^\d+ click: down hits=\[true\]')) { Write-Error "self-test: click hits=[true] pattern failed"; exit 1 }
if ($selfTestClickHitFalse -match '^\d+ click: down hits=\[true\]') { Write-Error "self-test: should not match hits=[false]"; exit 1 }
if (-not ($selfTestSettledFrame -match '^\d+\.?\d* frame: .* Grounded .* sprite\((-?\d+),(-?\d+)\) ')) { Write-Error "self-test: settled frame Grounded pattern failed"; exit 1 }
if ($selfTestSettledFrame -match 'walk#|Falling') { Write-Error "self-test: settled frame should not match walk# or Falling"; exit 1 }

$root = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
. (Join-Path $PSScriptRoot "..\helpers\wait-helpers.win.ps1")
$script:Evidence = $null

function Fail([string]$Message) {
    [Console]::Error.WriteLine("FAIL: $Message")
    if ($script:Evidence) { [Console]::Error.WriteLine("evidence in $($script:Evidence)") }
    # A self-check fails before the leaf seeds a data dir, so there is none to check.
    if ($data -and -not (Test-Path -LiteralPath (Join-Path $data "process.log") -ErrorAction SilentlyContinue)) {
        [Console]::Error.WriteLine("data_dir did not resolve to $data")
    }
    exit 1
}

function Check-Items([string]$File) {
    $lines = @(Get-Content -LiteralPath $File -Encoding utf8)
    foreach ($item in @("Chat$Ellipsis", "Settings$Ellipsis", "Quit")) {
        if (-not ($lines | Where-Object { $_ -ceq $item })) { Fail "no '$item' in the menu; see $File" }
        Write-Output "ok: the menu holds $item"
    }
}

if ($env:FIDGET_SCENARIO_MENU) {
    Check-Items $env:FIDGET_SCENARIO_MENU
    Write-Output "PASS: fixture dumps"
    exit 0
}

. (Join-Path $PSScriptRoot "..\helpers\bash-setup-win.ps1")
if (-not $bashPath) {
    [Console]::Error.WriteLine("SKIP: bash not found (tried Git Bash, then PATH)")
    exit 2
}

$TestBin = (Resolve-Path -LiteralPath $TestBin).Path
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$out = Join-Path $env:TEMP "fidget-scenario-control-click-menu-$stamp"
$script:Evidence = $out
$data = Initialize-ScenarioData $out @{ hide_in_fullscreen = $false }

$log = Join-Path $out "app.log"
$err = Join-Path $out "app.err"
$marks = Join-Path $out "harness.log"
Set-Content -LiteralPath $marks -Value "" -Encoding ascii

$rootBash = $root -replace '\\', '/'
$testBinBash = $TestBin -replace '\\', '/'
$marksBash = $marks -replace '\\', '/'
$harness = "$bashPath $rootBash/scripts/scenarios/helpers/fixture-harness.sh $testBinBash script=nop count=$marksBash"
$paths = @($harness -split '\s+' | Select-Object -Skip 1)
if ($paths.Count -ne 4) { Fail "a path in the Harness line holds a space: $harness" }

$env:FIDGET_HARNESS = $harness
$env:FIDGET_DIRECTOR_WAKE_SECS = "3600"
$env:FIDGET_DIRECTOR_API_KEY = "x"
$env:FIDGET_CAPTURABLE = "1"
$env:FIDGET_TRACE_FRAMES = "1"
# ax-window-win.ps1 presses once a hit-test line shows the click area followed the cursor.
$env:FIDGET_TRACE_HITTEST = "1"
$env:FIDGET_TRACE_WINDOWS = "1"
$env:FIDGET_CHARACTER = "bmo"
$env:FIDGET_CHARACTERS = Join-Path $root "characters"

. (Join-Path $PSScriptRoot "..\helpers\fullscreen-over-click.win.ps1")

$axPs1 = Join-Path $out "ax.ps1"
$ax = Join-Path $root "scripts\ax-window-win.ps1"
[System.IO.File]::WriteAllLines($axPs1, @(
        '[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false',
        '$OutputEncoding = [Console]::OutputEncoding',
        "& '$ax' @args"
    ), $utf8)

$startMs = Get-NowMs
$proc = Start-FidgetPiped $Bin @() $log $err
try {
    function Invoke-Ax([string]$Label, [hashtable]$AxArgs) {
        return (Invoke-AxWin (Join-Path $out "$Label.txt") $AxArgs)
    }
    function Traced([string]$Pattern) {
        $traceFile = Join-Path $data "process.log"
        if (Test-Path -LiteralPath $traceFile) {
            [bool](Get-Content -LiteralPath $traceFile | Where-Object { $_ -match $Pattern })
        } else {
            return $false
        }
    }
    function Wait-Settled {
        $script:rest = Wait-ForRest $traceFile
        if (-not $script:rest) {
            Fail "the sprite never came to rest within 10s; see $traceFile"
        }
        return $script:rest
    }

    $traceFile = Join-Path $data "process.log"
    if (-not (Wait-ForFile 3 $traceFile)) { Fail "process.log never appeared; see $err" }

    $settingsFile = Join-Path $data "settings.json"
    if (Test-Path -LiteralPath $settingsFile) {
        $settingsContent = Get-Content -LiteralPath $settingsFile -Raw | ConvertFrom-Json
        if ($settingsContent.hide_in_fullscreen -ne $false) {
            Fail "settings.json does not have hide_in_fullscreen: false as seeded"
        }
    }

    if (-not (Wait-ForTrace 3 $traceFile '^\d+ frame: .* sprite\(')) { Fail "Fidget traced no frame; see $traceFile" }

    if (Traced '^\d+ presence: hidden') {
        Fail "overlay hidden (presence: hidden) - see process.log window_source lines"
    }

    $settled = Wait-Settled
    $trace = Get-Content -LiteralPath $traceFile
    $size = $trace | Select-String -Pattern 'sprite (\d+)x(\d+);' | Select-Object -First 1
    if (-not $size) { Fail "no sprite size in $traceFile" }
    $w = [int]$size.Matches[0].Groups[1].Value
    $h = [int]$size.Matches[0].Groups[2].Value

    $attemptsFile = Join-Path $out "menu-attempts.txt"
    $maxAttempts = 3
    $success = $false
    for ($attempt = 1; $attempt -le $maxAttempts; $attempt++) {
        $attemptLog = "Attempt $attempt of $maxAttempts"
        Add-Content -LiteralPath $attemptsFile -Value $attemptLog
        $trace = Get-Content -LiteralPath $traceFile
        $menuCountBefore = @($trace | Select-String -Pattern '^\d+ verbs: .*\[Menu\]').Count
        Skip-IfFullscreenOverClick ($settled.X + [int]($w / 2)) ($settled.Y + [int]($h / 2)) $proc.Id $out "right-click"
        if (-not (Invoke-Ax "menu-$attempt" @{ Command = "menu"; ProcessId = $proc.Id; TraceFile = $traceFile; SpriteW = $w; SpriteH = $h })) {
            $msg = "no open menu in the UI Automation tree on attempt $attempt; see $out\menu-$attempt.txt"
            Add-Content -LiteralPath $attemptsFile -Value $msg
            if ($attempt -eq $maxAttempts) { Fail $msg }
            $settled = Wait-Settled
            continue
        }
        if (Wait-ForCount 2 $traceFile '^\d+ verbs: .*\[Menu\]' $menuCountBefore) {
            $menuFile = Join-Path $out "menu-$attempt.txt"
            $destFile = Join-Path $out "menu.txt"
            Copy-Item -LiteralPath $menuFile -Destination $destFile -Force
            $success = $true
            break
        }
        if ($attempt -lt $maxAttempts) {
            $settled = Wait-Settled
        }
    }
    if (-not $success) {
        Fail "right-click did not produce [Menu] verb after $maxAttempts attempts"
    }
    Write-Output "ok: the right-click landed as Menu"
    Check-Items (Join-Path $out "menu.txt")
    Write-Output "PASS: evidence in $out ($((Get-NowMs) - $startMs)ms)"
} finally {
    Stop-FidgetTree $proc "count=$marksBash"
}
