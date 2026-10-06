#!/usr/bin/env pwsh
# Scenario: thinking-row (Windows)
# On screen: launches Fidget as BMO with a fixture Harness. Chat opens by
#   itself and takes focus. One UI Automation dump of the Chat window while
#   the Harness thinks, and one after the reply. Fidget quits when the scenario ends.
# Input: none.
# Duration: about 30 s, 2 min at most.
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

$root = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$script:Evidence = $null

function Fail([string]$Message) {
    [Console]::Error.WriteLine("FAIL: $Message")
    if ($script:Evidence) { [Console]::Error.WriteLine("evidence in $($script:Evidence)") }
    exit 1
}

function Assert-Row([string]$File, [string]$Glyph, [string]$Label) {
    $row = Get-Content -LiteralPath $File -Encoding utf8 | Where-Object { $_ -match '(?i)^button\|.*thinking' } | Select-Object -First 1
    if (-not $row) { Fail "$Label`: no Thinking row in $File" }
    $prefix = "button|$Glyph".ToLower()
    if ($row.ToLower().StartsWith($prefix)) {
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
    Assert-Row $done $GlyphDone "done"
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
$out = Join-Path $env:TEMP "fidget-scenario-thinking-row-$stamp"
$script:Evidence = $out
New-Item -ItemType Directory -Force -Path (Join-Path $out "home") | Out-Null
$marks = Join-Path $out "harness.log"
$log = Join-Path $out "app.log"
Set-Content -LiteralPath $marks -Value "" -Encoding ascii

$harness = "$bashPath $root/scripts/scenarios/fixture-harness.sh $TestBin script=scenario-thinking count=$marks"
$paths = @($harness -split '\s+' | Select-Object -Skip 1)
if ($paths.Count -ne 4) { Fail "a path in the Harness line holds a space: $harness" }

$env:HOME = Join-Path $out "home"
$env:USERPROFILE = $env:HOME
$env:FIDGET_HARNESS = $harness
$env:FIDGET_DIRECTOR_WAKE_SECS = "6"
$env:FIDGET_DIRECTOR_API_KEY = "x"
$env:FIDGET_CAPTURABLE = "1"
$env:FIDGET_CHARACTER = "bmo"
$env:FIDGET_CHARACTERS = Join-Path $root "characters"

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
    if (-not (Wait-For 30 { Select-String -LiteralPath $marks -Pattern '^asked$' -Quiet })) {
        Fail "no wake reached the Harness; see $log"
    }
    if (-not (Wait-For 40 { Select-String -LiteralPath $marks -Pattern '^thought 2$' -Quiet })) {
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
    function Capture([string]$Name, [string]$Glyph) {
        $dump = Join-Path $out "$Name.ax.txt"
        $prefix = ("button|$Glyph").ToLower()
        $deadline = (Get-Date).AddSeconds(2)
        $code = 1
        do {
            & powershell -NoProfile -ExecutionPolicy Bypass -File $dumpPs1 dump -ProcessId $proc.Id -Title "BMO" |
                Out-File -FilePath $dump -Encoding utf8
            $code = $LASTEXITCODE
            if ($code -eq 0) {
                $row = Get-Content -LiteralPath $dump -Encoding utf8 |
                    Where-Object { $_ -match '(?i)^button\|.*thinking' } |
                    Select-Object -First 1
                if ($row -and $row.ToLower().StartsWith($prefix)) {
                    Write-Output "ok: ${Name}: $row"
                    return
                }
            }
            if ($proc.HasExited) { Fail "Fidget exited; see $log" }
            Start-Sleep -Milliseconds 200
        } while ((Get-Date) -lt $deadline)
        if ($code -ne 0) { Fail "${Name}: UI Automation dump failed; see $dump" }
        Assert-Row $dump $Glyph $Name
    }
    Capture "mid-thought" $GlyphOpen
    if (-not (Wait-For 15 { Select-String -LiteralPath $marks -Pattern '^replied$' -Quiet })) {
        Fail "no reply; see $log"
    }
    Start-Sleep -Milliseconds 1500
    Capture "after-reply" $GlyphDone
    Write-Output "PASS: evidence in $out"
} finally {
    if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
    Get-CimInstance Win32_Process -Filter "Name = 'bash.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.CommandLine -like "*count=$marks*" } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}
