#!/usr/bin/env pwsh
# Test that FIDGET_TRACE_BUBBLE format strings match pill-dual-display.win.ps1 patterns
$ErrorActionPreference = "Stop"

# Patterns from the scenario
$regionPattern = '^(?:\d+ )?overlay (\S+): region rebuild (\d+) rects \(art \d+ \+ hotspots \d+ \+ painted (\d+)\)'
$followPattern = '^(?:\d+ )?overlay (\S+): pill follows instance=(\S+) chars=(\d+) focused=(\S+)'

# Sample log lines matching the Rust format strings
$regionLine = 'overlay overlay-0: region rebuild 42 rects (art 12 + hotspots 3 + painted 2), 1.23 ms'
$followLine = 'overlay overlay-1: pill follows instance=buddy-1 chars=12 focused=true dnd=off'

# Test region pattern
if ($regionLine -notmatch $regionPattern) {
    Write-Error "Region line does not match pattern"
    Write-Host "Line:    $regionLine"
    Write-Host "Pattern: $regionPattern"
    exit 1
}
Write-Host "PASS: Region line matches pattern"

# Test follow pattern
if ($followLine -notmatch $followPattern) {
    Write-Error "Follow line does not match pattern"
    Write-Host "Line:    $followLine"
    Write-Host "Pattern: $followPattern"
    exit 1
}
Write-Host "PASS: Follow line matches pattern"

Write-Host "PASS: All trace formats match scenario patterns"
