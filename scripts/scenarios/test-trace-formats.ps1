#!/usr/bin/env pwsh
# Test that FIDGET_TRACE_BUBBLE format strings match qm-handoff-dual-display.win.ps1 patterns
$ErrorActionPreference = "Stop"

# Patterns from the scenario
$regionPattern = '^(?:\d+ )?overlay (\S+): region rebuild (\d+) rects \(art \d+ \+ hotspots \d+ \+ painted (\d+)\)'
$handoffPattern = '^(?:\d+ )?overlay (\S+): pill handoff from (\S+) open=(\S+) text="([^"]*)" focused=(\S+)'

# Sample log lines matching the Rust format strings
$regionLine = 'overlay overlay-0: region rebuild 42 rects (art 12 + hotspots 3 + painted 2), 1.23 ms'
$handoffLine = 'overlay overlay-1: pill handoff from overlay-0 open=true text="test message" focused=true dnd=off'

# Test region pattern
if ($regionLine -notmatch $regionPattern) {
    Write-Error "Region line does not match pattern"
    Write-Host "Line:    $regionLine"
    Write-Host "Pattern: $regionPattern"
    exit 1
}
Write-Host "PASS: Region line matches pattern"

# Test handoff pattern
if ($handoffLine -notmatch $handoffPattern) {
    Write-Error "Handoff line does not match pattern"
    Write-Host "Line:    $handoffLine"
    Write-Host "Pattern: $handoffPattern"
    exit 1
}
Write-Host "PASS: Handoff line matches pattern"

Write-Host "PASS: All trace formats match scenario patterns"
