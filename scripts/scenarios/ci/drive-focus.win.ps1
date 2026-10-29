#!/usr/bin/env pwsh
# Scenario: drive-focus (Windows), throwaway probe for #1920
# On screen: a plain 500x350 form near the top left, then Fidget as BMO with
#   the fixture Harness and FIDGET_PROBE_DRIVE_FOCUS set. Fidget and the form
#   quit when the scenario ends.
# Input: synthetic key presses and a held key, a click into the form, a click
#   on the sprite (keybd_event, mouse_event).
# Duration: about 5 s; every wait gives up 3 s after its trigger.
# Grants: a desktop session. bash on PATH for the fixture Harness wrapper.
# Asserts, each item on its own so one miss does not hide the rest: with no
#   pill open the overlay takes the keyboard; keydown, keyup and repeat reach
#   it; a click into the form passes through, focus leaves and typing lands in
#   the form; a sprite click takes the keyboard back.
#
# Usage: drive-focus.win.ps1 --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
$Go = if ($args.Count -gt 0) { $args[0] } else { "" }
$Bin = if ($args.Count -gt 1) { $args[1] } else { "" }
$TestBin = if ($args.Count -gt 2) { $args[2] } else { "" }
$ErrorActionPreference = "Stop"

if ($Go -ne "--go") {
    $header = @()
    foreach ($line in (Get-Content -LiteralPath $PSCommandPath | Select-Object -Skip 1)) {
        if ($line.StartsWith("#")) { $header += ($line -replace '^# ?', '') } elseif ($line.Trim()) { break }
    }
    $header -join "`n"
    exit 2
}

$root = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
. (Join-Path $PSScriptRoot "..\helpers\wait-helpers.win.ps1")
. (Join-Path $PSScriptRoot "..\helpers\bash-setup-win.ps1")
if (-not $bashPath) {
    [Console]::Error.WriteLine("SKIP: bash not found (tried Git Bash, then PATH)")
    exit 2
}

Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class DriveInput {
    [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] static extern void mouse_event(uint f, uint x, uint y, uint d, UIntPtr e);
    [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint f, UIntPtr e);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, System.Text.StringBuilder s, int n);
    public static void Move(int x, int y) { SetCursorPos(x, y); }
    public static void Click(int x, int y) {
        SetCursorPos(x, y);
        mouse_event(2, 0, 0, 0, UIntPtr.Zero);
        System.Threading.Thread.Sleep(30);
        mouse_event(4, 0, 0, 0, UIntPtr.Zero);
    }
    public static void Down(byte vk) { keybd_event(vk, 0, vk >= 0x25 && vk <= 0x28 ? 1u : 0u, UIntPtr.Zero); }
    public static void Up(byte vk) { keybd_event(vk, 0, (vk >= 0x25 && vk <= 0x28 ? 1u : 0u) | 2u, UIntPtr.Zero); }
    public static void Key(byte vk) { Down(vk); System.Threading.Thread.Sleep(30); Up(vk); }
    public static string Foreground() {
        var s = new System.Text.StringBuilder(256);
        GetWindowText(GetForegroundWindow(), s, 256);
        return s.ToString();
    }
}
"@

$TestBin = (Resolve-Path -LiteralPath $TestBin).Path
$out = Join-Path $env:TEMP "fidget-scenario-drive-focus-$(Get-Date -Format 'yyyyMMdd-HHmmss')"
$data = Initialize-ScenarioData $out @{ hide_in_fullscreen = $false }
$marks = Join-Path $out "harness.log"
$log = Join-Path $out "app.log"
$err = Join-Path $out "app.err"
$winlog = Join-Path $out "window.log"
$results = Join-Path $out "results.txt"
Set-Content -LiteralPath $marks -Value "" -Encoding ascii
Set-Content -LiteralPath $winlog -Value "" -Encoding ascii
$script:failed = $false
function Fail([string]$Message) {
    [Console]::Error.WriteLine("FAIL: $Message")
    [Console]::Error.WriteLine("evidence in $out")
    exit 1
}
function Proven([string]$Message) { Add-Content -LiteralPath $results -Value "proven: $Message"; Write-Output "proven: $Message" }
function Missed([string]$Message) {
    Add-Content -LiteralPath $results -Value "failed: $Message"
    [Console]::Error.WriteLine("failed: $Message")
    $script:failed = $true
}

$rootBash = $root -replace '\\', '/'
$testBinBash = $TestBin -replace '\\', '/'
$marksBash = $marks -replace '\\', '/'
$env:FIDGET_HARNESS = "$bashPath $rootBash/scripts/scenarios/helpers/fixture-harness.sh $testBinBash script=nop count=$marksBash"
$env:FIDGET_DIRECTOR_WAKE_SECS = "3600"
$env:FIDGET_DIRECTOR_API_KEY = "x"
$env:FIDGET_TRACE_FRAMES = "1"
$env:FIDGET_TRACE_BUBBLE = "1"
$env:FIDGET_PROBE_DRIVE_FOCUS = if ($env:FIDGET_PROBE_DRIVE_FOCUS) { $env:FIDGET_PROBE_DRIVE_FOCUS } else { "1" }
$env:FIDGET_CHARACTER = "bmo"
$env:FIDGET_CHARACTERS = Join-Path $root "characters"

