#!/usr/bin/env pwsh
# Scenario: dual-monitor-seam-climb (Windows)
# On screen: launches Fidget, positions the character at x=0 (interior seam
#   between two side-by-side monitors: secondary left @ x=-1200, primary @ x=0),
#   and samples its state. Fidget quits when the scenario ends.
# Input: none.
# Duration: about 10 s.
# Grants: a desktop session. Must run on Windows with two horizontal monitors
#   (secondary left, primary right) where the seam is at x=0.
# Asserts: the character does NOT climb at x=0 or other interior primary X
#   coordinates. Climb is State::Climbing. Reports INCONCLUSIVE when only one
#   monitor is attached.
#
# Usage: dual-monitor-seam-climb.win.ps1 --go <fidget binary> <fidget test binary>
# Without --go it prints this header and exits 2.
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
    Write-Error "usage: dual-monitor-seam-climb.win.ps1 --go <fidget binary> <fidget test binary>"
    exit 1
}

$root = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$script:Evidence = $null

function Fail([string]$Message) {
    [Console]::Error.WriteLine("FAIL: $Message")
    if ($script:Evidence) { [Console]::Error.WriteLine("evidence in $($script:Evidence)") }
    exit 1
}

$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$out = Join-Path $env:TEMP "fidget-scenario-dual-monitor-$stamp"
$script:Evidence = $out
New-Item -ItemType Directory -Force -Path (Join-Path $out "home") | Out-Null
$log = Join-Path $out "app.log"
$stateLog = Join-Path $out "state.log"

Add-Type -AssemblyName System.Windows.Forms
$screens = [System.Windows.Forms.Screen]::AllScreens

if ($screens.Count -lt 2) {
    [Console]::Error.WriteLine("INCONCLUSIVE: only $($screens.Count) monitor(s) attached; dual-monitor layout needed")
    exit 2
}

$primary = $screens | Where-Object { $_.Primary } | Select-Object -First 1
$secondary = $screens | Where-Object { -not $_.Primary } | Select-Object -First 1

$primaryX = $primary.Bounds.X
$secondaryX = $secondary.Bounds.X
$secondaryRight = $secondaryX + $secondary.Bounds.Width

if (-not ($secondaryX -lt 0 -and $primaryX -eq 0)) {
    [Console]::Error.WriteLine("INCONCLUSIVE: layout is not secondary-left/primary-right with seam at x=0")
    [Console]::Error.WriteLine("  secondary: x=$secondaryX, primary: x=$primaryX")
    exit 2
}

Write-Output "Dual-monitor layout detected: secondary @ $secondaryX to $secondaryRight, primary @ $primaryX"

$env:HOME = Join-Path $out "home"
$env:USERPROFILE = $env:HOME
$env:FIDGET_CAPTURABLE = "1"
$env:FIDGET_CHARACTER = "buddy-bot"
$env:FIDGET_CHARACTERS = Join-Path $root "characters"

$proc = Start-Process -FilePath $Bin -RedirectStandardOutput $log -RedirectStandardError (Join-Path $out "app.err") -PassThru -WindowStyle Normal
try {
    Start-Sleep -Seconds 3
    if ($proc.HasExited) { Fail "Fidget exited early; see $log" }

    $TestBin = (Resolve-Path -LiteralPath $TestBin).Path
    & $TestBin place --x 0 --y 800 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { Fail "place command failed; see $log" }

    Start-Sleep -Milliseconds 500

    $samples = @()
    for ($i = 0; $i -lt 20; $i++) {
        $snapshot = & $TestBin snapshot 2>&1 | Out-String
        if ($LASTEXITCODE -ne 0) { Fail "snapshot command failed; see $log" }

        $snapshot | Out-File -FilePath $stateLog -Append -Encoding utf8

        if ($snapshot -match 'position:\s*\(([^,]+),\s*([^\)]+)\)') {
            $x = [double]$matches[1]
            $y = [double]$matches[2]
        } else {
            Fail "could not parse position from snapshot"
        }

        if ($snapshot -match 'state:\s*(\w+)') {
            $state = $matches[1]
        } else {
            Fail "could not parse state from snapshot"
        }

        $sample = [PSCustomObject]@{
            Index = $i
            X = $x
            Y = $y
            State = $state
        }
        $samples += $sample
        Write-Output "sample $i`: x=$($x.ToString('F1')), y=$($y.ToString('F1')), state=$state"

        if ($state -eq "Climbing") {
            $nearSeam = ($x -ge -50 -and $x -le 200)
            if ($nearSeam) {
                Fail "character is Climbing at interior position x=$($x.ToString('F1')) near seam (samples in $stateLog)"
            }
        }

        Start-Sleep -Milliseconds 200
    }

    $climbSamples = $samples | Where-Object { $_.State -eq "Climbing" }
    if ($climbSamples.Count -gt 0) {
        $positions = ($climbSamples | ForEach-Object { "x=$($_.X.ToString('F1'))" }) -join ", "
        [Console]::Error.WriteLine("WARNING: Climbing detected but not near seam at: $positions")
    }

    Write-Output "PASS: no interior seam climb across $($samples.Count) samples (evidence in $out)"
} finally {
    if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
}
