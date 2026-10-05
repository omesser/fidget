#!/usr/bin/env pwsh
# Scenario: control-click-menu (Windows)
# On screen: launches Fidget as BMO with a fixture Harness. The scenario finds
#   the sprite, right-clicks its centre, waits for the menu to appear, and
#   reads its items. Escape closes the menu. Fidget quits when the scenario
#   ends.
# Input: one real right-click on the sprite and one Escape. Windows has no
#   Control-click; the right button is the menu gesture there.
# Duration: about 20 s, 1 min at most.
# Grants: a desktop session. UI Automation for the terminal that runs it. bash
#   on PATH for the fixture Harness wrapper.
# Asserts: the right-click lands as Menu, and the open menu's UI Automation
#   items include Chat..., Settings... and Quit.
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

$root = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$script:Evidence = $null

function Fail([string]$Message) {
    [Console]::Error.WriteLine("FAIL: $Message")
    if ($script:Evidence) { [Console]::Error.WriteLine("evidence in $($script:Evidence)") }
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

if (-not (Get-Command bash -ErrorAction SilentlyContinue)) {
    [Console]::Error.WriteLine("SKIP: bash is not on PATH, so the fixture Harness wrapper cannot run.")
    exit 2
}

$TestBin = (Resolve-Path -LiteralPath $TestBin).Path
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$out = Join-Path $env:TEMP "fidget-scenario-control-click-menu-$stamp"
$script:Evidence = $out
New-Item -ItemType Directory -Force -Path (Join-Path $out "home") | Out-Null
$log = Join-Path $out "app.log"
$err = Join-Path $out "app.err"

$harness = "bash $root/scripts/scenarios/fixture-harness.sh $TestBin script=nop"
$paths = @($harness -split '\s+' | Select-Object -Skip 1)
if ($paths.Count -ne 3) { Fail "a path in the Harness line holds a space: $harness" }

$env:HOME = Join-Path $out "home"
$env:USERPROFILE = $env:HOME
$env:FIDGET_HARNESS = $harness
$env:FIDGET_DIRECTOR_WAKE_SECS = "3600"
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
    function Traced([string]$Pattern) { [bool](Get-Content -LiteralPath $err | Where-Object { $_ -match $Pattern }) }

    # The overlay spans the display, so its centre is not the sprite. The
    # newest frame trace line says where the sprite is drawn.
    if (-not (Wait-For 15 { Traced '^frame: .* sprite\(' })) { Fail "Fidget traced no frame; see $err" }
    Start-Sleep -Seconds 2
    $trace = Get-Content -LiteralPath $err
    $size = $trace | Select-String -Pattern 'sprite (\d+)x(\d+);' | Select-Object -First 1
    if (-not $size) { Fail "no sprite size in $err" }
    $at = $trace | Select-String -Pattern '^frame: .* sprite\((-?\d+),(-?\d+)\) ' | Select-Object -Last 1
    $w = [int]$size.Matches[0].Groups[1].Value
    $h = [int]$size.Matches[0].Groups[2].Value
    $x = [int]$at.Matches[0].Groups[1].Value + [int]($w / 2)
    $y = [int]$at.Matches[0].Groups[2].Value + [int]($h / 2)
    Write-Output "ok: sprite centre ($x,$y)"

    if (-not (Invoke-Ax "menu" @("menu", "-X", $x, "-Y", $y))) { Fail "no open menu in the UI Automation tree; see $out\menu.txt" }
    if (-not (Wait-For 3 { Traced '^verbs: .*\[Menu\]' })) { Fail "the right-click did not land as Menu; see $err" }
    Write-Output "ok: the right-click landed as Menu"
    Check-Items (Join-Path $out "menu.txt")
    Write-Output "PASS: evidence in $out"
} finally {
    if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
    Get-CimInstance Win32_Process -Filter "Name = 'bash.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.CommandLine -like "*script=nop*" } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}
