#!/usr/bin/env pwsh
# Scenario: thinking-row (Windows)
# On screen: launches Fidget as BMO with a fixture Harness. Chat opens by
#   itself and takes focus. One UI Automation dump of the Chat window while
#   the Harness thinks, and one after the reply. Fidget quits when the scenario ends.
# Input: none.
# Duration: a few seconds, 10 s at most.
# Grants: a desktop session. UI Automation for the terminal that runs it. bash
#   on PATH for the fixture Harness wrapper.
# Asserts: the Thinking row is open while the Harness thinks, and collapsed
#   once the reply lands.
#
# Usage: thinking-row.win.ps1 --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
# Set FIDGET_SCENARIO_AX_OPEN and FIDGET_SCENARIO_AX_DONE to assert those dumps
# and skip the GUI. That checks the assertion, not the live window.
# No param() block: Windows PowerShell binds `--go` as -Go and swallows the
# next token, so $Go never equals "--go" and the leaf always exits 2.
$Go = if ($args.Count -gt 0) { $args[0] } else { "" }
$Bin = if ($args.Count -gt 1) { $args[1] } else { "" }
$TestBin = if ($args.Count -gt 2) { $args[2] } else { "" }
$ErrorActionPreference = "Stop"

$GlyphOpen = [string][char]0x25BE
$GlyphDone = [string][char]0x25B8

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
    Write-Error "usage: thinking-row.win.ps1 --go <fidget binary> <fidget test binary>"
    exit 1
}

$root = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
. (Join-Path $PSScriptRoot "..\helpers\wait-helpers.win.ps1")
$script:Evidence = $null

function Fail([string]$Message) {
    [Console]::Error.WriteLine("FAIL: $Message")
    if ($script:Evidence) { [Console]::Error.WriteLine("evidence in $($script:Evidence)") }
    exit 1
}

function Thinking-Rows([string]$File) {
    @(Get-Content -LiteralPath $File -Encoding utf8 | Where-Object { $_ -match '(?i)^button\|.*thinking' })
}

# The $Nth Thinking row (1-based), or the newest when $Nth is 0.
function Get-Row([string]$File, [int]$Nth) {
    $rows = @(Thinking-Rows $File)
    if ($rows.Count -eq 0 -or $Nth -gt $rows.Count) { return $null }
    if ($Nth -gt 0) { $rows[$Nth - 1] } else { $rows[-1] }
}

function Test-Row([string]$File, [string]$Glyph, [int]$Nth) {
    $row = Get-Row $File $Nth
    return ($row -and $row.ToLower().StartsWith("button|$Glyph".ToLower()))
}

function Assert-Row([string]$File, [string]$Glyph, [string]$Label, [int]$Nth = 0) {
    $row = Get-Row $File $Nth
    if (-not $row) { Fail "$Label`: no Thinking row in $File" }
    if (Test-Row $File $Glyph $Nth) {
        Write-Output "ok: ${Label}: $row"
    } else {
        Fail "$Label`: Thinking row reads '$row', want it to start with $Glyph"
    }
}

$open = $env:FIDGET_SCENARIO_AX_OPEN
$done = $env:FIDGET_SCENARIO_AX_DONE
if ($open -or $done) {
    if (-not $open -or -not $done) { Fail "set both FIDGET_SCENARIO_AX_OPEN and FIDGET_SCENARIO_AX_DONE" }
    Assert-Row $open $GlyphOpen "open"
    # As live: the row that was open is the one that must collapse.
    Assert-Row $done $GlyphDone "done" @(Thinking-Rows $open).Count
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
$out = Join-Path $env:TEMP "fidget-scenario-thinking-row-$stamp"
$script:Evidence = $out
$data = Initialize-ScenarioData $out
$marks = Join-Path $out "harness.log"
$log = Join-Path $out "app.log"
Set-Content -LiteralPath $marks -Value "" -Encoding ascii


$harness = "$bashPath $root/scripts/scenarios/helpers/fixture-harness.sh $TestBin script=scenario-thinking count=$marks"
$paths = @($harness -split '\s+' | Select-Object -Skip 1)
if ($paths.Count -ne 4) { Fail "a path in the Harness line holds a space: $harness" }

$env:FIDGET_HARNESS = $harness
$env:FIDGET_DIRECTOR_WAKE_SECS = "1"
$env:FIDGET_DIRECTOR_API_KEY = "x"
$env:FIDGET_CAPTURABLE = "1"
$env:FIDGET_CHARACTER = "bmo"
$env:FIDGET_CHARACTERS = Join-Path $root "characters"

$startMs = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$script:proc = Start-FidgetPiped $Bin @() $log (Join-Path $out "app.err")
try {
    if (-not (Wait-ForUp 5 (Join-Path $out "app.err"))) { Fail "Fidget never wrote its overlay line; see $out\app.err" }
    if (-not (Wait-ForMarks 3 $marks '^asked$')) {
        Fail "no wake reached the Harness; see $log"
    }
    if (-not (Wait-ForMarks 3 $marks '^thought 2$')) {
        Fail "no thinking turn; see $log"
    }
    $utf8 = New-Object System.Text.UTF8Encoding $false
    [Console]::OutputEncoding = $utf8
    $OutputEncoding = $utf8
    $dumpPs1 = Join-Path $out "dump-ax.ps1"
    $ax = Join-Path $root "scripts\ax-window-win.ps1"
    $dumpLines = @(
        '[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false',
        '$OutputEncoding = [Console]::OutputEncoding',
        "& '$ax' @args"
    )
    [System.IO.File]::WriteAllLines($dumpPs1, $dumpLines, $utf8)
    # Each wake is a new turn with its own row, and the next turn starts as
    # this one replies. So the newest row is open mid-thought, and that same
    # row is the one that must collapse.
    function Capture([string]$Name, [string]$Glyph, [int]$Nth) {
        $dump = Join-Path $out "$Name.ax.txt"
        $null = Wait-ForUia 3 $script:proc.Id {
            & powershell -NoProfile -ExecutionPolicy Bypass -File $dumpPs1 dump -ProcessId $script:proc.Id -Title "BMO" |
                Out-File -FilePath $dump -Encoding utf8
            Test-Row $dump $Glyph $Nth
        }
        if (-not (Test-Path -LiteralPath $dump)) { Fail "${Name}: UI Automation dump failed; see $log" }
        Assert-Row $dump $Glyph $Name $Nth
    }
    Capture "mid-thought" $GlyphOpen 0
    $openRow = @(Thinking-Rows (Join-Path $out "mid-thought.ax.txt")).Count
    if (-not (Wait-ForMarks 3 $marks '^replied$')) {
        Fail "no reply; see $log"
    }
    Capture "after-reply" $GlyphDone $openRow
    $elapsedMs = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() - $startMs
    Write-Output "PASS: evidence in $out (${elapsedMs}ms)"
} finally {
    Stop-FidgetTree $script:proc "count=$marks"
}
