#!/usr/bin/env pwsh
# Self-check for send-input.win.ps1: INPUT is the size SendInput expects.
# Sends nothing, so it runs under pwsh anywhere and Windows PowerShell 5.1.
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "send-input.win.ps1")

$failed = 0
function Check([bool]$Ok, [string]$What) {
    if ($Ok) { Write-Output "PASS: $What" } else { Write-Output "FAIL: $What"; $script:failed++ }
}

# sizeof(INPUT): 40 on 64-bit, 28 on 32-bit. The MOUSEINPUT arm sizes the union.
$want = 28
if ([IntPtr]::Size -eq 8) { $want = 40 }
Check ([SendInputWin]::InputSize -eq $want) "INPUT is $want bytes for $([IntPtr]::Size * 8)-bit (got $([SendInputWin]::InputSize))"

$msg = [SendInputWin]::Refusal(0, 1, 87, "Windows PowerShell", 4242)
Check ($msg -ceq "SendInput sent 0 of 1 events, error 87; foreground 'Windows PowerShell' pid 4242") "a refusal names the error and the foreground window ($msg)"

if ($failed -eq 0) { Write-Output "all checks passed"; exit 0 }
Write-Output "$failed checks failed"
exit 1
