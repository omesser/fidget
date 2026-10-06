#!/usr/bin/env pwsh
# Scenario: chat-header-narrow (Windows)
# On screen: launches Fidget as BMO with a fixture Harness. Chat opens by
#   itself and takes focus, then is resized to 420, 360 and 320 wide. One
#   UI Automation dump of the Chat window per width. Fidget quits when it ends.
# Input: none. The resizes go through Win32 SetWindowPos, not the mouse.
# Duration: about 30 s, 2 min at most.
# Grants: a desktop session. UI Automation for the terminal that runs it. bash
#   on PATH for the fixture Harness wrapper.
# Asserts: at each width the Instance name, the Character chip and the mind
#   line share one row, all three end inside the window, and the page is no
#   wider than its scroll area, so Chat never scrolls sideways. The mind line
#   names the fixture's long unbroken path, the shape #1090 reports.
#
# Usage: chat-header-narrow.win.ps1 --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
# Set FIDGET_SCENARIO_AX_420, FIDGET_SCENARIO_AX_360 and FIDGET_SCENARIO_AX_320
# to assert those dumps and skip the GUI. That checks the assertion, not the live window.
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
    Write-Error "usage: chat-header-narrow.win.ps1 --go <fidget binary> <fidget test binary>"
    exit 1
}

$root = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$script:Evidence = $null
$Mind = '^label\|[^|]* session [^|]+\|'

function Fail([string]$Message) {
    [Console]::Error.WriteLine("FAIL: $Message")
    if ($script:Evidence) { [Console]::Error.WriteLine("evidence in $($script:Evidence)") }
    exit 1
}

function Get-Rect([string]$Line) {
    $frame = ($Line -split '\|')[-1]
    return @($frame -split ',' | ForEach-Object { [int]$_ })
}

function Check-Dump([int]$Width, [string]$File) {
    if (-not (Test-Path -LiteralPath $File)) { Fail "${Width}: missing dump $File" }
    $labels = Get-Content -LiteralPath $File -Encoding utf8 | Where-Object { $_ -match '^label\|' }
    $mindIndex = -1
    for ($i = 0; $i -lt $labels.Count; $i++) {
        if ($labels[$i] -match $Mind) { $mindIndex = $i; break }
    }
    if ($mindIndex -lt 0) { Fail "${Width}: no mind line naming a session in $File" }
    if ($mindIndex -lt 2) { Fail "${Width}: no name and chip before the mind line in $File" }
    $rows = @($labels[$mindIndex - 2], $labels[$mindIndex - 1], $labels[$mindIndex])

    $frameLine = Get-Content -LiteralPath $File -Encoding utf8 | Where-Object { $_ -match '^frame\|' } | Select-Object -First 1
    if (-not $frameLine) { Fail "${Width}: no frame line in $File" }
    $frame = Get-Rect $frameLine
    $wx = $frame[0]; $ww = $frame[2]
    if ($ww -ne $Width) { Fail "${Width}: frame is $ww wide in $File" }
    $wr = $wx + $ww

    $top = -1; $bottom = -1
    foreach ($row in $rows) {
        $r = Get-Rect $row
        $x = $r[0]; $y = $r[1]; $ew = $r[2]; $eh = $r[3]
        $label = ($row -split '\|')[1]
        if (($x + $ew) -gt ($wr + 1)) {
            Fail "${Width}: '$label' ends at $($x + $ew), past the window edge at $wr"
        }
        if ($top -lt 0) {
            $top = $y; $bottom = $y + $eh
        } elseif ($y -ge $bottom -or ($y + $eh) -le $top) {
            Fail "${Width}: '$label' at y $y..$($y + $eh) left the name's row $top..$bottom"
        }
    }

    $scrollLine = Get-Content -LiteralPath $File -Encoding utf8 | Where-Object { $_ -match '^scroll-area\|' } | Select-Object -First 1
    $webLine = Get-Content -LiteralPath $File -Encoding utf8 | Where-Object { $_ -match '^web-area\|' } | Select-Object -First 1
    if (-not $scrollLine -and -not $webLine) { Fail "${Width}: missing scroll-area or web-area in $File" }
    if ($scrollLine) {
        $sw = (Get-Rect $scrollLine)[2]
        $aw = (Get-Rect $webLine)[2]
        if ($aw -gt ($sw + 1)) {
            Fail "${Width}: the page is $aw wide in a $sw scroll area, so Chat scrolls sideways"
        }
        Write-Output "ok: ${Width}: one row inside the window, page $aw of $sw"
    } else {
        $aw = (Get-Rect $webLine)[2]
        Write-Output "ok: ${Width}: one row inside the window, page $aw"
    }
}