$form = Start-Process powershell -ArgumentList @("-NoProfile", "-STA", "-ExecutionPolicy", "Bypass", "-File", (Join-Path $PSScriptRoot "..\helpers\drive-focus-window.win.ps1"), $winlog) -PassThru
$proc = $null
try {
    if (-not (Wait-ForLine 5 $winlog '^window: activated')) { Fail "the helper form never activated; see $winlog" }
    $proc = Start-FidgetPiped $Bin @() $log $err
    $script:proc = $proc
    if (-not (Wait-ForUp 5 $err)) { Fail "Fidget never wrote its overlay line; see $err" }
    $traceFile = Join-Path $data "process.log"
    if (-not (Wait-ForFile 3 $traceFile)) { Fail "process.log never appeared; see $err" }
    if (-not (Wait-ForTrace 3 $traceFile '^\S+ frame: .* sprite\(')) { Fail "Fidget traced no frame; see $traceFile" }
    $probe = '^\S+ drive-probe: overlay-0 '
    function Count([string]$File, [string]$Pattern) { @(Select-String -LiteralPath $File -Pattern $Pattern).Count }
    function Next([string]$File, [string]$Pattern, [scriptblock]$Trigger) {
        $before = Count $File $Pattern
        & $Trigger
        Wait-ForCount 3 $File $Pattern $before
    }

    # 1. Launch: the overlay takes the keyboard with no pill open.
    if (-not (Wait-ForTrace 3 $traceFile "${probe}request launch ok")) { Missed "take: no 'request launch ok'; see $traceFile" }
    $taken = (Wait-ForTrace 3 $traceFile "${probe}Focused\(true\)") -and (Wait-ForLine 3 $winlog '^window: deactivate')
    $front = [DriveInput]::Foreground()
    if ($taken) { Proven "take: Focused(true), the form deactivated, foreground is '$front'" }
    else { Missed "take: no Focused(true) or the form kept activation; foreground is '$front'" }

    # 2. Key edges reach the webview. 0x27 is Right Arrow.
    if ((Next $traceFile "${probe}keydown key=ArrowRight repeat=false" { [DriveInput]::Key(0x27) }) -and
        (Wait-ForTrace 3 $traceFile "${probe}keyup key=ArrowRight")) {
        Proven "keys: keydown and keyup for ArrowRight"
    } else { Missed "keys: no ArrowRight keydown/keyup; see $traceFile" }

    # 3. A held key: a second press with no release in between.
    if ((Next $traceFile "${probe}keydown key=ArrowRight" { [DriveInput]::Down(0x27) }) -and
        (Next $traceFile "${probe}keydown key=ArrowRight repeat=true" { [DriveInput]::Down(0x27) })) {
        Proven "repeat: a second keydown with no keyup arrives with repeat=true"
    } else { Missed "repeat: no keydown with repeat=true; see $traceFile" }
    [DriveInput]::Up(0x27)

    # 4. A click into the form, off the art, passes through and takes focus away.
    [DriveInput]::Click(350, 275)
    if (Wait-ForTrace 3 $traceFile "${probe}Focused\(false\)") { Proven "pause: Focused(false) after a click on the form" }
    else { Missed "pause: no Focused(false) after a click on the form; see $traceFile" }
    # 0x5A is Z.
    if ((Wait-ForLine 3 $winlog '^window: activated') -and
        (Next $winlog '^window: keydown Z' { [DriveInput]::Key(0x5A) }) -and
        -not (Test-FileMatches $traceFile "${probe}keydown key=z")) {
        Proven "pass-through: the form took the click and the typing; foreground is '$([DriveInput]::Foreground())'"
    } else { Missed "pass-through: the form missed the click or the typing; see $winlog" }

    # 5. A sprite click takes the keyboard back.
    $rest = $null
    if (-not (Wait-Until 3 { ($script:rest = Get-SpriteAtRest $traceFile) })) { Missed "resume: the sprite never came to rest" }
    $rest = $script:rest
    $size = Select-String -LiteralPath $traceFile -Pattern 'sprite (\d+)x(\d+);' | Select-Object -First 1
    if (-not $size -or -not $rest) { Fail "no sprite size or rest in $traceFile" }
    $w = [int]$size.Matches[0].Groups[1].Value
    $h = [int]$size.Matches[0].Groups[2].Value
    $before = Count $traceFile "${probe}Focused\(true\)"
    # The region takes the click one frame after the pointer arrives, as a
    # real hand gives it; then the pointer leaves so hover opens no pill.
    $frames = Count $traceFile '^\S+ frame: '
    [DriveInput]::Move($rest.X + [int]($w / 2), $rest.Y + [int]($h / 2))
    if (-not (Wait-ForCount 3 $traceFile '^\S+ frame: ' ($frames + 1))) { Missed "resume: no frame after the pointer reached the sprite" }
    [DriveInput]::Click($rest.X + [int]($w / 2), $rest.Y + [int]($h / 2))
    [DriveInput]::Move(900, 100)
    if (-not (Wait-ForTrace 3 $traceFile "${probe}request click ok")) { Missed "resume: no 'request click ok'; see $traceFile" }
    # 0x25 is Left Arrow.
    if ((Wait-ForCount 3 $traceFile "${probe}Focused\(true\)" $before) -and
        (Next $traceFile "${probe}keydown key=ArrowLeft" { [DriveInput]::Key(0x25) })) {
        Proven "resume: Focused(true) and ArrowLeft reached the overlay after a sprite click"
    } else { Missed "resume: no Focused(true) or no ArrowLeft after a sprite click; foreground is '$([DriveInput]::Foreground())'" }

    Select-String -LiteralPath $traceFile -Pattern $probe | ForEach-Object { $_.Line } | Set-Content -LiteralPath (Join-Path $out "probe-lines.txt")
    if ($script:failed) { Fail "see $results" }
    Write-Output "PASS: evidence in $out"
} finally {
    Stop-FidgetTree $proc "count=$marksBash"
    if ($form -and -not $form.HasExited) { Stop-Process -Id $form.Id -Force -ErrorAction SilentlyContinue }
}
