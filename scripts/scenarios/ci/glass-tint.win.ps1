#!/usr/bin/env pwsh
# Scenario: glass-tint (Windows)
# On screen: launches Fidget as BMO with a fixture Harness and the Glass Chat
#   design, once per palette. Chat opens by itself and takes focus; one desktop
#   capture cropped to the Chat window per palette. Fidget quits between runs.
# Input: none.
# Duration: a few seconds, 40 s at most.
# Grants: a desktop session. UI Automation for the terminal that runs it. bash
#   on PATH for the fixture Harness wrapper.
# Asserts: Chat opens under Glass in dark and light, and each capture covers
#   the Chat window's own bounds over the runner's desktop. What the tint does
#   to text is for the eye: the capture is the evidence, not a pixel rule.
#
# Usage: glass-tint.win.ps1 --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
# No param() block: Windows PowerShell binds `--go` as -Go and swallows the
# next token, so $Go never equals "--go" and the leaf always exits 2.
$Go = if ($args.Count -gt 0) { $args[0] } else { "" }
$Bin = if ($args.Count -gt 1) { $args[1] } else { "" }
$TestBin = if ($args.Count -gt 2) { $args[2] } else { "" }
$ErrorActionPreference = "Stop"

$Mind = '^label\|[^|]* session [^|]+\|'

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
    Write-Error "usage: glass-tint.win.ps1 --go <fidget binary> <fidget test binary>"
    exit 1
}

$root = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
. (Join-Path $PSScriptRoot "..\helpers\wait-helpers.win.ps1")
. (Join-Path $PSScriptRoot "..\helpers\bash-setup-win.ps1")
Add-Type -AssemblyName System.Drawing -ErrorAction SilentlyContinue
$script:Evidence = $null
$script:Run = $null
$script:ChatPid = $null
$script:Marks = $null
$script:proc = $null

function Fail([string]$Message) {
    [Console]::Error.WriteLine("FAIL: $Message")
    if ($script:Evidence) { [Console]::Error.WriteLine("evidence in $($script:Evidence)") }
    exit 1
}

# The Chat window frame is the dump's frame line; CopyFromScreen grabs that
# desktop region, so the capture holds what a user sees of Chat and its tint.
function Save-ChatShot([string]$Path) {
    $dump = Join-Path $script:Run "session.ax.txt"
    $frameLine = Get-Content -LiteralPath $dump -Encoding utf8 |
        Where-Object { $_ -match '^frame\|' } | Select-Object -First 1
    if (-not $frameLine) { Fail "no frame line in $dump" }
    $r = @(((($frameLine -split '\|')[-1]) -split ',') | ForEach-Object { [int]$_ })
    $bitmap = New-Object System.Drawing.Bitmap $r[2], $r[3]
    $g = [System.Drawing.Graphics]::FromImage($bitmap)
    $g.CopyFromScreen($r[0], $r[1], 0, 0, (New-Object System.Drawing.Size $r[2], $r[3]))
    $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bitmap.Dispose()
    Write-Output ("ok: $(Split-Path -Leaf $Path) covers $($r[0]),$($r[1]) $($r[2])x$($r[3])")
}

# Chat may still be painting; the session's own row says the webview is there.
function Rendered-Chat {
    $dump = Join-Path $script:Run "session.ax.txt"
    if (-not (Invoke-AxWin $dump @{ Command = "dump"; ProcessId = $script:ChatPid; Title = "BMO"; Arg3 = "frames" })) {
        return $false
    }
    if (-not (Test-Path -LiteralPath $dump)) { return $false }
    $lines = Get-Content -LiteralPath $dump -Encoding utf8
    if (-not ($lines | Where-Object { $_ -match '^frame\|' })) { return $false }
    return [bool]($lines | Where-Object { $_ -match $Mind })
}

$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$out = Join-Path $env:TEMP "fidget-scenario-glass-tint-$stamp"
$script:Evidence = $out

foreach ($palette in @("dark", "light")) {
    $script:Run = Join-Path $out $palette
    $script:Marks = Join-Path $script:Run "harness.log"
    $log = Join-Path $script:Run "app.log"
    try {
        Set-Content -LiteralPath $script:Marks -Value "" -Encoding ascii
        # The seed holds the design and palette; Glass on Windows takes the
        # heavier tint (the CSS rules are platform-scoped, no blur switcher).
        Initialize-ScenarioData $script:Run -Settings @{ chat_ui = "glass"; chat_appearance = $palette } | Out-Null

        $harness = "$bashPath $root/scripts/scenarios/helpers/fixture-harness.sh $TestBin script=scenario-thinking count=$script:Marks"
        $paths = @($harness -split '\s+' | Select-Object -Skip 1)
        if ($paths.Count -ne 4) { Fail "a path in the Harness line holds a space: $harness" }

        $env:FIDGET_HARNESS = $harness
        $env:FIDGET_DIRECTOR_WAKE_SECS = "1"
        $env:FIDGET_DIRECTOR_API_KEY = "x"
        $env:FIDGET_CAPTURABLE = "1"
        $env:FIDGET_CHARACTER = "bmo"
        $env:FIDGET_CHARACTERS = Join-Path $root "characters"

        $proc = Start-FidgetPiped $Bin @() $log (Join-Path $script:Run "app.err")
        $script:proc = $proc
        $script:ChatPid = $proc.Id

        if (-not (Wait-ForUp (Join-Path $script:Run "app.err"))) {
            Fail "${palette}: Fidget never wrote its overlay line; see $log"
        }
        if (-not (Wait-ForMarks 3 $script:Marks '^asked$')) {
            Fail "${palette}: no wake reached the Harness; see $log"
        }
        if (-not (Wait-ForUia 5 $script:ChatPid { Rendered-Chat })) {
            Fail "${palette}: Chat never drew a session; see $script:Run\session.ax.txt"
        }
        # A reply on screen keeps the Thinking row out of the capture.
        if (-not (Wait-ForMarks 3 $script:Marks '^replied$')) {
            Fail "${palette}: the Harness never replied; see $log"
        }
        Save-ChatShot (Join-Path $out "glass-$palette.png")
    } finally {
        if ($script:proc) { Stop-FidgetTree $script:proc "count=$script:Marks" }
    }
}

Write-Output "PASS: evidence in $out"
exit 0
