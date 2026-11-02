#!/usr/bin/env pwsh
# Self-check for wait-helpers.win.ps1's piped launch, under pwsh 7 and Windows
# PowerShell 5.1. The child echoes each stdin line to stderr; the wait watches
# an empty folder, so only the pipe can wake it.
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "wait-helpers.win.ps1")

$failed = 0
function Check([bool]$Ok, [string]$What) {
    if ($Ok) { Write-Output "PASS: $What" } else { Write-Output "FAIL: $What"; $script:failed++ }
}

$utf8 = New-Object System.Text.UTF8Encoding $false
$dir = Join-Path ([System.IO.Path]::GetTempPath()) "wait-helpers-$([guid]::NewGuid().ToString('n'))"
$quiet = Join-Path $dir "quiet"
New-Item -ItemType Directory -Path $quiet | Out-Null
$out = Join-Path $dir "app.log"
$err = Join-Path $dir "app.err"
$shell = [System.Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
# Raw UTF-8 both ways, so neither side's console code page touches the bytes.
$echo = '$e = New-Object System.Text.UTF8Encoding $false; ' +
    '$in = New-Object System.IO.StreamReader ([Console]::OpenStandardInput()), $e; ' +
    '$o = New-Object System.IO.StreamWriter ([Console]::OpenStandardError()), $e; $o.NewLine = "`n"; $o.AutoFlush = $true; ' +
    'while ($null -ne ($l = $in.ReadLine())) { $o.WriteLine($l) }'
$script:proc = Start-FidgetPiped $shell @("-NoProfile", "-Command", $echo) $out $err
function Send([string]$Line) {
    $bytes = $utf8.GetBytes("$Line`n")
    $script:proc.StandardInput.BaseStream.Write($bytes, 0, $bytes.Length)
    $script:proc.StandardInput.BaseStream.Flush()
}
function Holds([string]$Line) {
    (Test-Path -LiteralPath $err) -and @(Get-Content -LiteralPath $err -Encoding UTF8 | Where-Object { $_ -ceq $Line }).Count -gt 0
}
# Wait on $Line, sending it from inside the wait's first check: only a wake
# brings the wait back before the cap. True when it saw the line in time.
function Wake-On([string]$Line) {
    $script:sent = $false
    $start = Get-NowMs
    $got = Wait-OnChanges 3 $quiet {
        if (-not $script:sent) { $script:sent = $true; Send $Line; return $false }
        Holds $Line
    }
    $script:took = (Get-NowMs) - $start
    return ($got -and $script:took -lt 1000)
}

try {
    # Started first, so the child's start-up is not counted as a wake.
    Send "ready"
    Check (Wait-ForTrace 3 $err '^ready$') "the child is up and its first line reached the log"
    Check (Wake-On "one") "the piped line wakes the wait, not the cap ($script:took ms)"
    Check (Wake-On "two") "a second line wakes it again ($script:took ms)"
    $word = "h$([char]0xE9)llo $([char]0x2713) $([char]0x65E5)$([char]0x672C)"
    Check (Wake-On $word) "a non-ASCII line wakes it ($script:took ms)"
    # Read while the writer still holds the file open for writing.
    $read = New-Object System.IO.FileStream $err, "Open", "Read", "ReadWrite"
    $copy = New-Object System.IO.MemoryStream
    try { $read.CopyTo($copy) } finally { $read.Dispose() }
    $bytes = $copy.ToArray()
    $want = $utf8.GetBytes("ready`none`ntwo`n$word`n")
    Check ([Convert]::ToBase64String($bytes) -eq [Convert]::ToBase64String($want)) "the log holds the lines as UTF-8 with LF, no BOM or CR"

    $start = Get-NowMs
    Check (-not (Wait-OnChanges 1 $quiet { Holds "never" })) "a line that never comes fails the wait"
    $took = (Get-NowMs) - $start
    Check ($took -ge 900 -and $took -lt 2000) "the wait gives up at its 1 s cap ($took ms)"

    $script:proc.StandardInput.Close()
    Check ($script:proc.WaitForExit(3000)) "the child exits once its stdin closes"
    $start = Get-NowMs
    Check (-not (Wait-OnChanges 3 $quiet { Holds "never" })) "a dead child fails the wait"
    Check (((Get-NowMs) - $start) -lt 1000) "a dead child fails it at once"
    # The end-of-stream wake brings this back; the dead child must not end it.
    $child = $script:proc
    $script:proc = $null
    $start = Get-NowMs
    $closed = Wait-OnChanges 3 $quiet {
        try { [System.IO.File]::Open($err, "Open", "ReadWrite", "None").Dispose(); $true } catch { $false }
    }
    $script:proc = $child
    Check ($closed -and ((Get-NowMs) - $start) -lt 1000) "the log writer closed at end of stream ($((Get-NowMs) - $start) ms)"

    # Scoped to the form's pid and armed once its startup windows exist, so
    # only the form showing 2 s later can wake the wait.
    $formStart = New-Object System.Diagnostics.ProcessStartInfo $shell
    $formStart.Arguments = '-NoProfile -Command "Add-Type -AssemblyName System.Windows.Forms; ' +
        '$f = New-Object System.Windows.Forms.Form; $null = $f.Handle; [Console]::Out.WriteLine(''ready''); ' +
        'Start-Sleep 2; $f.Show(); Start-Sleep 2"'
    $formStart.UseShellExecute = $false
    $formStart.CreateNoWindow = $true
    $formStart.RedirectStandardOutput = $true
    $shown = [System.Diagnostics.Process]::Start($formStart)
    $ready = $shown.StandardOutput.ReadLineAsync()
    Check ($ready.Wait(15000) -and $ready.Result -eq "ready") "the form's process is ready"
    $start = Get-NowMs
    $signal = [FidgetUiaSignal]::new($shown.Id)
    Check (((Get-NowMs) - $start) -lt 1000) "the signal registers at once ($((Get-NowMs) - $start) ms)"
    $woke = $signal.Wait(10000)
    $took = (Get-NowMs) - $start
    Check ($woke -and $took -gt 1000) "the watched process's form wakes the signal ($took ms)"
    $start = Get-NowMs
    $signal.Dispose()
    Check (((Get-NowMs) - $start) -lt 1000) "the signal lets go at once ($((Get-NowMs) - $start) ms)"
    Check ($shown.WaitForExit(20000)) "the window's process exits"

    # A leaf that steps past its stall budget, then blocks: the trail names its
    # last step, the stand-in Fidget dies, and the leaf ends.
    $stall = Join-Path $dir "stall.ps1"
    [System.IO.File]::WriteAllLines($stall, @(
            ". '$(Join-Path $PSScriptRoot "wait-helpers.win.ps1")'",
            '$FidgetStallSeconds = 2',
            "`$p = Start-FidgetPiped '$shell' @('-NoProfile', '-Command', 'Start-Sleep 60') '$dir\stall.out' '$dir\stall.err'",
            '[Console]::Out.WriteLine($p.Id)',
            '1..4 | ForEach-Object { [FidgetTrail]::Mark("step $_"); Start-Sleep -Milliseconds 700 }',
            '[FidgetTrail]::Mark("blocked on nothing")',
            'Start-Sleep 60'
        ), $utf8)
    $leaf = New-Object System.Diagnostics.ProcessStartInfo $shell
    $leaf.Arguments = "-NoProfile -ExecutionPolicy Bypass -File `"$stall`""
    $leaf.UseShellExecute = $false
    $leaf.RedirectStandardOutput = $true
    $leaf.RedirectStandardError = $true
    $start = Get-NowMs
    $leafProc = [System.Diagnostics.Process]::Start($leaf)
    $errTask = $leafProc.StandardError.ReadToEndAsync()
    $standIn = [int]$leafProc.StandardOutput.ReadLine()
    $ended = $leafProc.WaitForExit(20000)
    Check ($ended -and ((Get-NowMs) - $start) -lt 15000) "a stalled leaf ends itself ($((Get-NowMs) - $start) ms)"
    $said = $errTask.Result
    Check ($said.Contains("FAIL: the leaf took no step for 2 s") -and $said.Contains("blocked on nothing")) "the stall waits out the steps and names the last: $said"
    Check ($null -eq (Get-Process -Id $standIn -ErrorAction SilentlyContinue)) "the stall killed Fidget (pid $standIn)"
} finally {
    if (-not $script:proc.HasExited) { $script:proc.Kill() }
    Remove-Item -Recurse -Force -LiteralPath $dir
}
if ($failed) { Write-Output "$failed check(s) failed"; exit 1 }
Write-Output "all checks passed"