$ax420 = $env:FIDGET_SCENARIO_AX_420
$ax360 = $env:FIDGET_SCENARIO_AX_360
$ax320 = $env:FIDGET_SCENARIO_AX_320
if ($ax420 -or $ax360 -or $ax320) {
    if (-not $ax420 -or -not $ax360 -or -not $ax320) {
        Fail "set FIDGET_SCENARIO_AX_420, FIDGET_SCENARIO_AX_360 and FIDGET_SCENARIO_AX_320"
    }
    Check-Dump 420 $ax420
    Check-Dump 360 $ax360
    Check-Dump 320 $ax320
    Write-Output "PASS: fixture dumps"
    exit 0
}

function Get-ShortPath([string]$Path) {
    $code = @"
    [DllImport("kernel32.dll", CharSet = CharSet.Auto, SetLastError = true)]
    public static extern uint GetShortPathName(string lpszLongPath, System.Text.StringBuilder lpszShortPath, uint cchBuffer);
"@
    $kernel32 = Add-Type -MemberDefinition $code -Name "Kernel32" -Namespace "Win32" -PassThru -ErrorAction SilentlyContinue
    $buffer = New-Object System.Text.StringBuilder 260
    $result = $kernel32::GetShortPathName($Path, $buffer, $buffer.Capacity)
    if ($result -gt 0) { return $buffer.ToString() }
    return $Path
}

$bashPath = $null
$gitBash = "C:\Program Files\Git\bin\bash.exe"
if (Test-Path -LiteralPath $gitBash -PathType Leaf) {
    $bashPath = Get-ShortPath $gitBash
} else {
    $bashCmd = Get-Command bash -ErrorAction SilentlyContinue
    if ($bashCmd) { $bashPath = Get-ShortPath $bashCmd.Path }
}
if (-not $bashPath) {
    [Console]::Error.WriteLine("SKIP: bash not found (tried Git Bash, then PATH)")
    exit 2
}

$TestBin = (Resolve-Path -LiteralPath $TestBin).Path
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$out = Join-Path $env:TEMP "fidget-scenario-chat-header-narrow-$stamp"
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

$ax = Join-Path $root "scripts/ax-window-win.ps1"
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
    function Invoke-Dump([string]$Name) {
        $dump = Join-Path $out "$Name.ax.txt"
        & powershell -NoProfile -ExecutionPolicy Bypass -File $ax dump -ProcessId $proc.Id -Title "BMO" frames |
            Out-File -FilePath $dump -Encoding utf8
        if ($LASTEXITCODE -ne 0) { return $null }
        return $dump
    }
    function Shows-Session {
        $dump = Invoke-Dump "session"
        if (-not $dump) { return $false }
        return [bool](Select-String -LiteralPath $dump -Pattern $Mind -Quiet)
    }
    if (-not (Wait-For 30 { Select-String -LiteralPath $marks -Pattern '^asked$' -Quiet })) {
        Fail "no wake reached the Harness; see $log"
    }
    if (-not (Wait-For 40 { Shows-Session })) {
        Fail "the mind line names no session; see $(Join-Path $out 'session.ax.txt')"
    }
    $sessionLine = Get-Content -LiteralPath (Join-Path $out "session.ax.txt") -Encoding utf8 |
        Where-Object { $_ -match $Mind } | Select-Object -First 1
    Write-Output ("ok: " + (($sessionLine -split '\|')[1]))

    foreach ($w in @(420, 360, 320)) {
        $frameFile = Join-Path $out "$w.frame.txt"
        & powershell -NoProfile -ExecutionPolicy Bypass -File $ax size -ProcessId $proc.Id -Title "BMO" $w 560 |
            Out-File -FilePath $frameFile -Encoding utf8
        if ($LASTEXITCODE -ne 0) { Fail "${w}: resize failed; see $frameFile" }
        $got = [int]((Get-Content -LiteralPath $frameFile -Encoding utf8 | Select-Object -First 1) -split ',')[2]
        if ($got -ne $w) { Fail "${w}: Chat is $got wide after the resize" }
        Start-Sleep -Seconds 1
        $dump = Invoke-Dump "$w"
        if (-not $dump) { Fail "${w}: UI Automation dump failed; see $out\$w.ax.txt" }
        Check-Dump $w $dump
    }
    Write-Output "PASS: evidence in $out"
} finally {
    if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
    Get-CimInstance Win32_Process -Filter "Name = 'bash.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.CommandLine -like "*count=$marks*" } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}
